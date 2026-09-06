use super::*;
use labello_domain::{Actor, DatasetRole, EventPayload, ImageId, TaskId, TaskState, TaskStatus};

#[tokio::test]
async fn daily_activity_cache_coalesces_invalidates_and_rebuilds_after_restart() {
    let (temp, repository) = crate::stats::tests::empty_repository().await;
    let image = labello_domain::ImageRecord {
        image_id: ImageId::from("image"),
        blake3: "hash".into(),
        canonical_path: "one.png".into(),
        known_paths: vec!["one.png".into()],
        duplicate_paths: vec![],
        file_name: "one.png".into(),
        byte_size: 1,
        width: 1,
        height: 1,
        media_type: "image/png".into(),
        source_memberships: None,
    };
    repository
        .save_images_index(&labello_domain::ImagesIndex {
            schema_version: labello_domain::SCHEMA_VERSION,
            image_count: 1,
            images_by_hash: BTreeMap::from([("hash".into(), image)]),
        })
        .await
        .unwrap();
    let at = labello_domain::now();
    let window = UtcActivityWindow::containing(at);
    let alice = UserId::from("alice");
    let bob = UserId::from("bob");
    repository.reset_event_load_count();
    let (a, b) = tokio::join!(
        repository.daily_activity(&alice, window),
        repository.daily_activity(&bob, window)
    );
    assert_eq!(a.unwrap(), DailyActivityCounts::default());
    assert_eq!(b.unwrap(), DailyActivityCounts::default());
    assert_eq!(repository.event_load_count(), 1);
    let payload = EventPayload::TaskStateChanged {
        task_state: TaskState {
            task_id: TaskId::from("boxes"),
            status: TaskStatus::Submitted,
            outcome: None,
            assigned_to: Some(alice.clone()),
            completed_by: Some(alice.clone()),
            completed_at: Some(at),
            updated_at: at,
        },
    };
    let actor = Actor {
        user_id: alice.clone(),
        role: DatasetRole::Annotator,
    };
    repository
        .append_payload(&ImageId::from("image"), &actor, payload.clone())
        .await
        .unwrap();
    repository
        .append_payload(&ImageId::from("image"), &actor, payload)
        .await
        .unwrap();
    let counts = repository.daily_activity(&alice, window).await.unwrap();
    assert_eq!(counts.annotation_tasks_submitted, 1);
    assert_eq!(
        repository.daily_activity(&bob, window).await.unwrap(),
        DailyActivityCounts::default()
    );
    repository
        .append_payload(
            &ImageId::from("image"),
            &actor,
            EventPayload::TaskStateChanged {
                task_state: TaskState::new(TaskId::from("boxes"), at),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        repository.daily_activity(&alice, window).await.unwrap(),
        counts
    );
    let reopened = DatasetRepository::new(temp.path());
    assert_eq!(
        reopened.daily_activity(&alice, window).await.unwrap(),
        counts
    );
    assert_eq!(
        reopened
            .daily_activity(&alice, UtcActivityWindow::containing(window.end))
            .await
            .unwrap(),
        DailyActivityCounts::default()
    );
    let (_other_root, other) = crate::stats::tests::empty_repository().await;
    assert_eq!(
        other.daily_activity(&alice, window).await.unwrap(),
        DailyActivityCounts::default()
    );
}
