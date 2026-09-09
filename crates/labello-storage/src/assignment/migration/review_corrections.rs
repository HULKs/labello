use super::*;
use labello_domain::{
    MigrationReviewCorrection, ReviewCorrectionChange, ReviewCorrectionSubmission,
};
use std::collections::BTreeSet;

impl DatasetRepository {
    pub async fn submit_review_corrections(
        &self,
        user_id: &UserId,
        context: AssignmentContext<'_>,
        submission: ReviewCorrectionSubmission,
    ) -> StorageResult<ImageState> {
        submission.validate()?;
        let (_config_guard, metadata, image) = self.load_migration_inputs(context.image_id).await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user_id,
            DatasetRole::Reviewer,
        )?;
        let task = metadata
            .task(context.task_id)
            .ok_or_else(|| conflict("review task is missing"))?;
        if context.kind != AssignmentKind::Review
            || !task.enabled
            || task.review.workflow != ReviewWorkflow::Approval
        {
            return Err(conflict(
                "corrections require an enabled approval review assignment",
            ));
        }
        let lock = self.image_lock(context.image_id);
        let _guard = lock.lock().await;
        let state = self.load_image_state(context.image_id).await?;
        let stored = state
            .assignments
            .iter()
            .find(|assignment| assignment.assignment_id == *context.assignment_id)
            .ok_or_else(|| conflict("review assignment is missing"))?;
        if stored.assigned_to != *user_id {
            return Err(StorageError::Unauthorized(
                "corrections belong to another reviewer".into(),
            ));
        }
        if stored.task_id != *context.task_id
            || stored.image_id != *context.image_id
            || stored.kind != AssignmentKind::Review
        {
            return Err(conflict("correction assignment target does not match"));
        }
        if let Some(committed) = state
            .review_correction_submissions
            .get(context.assignment_id)
        {
            return if committed == &submission {
                Ok(state)
            } else {
                Err(conflict(
                    "correction retry differs from the committed submission",
                ))
            };
        }
        if state
            .review_correction_submissions
            .values()
            .any(|old| old.correction_id == submission.correction_id)
        {
            return Err(conflict("correction identity was already used"));
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
        let captured = state
            .review_assignment_contexts
            .get(context.assignment_id)
            .ok_or_else(|| conflict("review context is missing; claim current work"))?;
        if captured.task != *task
            || captured.round != submission.round
            || state.review_round(context.task_id) != Some(&submission.round)
            || captured.target_fingerprint != submission.target_fingerprint
            || state.review_target_fingerprint(task) != submission.target_fingerprint
        {
            return Err(conflict("correction targets or task configuration changed"));
        }
        if captured.decision_revision {
            self.validate_review_revision_context(&state, task, &assignment)
                .await?;
        } else if state
            .task_states
            .get(context.task_id)
            .map(|task| &task.status)
            != Some(&TaskStatus::Submitted)
        {
            return Err(conflict("task is no longer submitted for review"));
        }
        let mut next = state.clone();
        let mut payloads = Vec::new();
        let mut seen = BTreeSet::new();
        for change in &submission.changes {
            let identity = match change {
                ReviewCorrectionChange::Edit { annotation_id, .. }
                | ReviewCorrectionChange::Add { annotation_id, .. }
                | ReviewCorrectionChange::Remove { annotation_id, .. } => {
                    format!("annotation:{annotation_id}")
                }
                ReviewCorrectionChange::MigrationObject {
                    object_group_id, ..
                } => format!("group:{object_group_id}"),
            };
            if !seen.insert(identity) {
                return Err(conflict("each correction target may occur only once"));
            }
            plan_review_change(
                &mut next,
                &mut payloads,
                task,
                &metadata,
                &image,
                user_id,
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
                actor_user_id: user_id.clone(),
                timestamp: now,
            };
            push_simulated(
                &mut next,
                &mut payloads,
                user_id,
                DatasetRole::Reviewer,
                now,
                EventPayload::MigrationFullImageConfirmed { confirmation },
            )?;
        } else {
            // Ordinary box edits may invalidate dependent migration work.
            append_guide_invalidation_payloads(&state, &mut payloads, now);
        }
        let review = ReviewRecord {
            review_id: command_review_id(
                user_id,
                context.assignment_id,
                submission.correction_id.as_str(),
            ),
            target: captured
                .targets
                .last()
                .cloned()
                .ok_or_else(|| conflict("final review target is missing"))?,
            reviewer_user_id: user_id.clone(),
            decision: ReviewDecision::Rejected,
            timestamp: now,
            comment: submission.reason.clone(),
        };
        for other in state.assignments.iter().filter(|other| {
            other.task_id == *context.task_id
                && other.kind == AssignmentKind::Review
                && other.status == AssignmentStatus::Active
                && other.assignment_id != *context.assignment_id
        }) {
            let mut other = other.clone();
            other.status = AssignmentStatus::Cancelled;
            other.updated_at = now;
            payloads.push(EventPayload::AssignmentUpdated { assignment: other });
        }
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
            user_id,
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

#[allow(
    clippy::too_many_arguments,
    reason = "the correction planner needs the exact state, task, actor and draft"
)]
fn plan_review_change(
    state: &mut ImageState,
    payloads: &mut Vec<EventPayload>,
    task: &TaskDefinition,
    metadata: &DatasetMetadata,
    image: &ImageRecord,
    user: &UserId,
    submission: &ReviewCorrectionSubmission,
    assignment_id: &AssignmentId,
    change: &ReviewCorrectionChange,
    now: Timestamp,
) -> StorageResult<()> {
    if let ReviewCorrectionChange::MigrationObject {
        object_group_id,
        expected_disposition_version,
        replacement,
    } = change
    {
        if task.manual_box_guide_migration.is_none() {
            return Err(conflict("task is not a guided migration"));
        }
        object_group_id
            .validate_path_segment()
            .map_err(|error| conflict(error.to_string()))?;
        let target = migration_target(state, &task.task_id, object_group_id)?.clone();
        let disposition = current_disposition(state, &task.task_id, object_group_id)?.clone();
        if disposition.disposition_version != *expected_disposition_version
            || has_dependency(state, &task.task_id, object_group_id)
        {
            return Err(conflict("migration disposition or guide changed"));
        }
        let current = state
            .current_annotation(&target.reserved_skeleton_annotation_id)
            .cloned();
        match replacement {
            MigrationReviewCorrection::Skeleton { skeleton } => {
                let guide = state
                    .current_annotation(&target.guide_annotation_id)
                    .ok_or_else(|| conflict("guide is missing"))?;
                if guide.deleted {
                    return Err(conflict("the canonical migration guide is deleted"));
                }
                if current.as_ref().is_some_and(|old| {
                    !old.deleted && old.geometry == AnnotationGeometry::Skeleton(skeleton.clone())
                }) {
                    return Err(conflict(
                        "corrected skeleton must differ from persisted work",
                    ));
                }
                let annotation = corrected_annotation(
                    task,
                    user,
                    submission,
                    now,
                    current.as_ref(),
                    target.reserved_skeleton_annotation_id.clone(),
                    only_class(task)?.clone(),
                    Some(object_group_id.clone()),
                    AnnotationGeometry::Skeleton(skeleton.clone()),
                );
                append_corrected_annotation(
                    state,
                    payloads,
                    task,
                    image,
                    user,
                    annotation,
                    current.as_ref().map(|old| old.version),
                    now,
                )?;
                let current = state
                    .current_annotation(&target.reserved_skeleton_annotation_id)
                    .expect("appended above");
                let status = MigrationDispositionStatus::Annotated {
                    skeleton_annotation_id: current.annotation_id.clone(),
                    skeleton_version: current.version,
                };
                let disposition = next_disposition(state, &task.task_id, object_group_id, status)?;
                push_simulated(
                    state,
                    payloads,
                    user,
                    DatasetRole::Reviewer,
                    now,
                    EventPayload::MigrationDispositionChanged {
                        task_id: task.task_id.clone(),
                        object_group_id: object_group_id.clone(),
                        disposition,
                    },
                )?;
            }
            MigrationReviewCorrection::Exclude { reason, note } => {
                if matches!(&disposition.status, MigrationDispositionStatus::Excluded { exclusion } if exclusion.reason == *reason)
                {
                    return Err(conflict("exclusion must differ from persisted work"));
                }
                if let Some(current) = current.filter(|old| !old.deleted) {
                    push_simulated(
                        state,
                        payloads,
                        user,
                        DatasetRole::Reviewer,
                        now,
                        EventPayload::AnnotationDeleted {
                            annotation_id: current.annotation_id,
                            version: current.version,
                            reason: submission.reason.clone(),
                        },
                    )?;
                }
                let status = MigrationDispositionStatus::Excluded {
                    exclusion: MigrationExclusion {
                        reason: *reason,
                        note: note.clone(),
                        actor_user_id: user.clone(),
                        timestamp: now,
                        event_id: command_event_id(
                            user,
                            assignment_id,
                            submission.correction_id.as_str(),
                            Some(payloads.len() as u32),
                        ),
                    },
                };
                let disposition = next_disposition(state, &task.task_id, object_group_id, status)?;
                push_simulated(
                    state,
                    payloads,
                    user,
                    DatasetRole::Reviewer,
                    now,
                    EventPayload::MigrationDispositionChanged {
                        task_id: task.task_id.clone(),
                        object_group_id: object_group_id.clone(),
                        disposition,
                    },
                )?;
            }
        }
        return Ok(());
    }
    let (id, expected, geometry, class) = match change {
        ReviewCorrectionChange::Edit {
            annotation_id,
            expected_version,
            geometry,
        } => (annotation_id, Some(*expected_version), Some(geometry), None),
        ReviewCorrectionChange::Add {
            annotation_id,
            class_id,
            geometry,
        } => (annotation_id, None, Some(geometry), Some(class_id)),
        ReviewCorrectionChange::Remove {
            annotation_id,
            expected_version,
        } => (annotation_id, Some(*expected_version), None, None),
        ReviewCorrectionChange::MigrationObject { .. } => unreachable!(),
    };
    id.validate_path_segment()
        .map_err(|error| conflict(error.to_string()))?;
    let current = state.current_annotation(id).cloned();
    if current.as_ref().map(|old| old.version) != expected
        || current
            .as_ref()
            .is_some_and(|old| old.deleted || old.task_id != task.task_id)
    {
        return Err(conflict(
            "annotation target changed or belongs to another task",
        ));
    }
    if task.manual_box_guide_migration.is_some()
        && current
            .as_ref()
            .is_some_and(|old| old.object_group_id.is_some())
    {
        return Err(conflict(
            "canonical skeleton corrections require their disposition target",
        ));
    }
    let guide = task
        .manual_box_guide_migration
        .as_ref()
        .map(|config| {
            metadata
                .task(&config.guide_task_id)
                .ok_or_else(|| conflict("migration guide task is missing"))
        })
        .transpose()?;
    if let Some(geometry) = geometry {
        if current
            .as_ref()
            .is_some_and(|old| old.geometry == *geometry)
        {
            return Err(conflict(
                "corrected geometry must differ from persisted work",
            ));
        }
        let class = class
            .cloned()
            .or_else(|| current.as_ref().map(|old| old.class_id.clone()))
            .expect("edit or add class");
        let annotation = corrected_annotation(
            task,
            user,
            submission,
            now,
            current.as_ref(),
            id.clone(),
            class,
            current.as_ref().and_then(|old| old.object_group_id.clone()),
            geometry.clone(),
        );
        append_corrected_annotation(
            state,
            payloads,
            task,
            image,
            user,
            annotation.clone(),
            expected,
            now,
        )?;
        if let Some(guide) = guide {
            update_discovered_companion(
                state,
                task,
                guide,
                image.dimensions(),
                &annotation,
                user,
                now,
                payloads,
                None,
            )?;
        }
    } else {
        if let Some(guide) = guide {
            delete_discovered_companion(state, task, guide, id, user, now, payloads)?;
        }
        push_simulated(
            state,
            payloads,
            user,
            DatasetRole::Reviewer,
            now,
            EventPayload::AnnotationDeleted {
                annotation_id: id.clone(),
                version: expected.expect("remove version"),
                reason: submission.reason.clone(),
            },
        )?;
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "annotation construction preserves immutable identity and provenance"
)]
fn corrected_annotation(
    task: &TaskDefinition,
    user: &UserId,
    submission: &ReviewCorrectionSubmission,
    now: Timestamp,
    current: Option<&AnnotationVersion>,
    id: AnnotationId,
    class_id: ClassId,
    group: Option<ObjectGroupId>,
    geometry: AnnotationGeometry,
) -> AnnotationVersion {
    AnnotationVersion {
        annotation_id: id,
        version: current.map_or(1, |old| old.version.saturating_add(1)),
        object_group_id: group,
        origin: current.map_or_else(AnnotationOrigin::native, |old| old.origin.clone()),
        task_id: task.task_id.clone(),
        class_id,
        annotation_type: task.annotation_type.clone(),
        revision_source: RevisionSource::ReviewerCorrection {
            correction_id: submission.correction_id.clone(),
        },
        geometry,
        author_user_id: user.clone(),
        created_at: current.map_or(now, |old| old.created_at),
        updated_at: now,
        deleted: false,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "validation and simulation share exact image and actor context"
)]
fn append_corrected_annotation(
    state: &mut ImageState,
    payloads: &mut Vec<EventPayload>,
    task: &TaskDefinition,
    image: &ImageRecord,
    user: &UserId,
    annotation: AnnotationVersion,
    previous_version: Option<u32>,
    now: Timestamp,
) -> StorageResult<()> {
    if previous_version == Some(u32::MAX) {
        return Err(conflict("annotation version is exhausted"));
    }
    annotation.validate_for_task(task, image.dimensions())?;
    if task.manual_box_guide_migration.is_some() {
        let AnnotationGeometry::Skeleton(skeleton) = &annotation.geometry else {
            return Err(conflict("migration requires skeleton geometry"));
        };
        validate_manual_migration_skeleton(skeleton)?;
    }
    push_simulated(
        state,
        payloads,
        user,
        DatasetRole::Reviewer,
        now,
        EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version,
            reason: None,
        },
    )
}
