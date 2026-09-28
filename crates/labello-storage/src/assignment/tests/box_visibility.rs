use super::*;

#[tokio::test]
async fn hidden_duplicate_survives_review_replay_restart_and_threshold_changes() {
    let (temp, repo, image_id, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let mut duplicate = repo
        .load_image_state(&image_id)
        .await
        .unwrap()
        .current_annotation(&"ann_1".into())
        .unwrap()
        .clone();
    duplicate.annotation_id = "ann_2".into();
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: annotator,
            role: DatasetRole::Annotator,
        },
        EventPayload::AnnotationVersionCreated {
            annotation: duplicate.clone(),
            previous_version: None,
            reason: None,
        },
    )
    .await
    .unwrap();
    let assignment = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let state = repo.load_image_state(&image_id).await.unwrap();
    assert_eq!(state.active_annotations().count(), 2);
    assert_eq!(state.visible_annotations().count(), 1);
    assert_eq!(
        state.review_assignment_contexts[&assignment.assignment_id]
            .targets
            .len(),
        2
    );
    let completed = finalize_test_review(&repo, &assignment, ReviewDecision::Approved).await;
    assert_eq!(
        completed.task_states[&task_id].status,
        TaskStatus::Completed
    );
    assert_eq!(
        completed.current_annotation(&duplicate.annotation_id),
        Some(&duplicate)
    );
    assert!(!completed.reviews.iter().any(|review| matches!(&review.target, ReviewTarget::AnnotationVersion { annotation_id, .. } if annotation_id == &duplicate.annotation_id)));
    let events = repo.load_events(&image_id).await.unwrap();
    for end in 0..=events.len() {
        labello_domain::rebuild_state(image_id.clone(), &events[..end]).unwrap();
    }
    assert_eq!(
        labello_domain::rebuild_state(image_id.clone(), &events).unwrap(),
        completed
    );
    tokio::fs::remove_file(repo.state_path(&image_id))
        .await
        .unwrap();
    let restarted = DatasetRepository::new(temp.path());
    assert_eq!(
        restarted.load_image_state(&image_id).await.unwrap(),
        completed
    );
    let mut metadata = restarted.load_dataset_config().await.unwrap();
    metadata.bounding_box_visibility.iou_threshold = 1.0;
    restarted.save_dataset(&metadata).await.unwrap();
    let exposed = restarted.load_image_state(&image_id).await.unwrap();
    assert_eq!(exposed.visible_annotations().count(), 2);
    assert_eq!(exposed.annotations, completed.annotations);
    assert_eq!(restarted.load_events(&image_id).await.unwrap(), events);
    // A policy update never reinterprets an already committed review during replay.
    assert_eq!(
        labello_domain::rebuild_state(image_id, &events).unwrap(),
        completed
    );
}

#[tokio::test]
async fn changed_visibility_rejects_stale_review_without_partial_events() {
    let (_temp, repo, image_id, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let mut duplicate = repo
        .load_image_state(&image_id)
        .await
        .unwrap()
        .current_annotation(&"ann_1".into())
        .unwrap()
        .clone();
    duplicate.annotation_id = "ann_2".into();
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: annotator,
            role: DatasetRole::Annotator,
        },
        EventPayload::AnnotationVersionCreated {
            annotation: duplicate,
            previous_version: None,
            reason: None,
        },
    )
    .await
    .unwrap();
    let assignment = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let before = repo.load_events(&image_id).await.unwrap();
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.bounding_box_visibility.iou_threshold = 1.0;
    repo.save_dataset(&metadata).await.unwrap();
    let result = repo
        .record_review_for_assignment(
            &reviewers[0],
            review_context(&assignment),
            ReviewRecord {
                review_id: ReviewId::generate(),
                target: ReviewTarget::AnnotationVersion {
                    annotation_id: "ann_1".into(),
                    version: 1,
                },
                reviewer_user_id: reviewers[0].clone(),
                decision: ReviewDecision::Approved,
                timestamp: now(),
                comment: None,
            },
        )
        .await;
    assert!(matches!(result, Err(StorageError::AssignmentConflict(_))));
    assert_eq!(repo.load_events(&image_id).await.unwrap(), before);
}

#[tokio::test]
async fn offline_event_visibility_cannot_override_server_policy() {
    let (_temp, repo, image_id, task_id, annotator, _) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let mut state = repo.load_image_state(&image_id).await.unwrap();
    let metadata = repo.load_dataset_config().await.unwrap();
    let task = metadata.task(&task_id).unwrap();
    let fingerprint = state.review_target_fingerprint(task);
    let mut historical = state.clone();
    historical.bounding_box_visibility = None;
    assert_eq!(historical.review_target_fingerprint(task), fingerprint);
    let mut duplicate = state.current_annotation(&"ann_1".into()).unwrap().clone();
    duplicate.annotation_id = "zz_offline_duplicate".into();
    let mut event = EventLogEntry::new(
        1,
        image_id.clone(),
        annotator,
        DatasetRole::Annotator,
        now(),
        EventPayload::AnnotationVersionCreated {
            annotation: duplicate,
            previous_version: None,
            reason: None,
        },
    );
    event.bounding_box_visibility =
        Some(labello_domain::BoundingBoxVisibility { iou_threshold: 1.0 });
    repo.append_resequenced_events(&image_id, &mut state, &[event])
        .await
        .unwrap();
    assert_eq!(state.visible_annotations().count(), 1);
    assert_eq!(state.active_annotations().count(), 2);
    let events = repo.load_events(&image_id).await.unwrap();
    assert_eq!(
        events.last().unwrap().bounding_box_visibility,
        Some(Default::default())
    );
    assert_eq!(
        labello_domain::rebuild_state(image_id, &events).unwrap(),
        state
    );
}
