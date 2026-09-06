use super::*;
use labello_domain::{ImageState, ReviewAssignmentContext, ReviewRevisionCommit};

fn conflict(message: &str) -> StorageError {
    StorageError::AssignmentConflict(message.into())
}

pub(super) fn capture_review_assignment(
    state: &ImageState,
    task: &TaskDefinition,
    assignment: &Assignment,
    source: Option<&Assignment>,
) -> StorageResult<EventPayload> {
    let round = state
        .review_round(&task.task_id)
        .cloned()
        .ok_or_else(|| conflict("review submission identity is missing"))?;
    let decision_revision = source.is_some_and(|source| {
        source.status == AssignmentStatus::Completed
            || state
                .review_assignment_contexts
                .get(&source.assignment_id)
                .is_some_and(|context| context.decision_revision)
    });
    let context = ReviewAssignmentContext {
        assignment_id: assignment.assignment_id.clone(),
        source_assignment_id: source.map(|source| source.assignment_id.clone()),
        round,
        task: task.clone(),
        target_fingerprint: state.review_target_fingerprint(task),
        targets: state.review_targets(task)?,
        superseded_review_ids: if decision_revision {
            state
                .effective_reviews_for_task(&task.task_id)
                .filter(|review| review.reviewer_user_id == assignment.assigned_to)
                .map(|review| review.review_id.clone())
                .collect()
        } else {
            Vec::new()
        },
        decision_revision,
    };
    Ok(EventPayload::ReviewAssignmentOpened {
        context: Box::new(context),
        assignment: assignment.clone(),
    })
}

pub(super) fn active_review_revision<'a>(
    state: &'a ImageState,
    task_id: &TaskId,
) -> Option<&'a Assignment> {
    let now = labello_domain::now();
    state.assignments.iter().find(|assignment| {
        assignment.task_id == *task_id
            && assignment.status == AssignmentStatus::Active
            && !assignment_is_expired(assignment, now)
            && state
                .review_assignment_contexts
                .get(&assignment.assignment_id)
                .is_some_and(|context| context.decision_revision)
    })
}

pub(super) fn reject_revision_mutation(state: &ImageState, task_id: &TaskId) -> StorageResult<()> {
    if active_review_revision(state, task_id).is_some() {
        return Err(conflict("this task has an exclusive review revision lease"));
    }
    Ok(())
}

pub(super) fn validate_revision_exclusivity(
    before: &ImageState,
    after: &ImageState,
    events: &[EventLogEntry],
) -> StorageResult<()> {
    for context in before
        .review_assignment_contexts
        .values()
        .filter(|context| context.decision_revision)
    {
        if active_review_revision(before, &context.task.task_id)
            .is_none_or(|assignment| assignment.assignment_id != context.assignment_id)
        {
            continue;
        }
        let own_commit = !events.is_empty()
            && matches!(&events[0].payload,
            EventPayload::ReviewRevisionCommitted { assignment, .. } if assignment.assignment_id == context.assignment_id);
        if own_commit
            && events.iter().skip(1).all(|event| match &event.payload {
                EventPayload::ReviewAssignmentFinished { assignment_id, .. } => {
                    assignment_id == &context.assignment_id
                }
                EventPayload::MissingObjectEvidenceRecorded { evidence, .. } => {
                    evidence.assignment_id == context.assignment_id
                }
                _ => false,
            })
        {
            continue;
        }
        if after.review_target_fingerprint(&context.task) != context.target_fingerprint
            || events.iter().any(|event| relevant_event(event, before, &context.task)
                && !matches!(&event.payload, EventPayload::AssignmentUpdated { assignment }
                    if assignment.assignment_id == context.assignment_id)
                && !matches!(&event.payload, EventPayload::ReviewAssignmentFinished { assignment_id, .. } if assignment_id == &context.assignment_id))
        {
            return Err(conflict("this task has an exclusive review revision lease"));
        }
    }
    Ok(())
}

pub(crate) fn finalize_review_transaction(
    before: &ImageState,
    after: &mut ImageState,
    events: &mut Vec<EventLogEntry>,
) -> StorageResult<()> {
    let Some(last) = events.last().cloned() else {
        return Ok(());
    };
    let finished = after
        .assignments
        .iter()
        .filter(|assignment| {
            assignment.kind == AssignmentKind::Review
                && matches!(
                    assignment.status,
                    AssignmentStatus::Completed | AssignmentStatus::Cancelled
                )
                && after
                    .review_assignment_contexts
                    .contains_key(&assignment.assignment_id)
                && !after
                    .review_finished_sequences
                    .contains_key(&assignment.assignment_id)
        })
        .map(|assignment| (assignment.assignment_id.clone(), assignment.task_id.clone()))
        .collect::<Vec<_>>();
    for (assignment_id, task_id) in finished {
        let event = EventLogEntry::new(
            after.current_sequence + 1,
            after.image_id.clone(),
            last.actor_user_id.clone(),
            last.actor_role.clone(),
            last.timestamp,
            EventPayload::ReviewAssignmentFinished {
                assignment_id,
                task_id,
            },
        );
        after.apply_event(&event)?;
        events.push(event);
    }
    validate_revision_exclusivity(before, after, events)
}

fn event_updates_assignment(event: &EventLogEntry, id: &AssignmentId) -> bool {
    match &event.payload {
        EventPayload::AssignmentUpdated { assignment }
        | EventPayload::ReviewAssignmentOpened { assignment, .. }
        | EventPayload::ReviewRevisionCommitted { assignment, .. } => {
            assignment.assignment_id == *id
        }
        EventPayload::ReviewerCorrectionRecorded { assignments, .. } => assignments
            .iter()
            .any(|assignment| assignment.assignment_id == *id),
        _ => false,
    }
}

fn relevant_event(event: &EventLogEntry, state: &ImageState, task: &TaskDefinition) -> bool {
    if event.task_id() == Some(&task.task_id) {
        return true;
    }
    match &event.payload {
        EventPayload::AnnotationDeleted { annotation_id, .. } => state
            .current_annotation(annotation_id)
            .is_some_and(|annotation| annotation.task_id == task.task_id),
        EventPayload::ReviewRecorded { review } => {
            state.review_target_task(&review.target) == Some(&task.task_id)
        }
        EventPayload::MigrationPassItemRecorded { pass_id, .. } => state
            .migration_passes
            .get(pass_id)
            .is_some_and(|pass| pass.task_id == task.task_id),
        _ => false,
    }
}

impl DatasetRepository {
    pub async fn reopen_review_assignment(
        &self,
        user_id: &UserId,
        assignment_id: &AssignmentId,
        image_id: &ImageId,
        task_id: &TaskId,
    ) -> StorageResult<Assignment> {
        let _config_guard = self.review_config_lock.read().await;
        let metadata = self.load_dataset().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user_id,
            DatasetRole::Reviewer,
        )
        .map_err(|_| StorageError::Unauthorized("reviewer role is required".into()))?;
        ensure_assignment_target_exists(&metadata, image_id, task_id)?;
        let task = metadata.task(task_id).expect("validated above");
        if !task.enabled || task.review.workflow != ReviewWorkflow::Approval {
            return Err(conflict(
                "approval review is no longer enabled for this task",
            ));
        }
        let lock = self.image_lock(image_id);
        let _guard = lock.lock().await;
        let state = self.load_image_state(image_id).await?;
        let source_index = state
            .assignments
            .iter()
            .position(|assignment| assignment.assignment_id == *assignment_id)
            .ok_or_else(|| conflict("previous review assignment is missing"))?;
        let source = &state.assignments[source_index];
        if source.assigned_to != *user_id {
            return Err(StorageError::Unauthorized(
                "previous review belongs to another user".into(),
            ));
        }
        if source.kind != AssignmentKind::Review
            || source.task_id != *task_id
            || source.image_id != *image_id
        {
            return Err(StorageError::InvalidAssignment(
                "previous assignment kind or target does not match".into(),
            ));
        }
        if !matches!(
            source.status,
            AssignmentStatus::Cancelled | AssignmentStatus::Completed
        ) {
            return Err(conflict(
                "previous review is not a skipped or completed assignment for this task",
            ));
        }
        let original_context = state.review_assignment_contexts.get(assignment_id)
            .ok_or_else(|| conflict("this historical review has no captured revision context; claim current work instead"))?;
        if original_context.task != *task
            || state.review_round(task_id) != Some(&original_context.round)
            || original_context.round.submitted_by == *user_id
        {
            return Err(conflict(
                "previous review submission or task configuration changed",
            ));
        }
        let now = labello_domain::now();
        let later = state
            .assignments
            .iter()
            .skip(source_index + 1)
            .filter(|assignment| assignment.task_id == *task_id)
            .collect::<Vec<_>>();
        if later.len() == 1 {
            let successor = later[0];
            if state
                .review_assignment_contexts
                .get(&successor.assignment_id)
                .is_some_and(|context| context.source_assignment_id.as_ref() == Some(assignment_id))
                && successor.assigned_to == *user_id
                && successor.status == AssignmentStatus::Active
                && !assignment_is_expired(successor, now)
            {
                self.validate_review_revision_context(&state, task, successor)
                    .await?;
                return Ok(successor.clone());
            }
        }
        if !later.is_empty() {
            return Err(conflict(
                "a later assignment attempt superseded this previous review",
            ));
        }
        if state.assignments.iter().any(|assignment| {
            assignment.task_id == *task_id
                && assignment.status == AssignmentStatus::Active
                && !assignment_is_expired(assignment, now)
        }) {
            return Err(conflict("another assignment still owns this task"));
        }
        let events = self.load_events(image_id).await?;
        let finished = state
            .review_finished_sequences
            .get(assignment_id)
            .ok_or_else(|| conflict("previous assignment terminal boundary is missing"))?;
        if events.iter().any(|event| {
            event.event_sequence > *finished
                && !matches!(event.payload, EventPayload::ReviewAssignmentFinished { .. })
                && relevant_event(event, &state, task)
        }) {
            return Err(conflict("later work superseded the previous review"));
        }
        let own_correction = state
            .reviewer_corrections
            .iter()
            .any(|correction| correction.assignment_id == *assignment_id);
        if !own_correction
            && original_context.target_fingerprint != state.review_target_fingerprint(task)
        {
            return Err(conflict(
                "previous review targets or migration confirmation changed",
            ));
        }
        if source.status == AssignmentStatus::Cancelled
            && !original_context.decision_revision
            && (state.task_states.get(task_id).map(|state| &state.status)
                != Some(&TaskStatus::Submitted)
                || state
                    .effective_review_for_target(
                        task_id,
                        &ReviewTarget::Task {
                            task_id: task_id.clone(),
                        },
                        user_id,
                    )
                    .is_some())
        {
            return Err(conflict("skipped review is no longer eligible"));
        }
        // The client supplies the exact previous ID, but cannot choose an older
        // terminal item from another image in this dataset/task.
        for other_id in metadata.images.keys().filter(|other| *other != image_id) {
            let other = self.load_image_state(other_id).await?;
            if other.assignments.iter().any(|assignment| {
                assignment.task_id == *task_id
                    && assignment.kind == AssignmentKind::Review
                    && assignment.assigned_to == *user_id
                    && matches!(
                        assignment.status,
                        AssignmentStatus::Cancelled | AssignmentStatus::Completed
                    )
                    && assignment.updated_at > source.updated_at
            }) {
                return Err(conflict(
                    "this is no longer the immediately previous review assignment",
                ));
            }
        }
        let assignment = Assignment {
            assignment_id: AssignmentId::generate(),
            image_id: image_id.clone(),
            task_id: task_id.clone(),
            assigned_to: user_id.clone(),
            kind: AssignmentKind::Review,
            status: AssignmentStatus::Active,
            expires_at: Some(lease_expiration(now)),
            created_at: now,
            updated_at: now,
        };
        let payload = capture_review_assignment(&state, task, &assignment, Some(source))?;
        self.append_payloads_unlocked(
            image_id,
            &Actor {
                user_id: user_id.clone(),
                role: DatasetRole::Reviewer,
            },
            vec![payload],
        )
        .await?;
        Ok(assignment)
    }

    pub(super) async fn validate_review_revision_context(
        &self,
        state: &ImageState,
        task: &TaskDefinition,
        assignment: &Assignment,
    ) -> StorageResult<()> {
        let context = state
            .review_assignment_contexts
            .get(&assignment.assignment_id)
            .ok_or_else(|| conflict("review revision context is missing"))?;
        if context.task != *task
            || state.review_round(&task.task_id) != Some(&context.round)
            || context.target_fingerprint != state.review_target_fingerprint(task)
        {
            return Err(conflict(
                "review revision targets or task configuration changed",
            ));
        }
        let events = self.load_events(&assignment.image_id).await?;
        let opened = events.iter().position(|event| matches!(&event.payload,
            EventPayload::ReviewAssignmentOpened { assignment: opened, .. } if opened.assignment_id == assignment.assignment_id))
            .ok_or_else(|| conflict("review revision opening event is missing"))?;
        if events.iter().skip(opened + 1).any(|event| {
            !event_updates_assignment(event, &assignment.assignment_id)
                && relevant_event(event, state, task)
        }) {
            return Err(conflict("later work invalidated this review revision"));
        }
        Ok(())
    }

    pub async fn commit_review_revision(
        &self,
        user_id: &UserId,
        context: AssignmentContext<'_>,
        replacement: ReviewRevisionCommit,
    ) -> StorageResult<ImageState> {
        let _config_guard = self.review_config_lock.read().await;
        if context.kind != AssignmentKind::Review {
            return Err(conflict("revision requires a review assignment"));
        }
        let metadata = self.load_dataset().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user_id,
            DatasetRole::Reviewer,
        )
        .map_err(|_| StorageError::Unauthorized("reviewer role is required".into()))?;
        ensure_assignment_target_exists(&metadata, context.image_id, context.task_id)?;
        let task = metadata.task(context.task_id).expect("validated above");
        let lock = self.image_lock(context.image_id);
        let _guard = lock.lock().await;
        let state = self.load_image_state(context.image_id).await?;
        let stored = state
            .assignments
            .iter()
            .find(|assignment| assignment.assignment_id == *context.assignment_id)
            .ok_or_else(|| conflict("revision assignment is missing"))?;
        if stored.assigned_to != *user_id {
            return Err(StorageError::Unauthorized(
                "revision belongs to another user".into(),
            ));
        }
        if stored.task_id != *context.task_id
            || stored.image_id != *context.image_id
            || stored.kind != AssignmentKind::Review
        {
            return Err(conflict("revision assignment target does not match"));
        }
        if let Some(committed) = state.review_revision_commits.get(context.assignment_id) {
            return if committed == &replacement {
                Ok(state)
            } else {
                Err(conflict("revision retry contains different decisions"))
            };
        }
        let now = labello_domain::now();
        let mut assignment = exact_active_assignment(
            &state.assignments,
            context.assignment_id,
            context.image_id,
            context.task_id,
            user_id,
            &AssignmentKind::Review,
            now,
        )?
        .clone();
        self.validate_review_revision_context(&state, task, &assignment)
            .await?;
        let captured = state
            .review_assignment_contexts
            .get(context.assignment_id)
            .expect("validated above");
        if !captured.decision_revision
            || !task.enabled
            || task.review.workflow != ReviewWorkflow::Approval
        {
            return Err(conflict(
                "this assignment is not an enabled decision revision",
            ));
        }
        let mut projected = state.clone();
        projected
            .superseded_review_ids
            .extend(captured.superseded_review_ids.iter().cloned());
        for review in &replacement.reviews {
            projected
                .review_record_rounds
                .insert(review.review_id.clone(), captured.round.event_id.clone());
            projected.reviews.push(review.clone());
        }
        let (status, outcome) = projected.effective_review_outcome(task);
        let task_state = TaskState {
            task_id: context.task_id.clone(),
            status,
            outcome,
            assigned_to: None,
            completed_by: Some(user_id.clone()),
            completed_at: Some(now),
            updated_at: now,
        };
        assignment.status = AssignmentStatus::Completed;
        assignment.updated_at = now;
        let evidence_payload = if replacement.missing_objects.is_empty() {
            None
        } else {
            let submission = labello_domain::MissingObjectRejection {
                review: replacement
                    .reviews
                    .last()
                    .ok_or_else(|| conflict("final review is missing"))?
                    .clone(),
                round: captured.round.clone(),
                locations: replacement.missing_objects.clone(),
            };
            let evidence = state
                .missing_object_evidence_for_submission(
                    &metadata.dataset_id,
                    context.assignment_id,
                    &submission,
                    now,
                )
                .map_err(|_| conflict("invalid missing-object evidence"))?;
            Some(EventPayload::MissingObjectEvidenceRecorded {
                evidence: Box::new(evidence),
                submission: Box::new(submission),
            })
        };
        let payload = EventPayload::ReviewRevisionCommitted {
            assignment,
            superseded_review_ids: captured.superseded_review_ids.clone(),
            replacement,
            task_state,
        };
        let mut payloads = vec![payload];
        payloads.extend(evidence_payload);
        let (_, state) = self
            .append_payloads_with_state_unlocked(
                context.image_id,
                &Actor {
                    user_id: user_id.clone(),
                    role: DatasetRole::Reviewer,
                },
                payloads,
            )
            .await
            .map_err(|error| match error {
                StorageError::Domain(
                    labello_domain::DomainError::InvalidReviewRevision(message)
                    | labello_domain::DomainError::InvalidMissingObjectEvidence(message),
                ) => conflict(&message),
                other => other,
            })?;
        Ok(state)
    }
}
