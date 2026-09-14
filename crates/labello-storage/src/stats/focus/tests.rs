use super::*;
use labello_domain::{
    AnnotationType, ClassId, DatasetId, DatasetMetadata, ImageId, ImageRecord, ImagesIndex,
    ReviewConfig, SCHEMA_VERSION, TaskDefinition, TaskId, TutorialContent,
};

#[tokio::test]
async fn focus_is_persisted_across_restart_changes_only_at_boundary_and_rejects_corruption() {
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
    let (initial, concurrent) =
        tokio::join!(repo.scoring_focus(timestamp), repo.scoring_focus(timestamp));
    let initial = initial.unwrap();
    assert_eq!(initial, concurrent.unwrap());
    assert_eq!(initial[0].task_id, Some(TaskId::from("a")));
    assert_eq!(initial[0].starts_at, timestamp);
    let boundary: Timestamp = "2026-01-02T12:20:00Z".parse().unwrap();
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
    assert_eq!(reopened.scoring_focus(timestamp).await.unwrap(), initial);
    let next = reopened.scoring_focus(boundary).await.unwrap();
    assert_eq!(next[1].task_id, Some(TaskId::from("b")));
    assert_eq!(next[1].starts_at, boundary);
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
