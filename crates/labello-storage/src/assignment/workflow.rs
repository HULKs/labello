use super::*;
use labello_domain::{
    ImageState, WorkflowAnnotation, WorkflowAssignmentContext, WorkflowConfirmation, WorkflowEvent,
    WorkflowItem, WorkflowItemRef, WorkflowObject, WorkflowPreparation, WorkflowSelection,
    WorkflowVariant,
};

mod availability;
mod edits;
mod history;
mod preparation;
#[cfg(test)]
mod tests;

fn workflow_payload(event: WorkflowEvent) -> EventPayload {
    EventPayload::Workflow {
        event: Box::new(event),
    }
}

fn reopen_annotation_payload(
    state: &ImageState,
    task: &TaskId,
    now: labello_domain::Timestamp,
) -> Option<EventPayload> {
    (!state.assignment_eligible(task)).then(|| EventPayload::TaskStateChanged {
        task_state: TaskState {
            task_id: task.clone(),
            status: TaskStatus::InProgress,
            outcome: None,
            assigned_to: None,
            completed_by: None,
            completed_at: None,
            updated_at: now,
        },
    })
}

fn validate_definition(
    context: &WorkflowAssignmentContext,
    task: &TaskDefinition,
) -> StorageResult<()> {
    if context.task_fingerprint != labello_domain::workflow_task_fingerprint(task) {
        return Err(StorageError::AssignmentConflict(
            "workflow definition changed since this item was claimed".into(),
        ));
    }
    Ok(())
}

fn initial_preparation(state: &ImageState, task: &TaskDefinition) -> WorkflowPreparation {
    let objects = if let Some(set) = state.migration_target_sets.get(&task.task_id) {
        let mut targets = set.targets.iter().collect::<Vec<_>>();
        targets.sort_by_key(|target| target.sequence_index);
        targets
            .into_iter()
            .map(|target| WorkflowObject::Migration {
                object_group_id: target.object_group_id.clone(),
            })
            .collect()
    } else {
        state
            .visible_annotations()
            .filter(|a| a.task_id == task.task_id)
            .map(|annotation| WorkflowObject::Annotation {
                annotation_id: annotation.annotation_id.clone(),
            })
            .collect()
    };
    WorkflowPreparation {
        objects,
        ..Default::default()
    }
}

fn candidates(
    state: &ImageState,
    task: &TaskDefinition,
    selection: &WorkflowSelection,
) -> StorageResult<Vec<WorkflowAssignmentContext>> {
    if !task.enabled || !state.included_in_completion_denominator(&task.task_id) {
        return Ok(vec![]);
    }
    let contexts = match selection.kind {
        AssignmentKind::Annotation if state.assignment_eligible(&task.task_id) => {
            if !state.workflow_preparations.contains_key(&task.task_id) {
                return Ok(vec![]);
            }
            let pending = state.workflow_pending_objects(&task.task_id);
            let items = match selection.variant {
                WorkflowVariant::Objects => pending
                    .into_iter()
                    .map(|object| WorkflowItem::Object { object })
                    .collect(),
                WorkflowVariant::Overview
                    if pending.is_empty()
                        && state.workflow_preparations[&task.task_id].status
                            == labello_domain::WorkflowPreparationStatus::Ready =>
                {
                    vec![WorkflowItem::Overview]
                }
                WorkflowVariant::Overview => vec![],
            };
            items
                .into_iter()
                .map(|item| WorkflowAssignmentContext {
                    item,
                    task_fingerprint: labello_domain::workflow_task_fingerprint(task),
                    overview_fingerprint: None,
                    review_target: None,
                    review_exception: false,
                    source_assignment_id: None,
                })
                .collect()
        }
        AssignmentKind::Review
            if task.review.workflow == ReviewWorkflow::Approval
                && state
                    .task_states
                    .get(&task.task_id)
                    .is_some_and(|s| s.status == TaskStatus::Submitted) =>
        {
            let pending = state.workflow_pending_reviews(task)?;
            let targets = match selection.variant {
                WorkflowVariant::Objects => pending,
                WorkflowVariant::Overview if pending.is_empty() => state
                    .review_targets(task)?
                    .into_iter()
                    .rev()
                    .take(1)
                    .collect(),
                WorkflowVariant::Overview => vec![],
            };
            targets
                .into_iter()
                .map(|target| WorkflowAssignmentContext {
                    task_fingerprint: labello_domain::workflow_task_fingerprint(task),
                    overview_fingerprint: (selection.variant == WorkflowVariant::Overview).then(
                        || {
                            let mut definition = task.clone();
                            definition.prelabel_config_ids.clear();
                            state.review_target_fingerprint(&definition)
                        },
                    ),
                    item: WorkflowItem::from_review_target(&target),
                    review_target: Some(target),
                    review_exception: false,
                    source_assignment_id: None,
                })
                .collect()
        }
        _ => vec![],
    };
    Ok(contexts)
}

fn claimable(
    state: &ImageState,
    selection: &WorkflowSelection,
    context: &WorkflowAssignmentContext,
    now: labello_domain::Timestamp,
) -> bool {
    !state.assignments.iter().any(|assignment| {
        assignment.task_id == selection.task_id
            && assignment.status == AssignmentStatus::Active
            && !assignment_is_expired(assignment, now)
            && state
                .workflow_assignments
                .get(&assignment.assignment_id)
                .is_none_or(|held| {
                    assignment.kind != selection.kind
                        || held.item == context.item
                        || held.item == WorkflowItem::Overview
                        || context.item == WorkflowItem::Overview
                })
    })
}

impl DatasetRepository {
    pub async fn save_workflow_draft(
        &self,
        user: &UserId,
        assignment: AssignmentContext<'_>,
        geometry: AnnotationGeometry,
        expected_sequence: u64,
    ) -> StorageResult<ImageState> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let _admission = self.assignment_claim_lock.lock().await;
        let metadata = self.load_dataset().await?;
        if assignment.kind != AssignmentKind::Annotation {
            return Err(StorageError::InvalidAssignment(
                "partial drafts require annotation work".into(),
            ));
        }
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            DatasetRole::Annotator,
        )?;
        ensure_assignment_target_exists(&metadata, assignment.image_id, assignment.task_id)?;
        let task = metadata.task(assignment.task_id).expect("validated task");
        labello_domain::validate_workflow_partial_geometry(task, &geometry)?;
        let lock = self.image_lock(assignment.image_id);
        let _image = lock.lock().await;
        let state = self.load_image_state(assignment.image_id).await?;
        let stored = exact_active_assignment(
            &state.assignments,
            assignment.assignment_id,
            assignment.image_id,
            assignment.task_id,
            user,
            &AssignmentKind::Annotation,
            labello_domain::now(),
        )?;
        let context = state
            .workflow_assignments
            .get(assignment.assignment_id)
            .ok_or_else(|| StorageError::AssignmentConflict("missing object context".into()))?;
        validate_definition(context, task)?;
        if let Some(previous) = state.workflow_object_draft(assignment.task_id, &context.item)
            && previous.geometry == geometry
            && previous.previous_sequence == expected_sequence
            && state.workflow_drafts.get(assignment.assignment_id) == Some(previous)
        {
            return Ok(state);
        }
        let mut renewed = stored.clone();
        renew_assignment(&mut renewed, labello_domain::now());
        let mut payloads = vec![workflow_payload(WorkflowEvent::DraftSaved {
            task_id: assignment.task_id.clone(),
            assignment_id: assignment.assignment_id.clone(),
            geometry,
            expected_sequence,
        })];
        payloads.extend(reopen_annotation_payload(
            &state,
            assignment.task_id,
            labello_domain::now(),
        ));
        payloads.push(EventPayload::AssignmentUpdated {
            assignment: renewed,
        });
        self.append_payloads_with_state_unlocked(
            assignment.image_id,
            &Actor {
                user_id: user.clone(),
                role: DatasetRole::Annotator,
            },
            payloads,
        )
        .await
        .map(|(_, state)| state)
    }

    pub async fn confirm_workflow_review(
        &self,
        user: &UserId,
        assignment: AssignmentContext<'_>,
        review: ReviewRecord,
    ) -> StorageResult<ImageState> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let _admission = self.assignment_claim_lock.lock().await;
        let metadata = self.load_dataset().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            DatasetRole::Reviewer,
        )?;
        ensure_assignment_target_exists(&metadata, assignment.image_id, assignment.task_id)?;
        let task = metadata.task(assignment.task_id).expect("validated task");
        let independent_available = self
            .has_independent_workflow_review(&metadata, user)
            .await?;
        let lock = self.image_lock(assignment.image_id);
        let _image = lock.lock().await;
        let state = self.load_image_state(assignment.image_id).await?;
        let context = state
            .workflow_assignments
            .get(assignment.assignment_id)
            .ok_or_else(|| StorageError::AssignmentConflict("item context is missing".into()))?;
        if review.reviewer_user_id != *user || assignment.kind != AssignmentKind::Review {
            return Err(StorageError::Unauthorized(
                "review belongs to another worker".into(),
            ));
        }
        if let Some(confirmed) = state.workflow_confirmations.get(assignment.assignment_id) {
            let mut retry = review.clone();
            if let Some(previous) = &confirmed.review {
                retry.timestamp = previous.timestamp;
                if retry == *previous {
                    return Ok(state);
                }
            }
            return Err(StorageError::AssignmentConflict(
                "changed review confirmation retry".into(),
            ));
        }
        exact_active_assignment(
            &state.assignments,
            assignment.assignment_id,
            assignment.image_id,
            assignment.task_id,
            user,
            &AssignmentKind::Review,
            labello_domain::now(),
        )?;
        validate_definition(context, task)?;
        if let Some(fingerprint) = &context.overview_fingerprint {
            let mut definition = task.clone();
            definition.prelabel_config_ids.clear();
            if state.review_target_fingerprint(&definition) != *fingerprint {
                return Err(StorageError::AssignmentConflict(
                    "Overview targets changed since this item was claimed".into(),
                ));
            }
        }
        if !task.enabled
            || task.review.workflow != ReviewWorkflow::Approval
            || !state.task_states.get(assignment.task_id).is_some_and(|s| {
                s.status == TaskStatus::Submitted
                    || (context.source_assignment_id.is_some() && s.status == TaskStatus::Completed)
            })
            || context.review_target.as_ref() != Some(&review.target)
            || !state.review_targets(task)?.contains(&review.target)
            || review.decision != ReviewDecision::Approved
        {
            return Err(StorageError::AssignmentConflict(
                "review target changed or needs a correction transaction".into(),
            ));
        }
        let independent = state.workflow_assignment_independent_reviewer(context, user);
        if !independent && (!context.review_exception || independent_available) {
            return Err(StorageError::AssignmentConflict(
                "independent review work is available".into(),
            ));
        }
        if context.item == WorkflowItem::Overview
            && !state.workflow_pending_reviews(task)?.is_empty()
        {
            return Err(StorageError::AssignmentConflict(
                "object reviews must finish before Overview".into(),
            ));
        }
        let now = labello_domain::now();
        let mut payloads = vec![
            EventPayload::ReviewRecorded {
                review: review.clone(),
            },
            workflow_payload(WorkflowEvent::ItemConfirmed {
                confirmation: WorkflowConfirmation {
                    assignment_id: assignment.assignment_id.clone(),
                    task_id: task.task_id.clone(),
                    annotation: None,
                    reviewed_targets: vec![review.target.clone()],
                    review: Some(review),
                },
            }),
        ];
        if context.item == WorkflowItem::Overview {
            payloads.push(EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: task.task_id.clone(),
                    status: TaskStatus::Completed,
                    outcome: Some(TaskOutcome::Approved),
                    assigned_to: None,
                    completed_by: Some(user.clone()),
                    completed_at: Some(now),
                    updated_at: now,
                },
            });
        }
        self.append_payloads_with_state_unlocked(
            assignment.image_id,
            &Actor {
                user_id: user.clone(),
                role: DatasetRole::Reviewer,
            },
            payloads,
        )
        .await
        .map(|(_, state)| state)
    }

    pub(super) async fn apply_workflow_annotation_unlocked(
        &self,
        state: &ImageState,
        task: &TaskDefinition,
        actor: &Actor,
        assignment_id: &AssignmentId,
        mut payloads: Vec<EventPayload>,
        complete: bool,
    ) -> StorageResult<ImageState> {
        let context = &state.workflow_assignments[assignment_id];
        let stored = state
            .assignments
            .iter()
            .find(|a| a.assignment_id == *assignment_id)
            .ok_or_else(|| StorageError::AssignmentConflict("item assignment is missing".into()))?;
        if stored.assigned_to != actor.user_id || stored.kind != AssignmentKind::Annotation {
            return Err(StorageError::Unauthorized(
                "item belongs to another worker".into(),
            ));
        }
        if complete
            && payloads.is_empty()
            && state.workflow_confirmations.contains_key(assignment_id)
        {
            return Ok(state.clone());
        }
        validate_definition(context, task)?;
        let now = labello_domain::now();
        let mut assignment = exact_active_assignment(
            &state.assignments,
            assignment_id,
            &state.image_id,
            &task.task_id,
            &actor.user_id,
            &AssignmentKind::Annotation,
            now,
        )?
        .clone();
        if (!state.assignment_eligible(&task.task_id) && context.source_assignment_id.is_none())
            || !state.workflow_seen.contains_key(assignment_id)
        {
            return Err(StorageError::AssignmentConflict(
                "item is not displayed annotation work".into(),
            ));
        }
        let mut next = state.clone();
        for payload in &payloads {
            let annotation = match payload {
                EventPayload::AnnotationVersionCreated { annotation, .. } => annotation,
                EventPayload::AnnotationDeleted { annotation_id, .. } => {
                    next.current_annotation(annotation_id).ok_or_else(|| {
                        StorageError::InvalidAssignment("deleted annotation is missing".into())
                    })?
                }
                _ => {
                    return Err(StorageError::InvalidAssignment(
                        "item saves accept annotation mutations only".into(),
                    ));
                }
            };
            if !next.workflow_object_matches_annotation(&context.item, annotation) {
                return Err(StorageError::AssignmentConflict(
                    "mutation belongs to another queue item".into(),
                ));
            }
            next.apply_event(&EventLogEntry::new(
                next.current_sequence + 1,
                state.image_id.clone(),
                actor.user_id.clone(),
                actor.role.clone(),
                now,
                payload.clone(),
            ))?;
        }
        let changed = !payloads.is_empty();
        if changed && task.manual_box_guide_migration.is_some() {
            self.append_workflow_companions(task, &mut next, actor, &mut payloads, now)
                .await?;
        }
        if context.item != WorkflowItem::Overview
            && next
                .active_annotations()
                .filter(|a| {
                    a.task_id == task.task_id
                        && next.workflow_object_matches_annotation(&context.item, a)
                })
                .count()
                > 1
        {
            return Err(StorageError::AssignmentConflict(
                "an object item can produce only one annotation".into(),
            ));
        }
        if changed {
            payloads.extend(reopen_annotation_payload(state, &task.task_id, now));
        }
        if complete {
            let annotation = match context.item {
                WorkflowItem::Object { .. } => next
                    .active_annotations()
                    .find(|a| {
                        a.task_id == task.task_id
                            && next.workflow_object_matches_annotation(&context.item, a)
                    })
                    .map(|a| WorkflowAnnotation {
                        annotation_id: a.annotation_id.clone(),
                        version: a.version,
                    }),
                WorkflowItem::Overview => {
                    if !next.workflow_pending_objects(&task.task_id).is_empty() {
                        return Err(StorageError::AssignmentConflict(
                            "object work must finish before Overview".into(),
                        ));
                    }
                    None
                }
            };
            if let Some(confirmed) = &annotation
                && let Some(draft) = state.workflow_object_draft(&task.task_id, &context.item)
                && next.current_annotation(&confirmed.annotation_id).is_some_and(|a| a.geometry != draft.geometry)
                && !payloads.iter().any(|p| matches!(p, EventPayload::AnnotationVersionCreated { annotation, .. } if annotation.annotation_id == confirmed.annotation_id))
            {
                return Err(StorageError::AssignmentConflict("finish or replace the saved partial object before confirming".into()));
            }
            payloads.push(workflow_payload(WorkflowEvent::ItemConfirmed {
                confirmation: WorkflowConfirmation {
                    assignment_id: assignment_id.clone(),
                    task_id: task.task_id.clone(),
                    annotation,
                    review: None,
                    reviewed_targets: vec![],
                },
            }));
            if context.item == WorkflowItem::Overview
                && (changed || state.assignment_eligible(&task.task_id))
            {
                let (status, outcome) = match task.review.workflow {
                    ReviewWorkflow::None => (
                        TaskStatus::Completed,
                        Some(TaskOutcome::AnnotationCompleted),
                    ),
                    ReviewWorkflow::Approval => (TaskStatus::Submitted, None),
                    ReviewWorkflow::LegacyIndependentAgreement => {
                        return Err(StorageError::InvalidAssignment(
                            "unsupported review workflow".into(),
                        ));
                    }
                };
                payloads.push(EventPayload::TaskStateChanged {
                    task_state: TaskState {
                        task_id: task.task_id.clone(),
                        status,
                        outcome,
                        assigned_to: None,
                        completed_by: Some(actor.user_id.clone()),
                        completed_at: Some(now),
                        updated_at: now,
                    },
                });
            }
        } else {
            renew_assignment(&mut assignment, now);
            payloads.push(EventPayload::AssignmentUpdated { assignment });
        }
        self.append_payloads_with_state_unlocked(&state.image_id, actor, payloads)
            .await
            .map(|(_, state)| state)
    }

    /// Claims one durable item. Dataset admission serializes the global review
    /// fallback check with other queue claims and submissions.
    pub async fn claim_workflow_item(
        &self,
        user: &UserId,
        selection: &WorkflowSelection,
        excluded: &[WorkflowItemRef],
    ) -> StorageResult<Option<Assignment>> {
        self.claim_workflow_item_with_prefetch(user, selection, excluded, false)
            .await
    }

    pub async fn claim_workflow_item_with_prefetch(
        &self,
        user: &UserId,
        selection: &WorkflowSelection,
        excluded: &[WorkflowItemRef],
        prefetch: bool,
    ) -> StorageResult<Option<Assignment>> {
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
        let task = metadata
            .task(&selection.task_id)
            .ok_or_else(|| StorageError::InvalidAssignment("workflow does not exist".into()))?;
        if !Self::task_supports_assignment(task, &selection.kind)?
            || self
                .task_is_overrepresented(&metadata, &task.task_id, &selection.kind)
                .await?
        {
            return Ok(None);
        }
        let exception = selection.kind == AssignmentKind::Review
            && !self
                .has_independent_workflow_review(&metadata, user)
                .await?;
        let progress = self.assignment_progress(&selection.kind).await?;
        let mut unseen = 0;
        for image in metadata.images.keys() {
            let state = self
                .workflow_polling_state(image, metadata.bounding_box_visibility)
                .await?;
            unseen += state
                .assignments
                .iter()
                .filter(|a| {
                    a.assigned_to == *user
                        && a.task_id == selection.task_id
                        && a.kind == selection.kind
                        && a.status == AssignmentStatus::Active
                        && !assignment_is_expired(a, labello_domain::now())
                        && !state.workflow_seen.contains_key(&a.assignment_id)
                        && state
                            .workflow_assignments
                            .get(&a.assignment_id)
                            .is_some_and(|c| c.item.variant() == selection.variant)
                })
                .count();
        }
        let restricted_images = if selection.variant == WorkflowVariant::Objects {
            if let Some(limit) = metadata.workflow_queue.max_pending_overviews {
                let in_flight = self
                    .workflow_images_awaiting_overview(&metadata, task, selection)
                    .await?;
                (in_flight.len() >= limit).then_some(in_flight)
            } else {
                None
            }
        } else {
            None
        };
        // Exclusion is a scheduling preference: only reuse a skipped item once
        // all alternatives in this queue have been considered.
        for allow_excluded in [false, true] {
            if allow_excluded && prefetch {
                break;
            }
            for image_id in metadata.images.keys() {
                if restricted_images
                    .as_ref()
                    .is_some_and(|images| !images.contains(image_id))
                {
                    continue;
                }
                // Use invalidated projections to skip unrelated histories. Admission
                // serializes claims; the selected image is still reloaded and validated
                // under its lock before any preparation or lease is published.
                let projected = self
                    .workflow_polling_state(image_id, metadata.bounding_box_visibility)
                    .await?;
                let mut prepared;
                let projected = if selection.kind == AssignmentKind::Annotation
                    && projected.assignment_eligible(&task.task_id)
                    && !projected.workflow_preparations.contains_key(&task.task_id)
                {
                    if labello_domain::workflow_prelabel_config(&metadata, task).is_some() {
                        continue;
                    }
                    prepared = projected.as_ref().clone();
                    prepared
                        .workflow_preparations
                        .insert(task.task_id.clone(), initial_preparation(&projected, task));
                    &prepared
                } else {
                    projected.as_ref()
                };
                let now = labello_domain::now();
                if !candidates(projected, task, selection)?
                    .iter()
                    .any(|context| {
                        excluded.contains(&WorkflowItemRef {
                            image_id: image_id.clone(),
                            item: context.item.clone(),
                        }) == allow_excluded
                            && (claimable(projected, selection, context, now)
                                || projected.assignments.iter().any(|a| {
                                    a.task_id == task.task_id
                                        && a.kind == selection.kind
                                        && a.assigned_to == *user
                                        && a.status == AssignmentStatus::Active
                                        && !assignment_is_expired(a, now)
                                        && projected
                                            .workflow_assignments
                                            .get(&a.assignment_id)
                                            .is_some_and(|held| held.item == context.item)
                                }))
                    })
                {
                    continue;
                }
                let lock = self.image_lock(image_id);
                let _image = lock.lock().await;
                let mut state = self.load_image_state(image_id).await?;
                let actor = Actor {
                    user_id: user.clone(),
                    role: role_for_kind(&selection.kind),
                };
                if selection.kind == AssignmentKind::Annotation
                    && state.assignment_eligible(&task.task_id)
                    && !state.workflow_preparations.contains_key(&task.task_id)
                {
                    // Configured model work is prepared by the managed generator.
                    if labello_domain::workflow_prelabel_config(&metadata, task).is_some() {
                        continue;
                    }
                    let preparation = initial_preparation(&state, task);
                    (_, state) = self
                        .append_payloads_with_state_unlocked(
                            image_id,
                            &actor,
                            vec![workflow_payload(WorkflowEvent::Prepared {
                                task_id: task.task_id.clone(),
                                preparation,
                            })],
                        )
                        .await?;
                }
                if selection.kind == AssignmentKind::Annotation
                    && state
                        .workflow_preparations
                        .get(&task.task_id)
                        .is_some_and(|p| {
                            p.config_digest
                                != labello_domain::workflow_prelabel_digest(&metadata, task)
                        })
                    && !state.workflow_item_seen(&task.task_id, &WorkflowItem::Overview)
                {
                    continue;
                }
                let now = labello_domain::now();
                for mut context in candidates(&state, task, selection)? {
                    if excluded.contains(&WorkflowItemRef {
                        image_id: image_id.clone(),
                        item: context.item.clone(),
                    }) != allow_excluded
                    {
                        continue;
                    }
                    if let Some(existing) = state.assignments.iter().find(|a| {
                        a.task_id == task.task_id
                            && a.kind == selection.kind
                            && a.assigned_to == *user
                            && a.status == AssignmentStatus::Active
                            && !assignment_is_expired(a, now)
                            && state
                                .workflow_assignments
                                .get(&a.assignment_id)
                                .is_some_and(|c| c.item == context.item)
                    }) {
                        return Ok(Some(existing.clone()));
                    }
                    if !claimable(&state, selection, &context, now) {
                        continue;
                    }
                    if unseen >= metadata.preload_queue_size + usize::from(!prefetch) {
                        continue;
                    }
                    if prefetch
                        && let Some(balance) = metadata.imbalance.as_ref().filter(|c| c.enforce)
                        && let Some(minimum) = super::claim::minimum_peer_count(
                            &metadata,
                            &selection.task_id,
                            &progress.counts,
                        )
                    {
                        let reserved = progress
                            .reservations
                            .iter()
                            .any(|a| a.task_id == selection.task_id && a.image_id == *image_id);
                        let projected = progress
                            .counts
                            .get(&selection.task_id)
                            .copied()
                            .unwrap_or_default()
                            .saturating_add(progress.outstanding(&selection.task_id))
                            .saturating_add(usize::from(!reserved));
                        if balance.blocks_count(projected, minimum) {
                            continue;
                        }
                    }
                    if let Some(target) = &context.review_target {
                        let independent =
                            state.workflow_independent_reviewer(&context.item, target, user);
                        if !independent && !exception {
                            continue;
                        }
                        context.review_exception = !independent;
                    }
                    let assignment = Assignment {
                        assignment_id: AssignmentId::generate(),
                        image_id: image_id.clone(),
                        task_id: task.task_id.clone(),
                        assigned_to: user.clone(),
                        kind: selection.kind.clone(),
                        status: AssignmentStatus::Active,
                        expires_at: Some(lease_expiration(now)),
                        created_at: now,
                        updated_at: now,
                    };
                    let mut payloads = vec![workflow_payload(WorkflowEvent::AssignmentOpened {
                        assignment: assignment.clone(),
                        context,
                    })];
                    if selection.kind == AssignmentKind::Annotation {
                        payloads.push(EventPayload::TaskStateChanged {
                            task_state: TaskState {
                                task_id: task.task_id.clone(),
                                status: TaskStatus::InProgress,
                                outcome: None,
                                assigned_to: None,
                                completed_by: None,
                                completed_at: None,
                                updated_at: now,
                            },
                        });
                    }
                    self.append_payloads_unlocked(image_id, &actor, payloads)
                        .await?;
                    return Ok(Some(assignment));
                }
            }
        }
        Ok(None)
    }

    pub(in crate::assignment) async fn has_independent_workflow_review(
        &self,
        metadata: &DatasetMetadata,
        user: &UserId,
    ) -> StorageResult<bool> {
        let mut eligible = std::collections::BTreeSet::new();
        let mut limits = std::collections::BTreeMap::new();
        for task in &metadata.tasks {
            if !Self::task_supports_assignment(task, &AssignmentKind::Review)?
                || self
                    .task_is_overrepresented(metadata, &task.task_id, &AssignmentKind::Review)
                    .await?
            {
                continue;
            }
            eligible.insert(task.task_id.clone());
            if let Some(limit) = metadata.workflow_queue.max_pending_overviews {
                let selection = WorkflowSelection {
                    task_id: task.task_id.clone(),
                    kind: AssignmentKind::Review,
                    variant: WorkflowVariant::Objects,
                };
                let images = self
                    .workflow_images_awaiting_overview(metadata, task, &selection)
                    .await?;
                if images.len() >= limit {
                    limits.insert(task.task_id.clone(), images);
                }
            }
        }
        for image_id in metadata.images.keys() {
            let state = self
                .workflow_polling_state(image_id, metadata.bounding_box_visibility)
                .await?;
            for task in metadata
                .tasks
                .iter()
                .filter(|t| eligible.contains(&t.task_id))
            {
                for variant in [WorkflowVariant::Objects, WorkflowVariant::Overview] {
                    if variant == WorkflowVariant::Objects
                        && limits
                            .get(&task.task_id)
                            .is_some_and(|images| !images.contains(image_id))
                    {
                        continue;
                    }
                    let selection = WorkflowSelection {
                        task_id: task.task_id.clone(),
                        kind: AssignmentKind::Review,
                        variant,
                    };
                    for context in candidates(&state, task, &selection)? {
                        if context.review_target.as_ref().is_some_and(|target| {
                            state.workflow_independent_reviewer(&context.item, target, user)
                        }) && (claimable(&state, &selection, &context, labello_domain::now())
                            || state.assignments.iter().any(|a| {
                                a.task_id == task.task_id
                                    && a.assigned_to == *user
                                    && a.kind == AssignmentKind::Review
                                    && a.status == AssignmentStatus::Active
                                    && !assignment_is_expired(a, labello_domain::now())
                                    && state
                                        .workflow_assignments
                                        .get(&a.assignment_id)
                                        .is_some_and(|c| c.item == context.item)
                            }))
                        {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok(false)
    }

    async fn workflow_images_awaiting_overview(
        &self,
        metadata: &DatasetMetadata,
        task: &TaskDefinition,
        selection: &WorkflowSelection,
    ) -> StorageResult<std::collections::BTreeSet<ImageId>> {
        let mut images = std::collections::BTreeSet::new();
        for image in metadata.images.keys() {
            let state = self
                .workflow_polling_state(image, metadata.bounding_box_visibility)
                .await?;
            let eligible = if selection.kind == AssignmentKind::Annotation {
                state.assignment_eligible(&task.task_id)
            } else {
                state
                    .task_states
                    .get(&task.task_id)
                    .is_some_and(|s| s.status == TaskStatus::Submitted)
            };
            if !eligible {
                continue;
            }
            let has_objects = if selection.kind == AssignmentKind::Annotation {
                state
                    .workflow_preparations
                    .get(&task.task_id)
                    .is_some_and(|p| !p.objects.is_empty())
            } else {
                !state.review_object_targets(task)?.is_empty()
            };
            let allocated = state.assignments.iter().any(|assignment| {
                assignment.task_id == task.task_id
                    && assignment.kind == selection.kind
                    && state
                        .workflow_assignments
                        .get(&assignment.assignment_id)
                        .is_some_and(|c| c.item.variant() == WorkflowVariant::Objects)
                    && (state.workflow_seen.contains_key(&assignment.assignment_id)
                        || (assignment.status == AssignmentStatus::Active
                            && !assignment_is_expired(assignment, labello_domain::now())))
            });
            if has_objects && allocated {
                images.insert(image.clone());
            }
        }
        Ok(images)
    }

    /// Called when an item is displayed, never for claim/prefetch alone.
    pub async fn display_workflow_item(
        &self,
        user: &UserId,
        context: AssignmentContext<'_>,
    ) -> StorageResult<ImageState> {
        self.refresh_workflow_item(user, context, true).await
    }

    pub(in crate::assignment) async fn refresh_workflow_item(
        &self,
        user: &UserId,
        context: AssignmentContext<'_>,
        mark_seen: bool,
    ) -> StorageResult<ImageState> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let _admission = self.assignment_claim_lock.lock().await;
        let metadata = self.load_dataset().await?;
        let role = role_for_kind(&context.kind);
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            role.clone(),
        )?;
        ensure_assignment_target_exists(&metadata, context.image_id, context.task_id)?;
        let task = metadata.task(context.task_id).expect("validated task");
        let independent_available = context.kind == AssignmentKind::Review
            && self
                .has_independent_workflow_review(&metadata, user)
                .await?;
        let lock = self.image_lock(context.image_id);
        let _image = lock.lock().await;
        let state = self.load_image_state(context.image_id).await?;
        let mut renewed = exact_active_assignment(
            &state.assignments,
            context.assignment_id,
            context.image_id,
            context.task_id,
            user,
            &context.kind,
            labello_domain::now(),
        )?
        .clone();
        let captured = state
            .workflow_assignments
            .get(context.assignment_id)
            .ok_or_else(|| StorageError::AssignmentConflict("missing item context".into()))?;
        validate_definition(captured, task)?;
        if !Self::task_supports_assignment(task, &context.kind)? {
            return Err(StorageError::AssignmentConflict(
                "workflow is no longer enabled".into(),
            ));
        }
        if context.kind == AssignmentKind::Annotation {
            if !state.assignment_eligible(context.task_id)
                && captured.source_assignment_id.is_none()
            {
                return Err(StorageError::AssignmentConflict(
                    "annotation is no longer queued".into(),
                ));
            }
            let preparation = state
                .workflow_preparations
                .get(context.task_id)
                .ok_or_else(|| {
                    StorageError::AssignmentConflict("workflow is not prepared".into())
                })?;
            let valid = match &captured.item {
                WorkflowItem::Object { object } => preparation.objects.contains(object),
                WorkflowItem::Overview => {
                    preparation.status == labello_domain::WorkflowPreparationStatus::Ready
                        && state.workflow_pending_objects(context.task_id).is_empty()
                }
            };
            if !valid
                || (!state.workflow_seen.contains_key(context.assignment_id)
                    && preparation.config_digest
                        != labello_domain::workflow_prelabel_digest(&metadata, task)
                    && !state.workflow_item_seen(context.task_id, &WorkflowItem::Overview))
            {
                return Err(StorageError::AssignmentConflict(
                    "workflow preparation changed before display".into(),
                ));
            }
        } else {
            if !state.task_states.get(context.task_id).is_some_and(|s| {
                s.status == TaskStatus::Submitted
                    || (captured.source_assignment_id.is_some()
                        && s.status == TaskStatus::Completed)
            }) || captured.review_target.as_ref().is_none_or(|target| {
                !state
                    .review_targets(task)
                    .is_ok_and(|targets| targets.contains(target))
            }) {
                return Err(StorageError::AssignmentConflict(
                    "review item changed before display".into(),
                ));
            }
            if captured.item == WorkflowItem::Overview {
                let mut definition = task.clone();
                definition.prelabel_config_ids.clear();
                if !state.workflow_pending_reviews(task)?.is_empty()
                    || captured.overview_fingerprint.as_ref()
                        != Some(&state.review_target_fingerprint(&definition))
                {
                    return Err(StorageError::AssignmentConflict(
                        "Overview targets changed before display".into(),
                    ));
                }
            }
        }
        if captured.review_target.is_some()
            && !state.workflow_assignment_independent_reviewer(captured, user)
            && (!captured.review_exception || independent_available)
        {
            return Err(StorageError::AssignmentConflict(
                "independent review work is available".into(),
            ));
        }
        let selection = WorkflowSelection {
            task_id: context.task_id.clone(),
            kind: context.kind.clone(),
            variant: captured.item.variant(),
        };
        let mut payloads = Vec::new();
        if mark_seen && !state.workflow_seen.contains_key(context.assignment_id) {
            payloads.push(workflow_payload(WorkflowEvent::ItemSeen {
                task_id: context.task_id.clone(),
                assignment_id: context.assignment_id.clone(),
            }));
        }
        renew_assignment(&mut renewed, labello_domain::now());
        payloads.push(EventPayload::AssignmentUpdated {
            assignment: renewed,
        });
        let state = self
            .append_payloads_with_state_unlocked(
                context.image_id,
                &Actor {
                    user_id: user.clone(),
                    role,
                },
                payloads,
            )
            .await?
            .1;
        drop(_image);
        if !mark_seen {
            return Ok(state);
        }
        self.prepare_review_history().await?;
        let keep = self
            .review_history_cache
            .workflow_history(user, &selection, metadata.workflow_queue.history_depth)?
            .into_iter()
            .map(|entry| entry.assignment_id)
            .collect::<Vec<_>>();
        self.release_workflow_reservations(&metadata, user, &selection, &keep, false)
            .await?;
        Ok(state)
    }
}
