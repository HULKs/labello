use std::collections::{BTreeMap, BTreeSet};

use labello_domain::{
    Actor, AnnotationId, BoundingBox, DatasetId, DatasetRoleAssignment, HumanRevisionKind,
    ImageRecord, ImagesIndex, ImportCoverage, ImportGeometryProvenance, ImportId,
    ImportTaskInitialization, ImportedOrigin, KeypointAnnotation, KeypointSpec, KeypointState,
    LabelClass, ManualBoxGuideMigration, MigrationCardinality, MigrationHashContext,
    MigrationSequence, MigrationTarget, MigrationTargetSetInitialization, NormalizedPoint,
    ReviewConfig, SCHEMA_VERSION, SkeletonSpec, SourceProfile, TutorialContent,
    migration_target_set_hash, rebuild_state,
};

use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    repository: DatasetRepository,
    image_id: ImageId,
    guide_task_id: TaskId,
    task_id: TaskId,
    annotator: UserId,
    reviewers: [UserId; 2],
    targets: Vec<MigrationTarget>,
}

struct MigrationPair {
    guide_task_id: TaskId,
    task_id: TaskId,
    targets: Vec<MigrationTarget>,
}

fn optional_skeleton_spec() -> SkeletonSpec {
    SkeletonSpec {
        keypoints: vec![
            KeypointSpec {
                name: "center".to_string(),
                required: false,
            },
            KeypointSpec {
                name: "tail".to_string(),
                required: false,
            },
        ],
        edges: Vec::new(),
        allow_hidden: true,
        allow_absent: true,
    }
}

fn all_not_present_skeleton() -> SkeletonGeometry {
    SkeletonGeometry {
        keypoints: ["center", "tail"]
            .into_iter()
            .map(|name| KeypointAnnotation {
                name: name.to_string(),
                state: KeypointState::Absent,
                point: None,
            })
            .collect(),
    }
}

fn mixed_optional_skeleton() -> SkeletonGeometry {
    SkeletonGeometry {
        keypoints: vec![
            KeypointAnnotation {
                name: "center".to_string(),
                state: KeypointState::Hidden,
                point: Some(NormalizedPoint { x: 0.4, y: 0.5 }),
            },
            KeypointAnnotation {
                name: "tail".to_string(),
                state: KeypointState::Absent,
                point: None,
            },
        ],
    }
}

#[tokio::test]
async fn migration_save_rejects_zero_positions_without_mutating_state() {
    let fixture =
        fixture_with_skeleton_spec(ReviewWorkflow::None, 1, optional_skeleton_spec()).await;
    let assignment = claim_annotator(&fixture).await;
    let before = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    let events_before = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    let expected = expectation(&before.image_state, &fixture.task_id, &fixture.targets[0]);

    let error = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            all_not_present_skeleton(),
            "reject-zero-position-save",
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::InvalidAssignment(message)
            if message
                == "invalid geometry: manual migration skeleton requires at least one positioned keypoint"
    ));

    let after = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    assert_eq!(after.image_state, before.image_state);
    assert_eq!(after.cursor, before.cursor);
    assert_eq!(
        fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap(),
        events_before
    );

    fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            mixed_optional_skeleton(),
            "save-mixed-optional",
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn migration_add_and_edit_reject_zero_positions_atomically() {
    let fixture =
        fixture_with_skeleton_spec(ReviewWorkflow::None, 0, optional_skeleton_spec()).await;
    let assignment = claim_annotator(&fixture).await;
    let before = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    assert_eq!(before.cursor, MigrationCursor::FullImage);
    let events_before = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();

    let add_error = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            all_not_present_skeleton(),
            "reject-zero-position-add",
        )
        .await
        .unwrap_err();
    assert!(
        matches!(add_error, StorageError::InvalidAssignment(message) if message.contains(
            "manual migration skeleton requires at least one positioned keypoint"
        ))
    );
    assert_eq!(
        fixture
            .repository
            .load_image_state(&fixture.image_id)
            .await
            .unwrap(),
        before.image_state
    );
    assert_eq!(
        fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap(),
        events_before
    );

    let added = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            mixed_optional_skeleton(),
            "add-mixed-optional",
        )
        .await
        .unwrap();
    let annotation_id = added.annotation_id.unwrap();
    let events_after_add = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();

    let edit_error = fixture
        .repository
        .edit_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &annotation_id,
            1,
            all_not_present_skeleton(),
            "reject-zero-position-edit",
        )
        .await
        .unwrap_err();
    assert!(
        matches!(edit_error, StorageError::InvalidAssignment(message) if message.contains(
            "manual migration skeleton requires at least one positioned keypoint"
        ))
    );
    assert_eq!(
        fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap(),
        events_after_add
    );
    assert_eq!(
        fixture
            .repository
            .load_image_state(&fixture.image_id)
            .await
            .unwrap()
            .current_annotation(&annotation_id)
            .unwrap()
            .version,
        1
    );
}

#[tokio::test]
async fn historical_all_not_present_save_replays_before_new_validation() {
    let fixture =
        fixture_with_skeleton_spec(ReviewWorkflow::None, 1, optional_skeleton_spec()).await;
    let assignment = claim_annotator(&fixture).await;
    let before = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    let target = &fixture.targets[0];
    let expected = expectation(&before.image_state, &fixture.task_id, target);
    let timestamp = labello_domain::now();
    let annotation = AnnotationVersion {
        annotation_id: target.reserved_skeleton_annotation_id.clone(),
        version: 1,
        object_group_id: Some(target.object_group_id.clone()),
        origin: AnnotationOrigin::native(),
        task_id: fixture.task_id.clone(),
        class_id: ClassId::from("person"),
        annotation_type: AnnotationType::Skeleton,
        revision_source: RevisionSource::Human {
            action: HumanRevisionKind::Authored,
        },
        geometry: AnnotationGeometry::Skeleton(all_not_present_skeleton()),
        author_user_id: fixture.annotator.clone(),
        created_at: timestamp,
        updated_at: timestamp,
        deleted: false,
    };
    fixture
        .repository
        .append_migration_command_unlocked(
            &fixture.image_id,
            &fixture.annotator,
            DatasetRole::Annotator,
            "historical-zero-position",
            &assignment.assignment_id,
            vec![EventPayload::AnnotationVersionCreated {
                annotation,
                previous_version: None,
                reason: None,
            }],
            0,
            timestamp,
        )
        .await
        .unwrap();
    let sequence = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap()
        .current_sequence;

    let replayed = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            all_not_present_skeleton(),
            "historical-zero-position",
        )
        .await
        .unwrap();
    assert_eq!(replayed.image_state.current_sequence, sequence);
}

#[tokio::test]
async fn full_image_migration_accepts_edits_and_binds_a_discovered_skeleton() {
    let fixture = fixture(ReviewWorkflow::None, 0).await;
    let assignment = fixture
        .repository
        .assign_next_image(
            &fixture.annotator,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap();
    let initial = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    assert_eq!(initial.cursor, MigrationCursor::FullImage);
    let initial_hash = initial
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();

    let added = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "add-discovered",
        )
        .await
        .unwrap();
    assert_eq!(added.cursor, MigrationCursor::FullImage);
    let annotation_id = added.annotation_id.clone().unwrap();
    let annotation = added
        .image_state
        .current_annotation(&annotation_id)
        .unwrap();
    assert!(annotation.object_group_id.is_none());
    assert!(matches!(
        annotation.origin,
        AnnotationOrigin::Native { legacy_v2: false }
    ));
    let state_hash = added
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    assert_ne!(state_hash, initial_hash);

    let retried = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "add-discovered",
        )
        .await
        .unwrap();
    assert_eq!(
        retried.image_state.current_sequence,
        added.image_state.current_sequence
    );

    let edited = fixture
        .repository
        .edit_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &annotation_id,
            1,
            skeleton(0.8),
            "edit-discovered",
        )
        .await
        .unwrap();
    let edited_annotation = edited
        .image_state
        .current_annotation(&annotation_id)
        .unwrap();
    assert_eq!(edited_annotation.version, 2);
    assert_eq!(
        edited_annotation.geometry,
        AnnotationGeometry::Skeleton(skeleton(0.8))
    );
    assert!(matches!(
        edited_annotation.revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::Edited
        }
    ));
    let edit_retry = fixture
        .repository
        .edit_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &annotation_id,
            1,
            skeleton(0.8),
            "edit-discovered",
        )
        .await
        .unwrap();
    assert_eq!(
        edit_retry.image_state.current_sequence,
        edited.image_state.current_sequence
    );
    assert!(
        fixture
            .repository
            .edit_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &annotation_id,
                1,
                skeleton(0.9),
                "stale-edit-discovered",
            )
            .await
            .is_err()
    );

    let edited_state_hash = edited
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    assert_ne!(
        edited_state_hash,
        added
            .image_state
            .current_migration_state_hash(&fixture.task_id)
            .unwrap()
    );

    let deleted = fixture
        .repository
        .delete_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &annotation_id,
            2,
            "delete-discovered",
        )
        .await
        .unwrap();
    assert!(
        deleted
            .image_state
            .current_annotation(&annotation_id)
            .unwrap()
            .deleted
    );
    let delete_retry = fixture
        .repository
        .delete_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &annotation_id,
            2,
            "delete-discovered",
        )
        .await
        .unwrap();
    assert_eq!(
        delete_retry.image_state.current_sequence,
        deleted.image_state.current_sequence
    );
    assert!(
        fixture
            .repository
            .delete_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &annotation_id,
                1,
                "stale-delete-discovered",
            )
            .await
            .is_err()
    );
    let state_hash = deleted
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    assert_ne!(state_hash, edited_state_hash);
    let target_hash = deleted.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    let completed = fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "confirm-discovered",
        )
        .await
        .unwrap();
    assert_eq!(
        completed.image_state.task_states[&fixture.task_id].status,
        TaskStatus::Completed
    );
    assert_eq!(
        completed.image_state.current_annotation(&annotation_id),
        deleted.image_state.current_annotation(&annotation_id)
    );
}

#[tokio::test]
async fn unchanged_discovered_skeleton_edit_remains_confirmable() {
    let fixture = fixture(ReviewWorkflow::None, 0).await;
    let assignment = fixture
        .repository
        .assign_next_image(
            &fixture.annotator,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap();
    let added = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "add-unchanged",
        )
        .await
        .unwrap();
    let annotation_id = added.annotation_id.unwrap();

    let unchanged = fixture
        .repository
        .edit_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &annotation_id,
            1,
            skeleton(0.7),
            "edit-unchanged",
        )
        .await
        .unwrap();
    let annotation = unchanged
        .image_state
        .current_annotation(&annotation_id)
        .unwrap();
    assert_eq!(annotation.version, 2);
    assert!(matches!(
        annotation.revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::AcceptedUnchanged
        }
    ));

    let target_hash = unchanged.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = unchanged
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    let completed = fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "confirm-unchanged",
        )
        .await
        .unwrap();

    assert_eq!(
        completed.image_state.task_states[&fixture.task_id].status,
        TaskStatus::Completed
    );
    assert!(matches!(
        completed
            .image_state
            .current_annotation(&annotation_id)
            .unwrap()
            .revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::AcceptedUnchanged
        }
    ));
}

#[tokio::test]
async fn migration_commands_are_canonical_idempotent_atomic_and_replayable() {
    let fixture = fixture(ReviewWorkflow::None, 2).await;
    let assignment = fixture
        .repository
        .assign_next_image(
            &fixture.annotator,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap();
    let initial = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    assert_eq!(
        initial.cursor,
        MigrationCursor::Object {
            object_group_id: fixture.targets[0].object_group_id.clone(),
            sequence_index: 0,
        }
    );

    let later = expectation(&initial.image_state, &fixture.task_id, &fixture.targets[1]);
    assert!(
        fixture
            .repository
            .exclude_migration_target(
                &fixture.annotator,
                context(&assignment),
                None,
                &later,
                MigrationExclusionReason::ObjectNotPresent,
                None,
                "jump",
            )
            .await
            .is_err(),
        "naming a later object must not move the server cursor"
    );

    let first = expectation(&initial.image_state, &fixture.task_id, &fixture.targets[0]);
    let geometry = skeleton(0.2);
    let saved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &first,
            geometry.clone(),
            "save-first",
        )
        .await
        .unwrap();
    let sequence_after_save = saved.image_state.current_sequence;
    let retried = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &first,
            geometry,
            "save-first",
        )
        .await
        .unwrap();
    assert_eq!(retried.image_state.current_sequence, sequence_after_save);
    assert!(
        fixture
            .repository
            .save_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &first,
                skeleton(0.9),
                "save-first",
            )
            .await
            .is_err(),
        "reusing a key for different content must conflict"
    );

    let second = expectation(&saved.image_state, &fixture.task_id, &fixture.targets[1]);
    let excluded = fixture
        .repository
        .exclude_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &second,
            MigrationExclusionReason::Other,
            Some("occluded by equipment".to_string()),
            "exclude-second",
        )
        .await
        .unwrap();
    assert_eq!(excluded.cursor, MigrationCursor::FullImage);
    assert_eq!(excluded.progress.annotated, 1);
    assert_eq!(excluded.progress.excluded, 1);

    let target_hash = excluded.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = excluded
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let pass = fixture
        .repository
        .start_migration_pass(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            "start-pass",
        )
        .await
        .unwrap();
    let pass_id = pass.active_pass.as_ref().unwrap().pass_id.clone();
    let pass_sequence = pass.image_state.current_sequence;
    let retried_pass = fixture
        .repository
        .start_migration_pass(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            "start-pass",
        )
        .await
        .unwrap();
    assert_eq!(retried_pass.active_pass.unwrap().pass_id, pass_id);
    assert_eq!(retried_pass.image_state.current_sequence, pass_sequence);

    let keep = expectation(&pass.image_state, &fixture.task_id, &fixture.targets[0]);
    let kept = fixture
        .repository
        .keep_migration_target(
            &fixture.annotator,
            context(&assignment),
            &pass_id,
            &keep,
            "keep-first",
        )
        .await
        .unwrap();
    let reopen = expectation(&kept.image_state, &fixture.task_id, &fixture.targets[1]);
    let reopened = fixture
        .repository
        .reopen_migration_target(
            &fixture.annotator,
            context(&assignment),
            Some(&pass_id),
            &reopen,
            "reopen-second",
        )
        .await
        .unwrap();
    assert!(matches!(
        reopened.cursor,
        MigrationCursor::Object { ref object_group_id, .. }
            if object_group_id == &fixture.targets[1].object_group_id
    ));
    let reexclude = expectation(&reopened.image_state, &fixture.task_id, &fixture.targets[1]);
    let completed_pass = fixture
        .repository
        .exclude_migration_target(
            &fixture.annotator,
            context(&assignment),
            Some(&pass_id),
            &reexclude,
            MigrationExclusionReason::NoValidSkeleton,
            None,
            "reexclude-second",
        )
        .await
        .unwrap();
    assert_eq!(completed_pass.cursor, MigrationCursor::FullImage);

    let target_hash = completed_pass.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = completed_pass
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    let submitted = fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "confirm",
        )
        .await
        .unwrap();
    let mut final_sequence = submitted.image_state.current_sequence;
    let retry = fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "confirm",
        )
        .await
        .unwrap();
    assert_eq!(retry.image_state.current_sequence, final_sequence);
    assert_eq!(
        retry.image_state.task_states[&fixture.task_id].status,
        TaskStatus::Completed
    );
    let reopened_assignment = fixture
        .repository
        .reopen_annotation_assignment(
            &fixture.annotator,
            &assignment.assignment_id,
            &fixture.image_id,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();
    let reopened_state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(reopened_assignment.status, AssignmentStatus::Active);
    assert_eq!(
        reopened_state.task_states[&fixture.task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert!(
        !reopened_state
            .migration_confirmations
            .contains_key(&fixture.task_id)
    );
    final_sequence = reopened_state.current_sequence;

    tokio::fs::remove_file(fixture.repository.state_path(&fixture.image_id))
        .await
        .unwrap();
    let reloaded = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(
        reloaded,
        rebuild_state(fixture.image_id.clone(), &events).unwrap()
    );
    assert_eq!(reloaded.current_sequence, final_sequence);
    assert!(
        events
            .iter()
            .all(|event| event.actor_user_id != UserId::from(""))
    );
}

#[tokio::test]
async fn selecting_a_pending_target_moves_the_cursor_and_returns_after_resolution() {
    let fixture = fixture(ReviewWorkflow::None, 2).await;
    let assignment = fixture
        .repository
        .assign_next_image(
            &fixture.annotator,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap();
    let initial = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    let selected_target = &fixture.targets[1];
    let selected = fixture
        .repository
        .revisit_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&initial.image_state, &fixture.task_id, selected_target),
            "select-pending-second",
        )
        .await
        .unwrap();
    assert!(matches!(
        selected.cursor,
        MigrationCursor::Object { ref object_group_id, .. }
            if object_group_id == &selected_target.object_group_id
    ));

    let saved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&selected.image_state, &fixture.task_id, selected_target),
            skeleton(0.6),
            "save-selected-second",
        )
        .await
        .unwrap();
    assert!(matches!(
        saved.cursor,
        MigrationCursor::Object { ref object_group_id, .. }
            if object_group_id == &fixture.targets[0].object_group_id
    ));
    assert!(
        !saved.image_state.migration_dependencies[&fixture.task_id]
            .contains_key(&selected_target.object_group_id)
    );
}

#[tokio::test]
async fn revisiting_a_resolved_target_is_audited_idempotent_and_returns_to_the_cursor() {
    let fixture = fixture(ReviewWorkflow::None, 2).await;
    let assignment = fixture
        .repository
        .assign_next_image(
            &fixture.annotator,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap();
    let initial = fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    let first = expectation(&initial.image_state, &fixture.task_id, &fixture.targets[0]);
    let saved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &first,
            skeleton(0.2),
            "revisit-save-first",
        )
        .await
        .unwrap();
    assert!(matches!(
        saved.cursor,
        MigrationCursor::Object { ref object_group_id, .. }
            if object_group_id == &fixture.targets[1].object_group_id
    ));

    let revisit = expectation(&saved.image_state, &fixture.task_id, &fixture.targets[0]);
    let revisited = fixture
        .repository
        .revisit_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &revisit,
            "revisit-first",
        )
        .await
        .unwrap();
    assert!(matches!(
        revisited.cursor,
        MigrationCursor::Object { ref object_group_id, .. }
            if object_group_id == &fixture.targets[0].object_group_id
    ));
    assert!(matches!(
        revisited.image_state.migration_dispositions[&fixture.task_id]
            [&fixture.targets[0].object_group_id]
            .status,
        MigrationDispositionStatus::Annotated { .. }
    ));
    assert_eq!(
        revisited.image_state.migration_dependencies[&fixture.task_id]
            [&fixture.targets[0].object_group_id]
            .kind,
        MigrationDependencyKind::ManualSelection
    );

    let sequence = revisited.image_state.current_sequence;
    let retry = fixture
        .repository
        .revisit_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &revisit,
            "revisit-first",
        )
        .await
        .unwrap();
    assert_eq!(retry.image_state.current_sequence, sequence);

    let corrected = expectation(
        &revisited.image_state,
        &fixture.task_id,
        &fixture.targets[0],
    );
    let resumed = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &corrected,
            skeleton(0.3),
            "revisit-correct-first",
        )
        .await
        .unwrap();
    assert!(matches!(
        resumed.cursor,
        MigrationCursor::Object { ref object_group_id, .. }
            if object_group_id == &fixture.targets[1].object_group_id
    ));
    assert!(
        !resumed.image_state.migration_dependencies[&fixture.task_id]
            .contains_key(&fixture.targets[0].object_group_id)
    );
}

#[tokio::test]
async fn concurrent_exact_version_writes_have_one_winner() {
    let fixture = fixture(ReviewWorkflow::None, 1).await;
    let assignment = fixture
        .repository
        .assign_next_image(
            &fixture.annotator,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap();
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let expected = expectation(&state, &fixture.task_id, &fixture.targets[0]);
    let left_repository = fixture.repository.clone();
    let right_repository = fixture.repository.clone();
    let left_assignment = assignment.clone();
    let right_assignment = assignment.clone();
    let left_expected = expected.clone();
    let right_expected = expected;
    let annotator = fixture.annotator.clone();
    let (left, right) = tokio::join!(
        async {
            left_repository
                .save_migration_skeleton(
                    &annotator,
                    context(&left_assignment),
                    None,
                    &left_expected,
                    skeleton(0.3),
                    "concurrent-left",
                )
                .await
        },
        async {
            right_repository
                .save_migration_skeleton(
                    &fixture.annotator,
                    context(&right_assignment),
                    None,
                    &right_expected,
                    skeleton(0.7),
                    "concurrent-right",
                )
                .await
        }
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(
        state
            .active_annotations()
            .filter(|annotation| annotation.task_id == fixture.task_id)
            .count(),
        1
    );
    assert_eq!(
        state.migration_dispositions[&fixture.task_id][&fixture.targets[0].object_group_id]
            .disposition_version,
        2
    );
}

#[tokio::test]
async fn exclusion_deletes_skeleton_atomically_and_survives_reload() {
    let fixture = fixture(ReviewWorkflow::None, 1).await;
    let assignment = claim_annotator(&fixture).await;
    let initial = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let expected = expectation(&initial, &fixture.task_id, &fixture.targets[0]);
    let resolved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            skeleton(0.4),
            "atomic-save",
        )
        .await
        .unwrap();
    let target_hash = resolved.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = resolved
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let pass = fixture
        .repository
        .start_migration_pass(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            "atomic-pass",
        )
        .await
        .unwrap();
    let pass_id = pass.active_pass.unwrap().pass_id;
    let expected = expectation(&pass.image_state, &fixture.task_id, &fixture.targets[0]);
    let excluded = fixture
        .repository
        .exclude_migration_target(
            &fixture.annotator,
            context(&assignment),
            Some(&pass_id),
            &expected,
            MigrationExclusionReason::InvalidSourceBox,
            None,
            "atomic-exclude",
        )
        .await
        .unwrap();
    let skeleton = excluded
        .image_state
        .current_annotation(&fixture.targets[0].reserved_skeleton_annotation_id)
        .unwrap();
    assert!(skeleton.deleted);
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    assert!(events.windows(2).any(|pair| {
        matches!(pair[0].payload, EventPayload::AnnotationDeleted { .. })
            && matches!(
                pair[1].payload,
                EventPayload::MigrationDispositionChanged { .. }
            )
    }));
    let rebuilt = rebuild_state(fixture.image_id.clone(), &events).unwrap();
    assert_eq!(rebuilt, excluded.image_state);
}

#[tokio::test]
async fn deleted_guide_resolves_only_through_atomic_exclusion() {
    let fixture = fixture(ReviewWorkflow::None, 1).await;
    let assignment = claim_annotator(&fixture).await;
    let initial = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let resolved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&initial, &fixture.task_id, &fixture.targets[0]),
            skeleton(0.4),
            "deleted-guide-save",
        )
        .await
        .unwrap();
    let guide = resolved
        .image_state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    fixture
        .repository
        .append_admin_repair_payload(
            &UserId::from("admin"),
            &fixture.image_id,
            resolved.image_state.current_sequence,
            EventPayload::AnnotationDeleted {
                annotation_id: guide.annotation_id.clone(),
                version: guide.version,
                reason: Some("invalid imported guide".to_string()),
            },
        )
        .await
        .unwrap();
    let deleted = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let expected = expectation(&deleted, &fixture.task_id, &fixture.targets[0]);
    assert!(expected.expected_guide_deleted);
    assert!(
        fixture
            .repository
            .keep_migration_target(
                &fixture.annotator,
                context(&assignment),
                &MigrationPassId::from("missing"),
                &expected,
                "deleted-guide-keep",
            )
            .await
            .is_err()
    );
    assert!(
        fixture
            .repository
            .save_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &expected,
                skeleton(0.5),
                "deleted-guide-annotate",
            )
            .await
            .is_err()
    );
    let excluded = fixture
        .repository
        .exclude_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            MigrationExclusionReason::InvalidSourceBox,
            Some("guide was deleted by data repair".to_string()),
            "deleted-guide-exclude",
        )
        .await
        .unwrap();
    assert_eq!(excluded.cursor, MigrationCursor::FullImage);
    assert!(
        !excluded.image_state.migration_dependencies[&fixture.task_id]
            .contains_key(&fixture.targets[0].object_group_id)
    );
    assert!(
        excluded
            .image_state
            .current_annotation(&fixture.targets[0].reserved_skeleton_annotation_id)
            .unwrap()
            .deleted
    );
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(
        rebuild_state(fixture.image_id.clone(), &events).unwrap(),
        excluded.image_state
    );
}

#[tokio::test]
async fn correction_pass_action_can_be_redone_for_new_exact_versions() {
    let fixture = fixture(ReviewWorkflow::None, 1).await;
    let assignment = claim_annotator(&fixture).await;
    let initial = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let resolved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&initial, &fixture.task_id, &fixture.targets[0]),
            skeleton(0.4),
            "redo-save",
        )
        .await
        .unwrap();
    let target_hash = resolved.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = resolved
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let pass = fixture
        .repository
        .start_migration_pass(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            "redo-pass",
        )
        .await
        .unwrap();
    let pass_id = pass.active_pass.unwrap().pass_id;
    let kept = fixture
        .repository
        .keep_migration_target(
            &fixture.annotator,
            context(&assignment),
            &pass_id,
            &expectation(&pass.image_state, &fixture.task_id, &fixture.targets[0]),
            "redo-keep",
        )
        .await
        .unwrap();
    assert_eq!(kept.cursor, MigrationCursor::FullImage);

    let guide_assignment = open_guide_assignment(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let guide = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let mut corrected_guide = guide.clone();
    corrected_guide.version += 1;
    corrected_guide.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.1,
        width: 0.2,
        height: 0.2,
    });
    corrected_guide.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    corrected_guide.author_user_id = fixture.annotator.clone();
    corrected_guide.updated_at = labello_domain::now();
    let changed = fixture
        .repository
        .apply_annotation_batch(
            &fixture.annotator,
            context(&guide_assignment),
            vec![EventPayload::AnnotationVersionCreated {
                annotation: corrected_guide,
                previous_version: Some(guide.version),
                reason: Some("guide correction after keep".to_string()),
            }],
            false,
        )
        .await
        .unwrap();
    assert!(matches!(
        changed
            .migration_cursor(&fixture.task_id, Some(&pass_id))
            .unwrap(),
        MigrationCursor::Object { .. }
    ));
    let redone = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            Some(&pass_id),
            &expectation(&changed, &fixture.task_id, &fixture.targets[0]),
            skeleton(0.6),
            "redo-corrected",
        )
        .await
        .unwrap();
    assert_eq!(redone.cursor, MigrationCursor::FullImage);
    let items = &redone.active_pass.unwrap().items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].object_group_id, items[1].object_group_id);
    assert_ne!(
        (
            items[0].guide_annotation_version,
            items[0].disposition_version
        ),
        (
            items[1].guide_annotation_version,
            items[1].disposition_version
        )
    );
}

#[tokio::test]
async fn concurrent_admin_repairs_use_one_exact_state_winner() {
    let fixture = fixture(ReviewWorkflow::None, 1).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let guide = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let repair = |x| {
        let mut annotation = guide.clone();
        annotation.version += 1;
        annotation.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
            x,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        });
        annotation.revision_source = RevisionSource::Human {
            action: HumanRevisionKind::Edited,
        };
        annotation.author_user_id = UserId::from("admin");
        annotation.updated_at = labello_domain::now();
        EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version: Some(guide.version),
            reason: Some("concurrent admin repair".to_string()),
        }
    };
    let left = fixture.repository.clone();
    let right = fixture.repository.clone();
    let image_id = fixture.image_id.clone();
    let admin = UserId::from("admin");
    let expected_sequence = state.current_sequence;
    let (left, right) = tokio::join!(
        left.append_admin_repair_payload(&admin, &image_id, expected_sequence, repair(0.2),),
        right.append_admin_repair_payload(
            &admin,
            &fixture.image_id,
            expected_sequence,
            repair(0.3),
        )
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let repaired = fixture
        .repository
        .rebuild_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(
        repaired
            .current_annotation(&fixture.targets[0].guide_annotation_id)
            .unwrap()
            .version,
        2
    );
    assert_eq!(
        repaired.migration_dependencies[&fixture.task_id][&fixture.targets[0].object_group_id].kind,
        MigrationDependencyKind::CorrectionRequired
    );
}

#[tokio::test]
async fn migration_review_is_sequential_and_rejection_cancels_competitors() {
    let fixture = fixture(ReviewWorkflow::Approval, 2).await;
    let annotation = claim_annotator(&fixture).await;
    let mut state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    for (index, target) in fixture.targets.iter().enumerate() {
        let expected = expectation(&state, &fixture.task_id, target);
        state = if index == 0 {
            fixture
                .repository
                .save_migration_skeleton(
                    &fixture.annotator,
                    context(&annotation),
                    None,
                    &expected,
                    skeleton(0.4),
                    "review-save",
                )
                .await
                .unwrap()
                .image_state
        } else {
            fixture
                .repository
                .exclude_migration_target(
                    &fixture.annotator,
                    context(&annotation),
                    None,
                    &expected,
                    MigrationExclusionReason::ObjectNotPresent,
                    None,
                    "review-exclude",
                )
                .await
                .unwrap()
                .image_state
        };
    }
    let target_hash = state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&annotation),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "review-submit",
        )
        .await
        .unwrap();
    let first_review = fixture
        .repository
        .assign_next_image(
            &fixture.reviewers[0],
            &fixture.task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();
    let second_review = fixture
        .repository
        .assign_next_image(
            &fixture.reviewers[1],
            &fixture.task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let second_version = state.migration_dispositions[&fixture.task_id]
        [&fixture.targets[1].object_group_id]
        .disposition_version;
    assert!(
        fixture
            .repository
            .review_migration(
                &fixture.reviewers[0],
                context(&first_review),
                &MigrationReviewTarget::Disposition {
                    object_group_id: fixture.targets[1].object_group_id.clone(),
                    disposition_version: second_version,
                },
                ReviewDecision::Approved,
                None,
                "review-jump",
            )
            .await
            .is_err()
    );
    let first_version = state.migration_dispositions[&fixture.task_id]
        [&fixture.targets[0].object_group_id]
        .disposition_version;
    fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&first_review),
            &MigrationReviewTarget::Disposition {
                object_group_id: fixture.targets[0].object_group_id.clone(),
                disposition_version: first_version,
            },
            ReviewDecision::Approved,
            None,
            "approve-first",
        )
        .await
        .unwrap();
    let rejected = fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&first_review),
            &MigrationReviewTarget::Disposition {
                object_group_id: fixture.targets[1].object_group_id.clone(),
                disposition_version: second_version,
            },
            ReviewDecision::Rejected,
            Some("object is not absent".to_string()),
            "reject-second",
        )
        .await
        .unwrap();
    let retried = fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&first_review),
            &MigrationReviewTarget::Disposition {
                object_group_id: fixture.targets[1].object_group_id.clone(),
                disposition_version: second_version,
            },
            ReviewDecision::Rejected,
            Some("object is not absent".to_string()),
            "reject-second",
        )
        .await
        .unwrap();
    assert_eq!(
        retried.image_state.current_sequence,
        rejected.image_state.current_sequence
    );
    assert_eq!(
        rejected.image_state.task_states[&fixture.task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert_eq!(
        assignment_status(&rejected.image_state, &first_review),
        AssignmentStatus::Completed
    );
    assert_eq!(
        assignment_status(&rejected.image_state, &second_review),
        AssignmentStatus::Cancelled
    );
    assert_eq!(
        rejected.image_state.migration_dependencies[&fixture.task_id]
            [&fixture.targets[1].object_group_id]
            .kind,
        MigrationDependencyKind::CorrectionRequired
    );
}

#[tokio::test]
async fn multiple_class_migrations_complete_and_review_independently() {
    let fixture = fixture(ReviewWorkflow::Approval, 1).await;
    let vehicle = add_migration_pair(&fixture, "vehicle", "wheel").await;
    let person = MigrationPair {
        guide_task_id: fixture.guide_task_id.clone(),
        task_id: fixture.task_id.clone(),
        targets: fixture.targets.clone(),
    };
    assert_ne!(person.guide_task_id, vehicle.guide_task_id);

    for (index, pair) in [&person, &vehicle].into_iter().enumerate() {
        let annotation = fixture
            .repository
            .assign_next_image(
                &fixture.annotator,
                &pair.task_id,
                AssignmentKind::Annotation,
            )
            .await
            .unwrap()
            .unwrap();
        let state = fixture
            .repository
            .load_image_state(&fixture.image_id)
            .await
            .unwrap();
        let resolved = fixture
            .repository
            .exclude_migration_target(
                &fixture.annotator,
                context(&annotation),
                None,
                &expectation(&state, &pair.task_id, &pair.targets[0]),
                MigrationExclusionReason::ObjectNotPresent,
                None,
                &format!("exclude-{index}"),
            )
            .await
            .unwrap();
        let target_hash = resolved.image_state.migration_target_sets[&pair.task_id]
            .target_set_hash
            .clone();
        let state_hash = resolved
            .image_state
            .current_migration_state_hash(&pair.task_id)
            .unwrap();
        let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
        fixture
            .repository
            .confirm_and_submit_migration(
                &fixture.annotator,
                context(&annotation),
                &target_hash,
                &state_hash,
                &confirmation_hash,
                &format!("confirm-{index}"),
            )
            .await
            .unwrap();

        let review = fixture
            .repository
            .assign_next_image(&fixture.reviewers[0], &pair.task_id, AssignmentKind::Review)
            .await
            .unwrap()
            .unwrap();
        let state = fixture
            .repository
            .load_image_state(&fixture.image_id)
            .await
            .unwrap();
        let disposition_version = state.migration_dispositions[&pair.task_id]
            [&pair.targets[0].object_group_id]
            .disposition_version;
        fixture
            .repository
            .review_migration(
                &fixture.reviewers[0],
                context(&review),
                &MigrationReviewTarget::Disposition {
                    object_group_id: pair.targets[0].object_group_id.clone(),
                    disposition_version,
                },
                ReviewDecision::Approved,
                None,
                &format!("review-object-{index}"),
            )
            .await
            .unwrap();
        fixture
            .repository
            .review_migration(
                &fixture.reviewers[0],
                context(&review),
                &MigrationReviewTarget::Confirmation { confirmation_hash },
                ReviewDecision::Approved,
                None,
                &format!("review-confirmation-{index}"),
            )
            .await
            .unwrap();

        let state = fixture
            .repository
            .load_image_state(&fixture.image_id)
            .await
            .unwrap();
        assert_eq!(
            state.task_states[&pair.task_id].status,
            TaskStatus::Completed
        );
        if index == 0 {
            assert_eq!(
                state.task_states[&vehicle.task_id].status,
                TaskStatus::Pending
            );
        }
    }

    let guide_assignment = open_guide_assignment(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let current = state
        .current_annotation(&person.targets[0].guide_annotation_id)
        .unwrap();
    let mut corrected = current.clone();
    corrected.version += 1;
    corrected.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    corrected.author_user_id = fixture.annotator.clone();
    corrected.updated_at = labello_domain::now();
    corrected.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.2,
        width: 0.2,
        height: 0.2,
    });
    fixture
        .repository
        .apply_annotation_batch(
            &fixture.annotator,
            context(&guide_assignment),
            vec![EventPayload::AnnotationVersionCreated {
                annotation: corrected,
                previous_version: Some(current.version),
                reason: Some("correct person guide".to_string()),
            }],
            false,
        )
        .await
        .unwrap();

    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(
        state.task_states[&person.task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert!(!state.migration_confirmations.contains_key(&person.task_id));
    assert_eq!(
        state.task_states[&vehicle.task_id].status,
        TaskStatus::Completed
    );
    assert!(state.migration_confirmations.contains_key(&vehicle.task_id));
}

#[tokio::test]
async fn reopening_submitted_migration_cancels_active_reviews() {
    let fixture = fixture(ReviewWorkflow::Approval, 1).await;
    let annotation = claim_annotator(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let expected = expectation(&state, &fixture.task_id, &fixture.targets[0]);
    let resolved = fixture
        .repository
        .exclude_migration_target(
            &fixture.annotator,
            context(&annotation),
            None,
            &expected,
            MigrationExclusionReason::ObjectNotPresent,
            None,
            "reopen-exclude",
        )
        .await
        .unwrap();
    let target_hash = resolved.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = resolved
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&annotation),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "reopen-submit",
        )
        .await
        .unwrap();
    let review = fixture
        .repository
        .assign_next_image(
            &fixture.reviewers[0],
            &fixture.task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();

    let reopened = fixture
        .repository
        .reopen_annotation_assignment(
            &fixture.annotator,
            &annotation.assignment_id,
            &fixture.image_id,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(reopened.status, AssignmentStatus::Active);
    assert_eq!(
        state.task_states[&fixture.task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert_eq!(
        assignment_status(&state, &review),
        AssignmentStatus::Cancelled
    );
}

#[tokio::test]
async fn final_review_is_digest_bound_and_guide_change_reopens_terminal_migration() {
    let fixture = fixture(ReviewWorkflow::Approval, 1).await;
    let annotation = claim_annotator(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let expected = expectation(&state, &fixture.task_id, &fixture.targets[0]);
    let resolved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&annotation),
            None,
            &expected,
            skeleton(0.5),
            "digest-save",
        )
        .await
        .unwrap();
    let target_hash = resolved.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = resolved
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&annotation),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "digest-submit",
        )
        .await
        .unwrap();
    let review = fixture
        .repository
        .assign_next_image(
            &fixture.reviewers[0],
            &fixture.task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();
    let disposition_version = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap()
        .migration_dispositions[&fixture.task_id][&fixture.targets[0].object_group_id]
        .disposition_version;
    fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&review),
            &MigrationReviewTarget::Disposition {
                object_group_id: fixture.targets[0].object_group_id.clone(),
                disposition_version,
            },
            ReviewDecision::Approved,
            None,
            "digest-object",
        )
        .await
        .unwrap();
    assert!(
        fixture
            .repository
            .review_migration(
                &fixture.reviewers[0],
                context(&review),
                &MigrationReviewTarget::Confirmation {
                    confirmation_hash: state_hash.clone(),
                },
                ReviewDecision::Approved,
                None,
                "wrong-digest",
            )
            .await
            .is_err()
    );
    let approved = fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&review),
            &MigrationReviewTarget::Confirmation {
                confirmation_hash: confirmation_hash.clone(),
            },
            ReviewDecision::Approved,
            None,
            "right-digest",
        )
        .await
        .unwrap();
    assert_eq!(
        approved.image_state.task_states[&fixture.task_id].status,
        TaskStatus::Completed
    );

    let guide_assignment = open_guide_assignment(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let current = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let mut corrected = current.clone();
    corrected.version += 1;
    corrected.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    corrected.author_user_id = fixture.annotator.clone();
    corrected.updated_at = labello_domain::now();
    corrected.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.15,
        y: 0.15,
        width: 0.2,
        height: 0.2,
    });
    fixture
        .repository
        .apply_annotation_batch(
            &fixture.annotator,
            context(&guide_assignment),
            vec![EventPayload::AnnotationVersionCreated {
                annotation: corrected,
                previous_version: Some(current.version),
                reason: Some("source correction".to_string()),
            }],
            false,
        )
        .await
        .unwrap();
    let reopened = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(
        reopened.task_states[&fixture.task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert!(
        !reopened
            .migration_confirmations
            .contains_key(&fixture.task_id)
    );
    assert_eq!(
        reopened.migration_dependencies[&fixture.task_id][&fixture.targets[0].object_group_id].kind,
        MigrationDependencyKind::CorrectionRequired
    );
}

#[tokio::test]
async fn admin_guide_repair_invalidates_submitted_migration_atomically() {
    let fixture = fixture(ReviewWorkflow::Approval, 1).await;
    let review = prepare_submitted_migration(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let current = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let mut corrected = current.clone();
    corrected.version += 1;
    corrected.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    corrected.author_user_id = UserId::from("admin");
    corrected.updated_at = labello_domain::now();
    corrected.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.1,
        width: 0.2,
        height: 0.2,
    });
    fixture
        .repository
        .append_payload(
            &fixture.image_id,
            &Actor {
                user_id: UserId::from("admin"),
                role: DatasetRole::DataAdmin,
            },
            EventPayload::AnnotationVersionCreated {
                annotation: corrected,
                previous_version: Some(current.version),
                reason: Some("admin guide repair".to_string()),
            },
        )
        .await
        .unwrap();
    assert_guide_invalidation(&fixture, &review).await;
}

#[tokio::test]
async fn annotation_batch_guide_change_invalidates_submitted_migration_atomically() {
    let fixture = fixture(ReviewWorkflow::Approval, 1).await;
    let review = prepare_submitted_migration(&fixture).await;
    let guide_assignment = open_guide_assignment(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let current = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let mut corrected = current.clone();
    corrected.version += 1;
    corrected.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    corrected.author_user_id = fixture.annotator.clone();
    corrected.updated_at = labello_domain::now();
    corrected.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.22,
        y: 0.1,
        width: 0.2,
        height: 0.2,
    });
    fixture
        .repository
        .append_for_assignment(
            &fixture.annotator,
            context(&guide_assignment),
            vec![EventPayload::AnnotationVersionCreated {
                annotation: corrected,
                previous_version: Some(current.version),
                reason: Some("batch guide correction".to_string()),
            }],
            false,
        )
        .await
        .unwrap();
    assert_guide_invalidation(&fixture, &review).await;
}

#[tokio::test]
async fn offline_guide_mutation_invalidates_submitted_migration_atomically() {
    let fixture = fixture(ReviewWorkflow::Approval, 1).await;
    let review = prepare_submitted_migration(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let guide = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let result = fixture
        .repository
        .sync_offline_events(labello_domain::OfflineSyncRequest::new(
            DatasetId::from("ds"),
            fixture.annotator.clone(),
            vec![labello_domain::OfflineMutationFragment {
                image_id: fixture.image_id.clone(),
                base_sequence: state.current_sequence,
                mutations: vec![labello_domain::OfflineMutation::AnnotationUpsert {
                    annotation_id: guide.annotation_id.clone(),
                    expected_version: Some(guide.version),
                    task_id: guide.task_id.clone(),
                    class_id: guide.class_id.clone(),
                    annotation_type: guide.annotation_type.clone(),
                    source: labello_domain::OfflineAnnotationSource::Human,
                    geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                        x: 0.25,
                        y: 0.1,
                        width: 0.2,
                        height: 0.2,
                    }),
                    reason: Some("offline guide correction".to_string()),
                }],
            }],
        ))
        .await
        .unwrap();
    assert_eq!(result.merged_events, 1);
    assert_guide_invalidation(&fixture, &review).await;
}

#[tokio::test]
async fn reviewer_guide_correction_invalidates_submitted_migration_atomically() {
    let fixture = fixture(ReviewWorkflow::Approval, 1).await;
    let mut metadata = fixture.repository.load_dataset_config().await.unwrap();
    metadata
        .tasks
        .iter_mut()
        .find(|task| task.task_id == fixture.guide_task_id)
        .unwrap()
        .review = ReviewConfig {
        required_reviews: 1,
        workflow: ReviewWorkflow::Approval,
        allow_reviewer_corrections: true,
        agreement_threshold: None,
    };
    fixture.repository.save_dataset(&metadata).await.unwrap();
    let migration_review = prepare_submitted_migration(&fixture).await;

    let guide_assignment = open_guide_assignment(&fixture).await;
    fixture
        .repository
        .complete_assignment(
            &fixture.annotator,
            &guide_assignment.assignment_id,
            &fixture.image_id,
            &fixture.guide_task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();
    let guide_review = fixture
        .repository
        .assign_next_image(
            &fixture.reviewers[1],
            &fixture.guide_task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();
    fixture
        .repository
        .correct_review_annotation(
            &fixture.reviewers[1],
            context(&guide_review),
            &labello_domain::CorrectionId::from("cor_guide"),
            &fixture.targets[0].guide_annotation_id,
            1,
            AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.3,
                y: 0.1,
                width: 0.2,
                height: 0.2,
            }),
            Some("reviewer guide correction".to_string()),
        )
        .await
        .unwrap();
    assert_guide_invalidation(&fixture, &migration_review).await;
}

async fn fixture(workflow: ReviewWorkflow, target_count: usize) -> Fixture {
    fixture_with_skeleton_spec(
        workflow,
        target_count,
        SkeletonSpec {
            keypoints: vec![KeypointSpec {
                name: "nose".to_string(),
                required: true,
            }],
            edges: Vec::new(),
            allow_hidden: true,
            allow_absent: false,
        },
    )
    .await
}

async fn fixture_with_skeleton_spec(
    workflow: ReviewWorkflow,
    target_count: usize,
    skeleton_spec: SkeletonSpec,
) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let repository = DatasetRepository::new(temp.path());
    let image_id = ImageId::from("img_1");
    let guide_task_id = TaskId::from("bounding_box:person");
    let task_id = TaskId::from("skeleton:person");
    let class_id = ClassId::from("person");
    let annotator = UserId::from("annotator");
    let reviewers = [UserId::from("reviewer_1"), UserId::from("reviewer_2")];
    let mut metadata =
        DatasetMetadata::new(DatasetId::from("ds"), "Dataset", labello_domain::now());
    metadata.label_classes.push(LabelClass {
        class_id: class_id.clone(),
        name: "Person".to_string(),
        color: "#ffffff".to_string(),
        description: None,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: guide_task_id.clone(),
        name: "Person boxes".to_string(),
        annotation_type: AnnotationType::BoundingBox,
        class_ids: vec![class_id.clone()],
        instructions: tutorial(),
        skeleton: None,
        review: ReviewConfig::default(),
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: None,
        enabled: true,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: task_id.clone(),
        name: "Person skeletons".to_string(),
        annotation_type: AnnotationType::Skeleton,
        class_ids: vec![class_id.clone()],
        instructions: tutorial(),
        skeleton: Some(skeleton_spec),
        review: ReviewConfig {
            required_reviews: 1,
            workflow,
            allow_reviewer_corrections: false,
            agreement_threshold: None,
        },
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: Some(ManualBoxGuideMigration {
            guide_task_id: guide_task_id.clone(),
            cardinality: MigrationCardinality::ExactlyOne,
            allow_exclusion: true,
            sequence: MigrationSequence::ImportedSpatialOrderV1,
        }),
        enabled: true,
    });
    metadata.role_assignments.push(DatasetRoleAssignment {
        dataset_id: metadata.dataset_id.clone(),
        user_id: annotator.clone(),
        roles: BTreeSet::from([DatasetRole::Annotator]),
        assigned_at: labello_domain::now(),
        assigned_by: None,
    });
    metadata.role_assignments.push(DatasetRoleAssignment {
        dataset_id: metadata.dataset_id.clone(),
        user_id: UserId::from("admin"),
        roles: BTreeSet::from([DatasetRole::DataAdmin]),
        assigned_at: labello_domain::now(),
        assigned_by: None,
    });
    metadata
        .role_assignments
        .extend(reviewers.iter().map(|reviewer| DatasetRoleAssignment {
            dataset_id: metadata.dataset_id.clone(),
            user_id: reviewer.clone(),
            roles: BTreeSet::from([DatasetRole::Reviewer]),
            assigned_at: labello_domain::now(),
            assigned_by: None,
        }));
    repository.initialize(metadata).await.unwrap();
    repository
        .save_images_index(&ImagesIndex {
            schema_version: SCHEMA_VERSION,
            image_count: 1,
            images_by_hash: BTreeMap::from([(
                "hash".to_string(),
                ImageRecord {
                    image_id: image_id.clone(),
                    blake3: "hash".to_string(),
                    canonical_path: "images/one.png".to_string(),
                    known_paths: vec!["images/one.png".to_string()],
                    duplicate_paths: Vec::new(),
                    file_name: "one.png".to_string(),
                    byte_size: 4,
                    width: 100,
                    height: 100,
                    media_type: "image/png".to_string(),
                    source_memberships: None,
                },
            )]),
        })
        .await
        .unwrap();

    let import_id = ImportId::from("import_1");
    let timestamp = labello_domain::now();
    let targets = (0..target_count)
        .map(|index| MigrationTarget {
            object_group_id: ObjectGroupId::from(format!("group_{index}")),
            guide_annotation_id: AnnotationId::from(format!("box_{index}")),
            reserved_skeleton_annotation_id: AnnotationId::from(format!("skeleton_{index}")),
            sequence_index: index as u64,
        })
        .collect::<Vec<_>>();
    let annotations = targets
        .iter()
        .enumerate()
        .map(|(index, target)| AnnotationVersion {
            annotation_id: target.guide_annotation_id.clone(),
            version: 1,
            object_group_id: Some(target.object_group_id.clone()),
            origin: AnnotationOrigin::Imported {
                imported: ImportedOrigin {
                    import_id: import_id.clone(),
                    source_profile: SourceProfile {
                        profile_id: "test".to_string(),
                        profile_version: 1,
                    },
                    source_namespace: "test".to_string(),
                    source_object_key: format!("object_{index}"),
                    geometry_provenance: ImportGeometryProvenance::Direct,
                },
            },
            task_id: guide_task_id.clone(),
            class_id: class_id.clone(),
            annotation_type: AnnotationType::BoundingBox,
            revision_source: RevisionSource::Import {
                import_id: import_id.clone(),
            },
            geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.1 + index as f32 * 0.3,
                y: 0.1,
                width: 0.2,
                height: 0.2,
            }),
            author_user_id: UserId::from("importer"),
            created_at: timestamp,
            updated_at: timestamp,
            deleted: false,
        })
        .collect::<Vec<_>>();
    let target_set_hash = migration_target_set_hash(
        &MigrationHashContext {
            dataset_id: &DatasetId::from("ds"),
            image_id: &image_id,
            guide_task_id: &guide_task_id,
            target_task_id: &task_id,
        },
        &targets,
    )
    .unwrap();
    repository
        .append_payload(
            &image_id,
            &Actor {
                user_id: UserId::from("importer"),
                role: DatasetRole::DataAdmin,
            },
            EventPayload::ImportInitialized {
                import_id,
                annotations,
                task_initializations: vec![
                    ImportTaskInitialization {
                        task_id: guide_task_id.clone(),
                        coverage: ImportCoverage::Complete,
                        initial_state: TaskState {
                            task_id: guide_task_id.clone(),
                            status: TaskStatus::Completed,
                            outcome: Some(TaskOutcome::ImportedGroundTruth),
                            assigned_to: None,
                            completed_by: Some(UserId::from("importer")),
                            completed_at: Some(timestamp),
                            updated_at: timestamp,
                        },
                    },
                    ImportTaskInitialization {
                        task_id: task_id.clone(),
                        coverage: ImportCoverage::Incomplete,
                        initial_state: TaskState::new(task_id.clone(), timestamp),
                    },
                ],
                migration_target_sets: vec![MigrationTargetSetInitialization {
                    dataset_id: DatasetId::from("ds"),
                    guide_task_id: guide_task_id.clone(),
                    target_task_id: task_id.clone(),
                    target_set_hash,
                    targets: targets.clone(),
                }],
            },
        )
        .await
        .unwrap();
    Fixture {
        _temp: temp,
        repository,
        image_id,
        guide_task_id,
        task_id,
        annotator,
        reviewers,
        targets,
    }
}

async fn add_migration_pair(fixture: &Fixture, class: &str, keypoint: &str) -> MigrationPair {
    let class_id = ClassId::from(class);
    let guide_task_id = TaskId::from(format!("bounding_box:{class}"));
    let task_id = TaskId::from(format!("skeleton:{class}"));
    let mut metadata = fixture.repository.load_dataset_config().await.unwrap();
    metadata.label_classes.push(LabelClass {
        class_id: class_id.clone(),
        name: class.to_string(),
        color: "#cccccc".to_string(),
        description: None,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: guide_task_id.clone(),
        name: format!("{class} boxes"),
        annotation_type: AnnotationType::BoundingBox,
        class_ids: vec![class_id.clone()],
        instructions: tutorial(),
        skeleton: None,
        review: ReviewConfig::default(),
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: None,
        enabled: true,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: task_id.clone(),
        name: format!("{class} skeletons"),
        annotation_type: AnnotationType::Skeleton,
        class_ids: vec![class_id.clone()],
        instructions: tutorial(),
        skeleton: Some(SkeletonSpec {
            keypoints: vec![KeypointSpec {
                name: keypoint.to_string(),
                required: true,
            }],
            edges: Vec::new(),
            allow_hidden: true,
            allow_absent: false,
        }),
        review: ReviewConfig {
            required_reviews: 1,
            workflow: ReviewWorkflow::Approval,
            allow_reviewer_corrections: false,
            agreement_threshold: None,
        },
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: Some(ManualBoxGuideMigration {
            guide_task_id: guide_task_id.clone(),
            cardinality: MigrationCardinality::ExactlyOne,
            allow_exclusion: true,
            sequence: MigrationSequence::ImportedSpatialOrderV1,
        }),
        enabled: true,
    });
    fixture.repository.save_dataset(&metadata).await.unwrap();

    let import_id = ImportId::from(format!("import_{class}"));
    let timestamp = labello_domain::now();
    let target = MigrationTarget {
        object_group_id: ObjectGroupId::from(format!("group_{class}")),
        guide_annotation_id: AnnotationId::from(format!("box_{class}")),
        reserved_skeleton_annotation_id: AnnotationId::from(format!("skeleton_{class}")),
        sequence_index: 0,
    };
    let annotation = AnnotationVersion {
        annotation_id: target.guide_annotation_id.clone(),
        version: 1,
        object_group_id: Some(target.object_group_id.clone()),
        origin: AnnotationOrigin::Imported {
            imported: ImportedOrigin {
                import_id: import_id.clone(),
                source_profile: SourceProfile {
                    profile_id: "test".to_string(),
                    profile_version: 1,
                },
                source_namespace: "test".to_string(),
                source_object_key: format!("object_{class}"),
                geometry_provenance: ImportGeometryProvenance::Direct,
            },
        },
        task_id: guide_task_id.clone(),
        class_id,
        annotation_type: AnnotationType::BoundingBox,
        revision_source: RevisionSource::Import {
            import_id: import_id.clone(),
        },
        geometry: AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.6,
            y: 0.6,
            width: 0.2,
            height: 0.2,
        }),
        author_user_id: UserId::from("importer"),
        created_at: timestamp,
        updated_at: timestamp,
        deleted: false,
    };
    let targets = vec![target];
    let target_set_hash = migration_target_set_hash(
        &MigrationHashContext {
            dataset_id: &DatasetId::from("ds"),
            image_id: &fixture.image_id,
            guide_task_id: &guide_task_id,
            target_task_id: &task_id,
        },
        &targets,
    )
    .unwrap();
    fixture
        .repository
        .append_payload(
            &fixture.image_id,
            &Actor {
                user_id: UserId::from("importer"),
                role: DatasetRole::DataAdmin,
            },
            EventPayload::ImportInitialized {
                import_id,
                annotations: vec![annotation],
                task_initializations: vec![
                    ImportTaskInitialization {
                        task_id: guide_task_id.clone(),
                        coverage: ImportCoverage::Complete,
                        initial_state: TaskState {
                            task_id: guide_task_id.clone(),
                            status: TaskStatus::Completed,
                            outcome: Some(TaskOutcome::ImportedGroundTruth),
                            assigned_to: None,
                            completed_by: Some(UserId::from("importer")),
                            completed_at: Some(timestamp),
                            updated_at: timestamp,
                        },
                    },
                    ImportTaskInitialization {
                        task_id: task_id.clone(),
                        coverage: ImportCoverage::Incomplete,
                        initial_state: TaskState::new(task_id.clone(), timestamp),
                    },
                ],
                migration_target_sets: vec![MigrationTargetSetInitialization {
                    dataset_id: DatasetId::from("ds"),
                    guide_task_id: guide_task_id.clone(),
                    target_task_id: task_id.clone(),
                    target_set_hash,
                    targets: targets.clone(),
                }],
            },
        )
        .await
        .unwrap();
    MigrationPair {
        guide_task_id,
        task_id,
        targets,
    }
}

async fn claim_annotator(fixture: &Fixture) -> Assignment {
    fixture
        .repository
        .assign_next_image(
            &fixture.annotator,
            &fixture.task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap()
}

async fn prepare_submitted_migration(fixture: &Fixture) -> Assignment {
    let annotation = claim_annotator(fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let resolved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&annotation),
            None,
            &expectation(&state, &fixture.task_id, &fixture.targets[0]),
            skeleton(0.5),
            "invalidation-save",
        )
        .await
        .unwrap();
    let target_hash = resolved.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = resolved
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&annotation),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "invalidation-submit",
        )
        .await
        .unwrap();
    let review = fixture
        .repository
        .assign_next_image(
            &fixture.reviewers[0],
            &fixture.task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();
    let disposition_version = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap()
        .migration_dispositions[&fixture.task_id][&fixture.targets[0].object_group_id]
        .disposition_version;
    fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&review),
            &MigrationReviewTarget::Disposition {
                object_group_id: fixture.targets[0].object_group_id.clone(),
                disposition_version,
            },
            ReviewDecision::Approved,
            None,
            "invalidation-review",
        )
        .await
        .unwrap();
    review
}

async fn assert_guide_invalidation(fixture: &Fixture, review: &Assignment) {
    let state = fixture
        .repository
        .rebuild_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(
        state.task_states[&fixture.task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert!(!state.migration_confirmations.contains_key(&fixture.task_id));
    assert_eq!(
        state.migration_dependencies[&fixture.task_id][&fixture.targets[0].object_group_id].kind,
        MigrationDependencyKind::CorrectionRequired
    );
    assert_eq!(
        assignment_status(&state, review),
        AssignmentStatus::Cancelled
    );
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    assert!(current_migration_reviews(&events, &fixture.task_id).is_empty());
}

async fn open_guide_assignment(fixture: &Fixture) -> Assignment {
    let now = labello_domain::now();
    let assignment = Assignment {
        assignment_id: AssignmentId::generate(),
        image_id: fixture.image_id.clone(),
        task_id: fixture.guide_task_id.clone(),
        assigned_to: fixture.annotator.clone(),
        kind: AssignmentKind::Annotation,
        status: AssignmentStatus::Active,
        expires_at: Some(lease_expiration(now)),
        created_at: now,
        updated_at: now,
    };
    fixture
        .repository
        .append_payloads_unlocked(
            &fixture.image_id,
            &Actor {
                user_id: fixture.annotator.clone(),
                role: DatasetRole::Annotator,
            },
            vec![
                EventPayload::TaskStateChanged {
                    task_state: TaskState {
                        task_id: fixture.guide_task_id.clone(),
                        status: TaskStatus::InProgress,
                        outcome: None,
                        assigned_to: Some(fixture.annotator.clone()),
                        completed_by: None,
                        completed_at: None,
                        updated_at: now,
                    },
                },
                EventPayload::AssignmentUpdated {
                    assignment: assignment.clone(),
                },
            ],
        )
        .await
        .unwrap();
    assignment
}

fn context(assignment: &Assignment) -> AssignmentContext<'_> {
    AssignmentContext {
        assignment_id: &assignment.assignment_id,
        image_id: &assignment.image_id,
        task_id: &assignment.task_id,
        kind: assignment.kind.clone(),
    }
}

fn expectation(
    state: &ImageState,
    task_id: &TaskId,
    target: &MigrationTarget,
) -> MigrationTargetExpectation {
    let guide = state
        .current_annotation(&target.guide_annotation_id)
        .unwrap();
    let skeleton = state
        .current_annotation(&target.reserved_skeleton_annotation_id)
        .filter(|annotation| !annotation.deleted);
    MigrationTargetExpectation {
        object_group_id: target.object_group_id.clone(),
        expected_guide_annotation_version: guide.version,
        expected_guide_deleted: guide.deleted,
        expected_disposition_version: state.migration_dispositions[task_id]
            [&target.object_group_id]
            .disposition_version,
        expected_skeleton_version: skeleton.map(|annotation| annotation.version),
    }
}

fn skeleton(x: f32) -> SkeletonGeometry {
    SkeletonGeometry {
        keypoints: vec![KeypointAnnotation {
            name: "nose".to_string(),
            state: KeypointState::Visible,
            point: Some(NormalizedPoint { x, y: 0.5 }),
        }],
    }
}

fn tutorial() -> TutorialContent {
    TutorialContent {
        title: "Instructions".to_string(),
        example_text: "Annotate".to_string(),
        example_images: Vec::new(),
    }
}

fn assignment_status(state: &ImageState, assignment: &Assignment) -> AssignmentStatus {
    state
        .assignments
        .iter()
        .find(|candidate| candidate.assignment_id == assignment.assignment_id)
        .unwrap()
        .status
        .clone()
}

#[tokio::test]
async fn discovery_companion_transaction_retries_replays_updates_and_withdraws() {
    let fixture = fixture(ReviewWorkflow::None, 0).await;
    let assignment = claim_annotator(&fixture).await;
    let initial = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let added = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "pair-create",
        )
        .await
        .unwrap();
    let skeleton_id = added.annotation_id.clone().unwrap();
    let link = added.image_state.migration_companions[&skeleton_id].clone();
    assert_eq!(link.skeleton_version, 1);
    assert_eq!(link.box_version, 1);
    let bounding_box = added
        .image_state
        .migration_companion_box(&skeleton_id)
        .unwrap();
    assert_eq!(bounding_box.task_id, fixture.guide_task_id);
    assert!(bounding_box.object_group_id.is_none());
    assert!(
        added
            .image_state
            .migration_companion_is_derived(&skeleton_id)
    );
    assert_eq!(
        added.image_state.task_states[&fixture.guide_task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert!(
        added.image_state.task_states[&fixture.guide_task_id]
            .outcome
            .is_none()
    );
    assert_eq!(
        added.image_state.migration_target_sets,
        initial.migration_target_sets
    );
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    let replayed = rebuild_state(fixture.image_id.clone(), &events).unwrap();
    assert_eq!(replayed, added.image_state);
    for boundary in 0..=events.len() {
        let prefix = rebuild_state(fixture.image_id.clone(), &events[..boundary]).unwrap();
        let decoded: ImageState =
            serde_json::from_slice(&serde_json::to_vec(&prefix).unwrap()).unwrap();
        assert_eq!(prefix, decoded);
    }
    let derived_event = events
        .iter()
        .find(|event| {
            matches!(&event.payload,
        EventPayload::AnnotationVersionCreated { annotation, .. }
            if matches!(annotation.revision_source, RevisionSource::MigrationSkeleton { .. }))
        })
        .unwrap();
    let mut legacy_encoding = derived_event.clone();
    legacy_encoding.schema_version = 2;
    assert!(serde_json::to_value(&legacy_encoding).is_err());
    let schema = serde_json::to_value(labello_domain::labello_schema_bundle())
        .unwrap()
        .to_string();
    assert!(
        schema.contains("migration_companion_linked") && schema.contains("migrationCompanions")
    );
    let roundtrip: ImageState =
        serde_json::from_slice(&serde_json::to_vec(&replayed).unwrap()).unwrap();
    assert_eq!(roundtrip, replayed);
    let retried = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "pair-create",
        )
        .await
        .unwrap();
    assert_eq!(
        retried.image_state.current_sequence,
        added.image_state.current_sequence
    );
    let edited = fixture
        .repository
        .edit_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &skeleton_id,
            1,
            skeleton(0.8),
            "pair-edit",
        )
        .await
        .unwrap();
    let edited_link = &edited.image_state.migration_companions[&skeleton_id];
    assert_eq!(edited_link.box_annotation_id, link.box_annotation_id);
    assert_eq!(edited_link.box_version, 2);
    assert_eq!(edited_link.skeleton_version, 2);
    assert_eq!(
        edited.image_state.migration_target_sets,
        initial.migration_target_sets
    );
    let removed = fixture
        .repository
        .delete_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &skeleton_id,
            2,
            "pair-delete",
        )
        .await
        .unwrap();
    assert!(
        removed
            .image_state
            .current_annotation(&link.box_annotation_id)
            .unwrap()
            .deleted
    );
    assert!(
        removed
            .image_state
            .current_annotation(&skeleton_id)
            .unwrap()
            .deleted
    );
    let retried = fixture
        .repository
        .delete_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &skeleton_id,
            2,
            "pair-delete",
        )
        .await
        .unwrap();
    assert_eq!(
        retried.image_state.current_sequence,
        removed.image_state.current_sequence
    );
}

#[tokio::test]
async fn discovery_companion_conflicts_preserve_both_objects_and_require_explicit_reconciliation() {
    let fixture = fixture(ReviewWorkflow::None, 0).await;
    let assignment = claim_annotator(&fixture).await;
    let added = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "conflict-create",
        )
        .await
        .unwrap();
    let skeleton_id = added.annotation_id.unwrap();
    let mut bounding_box = added
        .image_state
        .migration_companion_box(&skeleton_id)
        .unwrap()
        .clone();
    bounding_box.version += 1;
    bounding_box.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    bounding_box.updated_at = labello_domain::now();
    fixture
        .repository
        .append_payloads_unlocked(
            &fixture.image_id,
            &Actor {
                user_id: fixture.annotator.clone(),
                role: DatasetRole::Annotator,
            },
            vec![EventPayload::AnnotationVersionCreated {
                annotation: bounding_box.clone(),
                previous_version: Some(1),
                reason: Some("ordinary box correction".into()),
            }],
        )
        .await
        .unwrap();
    let before = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    assert!(
        fixture
            .repository
            .edit_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &skeleton_id,
                1,
                skeleton(0.8),
                "blocked-edit"
            )
            .await
            .is_err()
    );
    assert!(
        fixture
            .repository
            .delete_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &skeleton_id,
                1,
                "blocked-delete"
            )
            .await
            .is_err()
    );
    assert!(
        fixture
            .repository
            .reconcile_migration_companion(
                &fixture.annotator,
                context(&assignment),
                None,
                &skeleton_id,
                1,
                Some(1),
                "stale-reconcile"
            )
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap(),
        before
    );
    let reconciled = fixture
        .repository
        .reconcile_migration_companion(
            &fixture.annotator,
            context(&assignment),
            None,
            &skeleton_id,
            1,
            Some(2),
            "explicit-reconcile",
        )
        .await
        .unwrap();
    assert_eq!(
        reconciled.image_state.migration_companions[&skeleton_id].box_version,
        3
    );
    assert!(
        reconciled
            .image_state
            .migration_companion_is_derived(&skeleton_id)
    );
    let retry = fixture
        .repository
        .reconcile_migration_companion(
            &fixture.annotator,
            context(&assignment),
            None,
            &skeleton_id,
            1,
            Some(2),
            "explicit-reconcile",
        )
        .await
        .unwrap();
    assert_eq!(
        retry.image_state.current_sequence,
        reconciled.image_state.current_sequence
    );
    fixture
        .repository
        .edit_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &skeleton_id,
            1,
            skeleton(0.8),
            "retained-edit",
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn discovery_companion_rejects_competing_guide_assignment_and_wrong_actor_atomically() {
    let fixture = fixture(ReviewWorkflow::None, 0).await;
    let assignment = claim_annotator(&fixture).await;
    open_guide_assignment(&fixture).await;
    let before = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    let error = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "competing-create",
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("active assignment"));
    assert!(
        fixture
            .repository
            .add_migration_skeleton(
                &fixture.reviewers[0],
                context(&assignment),
                None,
                skeleton(0.7),
                "wrong-actor-create"
            )
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn legacy_discovery_reconciliation_is_explicit_resumable_and_preserves_unresolved_objects() {
    let fixture =
        fixture_with_skeleton_spec(ReviewWorkflow::None, 0, optional_skeleton_spec()).await;
    let assignment = claim_annotator(&fixture).await;
    let timestamp = labello_domain::now();
    let mut payloads = Vec::new();
    for (name, geometry, proven) in [
        ("legacy-one", mixed_optional_skeleton(), true),
        ("legacy-two", mixed_optional_skeleton(), true),
        ("legacy-no-positions", all_not_present_skeleton(), true),
        ("ordinary-skeleton", mixed_optional_skeleton(), false),
    ] {
        payloads.push(EventPayload::AnnotationVersionCreated {
            annotation: AnnotationVersion {
                annotation_id: AnnotationId::from(name),
                version: 1,
                object_group_id: None,
                origin: AnnotationOrigin::native(),
                task_id: fixture.task_id.clone(),
                class_id: ClassId::from("person"),
                annotation_type: AnnotationType::Skeleton,
                revision_source: RevisionSource::Human {
                    action: HumanRevisionKind::Authored,
                },
                geometry: AnnotationGeometry::Skeleton(geometry),
                author_user_id: fixture.annotator.clone(),
                created_at: timestamp,
                updated_at: timestamp,
                deleted: false,
            },
            previous_version: None,
            reason: proven.then(|| "object discovered during full-image migration review".into()),
        });
    }
    fixture
        .repository
        .append_payloads_unlocked(
            &fixture.image_id,
            &Actor {
                user_id: fixture.annotator.clone(),
                role: DatasetRole::Annotator,
            },
            payloads,
        )
        .await
        .unwrap();
    let initial = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert!(initial.migration_companions.is_empty());
    assert!(
        initial
            .migration_discovery_focus(
                &AnnotationId::from("legacy-no-positions"),
                labello_domain::ImageDimensions {
                    width: 100,
                    height: 100
                }
            )
            .is_none()
    );
    // A read cannot silently repair historical data.
    fixture
        .repository
        .current_manual_migration(&fixture.annotator, context(&assignment), None)
        .await
        .unwrap();
    assert_eq!(
        initial,
        fixture
            .repository
            .load_image_state(&fixture.image_id)
            .await
            .unwrap()
    );
    for name in ["legacy-no-positions", "ordinary-skeleton"] {
        assert!(
            fixture
                .repository
                .reconcile_migration_companion(
                    &fixture.annotator,
                    context(&assignment),
                    None,
                    &AnnotationId::from(name),
                    1,
                    None,
                    name
                )
                .await
                .is_err()
        );
    }
    let first = fixture
        .repository
        .reconcile_migration_companion(
            &fixture.annotator,
            context(&assignment),
            None,
            &AnnotationId::from("legacy-one"),
            1,
            None,
            "repair-one",
        )
        .await
        .unwrap();
    assert_eq!(first.image_state.migration_companions.len(), 1);
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    let rebuilt = rebuild_state(fixture.image_id.clone(), &events).unwrap();
    assert_eq!(rebuilt, first.image_state);
    // Resume after one committed object, retaining the original skeleton IDs and versions.
    let second = fixture
        .repository
        .reconcile_migration_companion(
            &fixture.annotator,
            context(&assignment),
            None,
            &AnnotationId::from("legacy-two"),
            1,
            None,
            "repair-two",
        )
        .await
        .unwrap();
    let retry = fixture
        .repository
        .reconcile_migration_companion(
            &fixture.annotator,
            context(&assignment),
            None,
            &AnnotationId::from("legacy-one"),
            1,
            None,
            "repair-one",
        )
        .await
        .unwrap();
    assert_eq!(
        retry.image_state.current_sequence,
        second.image_state.current_sequence
    );
    assert_eq!(retry.image_state.migration_companions.len(), 2);
    assert_eq!(
        retry.image_state.migration_target_sets,
        initial.migration_target_sets
    );
    for name in [
        "legacy-one",
        "legacy-two",
        "legacy-no-positions",
        "ordinary-skeleton",
    ] {
        assert_eq!(
            retry
                .image_state
                .current_annotation(&AnnotationId::from(name)),
            initial.current_annotation(&AnnotationId::from(name))
        );
    }
    let bundle = fixture
        .repository
        .create_offline_bundle(&fixture.annotator, 10, false)
        .await
        .unwrap();
    let roundtrip: labello_domain::OfflineBundle =
        serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert_eq!(
        roundtrip.images[0].state.migration_companions,
        retry.image_state.migration_companions
    );
    let snapshot = fixture.repository.create_snapshot().await.unwrap();
    let state_file = snapshot
        .files
        .iter()
        .find(|file| file.path.ends_with("/state.json"))
        .unwrap();
    let snapshot_state: ImageState = serde_json::from_slice(
        &tokio::fs::read(
            fixture
                .repository
                .snapshots_dir()
                .join(&snapshot.snapshot_id)
                .join(&state_file.path),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot_state.migration_companions,
        retry.image_state.migration_companions
    );
}

#[tokio::test]
async fn discovered_skeleton_review_precedes_confirmation_and_rejection_requires_a_new_version() {
    let fixture = fixture(ReviewWorkflow::Approval, 0).await;
    let annotation = claim_annotator(&fixture).await;
    let added = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&annotation),
            None,
            skeleton(0.7),
            "review-discovery-create",
        )
        .await
        .unwrap();
    let skeleton_id = added.annotation_id.unwrap();
    let target_hash = added.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = added
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&annotation),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "submit-discovery",
        )
        .await
        .unwrap();
    let reviewer = fixture
        .repository
        .assign_next_image(
            &fixture.reviewers[0],
            &fixture.task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();
    let before = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    assert!(
        fixture
            .repository
            .review_migration(
                &fixture.reviewers[0],
                context(&reviewer),
                &MigrationReviewTarget::Confirmation { confirmation_hash },
                ReviewDecision::Approved,
                None,
                "skip-discovery"
            )
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap(),
        before
    );
    let rejected = fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&reviewer),
            &MigrationReviewTarget::Discovered {
                annotation_id: skeleton_id.clone(),
                version: 1,
            },
            ReviewDecision::Rejected,
            None,
            "reject-discovery",
        )
        .await
        .unwrap();
    assert_eq!(
        rejected.image_state.task_states[&fixture.task_id].status,
        TaskStatus::NeedsCorrection
    );
    assert!(
        rejected
            .image_state
            .migration_discovery_requires_correction(
                rejected
                    .image_state
                    .current_annotation(&skeleton_id)
                    .unwrap()
            )
    );
    assert!(
        rejected.image_state.migration_target_sets[&fixture.task_id]
            .targets
            .is_empty()
    );
    let correction = claim_annotator(&fixture).await;
    let state_hash = rejected
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    assert!(
        fixture
            .repository
            .confirm_and_submit_migration(
                &fixture.annotator,
                context(&correction),
                &target_hash,
                &state_hash,
                &confirmation_hash,
                "unchanged-rejected-discovery"
            )
            .await
            .is_err()
    );
    fixture
        .repository
        .edit_migration_skeleton(
            &fixture.annotator,
            context(&correction),
            None,
            &skeleton_id,
            1,
            skeleton(0.8),
            "correct-discovery",
        )
        .await
        .unwrap();
}

mod config_races;

#[tokio::test]
async fn direct_revisit_non_adjacent_save_returns_to_full_image_and_replays_retries() {
    let fixture = fixture(ReviewWorkflow::None, 3).await;
    let assignment = claim_annotator(&fixture).await;
    let mut state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    for (index, target) in fixture.targets.iter().enumerate() {
        let result = fixture
            .repository
            .exclude_migration_target(
                &fixture.annotator,
                context(&assignment),
                None,
                &expectation(&state, &fixture.task_id, target),
                MigrationExclusionReason::ObjectNotPresent,
                None,
                &format!("direct-exclude-{index}"),
            )
            .await
            .unwrap();
        state = result.image_state;
    }
    assert_eq!(
        state.migration_cursor(&fixture.task_id, None).unwrap(),
        MigrationCursor::FullImage
    );
    let unchanged = state.migration_dispositions[&fixture.task_id].clone();
    let expected = expectation(&state, &fixture.task_id, &fixture.targets[0]);
    let revisit = fixture
        .repository
        .revisit_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            "direct-revisit-first",
        )
        .await
        .unwrap();
    let save_expected = expectation(&revisit.image_state, &fixture.task_id, &fixture.targets[0]);
    let saved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &save_expected,
            skeleton(0.2),
            "direct-save-first",
        )
        .await
        .unwrap();
    assert_eq!(saved.cursor, MigrationCursor::FullImage);
    for target in &fixture.targets[1..] {
        assert_eq!(
            saved.image_state.migration_dispositions[&fixture.task_id][&target.object_group_id],
            unchanged[&target.object_group_id]
        );
    }
    let retry = fixture
        .repository
        .revisit_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            "direct-revisit-first",
        )
        .await
        .unwrap();
    assert_eq!(retry.cursor, MigrationCursor::FullImage);
    assert_eq!(
        retry.image_state.current_sequence,
        saved.image_state.current_sequence
    );
    let retry = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &save_expected,
            skeleton(0.2),
            "direct-save-first",
        )
        .await
        .unwrap();
    assert_eq!(retry.image_state, saved.image_state);
    let fresh = DatasetRepository::new(fixture._temp.path())
        .rebuild_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert_eq!(fresh, saved.image_state);
    assert_eq!(
        fresh
            .current_annotation(&fixture.targets[0].reserved_skeleton_annotation_id)
            .unwrap()
            .version,
        1
    );
    assert!(
        fixture
            .repository
            .revisit_migration_target(
                &fixture.reviewers[0],
                context(&assignment),
                None,
                &expectation(&fresh, &fixture.task_id, &fixture.targets[1]),
                "wrong-revisit-owner"
            )
            .await
            .is_err()
    );
    assert!(
        fixture
            .repository
            .revisit_migration_target(
                &fixture.annotator,
                context(&assignment),
                None,
                &expected,
                "stale-revisit-version"
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn direct_revisit_finishes_selected_object_before_a_new_earlier_dependency() {
    let fixture = fixture(ReviewWorkflow::None, 3).await;
    let assignment = claim_annotator(&fixture).await;
    let mut state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    for (index, target) in fixture.targets.iter().enumerate() {
        state = fixture
            .repository
            .save_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &expectation(&state, &fixture.task_id, target),
                skeleton(0.2),
                &format!("initial-direct-{index}"),
            )
            .await
            .unwrap()
            .image_state;
    }
    let expected = expectation(&state, &fixture.task_id, &fixture.targets[2]);
    let revisited = fixture
        .repository
        .revisit_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            "direct-third",
        )
        .await
        .unwrap();
    let guide_assignment = open_guide_assignment(&fixture).await;
    let old = revisited
        .image_state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let mut corrected = old.clone();
    corrected.version += 1;
    corrected.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    corrected.updated_at = labello_domain::now();
    fixture
        .repository
        .append_for_assignment(
            &fixture.annotator,
            context(&guide_assignment),
            vec![EventPayload::AnnotationVersionCreated {
                annotation: corrected,
                previous_version: Some(old.version),
                reason: Some("synthetic earlier-guide correction".into()),
            }],
            false,
        )
        .await
        .unwrap();
    let changed = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    assert!(
        matches!(changed.migration_cursor(&fixture.task_id,None).unwrap(),MigrationCursor::Object{object_group_id,..} if object_group_id==fixture.targets[2].object_group_id)
    );
    let saved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expected,
            skeleton(0.3),
            "direct-third-save",
        )
        .await
        .unwrap();
    assert!(
        matches!(saved.cursor,MigrationCursor::Object{object_group_id,..} if object_group_id==fixture.targets[0].object_group_id)
    );
    assert!(
        saved.image_state.migration_dependencies[&fixture.task_id]
            .contains_key(&fixture.targets[0].object_group_id)
    );
    assert!(
        !saved
            .image_state
            .migration_confirmations
            .contains_key(&fixture.task_id)
    );
    let stale = expectation(&state, &fixture.task_id, &fixture.targets[0]);
    assert!(
        fixture
            .repository
            .save_migration_skeleton(
                &fixture.annotator,
                context(&assignment),
                None,
                &stale,
                skeleton(0.2),
                "stale-guide-save"
            )
            .await
            .is_err()
    );
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    for end in 0..=events.len() {
        rebuild_state(fixture.image_id.clone(), &events[..end]).unwrap();
    }
}
#[tokio::test]
async fn latest_historical_pass_recovers_after_restart_without_reopening_older_passes() {
    let fixture = fixture(ReviewWorkflow::None, 2).await;
    let assignment = claim_annotator(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let saved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&state, &fixture.task_id, &fixture.targets[0]),
            skeleton(0.3),
            "legacy-initial-save",
        )
        .await
        .unwrap();
    let resolved = fixture
        .repository
        .exclude_migration_target(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&saved.image_state, &fixture.task_id, &fixture.targets[1]),
            MigrationExclusionReason::NoValidSkeleton,
            None,
            "legacy-initial-exclude",
        )
        .await
        .unwrap();
    let target_hash = resolved.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let original_hash = resolved
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let original_confirmation = migration_confirmation_hash(&target_hash, &original_hash).unwrap();
    let first = fixture
        .repository
        .start_migration_pass(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &original_hash,
            "legacy-first-pass",
        )
        .await
        .unwrap();
    let first_id = first.active_pass.unwrap().pass_id;
    let mut state = first.image_state;
    for (index, target) in fixture.targets.iter().enumerate() {
        state = fixture
            .repository
            .keep_migration_target(
                &fixture.annotator,
                context(&assignment),
                &first_id,
                &expectation(&state, &fixture.task_id, target),
                &format!("legacy-first-keep-{index}"),
            )
            .await
            .unwrap()
            .image_state;
    }
    let original_pass = state.migration_passes[&first_id].clone();
    let second = fixture
        .repository
        .start_migration_pass(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &original_hash,
            "legacy-second-pass",
        )
        .await
        .unwrap();
    let latest_id = second.active_pass.unwrap().pass_id;
    let edited = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            Some(&latest_id),
            &expectation(&second.image_state, &fixture.task_id, &fixture.targets[0]),
            skeleton(0.6),
            "legacy-second-edit",
        )
        .await
        .unwrap();
    assert!(
        matches!(edited.cursor, MigrationCursor::Object { ref object_group_id, .. }
        if object_group_id == &fixture.targets[1].object_group_id)
    );
    assert_ne!(
        edited
            .image_state
            .migration_cursor(&fixture.task_id, Some(&first_id))
            .unwrap(),
        MigrationCursor::FullImage
    );

    let restarted = DatasetRepository::new(fixture._temp.path());
    assert_eq!(
        restarted
            .rebuild_image_state(&fixture.image_id)
            .await
            .unwrap(),
        edited.image_state
    );
    let loaded = restarted
        .current_manual_migration(&fixture.annotator, context(&assignment), Some(&latest_id))
        .await
        .unwrap();
    assert_eq!(loaded.cursor, edited.cursor);
    assert_eq!(
        assignment_pass(
            &loaded.image_state,
            &assignment.assignment_id,
            &fixture.task_id
        ),
        Some(&latest_id)
    );
    let current_hash = loaded
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let current_confirmation = migration_confirmation_hash(&target_hash, &current_hash).unwrap();
    let before = restarted.load_events(&fixture.image_id).await.unwrap();
    let error = restarted
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &current_hash,
            &current_confirmation,
            "legacy-incomplete",
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, StorageError::AssignmentConflict(message) if message == "migration correction pass is incomplete")
    );
    assert_eq!(
        restarted.load_events(&fixture.image_id).await.unwrap(),
        before
    );
    let completed = restarted
        .keep_migration_target(
            &fixture.annotator,
            context(&assignment),
            &latest_id,
            &expectation(&loaded.image_state, &fixture.task_id, &fixture.targets[1]),
            "legacy-resume-excluded",
        )
        .await
        .unwrap();
    assert_eq!(completed.cursor, MigrationCursor::FullImage);
    assert_eq!(
        completed.image_state.migration_passes[&first_id],
        original_pass
    );
    assert_eq!(
        completed.image_state.migration_passes[&latest_id]
            .items
            .len(),
        2
    );
    let before = restarted.load_events(&fixture.image_id).await.unwrap();
    let stale = restarted
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &original_hash,
            &original_confirmation,
            "legacy-stale-digest",
        )
        .await
        .unwrap_err();
    assert!(matches!(stale, StorageError::AssignmentConflict(message)
        if message == "migration confirmation digest is stale"));
    assert_eq!(
        restarted.load_events(&fixture.image_id).await.unwrap(),
        before
    );
    let submitted = restarted
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &current_hash,
            &current_confirmation,
            "legacy-submit",
        )
        .await
        .unwrap();
    assert_eq!(
        submitted.image_state.task_states[&fixture.task_id].status,
        TaskStatus::Completed
    );
    assert_eq!(
        submitted.image_state.migration_passes[&first_id],
        original_pass
    );
    assert_eq!(submitted.image_state.migration_passes.len(), 2);
    let events = restarted.load_events(&fixture.image_id).await.unwrap();
    for end in 0..=events.len() {
        rebuild_state(fixture.image_id.clone(), &events[..end]).unwrap();
    }
    assert_eq!(
        DatasetRepository::new(fixture._temp.path())
            .rebuild_image_state(&fixture.image_id)
            .await
            .unwrap(),
        submitted.image_state
    );
}
