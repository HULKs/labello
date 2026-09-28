use super::*;

#[tokio::test]
async fn overlapping_guide_skips_pending_target_and_migration_completes_with_replay() {
    let fixture = fixture(ReviewWorkflow::Approval, 2).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let first = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let mut second = state
        .current_annotation(&fixture.targets[1].guide_annotation_id)
        .unwrap()
        .clone();
    second.geometry = first.geometry.clone();
    second.version += 1;
    fixture
        .repository
        .append_payload(
            &fixture.image_id,
            &Actor {
                user_id: fixture.annotator.clone(),
                role: DatasetRole::DataAdmin,
            },
            EventPayload::AnnotationVersionCreated {
                annotation: second.clone(),
                previous_version: Some(1),
                reason: None,
            },
        )
        .await
        .unwrap();
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
            skeleton(0.2),
            "visible-guide",
        )
        .await
        .unwrap();
    assert_eq!(saved.cursor, MigrationCursor::FullImage);
    assert_eq!(saved.progress.expected, 1);
    assert_eq!(saved.progress.pending, 0);
    assert!(matches!(
        saved.image_state.migration_dispositions[&fixture.task_id]
            [&fixture.targets[1].object_group_id]
            .status,
        MigrationDispositionStatus::Pending
    ));
    assert_eq!(
        saved.image_state.current_annotation(&second.annotation_id),
        Some(&second)
    );
    let target_hash = saved.image_state.migration_target_sets[&fixture.task_id]
        .target_set_hash
        .clone();
    let state_hash = saved
        .image_state
        .current_migration_state_hash(&fixture.task_id)
        .unwrap();
    let confirmation_hash = migration_confirmation_hash(&target_hash, &state_hash).unwrap();
    fixture
        .repository
        .confirm_and_submit_migration(
            &fixture.annotator,
            context(&assignment),
            &target_hash,
            &state_hash,
            &confirmation_hash,
            "confirm-visible-guide",
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
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let metadata = fixture.repository.load_dataset_config().await.unwrap();
    assert_eq!(
        state
            .review_object_targets(metadata.task(&fixture.task_id).unwrap())
            .unwrap()
            .len(),
        1
    );
    let disposition_version = state.migration_dispositions[&fixture.task_id]
        [&fixture.targets[0].object_group_id]
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
            "review-visible-guide",
        )
        .await
        .unwrap();
    let completed = fixture
        .repository
        .review_migration(
            &fixture.reviewers[0],
            context(&review),
            &MigrationReviewTarget::Confirmation { confirmation_hash },
            ReviewDecision::Approved,
            None,
            "review-visible-final",
        )
        .await
        .unwrap();
    assert_eq!(
        completed.image_state.task_states[&fixture.task_id].status,
        TaskStatus::Completed
    );
    assert!(
        completed
            .image_state
            .current_annotation(&fixture.targets[1].reserved_skeleton_annotation_id)
            .is_none()
    );
    assert!(
        completed
            .image_state
            .current_annotation(&fixture.targets[0].reserved_skeleton_annotation_id)
            .is_some()
    );
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    for end in 0..=events.len() {
        rebuild_state(fixture.image_id.clone(), &events[..end]).unwrap();
    }
    assert_eq!(
        rebuild_state(fixture.image_id, &events).unwrap(),
        completed.image_state
    );
}

#[tokio::test]
async fn hidden_guide_preserves_existing_skeleton_and_its_review_target() {
    let fixture = fixture(ReviewWorkflow::Approval, 2).await;
    let assignment = claim_annotator(&fixture).await;
    let state = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let state = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&state, &fixture.task_id, &fixture.targets[0]),
            skeleton(0.2),
            "first-skeleton",
        )
        .await
        .unwrap()
        .image_state;
    let saved = fixture
        .repository
        .save_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            &expectation(&state, &fixture.task_id, &fixture.targets[1]),
            skeleton(0.6),
            "existing-skeleton",
        )
        .await
        .unwrap();
    let existing = saved
        .image_state
        .current_annotation(&fixture.targets[1].reserved_skeleton_annotation_id)
        .unwrap()
        .clone();
    // Overlay the new policy on the same saved image after the guide becomes a duplicate.
    // The disposition is already Annotated, so hiding its guide cannot skip its skeleton.
    let mut state = saved.image_state;
    let first_geometry = state
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap()
        .geometry
        .clone();
    state
        .annotations
        .get_mut(&fixture.targets[1].guide_annotation_id)
        .unwrap()
        .last_mut()
        .unwrap()
        .geometry = first_geometry;
    assert!(
        state
            .bounding_box_exclusions()
            .contains_key(&fixture.targets[1].guide_annotation_id)
    );
    assert!(state.skipped_migration_groups(&fixture.task_id).is_empty());
    assert_eq!(
        state.current_annotation(&existing.annotation_id),
        Some(&existing)
    );
    assert!(
        state
            .visible_annotations()
            .any(|annotation| annotation == &existing)
    );
    let metadata = fixture.repository.load_dataset_config().await.unwrap();
    assert!(state.review_object_targets(metadata.task(&fixture.task_id).unwrap()).unwrap().iter().any(|target| matches!(target, labello_domain::ReviewTarget::AnnotationVersion { annotation_id, version } if annotation_id == &existing.annotation_id && *version == existing.version)));
}
