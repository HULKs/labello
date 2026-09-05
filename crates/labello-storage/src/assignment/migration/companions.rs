use super::*;
use labello_domain::{MigrationCompanion, migration_skeleton_bounds};

impl DatasetRepository {
    /// Each object is one durable reconciliation unit. Completed links survive
    /// restart and are the progress record for a later pass over missing links.
    #[allow(
        clippy::too_many_arguments,
        reason = "reconciliation checks both persisted object versions and exact assignment ownership"
    )]
    pub async fn reconcile_migration_companion(
        &self,
        user_id: &UserId,
        context: AssignmentContext<'_>,
        pass_id: Option<&MigrationPassId>,
        annotation_id: &AnnotationId,
        expected_skeleton_version: u32,
        expected_box_version: Option<u32>,
        idempotency_key: &str,
    ) -> StorageResult<ManualMigrationCommandResult> {
        validate_idempotency_key(idempotency_key)?;
        let (metadata, image) = self.load_migration_inputs(context.image_id).await?;
        let (task, guide, dimensions) = migration_metadata(&metadata, &image, context.task_id)?;
        require_annotation_context(&metadata, user_id, &context)?;
        let lock = self.image_lock(context.image_id);
        let _guard = lock.lock().await;
        let mut state = self.load_image_state(context.image_id).await?;
        let primary_id = command_event_id(user_id, context.assignment_id, idempotency_key, None);
        if let Some(command) = find_command(self, context.image_id, &primary_id).await? {
            let matches = matches!(&command.primary.payload, EventPayload::MigrationCompanionLinked { companion }
                if companion.skeleton_annotation_id == *annotation_id
                    && companion.skeleton_version == expected_skeleton_version
                    && companion.box_version == expected_box_version.map_or(1, |version| version.saturating_add(1))
                    && companion.migration_task_id == *context.task_id)
                && assignment_pass(&command.before, context.assignment_id, context.task_id)
                    == pass_id;
            return replay_retry(
                matches,
                state,
                context.task_id,
                pass_id,
                Some(context.assignment_id),
                Some(annotation_id.clone()),
            );
        }
        let now = labello_domain::now();
        let mut assignment = exact_active_assignment(
            &state.assignments,
            context.assignment_id,
            context.image_id,
            context.task_id,
            user_id,
            &AssignmentKind::Annotation,
            now,
        )?
        .clone();
        ensure_annotation_status(&state, context.task_id)?;
        if assignment_pass(&state, context.assignment_id, context.task_id) != pass_id
            || state.migration_cursor(context.task_id, pass_id)? != MigrationCursor::FullImage
        {
            return Err(conflict(
                "companion reconciliation requires this assignment's full-image migration step",
            ));
        }
        let skeleton = current_discovered_skeleton(
            &state,
            task,
            context.task_id,
            annotation_id,
            expected_skeleton_version,
        )?;
        let existing = state.migration_companions.get(annotation_id);
        if existing.is_some() != expected_box_version.is_some() {
            return Err(conflict(
                "companion pairing changed; reload the object before explicit reconciliation",
            ));
        }
        if existing.is_none() {
            let events = self.load_events(context.image_id).await?;
            let proven_discovery = events.iter().any(|event| matches!(&event.payload,
                EventPayload::AnnotationVersionCreated { annotation, previous_version: None, reason }
                    if annotation.annotation_id == *annotation_id && annotation.task_id == *context.task_id
                        && reason.as_deref() == Some("object discovered during full-image migration review")));
            if !proven_discovery {
                return Err(conflict(
                    "this group-less skeleton has no unambiguous migration discovery history; reconciliation requires a maintainer to resolve its provenance",
                ));
            }
        }
        let mut payloads = Vec::new();
        update_discovered_companion(
            &mut state,
            task,
            guide,
            dimensions,
            &skeleton,
            user_id,
            now,
            &mut payloads,
            expected_box_version,
        )?;
        renew(&mut assignment, now);
        payloads.push(EventPayload::AssignmentUpdated {
            assignment: assignment.clone(),
        });
        let primary_index = payloads
            .iter()
            .position(|payload| matches!(payload, EventPayload::MigrationCompanionLinked { .. }))
            .expect("reconciliation records its exact companion derivation");
        let state = self
            .append_migration_command_unlocked(
                context.image_id,
                user_id,
                DatasetRole::Annotator,
                idempotency_key,
                context.assignment_id,
                payloads,
                primary_index,
                now,
            )
            .await?;
        command_result(
            state,
            context.task_id,
            pass_id,
            Some(assignment),
            Some(annotation_id.clone()),
        )
    }
}

pub(super) fn validate_companion_task(
    state: &ImageState,
    task: &TaskDefinition,
    guide: &TaskDefinition,
    now: Timestamp,
) -> StorageResult<()> {
    let set = state
        .migration_target_sets
        .get(&task.task_id)
        .ok_or_else(|| conflict("migration target set is missing"))?;
    if !task.enabled
        || !guide.enabled
        || set.guide_task_id != guide.task_id
        || set.target_task_id != task.task_id
        || task.class_ids != guide.class_ids
    {
        return Err(conflict(
            "migration guide configuration changed; reload the workflow before saving the retained draft",
        ));
    }
    if state.assignments.iter().any(|assignment| {
        assignment.task_id == guide.task_id
            && assignment.status == AssignmentStatus::Active
            && !super::super::assignment_is_expired(assignment, now)
    }) {
        return Err(conflict(
            "the bounding-box workflow has an active assignment; finish or release it before retrying the retained skeleton draft",
        ));
    }
    if state
        .task_states
        .get(&guide.task_id)
        .is_some_and(|task_state| task_state.status == TaskStatus::AdjudicationRequired)
    {
        return Err(conflict(
            "the bounding-box workflow requires adjudication before companion reconciliation",
        ));
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the compound transaction keeps both task policies and exact source version explicit"
)]
pub(super) fn update_discovered_companion(
    state: &mut ImageState,
    task: &TaskDefinition,
    guide: &TaskDefinition,
    dimensions: labello_domain::ImageDimensions,
    skeleton: &AnnotationVersion,
    user_id: &UserId,
    now: Timestamp,
    payloads: &mut Vec<EventPayload>,
    explicitly_reconciled_box_version: Option<u32>,
) -> StorageResult<()> {
    validate_companion_task(state, task, guide, now)?;
    let AnnotationGeometry::Skeleton(geometry) = &skeleton.geometry else {
        return Err(conflict("companion source must be a skeleton"));
    };
    let bounds = migration_skeleton_bounds(geometry, dimensions)?
        .ok_or_else(|| conflict("this historical skeleton has no positioned keypoints; add positions before creating a companion box"))?;
    let existing = state
        .migration_companions
        .get(&skeleton.annotation_id)
        .cloned();
    let current_box = if let Some(link) = &existing {
        if link.migration_task_id != task.task_id
            || link.guide_task_id != guide.task_id
            || link.class_id != skeleton.class_id
        {
            return Err(conflict(
                "the existing companion relationship conflicts with the configured guide task",
            ));
        }
        let current = state
            .current_annotation(&link.box_annotation_id)
            .ok_or_else(|| {
                conflict("the linked box is missing; explicit reconciliation is required")
            })?;
        if let Some(expected) = explicitly_reconciled_box_version {
            if current.version != expected {
                return Err(conflict(
                    "the companion changed before explicit reconciliation; reload and retry",
                ));
            }
        } else if !state.migration_companion_is_derived(&skeleton.annotation_id)
            || link.skeleton_version.checked_add(1) != Some(skeleton.version)
        {
            return Err(conflict(
                "the companion box was independently edited or reviewed; explicitly reconcile its current version before retrying the retained skeleton draft",
            ));
        }
        Some(current.clone())
    } else {
        None
    };
    let box_id = current_box
        .as_ref()
        .map(|annotation| annotation.annotation_id.clone())
        .unwrap_or_else(|| {
            AnnotationId::from(format!(
                "ann_migration_companion_{}",
                blake3::hash(skeleton.annotation_id.as_str().as_bytes()).to_hex()
            ))
        });
    if existing.is_none() && state.current_annotation(&box_id).is_some() {
        return Err(conflict(
            "a candidate companion identity already exists without an unambiguous link; explicit reconciliation is required",
        ));
    }
    let bounding_box = AnnotationVersion {
        annotation_id: box_id.clone(),
        version: current_box.as_ref().map_or(Ok(1), |annotation| {
            annotation
                .version
                .checked_add(1)
                .ok_or_else(|| conflict("companion version overflow"))
        })?,
        object_group_id: None,
        origin: AnnotationOrigin::native(),
        task_id: guide.task_id.clone(),
        class_id: skeleton.class_id.clone(),
        annotation_type: AnnotationType::BoundingBox,
        revision_source: RevisionSource::MigrationSkeleton {
            annotation_id: skeleton.annotation_id.clone(),
            version: skeleton.version,
        },
        geometry: AnnotationGeometry::BoundingBox(bounds),
        author_user_id: user_id.clone(),
        created_at: current_box
            .as_ref()
            .map_or(now, |annotation| annotation.created_at),
        updated_at: now,
        deleted: false,
    };
    bounding_box.validate_for_task(guide, dimensions)?;
    push_simulated(
        state,
        payloads,
        user_id,
        DatasetRole::Annotator,
        now,
        EventPayload::AnnotationVersionCreated {
            annotation: bounding_box.clone(),
            previous_version: current_box.as_ref().map(|annotation| annotation.version),
            reason: Some(
                "companion box derived from the exact manually authored migration skeleton version"
                    .into(),
            ),
        },
    )?;
    push_simulated(
        state,
        payloads,
        user_id,
        DatasetRole::Annotator,
        now,
        EventPayload::MigrationCompanionLinked {
            companion: MigrationCompanion {
                migration_task_id: task.task_id.clone(),
                guide_task_id: guide.task_id.clone(),
                class_id: skeleton.class_id.clone(),
                skeleton_annotation_id: skeleton.annotation_id.clone(),
                skeleton_version: skeleton.version,
                box_annotation_id: box_id,
                box_version: bounding_box.version,
            },
        },
    )?;
    reopen_companion_task(state, guide, user_id, now, payloads)
}

pub(super) fn delete_discovered_companion(
    state: &mut ImageState,
    task: &TaskDefinition,
    guide: &TaskDefinition,
    skeleton_id: &AnnotationId,
    user_id: &UserId,
    now: Timestamp,
    payloads: &mut Vec<EventPayload>,
) -> StorageResult<()> {
    validate_companion_task(state, task, guide, now)?;
    let Some(link) = state.migration_companions.get(skeleton_id).cloned() else {
        return Ok(());
    };
    if !state.migration_companion_is_derived(skeleton_id) {
        return Err(conflict(
            "the companion box was independently edited or reviewed; explicitly reconcile it before removing the skeleton",
        ));
    }
    push_simulated(
        state,
        payloads,
        user_id,
        DatasetRole::Annotator,
        now,
        EventPayload::AnnotationDeleted {
            annotation_id: link.box_annotation_id,
            version: link.box_version,
            reason: Some(
                "withdrawn still-derived companion of a removed migration skeleton".into(),
            ),
        },
    )?;
    reopen_companion_task(state, guide, user_id, now, payloads)
}

fn reopen_companion_task(
    state: &mut ImageState,
    guide: &TaskDefinition,
    user_id: &UserId,
    now: Timestamp,
    payloads: &mut Vec<EventPayload>,
) -> StorageResult<()> {
    push_simulated(
        state,
        payloads,
        user_id,
        DatasetRole::Annotator,
        now,
        task_state_payload(&guide.task_id, TaskStatus::NeedsCorrection, None, now),
    )?;
    cancel_competing_reviews(
        state,
        &guide.task_id,
        &AssignmentId::from(""),
        now,
        payloads,
    );
    Ok(())
}
