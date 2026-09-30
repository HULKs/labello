use super::*;
use labello_domain::{
    Actor, WorkflowAnnotation, WorkflowAssignmentContext, WorkflowConfirmation, WorkflowEvent,
    WorkflowItem, WorkflowObject,
};

pub(super) fn item_cursor(
    state: &ImageState,
    task: &TaskId,
    item: &WorkflowItem,
) -> StorageResult<MigrationCursor> {
    Ok(match item {
        WorkflowItem::Object {
            object: WorkflowObject::Migration { object_group_id },
        } => MigrationCursor::Object {
            object_group_id: object_group_id.clone(),
            sequence_index: migration_target(state, task, object_group_id)?.sequence_index,
        },
        WorkflowItem::Overview => state.migration_cursor(task, None)?,
        _ => MigrationCursor::FullImage,
    })
}

pub(super) fn validate_migration_item(
    state: &ImageState,
    assignment: &AssignmentContext<'_>,
    captured: &WorkflowAssignmentContext,
    group: &ObjectGroupId,
) -> StorageResult<()> {
    let owns_target = match &captured.item {
        WorkflowItem::Object {
            object: WorkflowObject::Migration { object_group_id },
        } => object_group_id == group,
        WorkflowItem::Overview => matches!(state.migration_cursor(assignment.task_id, None)?,
            MigrationCursor::Object { object_group_id, .. } if &object_group_id == group),
        _ => false,
    };
    if !state.workflow_seen.contains_key(assignment.assignment_id) || !owns_target {
        return Err(conflict(
            "migration target does not match the displayed queue item",
        ));
    }
    if captured.source_assignment_id.is_none() {
        ensure_annotation_status(state, assignment.task_id)?;
    }
    Ok(())
}

impl DatasetRepository {
    pub(in crate::assignment) async fn append_workflow_companions(
        &self,
        task: &TaskDefinition,
        state: &mut ImageState,
        actor: &Actor,
        payloads: &mut Vec<EventPayload>,
        now: Timestamp,
    ) -> StorageResult<()> {
        let metadata = self.load_dataset().await?;
        let image = metadata
            .images
            .get(&state.image_id)
            .ok_or_else(|| conflict("image is missing"))?;
        let (_, guide, dimensions) = migration_metadata(&metadata, image, &task.task_id)?;
        let changed = payloads
            .iter()
            .filter_map(|payload| match payload {
                EventPayload::AnnotationVersionCreated { annotation, .. } => {
                    Some(annotation.annotation_id.clone())
                }
                EventPayload::AnnotationDeleted { annotation_id, .. } => {
                    Some(annotation_id.clone())
                }
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        validate_exact_one(state, task)?;
        for id in changed {
            let annotation = state
                .current_annotation(&id)
                .ok_or_else(|| conflict("object is missing"))?
                .clone();
            if annotation.object_group_id.is_some() {
                return Err(conflict(
                    "reserved migration targets require migration commands",
                ));
            }
            if annotation.deleted {
                delete_discovered_companion(
                    state,
                    task,
                    guide,
                    &id,
                    &actor.user_id,
                    now,
                    payloads,
                )?;
            } else {
                let AnnotationGeometry::Skeleton(skeleton) = &annotation.geometry else {
                    return Err(conflict("migration objects must be skeletons"));
                };
                validate_manual_migration_skeleton(skeleton)?;
                update_discovered_companion(
                    state,
                    task,
                    guide,
                    dimensions,
                    &annotation,
                    &actor.user_id,
                    now,
                    payloads,
                    None,
                )?;
            }
        }
        Ok(())
    }

    pub(super) async fn migration_workflow_payloads(
        &self,
        image: &ImageId,
        user: &UserId,
        role: DatasetRole,
        assignment_id: &AssignmentId,
        mut payloads: Vec<EventPayload>,
        now: Timestamp,
    ) -> StorageResult<Vec<EventPayload>> {
        let state = self.load_image_state(image).await?;
        let Some(context) = state.workflow_assignments.get(assignment_id) else {
            return Ok(payloads);
        };
        if role == DatasetRole::Reviewer && payloads.iter().any(|p| matches!(p, EventPayload::ReviewCorrectionSubmitted { assignment, .. } if assignment.assignment_id == *assignment_id)) {
            return Ok(payloads);
        }
        let assignment = state
            .assignments
            .iter()
            .find(|a| a.assignment_id == *assignment_id)
            .expect("queue assignment");
        exact_active_assignment(
            &state.assignments,
            assignment_id,
            image,
            &assignment.task_id,
            user,
            &AssignmentKind::Annotation,
            now,
        )?;
        if role != DatasetRole::Annotator || !state.workflow_seen.contains_key(assignment_id) {
            return Err(conflict(
                "migration command requires a displayed annotation item",
            ));
        }
        let metadata = self.load_dataset_config().await?;
        let task = metadata
            .task(&assignment.task_id)
            .ok_or_else(|| conflict("workflow does not exist"))?;
        if context.task_fingerprint != labello_domain::workflow_task_fingerprint(task) {
            return Err(conflict(
                "workflow definition changed since this item was claimed",
            ));
        }
        if payloads.iter().any(|p| {
            matches!(
                p,
                EventPayload::MigrationPassStarted { .. }
                    | EventPayload::MigrationPassItemRecorded { .. }
            )
        }) {
            return Err(conflict("object queues do not use migration passes"));
        }
        let complete_overview = payloads
            .iter()
            .any(|p| matches!(p, EventPayload::MigrationFullImageConfirmed { .. }));
        if complete_overview
            && (context.item != WorkflowItem::Overview
                || !state
                    .workflow_pending_objects(&assignment.task_id)
                    .is_empty())
        {
            return Err(conflict("finish Objects before submitting Overview"));
        }
        if context.item != WorkflowItem::Overview {
            for payload in &payloads {
                let allowed = match payload {
                    EventPayload::AnnotationVersionCreated { annotation, .. } => {
                        annotation.task_id == assignment.task_id
                            && state.workflow_object_matches_annotation(&context.item, annotation)
                    }
                    EventPayload::AnnotationDeleted { annotation_id, .. } => {
                        state.current_annotation(annotation_id).is_some_and(|a| {
                            a.task_id == assignment.task_id
                                && state.workflow_object_matches_annotation(&context.item, a)
                        })
                    }
                    EventPayload::MigrationDispositionChanged {
                        task_id,
                        object_group_id,
                        ..
                    }
                    | EventPayload::MigrationDispositionReopened {
                        task_id,
                        object_group_id,
                        ..
                    } => {
                        *task_id == assignment.task_id
                            && context.item
                                == (WorkflowItem::Object {
                                    object: WorkflowObject::Migration {
                                        object_group_id: object_group_id.clone(),
                                    },
                                })
                    }
                    EventPayload::TaskStateChanged { task_state } => {
                        task_state.task_id == assignment.task_id
                            && matches!(
                                task_state.status,
                                TaskStatus::InProgress | TaskStatus::NeedsCorrection
                            )
                    }
                    EventPayload::AssignmentUpdated {
                        assignment: renewed,
                    } => {
                        (renewed.assignment_id == *assignment_id
                            && renewed.status == AssignmentStatus::Active)
                            || (renewed.task_id == assignment.task_id
                                && renewed.kind == AssignmentKind::Review
                                && renewed.status == AssignmentStatus::Cancelled)
                    }
                    EventPayload::MigrationDependencyCleared {
                        task_id,
                        object_group_id,
                        ..
                    } => {
                        *task_id == assignment.task_id
                            && context.item
                                == (WorkflowItem::Object {
                                    object: WorkflowObject::Migration {
                                        object_group_id: object_group_id.clone(),
                                    },
                                })
                    }
                    _ => false,
                };
                if !allowed {
                    return Err(conflict("migration mutation belongs to another queue item"));
                }
            }
        }
        if context.item == WorkflowItem::Overview
            && !complete_overview
            && payloads.iter().any(|payload| {
                matches!(
                    payload,
                    EventPayload::AnnotationVersionCreated { .. }
                        | EventPayload::AnnotationDeleted { .. }
                )
            })
            && let Some(draft) =
                state.pending_workflow_edits(&assignment.task_id, &context.item, &assignment.kind)
        {
            // A discovery command publishes the current editor's geometry. Clear its
            // proposal in the same transaction, so replay cannot restore it twice.
            payloads.insert(
                0,
                EventPayload::Workflow {
                    event: Box::new(WorkflowEvent::EditsSaved {
                        task_id: assignment.task_id.clone(),
                        assignment_id: assignment_id.clone(),
                        edits: labello_domain::WorkflowEdits::default(),
                        expected_sequence: draft.sequence,
                    }),
                },
            );
        }
        // Completion is owned by the receipt, retaining the lease through all
        // geometry/disposition events in this atomic transaction.
        payloads.retain(|p| !matches!(p, EventPayload::AssignmentUpdated { assignment } if assignment.assignment_id == *assignment_id && assignment.status == AssignmentStatus::Completed));
        let mut next = state.clone();
        for payload in &payloads {
            next.apply_event(&EventLogEntry::new(
                next.current_sequence + 1,
                image.clone(),
                user.clone(),
                role.clone(),
                now,
                payload.clone(),
            ))?;
        }
        let complete_object = match &context.item {
            WorkflowItem::Object {
                object: WorkflowObject::Migration { object_group_id },
            } => next.workflow_migration_object_complete(&assignment.task_id, object_group_id),
            _ => false,
        };
        if complete_object || complete_overview {
            let annotation = (context.item != WorkflowItem::Overview)
                .then(|| {
                    next.active_annotations().find(|a| {
                        a.task_id == assignment.task_id
                            && next.workflow_object_matches_annotation(&context.item, a)
                    })
                })
                .flatten()
                .map(|a| WorkflowAnnotation {
                    annotation_id: a.annotation_id.clone(),
                    version: a.version,
                });
            payloads.push(EventPayload::Workflow {
                event: Box::new(WorkflowEvent::ItemConfirmed {
                    confirmation: WorkflowConfirmation {
                        assignment_id: assignment_id.clone(),
                        task_id: assignment.task_id.clone(),
                        annotation,
                        review: None,
                        reviewed_targets: vec![],
                    },
                }),
            });
        }
        Ok(payloads)
    }
}
