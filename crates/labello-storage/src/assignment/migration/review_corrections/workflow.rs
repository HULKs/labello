use super::*;
use labello_domain::WorkflowItem;

impl DatasetRepository {
    pub(super) async fn submit_workflow_corrections(
        &self,
        user: &UserId,
        context: AssignmentContext<'_>,
        submission: ReviewCorrectionSubmission,
    ) -> StorageResult<ImageState> {
        let (_guards, metadata, image) = self.load_migration_inputs(context.image_id).await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            DatasetRole::Reviewer,
        )?;
        let task = metadata
            .task(context.task_id)
            .ok_or_else(|| conflict("review task is missing"))?;
        if context.kind != AssignmentKind::Review
            || !task.enabled
            || task.review.workflow != ReviewWorkflow::Approval
        {
            return Err(conflict("corrections require an enabled review workflow"));
        }
        // load_migration_inputs returns configuration only. Global fallback needs
        // the complete image membership, under the same admission guard.
        let dataset = self.load_dataset().await?;
        let independent_available = self.has_independent_workflow_review(&dataset, user).await?;
        let lock = self.image_lock(context.image_id);
        let _image = lock.lock().await;
        let state = self.load_image_state(context.image_id).await?;
        let stored = state
            .assignments
            .iter()
            .find(|a| a.assignment_id == *context.assignment_id)
            .ok_or_else(|| conflict("review assignment is missing"))?;
        if stored.assigned_to != *user
            || stored.task_id != *context.task_id
            || stored.kind != context.kind
        {
            return Err(StorageError::Unauthorized(
                "review item belongs to another worker or workflow".into(),
            ));
        }
        if let Some(previous) = state
            .review_correction_submissions
            .get(context.assignment_id)
        {
            return if *previous == submission {
                Ok(state)
            } else {
                Err(conflict("changed correction retry"))
            };
        }
        let now = labello_domain::now();
        let mut assignment = exact_active_assignment(
            &state.assignments,
            context.assignment_id,
            context.image_id,
            context.task_id,
            user,
            &context.kind,
            now,
        )?
        .clone();
        let captured = state
            .workflow_assignments
            .get(context.assignment_id)
            .ok_or_else(|| conflict("missing queue context"))?;
        let target = captured
            .review_target
            .as_ref()
            .ok_or_else(|| conflict("missing review target"))?;
        if captured.task_fingerprint != labello_domain::workflow_task_fingerprint(task)
            || !state.workflow_seen.contains_key(context.assignment_id)
            || state.review_round(context.task_id) != Some(&submission.round)
            || state.review_target_fingerprint(task) != submission.target_fingerprint
            || !state.review_targets(task)?.contains(target)
            || !state.task_states.get(context.task_id).is_some_and(|s| {
                s.status == TaskStatus::Submitted
                    || (captured.source_assignment_id.is_some()
                        && s.status == TaskStatus::Completed)
            })
            || submission
                .changes
                .iter()
                .any(|change| !state.workflow_correction_in_scope(&captured.item, change))
        {
            return Err(conflict("correction does not match the current queue item"));
        }
        if captured.item == WorkflowItem::Overview {
            let mut definition = task.clone();
            definition.prelabel_config_ids.clear();
            if captured.overview_fingerprint.as_ref()
                != Some(&state.review_target_fingerprint(&definition))
                || !state.workflow_pending_reviews(task)?.is_empty()
            {
                return Err(conflict("Overview targets or prerequisites changed"));
            }
        }
        if !state.workflow_assignment_independent_reviewer(captured, user)
            && (!captured.review_exception || independent_available)
        {
            return Err(conflict("independent review work is available"));
        }
        let mut next = state.clone();
        let mut payloads = Vec::new();
        for change in &submission.changes {
            plan_review_change(
                &mut next,
                &mut payloads,
                task,
                &metadata,
                &image,
                user,
                &submission,
                context.assignment_id,
                change,
                now,
            )?;
        }
        if task.manual_box_guide_migration.is_some() {
            validate_exact_one(&next, task)?;
            let set = &next.migration_target_sets[context.task_id];
            let state_hash = next.current_migration_state_hash(context.task_id)?;
            let confirmation = MigrationConfirmation {
                task_id: context.task_id.clone(),
                confirmation_hash: migration_confirmation_hash(&set.target_set_hash, &state_hash)?,
                target_set_hash: set.target_set_hash.clone(),
                state_hash,
                actor_user_id: user.clone(),
                timestamp: now,
            };
            push_simulated(
                &mut next,
                &mut payloads,
                user,
                DatasetRole::Reviewer,
                now,
                EventPayload::MigrationFullImageConfirmed { confirmation },
            )?;
        } else {
            append_guide_invalidation_payloads(&state, &mut payloads, now);
        }
        // Existing exact-version object approvals survive. Any overview lease is
        // stale after a correction; newly added objects enter Objects first.
        for other in state.assignments.iter().filter(|a| {
            a.task_id == *context.task_id
                && a.status == AssignmentStatus::Active
                && a.assignment_id != *context.assignment_id
        }) {
            let still_current = next
                .workflow_assignments
                .get(&other.assignment_id)
                .is_some_and(|c| {
                    c.item != WorkflowItem::Overview
                        && c.review_target.as_ref().is_some_and(|t| {
                            next.review_targets(task)
                                .is_ok_and(|targets| targets.contains(t))
                        })
                });
            if !still_current {
                let mut released = other.clone();
                released.status = AssignmentStatus::Cancelled;
                released.updated_at = now;
                payloads.push(EventPayload::AssignmentUpdated {
                    assignment: released,
                });
            }
        }
        let review = ReviewRecord {
            review_id: command_review_id(
                user,
                context.assignment_id,
                submission.correction_id.as_str(),
            ),
            target: target.clone(),
            reviewer_user_id: user.clone(),
            decision: ReviewDecision::Rejected,
            timestamp: now,
            comment: submission.reason.clone(),
        };
        assignment.status = AssignmentStatus::Completed;
        assignment.updated_at = now;
        let key = submission.correction_id.to_string();
        let primary_index = payloads.len();
        payloads.push(EventPayload::ReviewCorrectionSubmitted {
            assignment,
            submission: Box::new(submission),
            review,
            task_state: TaskState {
                task_id: context.task_id.clone(),
                status: TaskStatus::Submitted,
                outcome: None,
                assigned_to: None,
                completed_by: None,
                completed_at: None,
                updated_at: now,
            },
        });
        self.append_migration_command_unlocked(
            context.image_id,
            user,
            DatasetRole::Reviewer,
            &key,
            context.assignment_id,
            payloads,
            primary_index,
            now,
        )
        .await
    }
}
