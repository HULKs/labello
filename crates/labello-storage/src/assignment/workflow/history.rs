use super::*;
use labello_domain::WorkflowHistoryEntry;

impl DatasetRepository {
    pub async fn workflow_history(
        &self,
        user: &UserId,
        selection: &WorkflowSelection,
    ) -> StorageResult<Vec<WorkflowHistoryEntry>> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let metadata = self.load_dataset_config().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            role_for_kind(&selection.kind),
        )?;
        self.prepare_review_history().await?;
        self.review_history_cache.workflow_history(
            user,
            selection,
            metadata.workflow_queue.history_depth,
        )
    }

    /// Reacquisition preserves the original visit's position. Completed work is
    /// not reopened for other workers until this user actually changes it.
    pub async fn reopen_workflow_item(
        &self,
        user: &UserId,
        requested: AssignmentContext<'_>,
    ) -> StorageResult<Assignment> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let _admission = self.assignment_claim_lock.lock().await;
        let metadata = self.load_dataset().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            role_for_kind(&requested.kind),
        )?;
        ensure_assignment_target_exists(&metadata, requested.image_id, requested.task_id)?;
        let task = metadata.task(requested.task_id).expect("validated task");
        if !Self::task_supports_assignment(task, &requested.kind)? {
            return Err(StorageError::AssignmentConflict(
                "workflow is disabled".into(),
            ));
        }
        self.prepare_review_history().await?;
        let independent_available = requested.kind == AssignmentKind::Review
            && self
                .has_independent_workflow_review(&metadata, user)
                .await?;
        let lock = self.image_lock(requested.image_id);
        let _image = lock.lock().await;
        let mut state = self.load_image_state(requested.image_id).await?;
        let source = state
            .assignments
            .iter()
            .find(|a| a.assignment_id == *requested.assignment_id)
            .cloned()
            .ok_or_else(|| {
                StorageError::AssignmentConflict("history item no longer exists".into())
            })?;
        if source.assigned_to != *user
            || source.task_id != *requested.task_id
            || source.kind != requested.kind
        {
            return Err(StorageError::Unauthorized(
                "history item belongs to another worker or workflow".into(),
            ));
        }
        let mut captured = state
            .workflow_assignments
            .get(&source.assignment_id)
            .cloned()
            .ok_or_else(|| {
                StorageError::AssignmentConflict("missing history item context".into())
            })?;
        let root = captured
            .source_assignment_id
            .clone()
            .unwrap_or_else(|| source.assignment_id.clone());
        let selection = WorkflowSelection {
            task_id: task.task_id.clone(),
            kind: requested.kind.clone(),
            variant: captured.item.variant(),
        };
        let history = self.review_history_cache.workflow_history(
            user,
            &selection,
            metadata.workflow_queue.history_depth,
        )?;
        if !history
            .iter()
            .any(|entry| entry.image_id == *requested.image_id && entry.assignment_id == root)
        {
            return Err(StorageError::AssignmentConflict(
                "item is outside the configured history window".into(),
            ));
        }
        validate_definition(&captured, task)?;
        let now = labello_domain::now();
        let mut payloads = Vec::new();
        for existing in state.assignments.iter_mut().filter(|a| {
            a.assigned_to == *user
                && a.task_id == task.task_id
                && a.kind == selection.kind
                && a.status == AssignmentStatus::Active
                && !assignment_is_expired(a, now)
                && state
                    .workflow_assignments
                    .get(&a.assignment_id)
                    .is_some_and(|c| c.item == captured.item)
        }) {
            let held = &state.workflow_assignments[&existing.assignment_id];
            if held
                .source_assignment_id
                .as_ref()
                .unwrap_or(&existing.assignment_id)
                == &root
            {
                return Ok(existing.clone());
            }
            // A fresh prefetch is not the historical visit being requested.
            // Replace its lease atomically so display retains the visit's order.
            existing.status = AssignmentStatus::Cancelled;
            existing.updated_at = now;
            payloads.push(EventPayload::AssignmentUpdated {
                assignment: existing.clone(),
            });
        }
        if !claimable(&state, &selection, &captured, now) {
            return Err(StorageError::AssignmentConflict(
                "history item is assigned to another worker".into(),
            ));
        }
        let source_position = state
            .assignments
            .iter()
            .position(|a| a.assignment_id == source.assignment_id)
            .expect("source exists");
        if state.assignments.iter().skip(source_position + 1).any(|a| {
            a.task_id == task.task_id
                && a.assigned_to != *user
                && (state.workflow_confirmations.contains_key(&a.assignment_id)
                    || state.workflow_drafts.contains_key(&a.assignment_id)
                    || state.workflow_edit_drafts.contains_key(&a.assignment_id))
                && state
                    .workflow_assignments
                    .get(&a.assignment_id)
                    .is_none_or(|c| {
                        c.item == captured.item
                            || c.item == WorkflowItem::Overview
                            || captured.item == WorkflowItem::Overview
                    })
        }) {
            return Err(StorageError::AssignmentConflict(
                "another worker has changed or reviewed this history item".into(),
            ));
        }
        captured.source_assignment_id = Some(root);
        if requested.kind == AssignmentKind::Review {
            if !state
                .task_states
                .get(&task.task_id)
                .is_some_and(|s| matches!(s.status, TaskStatus::Submitted | TaskStatus::Completed))
            {
                return Err(StorageError::AssignmentConflict(
                    "annotation must finish before returning to review".into(),
                ));
            }
            let target = captured.review_target.as_ref().expect("review context");
            if !state.review_targets(task)?.contains(target) {
                return Err(StorageError::AssignmentConflict(
                    "history review target has changed".into(),
                ));
            }
            let independent = state.workflow_assignment_independent_reviewer(&captured, user);
            if !independent && independent_available {
                return Err(StorageError::AssignmentConflict(
                    "independent review work is available".into(),
                ));
            }
            captured.review_exception = !independent;
            if captured.item == WorkflowItem::Overview
                && !state.workflow_pending_reviews(task)?.is_empty()
            {
                return Err(StorageError::AssignmentConflict(
                    "object reviews must finish before Overview".into(),
                ));
            }
        } else if captured.item == WorkflowItem::Overview
            && !state.workflow_pending_objects(&task.task_id).is_empty()
        {
            return Err(StorageError::AssignmentConflict(
                "object work must finish before Overview".into(),
            ));
        }
        let assignment = Assignment {
            assignment_id: AssignmentId::generate(),
            image_id: requested.image_id.clone(),
            task_id: task.task_id.clone(),
            assigned_to: user.clone(),
            kind: requested.kind.clone(),
            status: AssignmentStatus::Active,
            expires_at: Some(lease_expiration(now)),
            created_at: now,
            updated_at: now,
        };
        payloads.push(workflow_payload(WorkflowEvent::AssignmentOpened {
            assignment: assignment.clone(),
            context: captured,
        }));
        self.append_payloads_unlocked(
            requested.image_id,
            &Actor {
                user_id: user.clone(),
                role: role_for_kind(&requested.kind),
            },
            payloads,
        )
        .await?;
        Ok(assignment)
    }

    pub async fn leave_workflow(
        &self,
        user: &UserId,
        selection: &WorkflowSelection,
    ) -> StorageResult<()> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let _admission = self.assignment_claim_lock.lock().await;
        let metadata = self.load_dataset().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            role_for_kind(&selection.kind),
        )?;
        self.release_workflow_reservations(&metadata, user, selection, &[], true)
            .await
    }

    /// The caller holds dataset admission. Keep only the bounded history window;
    /// saved geometry and visit records survive lease release.
    pub(super) async fn release_workflow_reservations(
        &self,
        metadata: &DatasetMetadata,
        user: &UserId,
        selection: &WorkflowSelection,
        keep: &[AssignmentId],
        release_unseen: bool,
    ) -> StorageResult<()> {
        let now = labello_domain::now();
        let releasable = |state: &ImageState, assignment: &Assignment| {
            let Some(context) = state.workflow_assignments.get(&assignment.assignment_id) else {
                return false;
            };
            let root = context
                .source_assignment_id
                .as_ref()
                .unwrap_or(&assignment.assignment_id);
            assignment.assigned_to == *user
                && assignment.task_id == selection.task_id
                && assignment.kind == selection.kind
                && context.item.variant() == selection.variant
                && assignment.status == AssignmentStatus::Active
                && !keep.contains(root)
                && (release_unseen || state.workflow_seen.contains_key(&assignment.assignment_id))
        };
        // Display cleanup can release only seen visits. The committed history
        // projection already identifies their images; unrelated images cannot
        // contain a releasable visit. Departure also releases unseen prefetches.
        let images = if release_unseen {
            metadata.images.keys().cloned().collect()
        } else {
            self.review_history_cache
                .workflow_history_images(user, selection)?
        };
        for image_id in &images {
            let cached = self
                .workflow_polling_state(image_id, metadata.bounding_box_visibility)
                .await?;
            if !cached.assignments.iter().any(|a| releasable(&cached, a)) {
                continue;
            }
            // Cached facts select candidates only. Recheck exact ownership and
            // retained history under the image lock before publishing a release.
            let lock = self.image_lock(image_id);
            let _image = lock.lock().await;
            let state = self.load_image_state(image_id).await?;
            let payloads = state
                .assignments
                .iter()
                .filter(|assignment| releasable(&state, assignment))
                .map(|assignment| {
                    let mut released = assignment.clone();
                    released.status = AssignmentStatus::Cancelled;
                    released.updated_at = now;
                    EventPayload::AssignmentUpdated {
                        assignment: released,
                    }
                })
                .collect::<Vec<_>>();
            if !payloads.is_empty() {
                self.append_payloads_unlocked(
                    image_id,
                    &Actor {
                        user_id: user.clone(),
                        role: role_for_kind(&selection.kind),
                    },
                    payloads,
                )
                .await?;
            }
        }
        Ok(())
    }
}
