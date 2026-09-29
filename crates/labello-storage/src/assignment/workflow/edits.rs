use super::*;
use labello_domain::{ReviewCorrectionChange, WorkflowEdits};

impl DatasetRepository {
    pub async fn save_workflow_edits(
        &self,
        user: &UserId,
        assignment: AssignmentContext<'_>,
        edits: WorkflowEdits,
        expected_sequence: u64,
    ) -> StorageResult<ImageState> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let _admission = self.assignment_claim_lock.lock().await;
        let metadata = self.load_dataset().await?;
        let role = role_for_kind(&assignment.kind);
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            role.clone(),
        )?;
        ensure_assignment_target_exists(&metadata, assignment.image_id, assignment.task_id)?;
        let task = metadata.task(assignment.task_id).expect("validated task");
        let independent_available = assignment.kind == AssignmentKind::Review
            && self
                .has_independent_workflow_review(&metadata, user)
                .await?;
        let lock = self.image_lock(assignment.image_id);
        let _image = lock.lock().await;
        let state = self.load_image_state(assignment.image_id).await?;
        let stored = exact_active_assignment(
            &state.assignments,
            assignment.assignment_id,
            assignment.image_id,
            assignment.task_id,
            user,
            &assignment.kind,
            labello_domain::now(),
        )?;
        let context = state
            .workflow_assignments
            .get(assignment.assignment_id)
            .ok_or_else(|| StorageError::AssignmentConflict("missing item context".into()))?;
        validate_definition(context, task)?;
        if let Some(target) = &context.review_target
            && (!state.review_targets(task)?.contains(target)
                || (!state.workflow_independent_reviewer(&context.item, target, user)
                    && (!context.review_exception || independent_available)))
        {
            return Err(StorageError::AssignmentConflict(
                "review item eligibility changed".into(),
            ));
        }
        state.validate_workflow_edits(assignment.task_id, &context.item, &edits)?;
        for change in &edits.changes {
            let geometry = match change {
                ReviewCorrectionChange::Add {
                    class_id, geometry, ..
                } => {
                    if !task.class_ids.contains(class_id) {
                        return Err(StorageError::InvalidAssignment(
                            "draft class differs from workflow".into(),
                        ));
                    }
                    Some(geometry.clone())
                }
                ReviewCorrectionChange::Edit { geometry, .. } => Some(geometry.clone()),
                ReviewCorrectionChange::MigrationObject {
                    replacement: labello_domain::MigrationReviewCorrection::Skeleton { skeleton },
                    ..
                } => Some(AnnotationGeometry::Skeleton(skeleton.clone())),
                _ => None,
            };
            if let Some(geometry) = geometry {
                labello_domain::validate_workflow_partial_geometry(task, &geometry)?;
            }
        }
        if let Some(previous) = state.workflow_edit_drafts.get(assignment.assignment_id)
            && previous.edits == edits
            && previous.previous_sequence == expected_sequence
            && state.workflow_edit_draft(assignment.task_id, &context.item, &assignment.kind)
                == Some(previous)
        {
            return Ok(state);
        }
        let mut renewed = stored.clone();
        renew_assignment(&mut renewed, labello_domain::now());
        let mut payloads = vec![workflow_payload(WorkflowEvent::EditsSaved {
            task_id: assignment.task_id.clone(),
            assignment_id: assignment.assignment_id.clone(),
            edits,
            expected_sequence,
        })];
        if assignment.kind == AssignmentKind::Annotation {
            payloads.extend(reopen_annotation_payload(
                &state,
                assignment.task_id,
                labello_domain::now(),
            ));
        }
        payloads.push(EventPayload::AssignmentUpdated {
            assignment: renewed,
        });
        self.append_payloads_with_state_unlocked(
            assignment.image_id,
            &Actor {
                user_id: user.clone(),
                role,
            },
            payloads,
        )
        .await
        .map(|(_, state)| state)
    }
}
