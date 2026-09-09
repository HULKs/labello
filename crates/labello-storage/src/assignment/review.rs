use super::*;

impl DatasetRepository {
    pub async fn current_task_reviews(
        &self,
        image_id: &ImageId,
        task_id: &TaskId,
    ) -> StorageResult<Vec<ReviewRecord>> {
        let events = self.load_events(image_id).await?;
        Ok(current_task_reviews(&events, task_id))
    }

    pub async fn record_review_for_assignment(
        &self,
        user_id: &UserId,
        assignment_context: AssignmentContext<'_>,
        review: ReviewRecord,
    ) -> StorageResult<labello_domain::ImageState> {
        self.record_review_submission(user_id, assignment_context, review, None)
            .await
    }

    pub async fn reject_missing_objects_for_assignment(
        &self,
        user_id: &UserId,
        assignment_context: AssignmentContext<'_>,
        submission: labello_domain::MissingObjectRejection,
    ) -> StorageResult<labello_domain::ImageState> {
        self.record_review_submission(
            user_id,
            assignment_context,
            submission.review.clone(),
            Some(submission),
        )
        .await
    }

    async fn record_review_submission(
        &self,
        user_id: &UserId,
        assignment_context: AssignmentContext<'_>,
        review: ReviewRecord,
        missing_objects: Option<labello_domain::MissingObjectRejection>,
    ) -> StorageResult<labello_domain::ImageState> {
        if review.decision == ReviewDecision::Rejected || missing_objects.is_some() {
            return Err(StorageError::InvalidCorrection(
                "rejection requires a substantive correction submission".into(),
            ));
        }
        let _config_guard = self.review_config_lock.read().await;
        let AssignmentContext {
            assignment_id,
            image_id,
            task_id,
            kind,
        } = assignment_context;
        if kind != AssignmentKind::Review {
            return Err(StorageError::InvalidAssignment(
                "reviews require a review assignment".to_string(),
            ));
        }
        if review.reviewer_user_id != *user_id {
            return Err(StorageError::Unauthorized(
                "cannot record reviews for another user".to_string(),
            ));
        }
        let metadata = self.load_dataset().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user_id,
            DatasetRole::Reviewer,
        )?;
        ensure_assignment_target_exists(&metadata, image_id, task_id)?;
        let task = metadata.task(task_id).ok_or_else(|| {
            StorageError::InvalidAssignment(format!("task {task_id} does not exist"))
        })?;
        if task.manual_box_guide_migration.is_some() {
            return Err(StorageError::InvalidAssignment(
                "manual migration reviews require the migration review workflow".to_string(),
            ));
        }
        if task.review.workflow != ReviewWorkflow::Approval {
            return Err(StorageError::InvalidAssignment(format!(
                "approval reviews are not enabled for task {task_id}"
            )));
        }
        let lock = self.image_lock(image_id);
        let _guard = lock.lock().await;
        let state = self.load_image_state(image_id).await?;
        if let Some(submission) = &missing_objects {
            if let Some(committed) = state.missing_object_submissions.get(assignment_id) {
                let owned = state.assignments.iter().any(|assignment| {
                    assignment.assignment_id == *assignment_id
                        && assignment.assigned_to == *user_id
                        && assignment.task_id == *task_id
                        && assignment.image_id == *image_id
                });
                if !owned {
                    return Err(StorageError::Unauthorized(
                        "missing-object rejection belongs to another assignment owner".into(),
                    ));
                }
                return if committed == submission {
                    Ok(state)
                } else {
                    Err(StorageError::AssignmentConflict(
                        "missing-object rejection retry differs from the committed request".into(),
                    ))
                };
            }
            let context = state
                .review_assignment_contexts
                .get(assignment_id)
                .ok_or_else(|| {
                    StorageError::AssignmentConflict("review context is missing".into())
                })?;
            if context.decision_revision
                || context.task != *task
                || context.round != submission.round
                || context.target_fingerprint != state.review_target_fingerprint(task)
                || state
                    .reviews
                    .iter()
                    .any(|old| old.review_id == review.review_id)
                || state.review_object_targets(task)?.iter().any(|target| {
                    state
                        .effective_review_for_target(task_id, target, user_id)
                        .is_none()
                })
            {
                return Err(StorageError::AssignmentConflict(
                    "missing-object evidence requires the current final review".into(),
                ));
            }
            state
                .missing_object_evidence_for_submission(
                    &metadata.dataset_id,
                    assignment_id,
                    submission,
                    labello_domain::now(),
                )
                .map_err(|_| {
                    StorageError::AssignmentConflict(
                        "missing-object evidence does not match this review".into(),
                    )
                })?;
        }
        super::revision::reject_revision_mutation(&state, task_id)?;
        let now = labello_domain::now();
        let mut assignment = exact_active_assignment(
            &state.assignments,
            assignment_id,
            image_id,
            task_id,
            user_id,
            &AssignmentKind::Review,
            now,
        )?
        .clone();
        if state
            .task_states
            .get(task_id)
            .map(|task_state| &task_state.status)
            != Some(&TaskStatus::Submitted)
        {
            return Err(StorageError::AssignmentConflict(format!(
                "task {task_id} is no longer eligible for review"
            )));
        }

        let captured = state
            .review_assignment_contexts
            .get(assignment_id)
            .ok_or_else(|| StorageError::AssignmentConflict("review context is missing".into()))?;
        if captured.task != *task
            || state.review_round(task_id) != Some(&captured.round)
            || captured.target_fingerprint != state.review_target_fingerprint(task)
            || !captured.targets.contains(&review.target)
        {
            return Err(StorageError::AssignmentConflict(
                "review target or submission changed".into(),
            ));
        }
        if matches!(review.target, ReviewTarget::Task { .. })
            && state.review_object_targets(task)?.iter().any(|target| {
                state
                    .effective_review_for_target(task_id, target, user_id)
                    .is_none_or(|review| review.decision != ReviewDecision::Approved)
            })
        {
            return Err(StorageError::AssignmentConflict(
                "approve every current object before the final image check".into(),
            ));
        }

        let complete = match &review.target {
            ReviewTarget::Image {
                image_id: reviewed_image_id,
            } => {
                if reviewed_image_id != image_id {
                    return Err(StorageError::InvalidAssignment(
                        "review target image does not match assignment image".to_string(),
                    ));
                }
                false
            }
            ReviewTarget::Task {
                task_id: reviewed_task_id,
            } => {
                if reviewed_task_id != task_id {
                    return Err(StorageError::InvalidAssignment(
                        "review target task does not match assignment task".to_string(),
                    ));
                }
                true
            }
            ReviewTarget::AnnotationVersion {
                annotation_id,
                version,
            } => {
                let annotation = state
                    .annotations
                    .get(annotation_id)
                    .and_then(|versions| {
                        versions
                            .iter()
                            .find(|candidate| candidate.version == *version)
                    })
                    .ok_or_else(|| {
                        StorageError::InvalidAssignment(format!(
                            "annotation {annotation_id} version {version} does not exist"
                        ))
                    })?;
                if annotation.task_id != *task_id {
                    return Err(StorageError::InvalidAssignment(
                        "review target task does not match assignment task".to_string(),
                    ));
                }
                false
            }
            ReviewTarget::MigrationDisposition {
                task_id: reviewed_task_id,
                object_group_id,
                disposition_version,
            } => {
                let disposition = state
                    .migration_dispositions
                    .get(reviewed_task_id)
                    .and_then(|dispositions| dispositions.get(object_group_id));
                if reviewed_task_id != task_id
                    || disposition.map(|value| value.disposition_version)
                        != Some(*disposition_version)
                {
                    return Err(StorageError::InvalidAssignment(
                        "migration disposition review target is stale or belongs to another task"
                            .to_string(),
                    ));
                }
                false
            }
            ReviewTarget::MigrationConfirmation {
                task_id: reviewed_task_id,
                confirmation_hash,
            } => {
                let confirmation = state.migration_confirmations.get(reviewed_task_id);
                if reviewed_task_id != task_id
                    || confirmation.map(|value| &value.confirmation_hash) != Some(confirmation_hash)
                {
                    return Err(StorageError::InvalidAssignment(
                        "migration confirmation review target is stale or belongs to another task"
                            .to_string(),
                    ));
                }
                false
            }
        };

        let mut payloads = vec![EventPayload::ReviewRecorded {
            review: review.clone(),
        }];
        if complete {
            let events = self.load_events(image_id).await?;
            let current_reviews = current_task_reviews(&events, task_id);
            if has_task_review_by_user(&current_reviews, task_id, user_id) {
                return Err(StorageError::AssignmentConflict(format!(
                    "user {user_id} already reviewed task {task_id} in this round"
                )));
            }
            let status = match review.decision {
                ReviewDecision::Approved => TaskStatus::Completed,
                ReviewDecision::Rejected => TaskStatus::NeedsCorrection,
            };
            payloads.push(EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: task_id.clone(),
                    outcome: (review.decision == ReviewDecision::Approved)
                        .then_some(TaskOutcome::Approved),
                    status,
                    assigned_to: None,
                    completed_by: Some(user_id.clone()),
                    completed_at: Some(now),
                    updated_at: now,
                },
            });
            for competing in state.assignments.iter().filter(|other| {
                other.task_id == *task_id
                    && other.kind == AssignmentKind::Review
                    && other.status == AssignmentStatus::Active
                    && other.assignment_id != *assignment_id
            }) {
                let mut cancelled = competing.clone();
                cancelled.status = AssignmentStatus::Cancelled;
                cancelled.updated_at = now;
                payloads.push(EventPayload::AssignmentUpdated {
                    assignment: cancelled,
                });
            }
            assignment.status = AssignmentStatus::Completed;
            assignment.updated_at = now;
        } else {
            renew_assignment(&mut assignment, now);
        }
        payloads.push(EventPayload::AssignmentUpdated { assignment });
        if let Some(submission) = missing_objects {
            let evidence = state.missing_object_evidence_for_submission(
                &metadata.dataset_id,
                assignment_id,
                &submission,
                now,
            )?;
            payloads.push(EventPayload::MissingObjectEvidenceRecorded {
                evidence: Box::new(evidence),
                submission: Box::new(submission),
            });
        }
        let (_, state) = self
            .append_payloads_with_state_unlocked(
                image_id,
                &Actor {
                    user_id: user_id.clone(),
                    role: DatasetRole::Reviewer,
                },
                payloads,
            )
            .await?;
        Ok(state)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the public correction boundary keeps actor, assignment, and correction inputs explicit"
    )]
    pub async fn correct_review_annotation(
        &self,
        user_id: &UserId,
        assignment_context: AssignmentContext<'_>,
        correction_id: &CorrectionId,
        annotation_id: &AnnotationId,
        expected_version: u32,
        geometry: AnnotationGeometry,
        reason: Option<String>,
    ) -> StorageResult<EventLogEntry> {
        let state = self.load_image_state(assignment_context.image_id).await?;
        let captured = state
            .review_assignment_contexts
            .get(assignment_context.assignment_id)
            .ok_or_else(|| {
                StorageError::AssignmentConflict(
                    "review context is missing; claim current work".into(),
                )
            })?;
        let submission = labello_domain::ReviewCorrectionSubmission {
            correction_id: correction_id.clone(),
            round: captured.round.clone(),
            target_fingerprint: captured.target_fingerprint.clone(),
            changes: vec![labello_domain::ReviewCorrectionChange::Edit {
                annotation_id: annotation_id.clone(),
                expected_version,
                geometry,
            }],
            reason,
        };
        let image_id = assignment_context.image_id.clone();
        self.submit_review_corrections(user_id, assignment_context, submission)
            .await?;
        self.load_events(&image_id).await?.into_iter().find(|event| matches!(&event.payload,
            EventPayload::ReviewCorrectionSubmitted { submission, .. } if submission.correction_id == *correction_id))
            .ok_or_else(|| StorageError::AssignmentConflict("correction receipt is missing".into()))
    }
}
