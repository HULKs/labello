use super::*;
use labello_domain::{
    AnnotationType, ClassId, DatasetId, DatasetMetadata, ImageId, ImageRecord, ImagesIndex,
    ReviewConfig, SCHEMA_VERSION, TaskDefinition, TaskId, TutorialContent,
};
use std::time::Duration;

async fn repository() -> (tempfile::TempDir, DatasetRepository, Timestamp) {
    let directory = tempfile::tempdir().unwrap();
    let repo = DatasetRepository::new(directory.path());
    let timestamp: Timestamp = "2026-01-02T12:03:00Z".parse().unwrap();
    let mut metadata = DatasetMetadata::new(DatasetId::from("dataset"), "Dataset", timestamp);
    for id in ["a", "b"] {
        metadata.tasks.push(TaskDefinition {
            task_id: TaskId::from(id),
            name: id.into(),
            annotation_type: AnnotationType::BoundingBox,
            class_ids: vec![ClassId::from(id)],
            instructions: TutorialContent {
                title: id.into(),
                example_text: String::new(),
                example_images: Vec::new(),
            },
            skeleton: None,
            review: ReviewConfig::default(),
            prelabel_config_ids: Vec::new(),
            manual_box_guide_migration: None,
            enabled: true,
        });
    }
    repo.initialize(metadata).await.unwrap();
    let image = ImageRecord {
        image_id: ImageId::from("image"),
        blake3: "hash".into(),
        canonical_path: "images/test.png".into(),
        known_paths: Vec::new(),
        duplicate_paths: Vec::new(),
        file_name: "test.png".into(),
        byte_size: 1,
        width: 1,
        height: 1,
        media_type: "image/png".into(),
        source_memberships: None,
    };
    repo.save_images_index(&ImagesIndex {
        schema_version: SCHEMA_VERSION,
        image_count: 1,
        images_by_hash: std::collections::BTreeMap::from([("hash".into(), image)]),
    })
    .await
    .unwrap();
    (directory, repo, timestamp)
}

#[tokio::test]
async fn focus_is_rolling_persisted_and_rejects_corruption() {
    let (directory, repo, timestamp) = repository().await;
    let (initial, concurrent) =
        tokio::join!(repo.scoring_focus(timestamp), repo.scoring_focus(timestamp));
    let initial = initial.unwrap();
    assert_eq!(initial, concurrent.unwrap());
    assert_eq!(initial[0].task_id, Some(TaskId::from("a")));
    assert_eq!(initial[0].starts_at, timestamp);
    let boundary: Timestamp = "2026-01-02T12:13:00Z".parse().unwrap();
    assert_eq!(initial[0].ends_at, boundary);
    let submitted_at: Timestamp = "2026-01-02T12:05:00Z".parse().unwrap();
    let mut task_state = labello_domain::TaskState::new(TaskId::from("a"), submitted_at);
    task_state.status = TaskStatus::Submitted;
    task_state.completed_by = Some(labello_domain::UserId::from("author"));
    task_state.completed_at = Some(submitted_at);
    repo.append_events_atomic(
        &ImageId::from("image"),
        &[labello_domain::EventLogEntry::new(
            1,
            ImageId::from("image"),
            labello_domain::UserId::from("author"),
            labello_domain::DatasetRole::Annotator,
            submitted_at,
            labello_domain::EventPayload::TaskStateChanged { task_state },
        )],
    )
    .await
    .unwrap();
    let reopened = DatasetRepository::new(directory.path());
    assert_eq!(reopened.scoring_focus(submitted_at).await.unwrap(), initial);
    let selected_at = boundary + Duration::from_secs(17);
    let next = reopened.scoring_focus(selected_at).await.unwrap();
    assert_eq!(next[1].task_id, Some(TaskId::from("b")));
    assert_eq!(next[1].starts_at, selected_at);
    assert_eq!(next[1].ends_at, selected_at + Duration::from_secs(600));
    assert_eq!(next[0], initial[0]);
    let snapshot = reopened.create_snapshot().await.unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == ".labello/scoring/focus-v1.json")
    );
    let path = directory.path().join(".labello/scoring/focus-v1.json");
    write_json_atomic(&path, &serde_json::json!({"version":99,"windows":[]}))
        .await
        .unwrap();
    assert!(
        DatasetRepository::new(directory.path())
            .scoring_focus(boundary)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn legacy_focus_upgrade_preserves_past_and_restarts_ten_minutes() {
    let (directory, repo, timestamp) = repository().await;
    let legacy = FocusHistory {
        version: 1,
        review_windows: Vec::new(),
        windows: vec![
            FocusWindow {
                starts_at: "2026-01-02T11:40:00Z".parse().unwrap(),
                ends_at: "2026-01-02T12:00:00Z".parse().unwrap(),
                task_id: Some(TaskId::from("b")),
            },
            FocusWindow {
                starts_at: "2026-01-02T12:00:00Z".parse().unwrap(),
                ends_at: "2026-01-02T12:20:00Z".parse().unwrap(),
                task_id: Some(TaskId::from("a")),
            },
        ],
    };
    let path = directory.path().join(".labello/scoring/focus-v1.json");
    let mut legacy_wire = serde_json::to_value(&legacy).unwrap();
    legacy_wire.as_object_mut().unwrap().remove("reviewWindows");
    write_json_atomic(&path, &legacy_wire).await.unwrap();
    let upgraded = repo.scoring_focus(timestamp).await.unwrap();
    assert_eq!(upgraded.len(), 3);
    assert_eq!(upgraded[0], legacy.windows[0]);
    assert_eq!(upgraded[1].starts_at, legacy.windows[1].starts_at);
    assert_eq!(upgraded[1].ends_at, timestamp);
    assert_eq!(upgraded[1].task_id, legacy.windows[1].task_id);
    assert_eq!(upgraded[2].task_id, Some(TaskId::from("a")));
    assert_eq!(upgraded[2].starts_at, timestamp);
    assert_eq!(upgraded[2].ends_at, timestamp + Duration::from_secs(600));
    let saved: FocusHistory = read_json(&path).await.unwrap();
    assert_eq!(saved.version, FOCUS_HISTORY_VERSION);
    assert_eq!(saved.windows, upgraded);
    assert!(saved.review_windows.is_empty());
    let reopened = DatasetRepository::new(directory.path());
    assert_eq!(reopened.scoring_focus(timestamp).await.unwrap(), upgraded);
    assert_eq!(
        reopened
            .scoring_focus(timestamp + Duration::from_secs(60))
            .await
            .unwrap(),
        upgraded
    );
}

#[tokio::test]
async fn imbalance_switch_preserves_submission_focus_and_resets_full_period() {
    let (directory, repo, _) = repository().await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.imbalance = Some(labello_domain::ImbalanceConfig {
        max_difference: 0,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();
    let timestamp = labello_domain::now() - Duration::from_secs(60);
    let initial = repo.scoring_focus(timestamp).await.unwrap();
    assert_eq!(initial[0].task_id, Some(TaskId::from("a")));
    let mut task_state = labello_domain::TaskState::new(TaskId::from("a"), timestamp);
    task_state.status = TaskStatus::Submitted;
    task_state.completed_by = Some(labello_domain::UserId::from("author"));
    task_state.completed_at = Some(labello_domain::now());
    let event = repo
        .append_payload(
            &ImageId::from("image"),
            &labello_domain::Actor {
                user_id: labello_domain::UserId::from("author"),
                role: labello_domain::DatasetRole::Annotator,
            },
            labello_domain::EventPayload::TaskStateChanged { task_state },
        )
        .await
        .unwrap();
    // The transaction prepared focus before publishing the imbalance-changing event.
    let committed: FocusHistory =
        read_json(&directory.path().join(".labello/scoring/focus-v1.json"))
            .await
            .unwrap();
    assert_eq!(committed.windows, initial);
    let switched_at = event.timestamp + Duration::from_secs(1);
    let switched = repo.scoring_focus(switched_at).await.unwrap();
    assert_eq!(switched.len(), 2);
    assert!(switched[0].contains(event.timestamp));
    assert_eq!(switched[0].ends_at, switched_at);
    assert_eq!(switched[1].task_id, Some(TaskId::from("b")));
    assert_eq!(switched[1].starts_at, switched_at);
    assert_eq!(switched[1].ends_at, switched_at + Duration::from_secs(600));
    // Identical and older timestamps cannot create zero-length or overlapping periods.
    assert_eq!(repo.scoring_focus(switched_at).await.unwrap(), switched);
    assert_eq!(repo.scoring_focus(timestamp).await.unwrap(), switched);
    let reopened = DatasetRepository::new(directory.path());
    assert_eq!(
        reopened
            .scoring_focus(switched_at + Duration::from_secs(1))
            .await
            .unwrap(),
        switched
    );
}

#[tokio::test]
async fn reviewer_focus_has_separate_eligibility_timer_and_durable_activation() {
    let (directory, repo, _) = repository().await;
    let mut metadata = repo.load_dataset().await.unwrap();
    let mut no_review = metadata.tasks[0].clone();
    no_review.task_id = TaskId::from("0-no-review");
    no_review.review.workflow = labello_domain::ReviewWorkflow::None;
    metadata.tasks.push(no_review);
    metadata.imbalance = Some(labello_domain::ImbalanceConfig {
        max_difference: 0,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();
    let image = ImageId::from("image");
    let submitted_at = labello_domain::now() - Duration::from_secs(120);
    let events = ["a", "b", "0-no-review"]
        .into_iter()
        .enumerate()
        .map(|(index, task)| {
            let mut task_state = labello_domain::TaskState::new(TaskId::from(task), submitted_at);
            task_state.status = TaskStatus::Submitted;
            labello_domain::EventLogEntry::new(
                index as u64 + 1,
                image.clone(),
                labello_domain::UserId::from("author"),
                labello_domain::DatasetRole::Annotator,
                submitted_at,
                EventPayload::TaskStateChanged { task_state },
            )
        })
        .collect::<Vec<_>>();
    repo.append_events_atomic(&image, &events).await.unwrap();
    let annotation = repo
        .scoring_focus(labello_domain::now() - Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(annotation[0].task_id, None);
    let actor = labello_domain::Actor {
        user_id: labello_domain::UserId::from("reviewer"),
        role: labello_domain::DatasetRole::Reviewer,
    };
    let review = repo
        .append_payload(
            &image,
            &actor,
            EventPayload::ReviewRecorded {
                review: labello_domain::ReviewRecord {
                    review_id: labello_domain::ReviewId::from("review"),
                    target: labello_domain::ReviewTarget::Task {
                        task_id: TaskId::from("a"),
                    },
                    reviewer_user_id: actor.user_id.clone(),
                    decision: labello_domain::ReviewDecision::Approved,
                    timestamp: labello_domain::now(),
                    comment: None,
                },
            },
        )
        .await
        .unwrap();
    let path = directory.path().join(".labello/scoring/focus-v1.json");
    let saved: FocusHistory = read_json(&path).await.unwrap();
    assert_eq!(saved.windows, annotation);
    assert_eq!(saved.review_windows.len(), 1);
    assert_eq!(saved.review_windows[0].task_id, Some(TaskId::from("a")));
    assert_eq!(saved.review_windows[0].starts_at, review.timestamp);
    assert_eq!(
        saved.review_windows[0].ends_at,
        review.timestamp + Duration::from_secs(600)
    );
    assert!(!saved.review_windows[0].contains(submitted_at));

    let mut task_state = labello_domain::TaskState::new(TaskId::from("a"), review.timestamp);
    task_state.status = TaskStatus::Completed;
    task_state.outcome = Some(TaskOutcome::Approved);
    let completion = repo
        .append_payload(
            &image,
            &actor,
            EventPayload::TaskStateChanged { task_state },
        )
        .await
        .unwrap();
    let switched_at = completion.timestamp + Duration::from_secs(1);
    let switched = repo.review_scoring_focus(switched_at).await.unwrap();
    assert_eq!(switched.len(), 2);
    assert_eq!(switched[0].ends_at, switched_at);
    assert!(switched[0].contains(review.timestamp));
    assert_eq!(switched[1].task_id, Some(TaskId::from("b")));
    assert_eq!(switched[1].starts_at, switched_at);
    assert_eq!(switched[1].ends_at, switched_at + Duration::from_secs(600));
    assert_eq!(repo.scoring_focus(switched_at).await.unwrap(), annotation);
    let reopened = DatasetRepository::new(directory.path());
    assert_eq!(
        reopened
            .review_scoring_focus(switched_at + Duration::from_secs(1))
            .await
            .unwrap(),
        switched
    );
    let mut saved: FocusHistory = read_json(&path).await.unwrap();
    saved.review_windows[0].ends_at = switched_at + Duration::from_secs(1);
    write_json_atomic(&path, &saved).await.unwrap();
    assert!(
        DatasetRepository::new(directory.path())
            .review_scoring_focus(switched_at)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn score_receipt_rejects_a_known_stale_scan_even_when_sequence_is_covered() {
    let (_directory, repo, _) = repository().await;
    let pause = repo.stats_cache.pause_after_next_scan().await;
    let worker = repo.clone();
    let receipt = tokio::spawn(async move {
        worker
            .image_score(
                &ImageId::from("image"),
                &labello_domain::UserId::from("author"),
                0,
                0,
            )
            .await
            .unwrap()
    });
    pause.started.notified().await;
    repo.stats_cache.invalidate();
    pause.resume.notify_one();
    assert_eq!(receipt.await.unwrap(), None);
    assert_eq!(
        repo.image_score(
            &ImageId::from("image"),
            &labello_domain::UserId::from("author"),
            0,
            0
        )
        .await
        .unwrap(),
        Some(0)
    );
}
