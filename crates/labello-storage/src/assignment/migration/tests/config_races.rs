use std::{future::Future, sync::Arc, task::Poll, time::Duration};

use super::*;

#[derive(Clone, Copy, Debug)]
enum CompanionCommand {
    Add,
    Edit,
    Delete,
    Reconcile,
}

const COMMANDS: [CompanionCommand; 4] = [
    CompanionCommand::Add,
    CompanionCommand::Edit,
    CompanionCommand::Delete,
    CompanionCommand::Reconcile,
];

#[tokio::test]
async fn admin_repair_serializes_role_revocation_and_rechecks_later_requests() {
    let fixture = fixture(ReviewWorkflow::None, 1).await;
    let before = fixture
        .repository
        .load_image_state(&fixture.image_id)
        .await
        .unwrap();
    let guide = before
        .current_annotation(&fixture.targets[0].guide_annotation_id)
        .unwrap();
    let mut annotation = guide.clone();
    annotation.version += 1;
    annotation.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    annotation.author_user_id = UserId::from("admin");
    annotation.updated_at = labello_domain::now();
    let payload = EventPayload::AnnotationVersionCreated {
        annotation,
        previous_version: Some(guide.version),
        reason: Some("admin repair".into()),
    };
    let captured = Arc::new(tokio::sync::Notify::new());
    *fixture.repository.migration_config_captured.lock() = Some(captured.clone());
    let image_lock = fixture.repository.image_lock(&fixture.image_id);
    let image_guard = image_lock.lock().await;
    let repo = fixture.repository.clone();
    let image = fixture.image_id.clone();
    let retry_payload = payload.clone();
    let pending = tokio::spawn(async move {
        repo.append_admin_repair_payload(
            &UserId::from("admin"),
            &image,
            before.current_sequence,
            payload,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), captured.notified())
        .await
        .unwrap();
    assert!(fixture.repository.review_config_lock.try_write().is_err());
    let mut metadata = fixture.repository.load_dataset_config().await.unwrap();
    metadata
        .role_assignments
        .retain(|role| role.user_id != UserId::from("admin"));
    assert!(fixture.repository.review_config_lock.try_read().is_ok());
    let mut publication = std::pin::pin!(fixture.repository.save_dataset(&metadata));
    std::future::poll_fn(|cx| {
        assert!(publication.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(
        fixture.repository.review_config_lock.try_read().is_err(),
        "configuration publication must queue its writer on the same guard"
    );
    drop(image_guard);
    tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), publication)
        .await
        .unwrap()
        .unwrap();
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    assert!(
        fixture
            .repository
            .append_admin_repair_payload(
                &UserId::from("admin"),
                &fixture.image_id,
                events.len() as u64,
                retry_payload,
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
        events
    );
}

async fn prepare() -> (Fixture, Assignment, AnnotationId) {
    let fixture = fixture(ReviewWorkflow::None, 0).await;
    let assignment = claim_annotator(&fixture).await;
    let added = fixture
        .repository
        .add_migration_skeleton(
            &fixture.annotator,
            context(&assignment),
            None,
            skeleton(0.7),
            "config-race-prepare",
        )
        .await
        .unwrap();
    (fixture, assignment, added.annotation_id.unwrap())
}

async fn run_command(
    command: CompanionCommand,
    repo: &DatasetRepository,
    actor: &UserId,
    assignment: &Assignment,
    annotation: &AnnotationId,
) -> StorageResult<ManualMigrationCommandResult> {
    match command {
        CompanionCommand::Add => {
            repo.add_migration_skeleton(
                actor,
                context(assignment),
                None,
                skeleton(0.8),
                "config-race-add",
            )
            .await
        }
        CompanionCommand::Edit => {
            repo.edit_migration_skeleton(
                actor,
                context(assignment),
                None,
                annotation,
                1,
                skeleton(0.8),
                "config-race-edit",
            )
            .await
        }
        CompanionCommand::Delete => {
            repo.delete_migration_skeleton(
                actor,
                context(assignment),
                None,
                annotation,
                1,
                "config-race-delete",
            )
            .await
        }
        CompanionCommand::Reconcile => {
            repo.reconcile_migration_companion(
                actor,
                context(assignment),
                None,
                annotation,
                1,
                Some(1),
                "config-race-reconcile",
            )
            .await
        }
    }
}

#[tokio::test]
async fn configuration_publication_waits_for_each_companion_transaction() {
    for command in COMMANDS {
        let (fixture, assignment, annotation) = prepare().await;
        let before = fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap();
        let captured = Arc::new(tokio::sync::Notify::new());
        *fixture.repository.migration_config_captured.lock() = Some(captured.clone());
        let image_lock = fixture.repository.image_lock(&fixture.image_id);
        let image_guard = image_lock.lock().await;
        let repo = fixture.repository.clone();
        let actor = fixture.annotator.clone();
        let pending = tokio::spawn(async move {
            run_command(command, &repo, &actor, &assignment, &annotation).await
        });
        tokio::time::timeout(Duration::from_secs(5), captured.notified())
            .await
            .expect("migration captured configuration before waiting on image");
        assert!(
            fixture.repository.review_config_lock.try_write().is_err(),
            "{command:?}: captured configuration is not protected during the image wait"
        );
        let mut metadata = fixture.repository.load_dataset_config().await.unwrap();
        metadata
            .tasks
            .iter_mut()
            .find(|task| task.task_id == fixture.guide_task_id)
            .unwrap()
            .enabled = false;
        assert!(fixture.repository.review_config_lock.try_read().is_ok());
        let mut publication = std::pin::pin!(fixture.repository.save_dataset(&metadata));
        // Poll the production writer while the image transaction is waiting.
        // A timer would only prove the scheduler had not run it yet.
        std::future::poll_fn(|cx| {
            assert!(
                publication.as_mut().poll(cx).is_pending(),
                "{command:?}: configuration published while migration held its captured inputs"
            );
            Poll::Ready(())
        })
        .await;
        assert!(
            fixture.repository.review_config_lock.try_read().is_err(),
            "{command:?}: configuration publication must queue its writer on the same guard"
        );
        assert!(
            fixture
                .repository
                .load_dataset_config()
                .await
                .unwrap()
                .task(&fixture.guide_task_id)
                .unwrap()
                .enabled
        );
        assert_eq!(
            fixture
                .repository
                .load_events(&fixture.image_id)
                .await
                .unwrap(),
            before
        );
        drop(image_guard);
        let result = tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .expect("migration must finish without lock inversion")
            .unwrap()
            .unwrap();
        assert!(result.image_state.current_sequence > before.len() as u64);
        tokio::time::timeout(Duration::from_secs(5), publication)
            .await
            .expect("publication must resume after migration releases configuration")
            .unwrap();
        assert!(
            !fixture
                .repository
                .load_dataset_config()
                .await
                .unwrap()
                .task(&fixture.guide_task_id)
                .unwrap()
                .enabled
        );
        let events = fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap();
        assert_eq!(
            rebuild_state(fixture.image_id.clone(), &events).unwrap(),
            result.image_state
        );
    }
}

#[tokio::test]
async fn each_companion_transaction_rejects_published_configuration_and_role_changes() {
    for command in COMMANDS {
        for change in [
            "guide-disabled",
            "guide-type",
            "guide-class",
            "task-disabled",
            "role-revoked",
        ] {
            let (fixture, assignment, annotation) = prepare().await;
            let before = fixture
                .repository
                .load_events(&fixture.image_id)
                .await
                .unwrap();
            let mut metadata = fixture.repository.load_dataset_config().await.unwrap();
            match change {
                "guide-disabled" => {
                    metadata
                        .tasks
                        .iter_mut()
                        .find(|t| t.task_id == fixture.guide_task_id)
                        .unwrap()
                        .enabled = false
                }
                "guide-type" => {
                    metadata
                        .tasks
                        .iter_mut()
                        .find(|t| t.task_id == fixture.guide_task_id)
                        .unwrap()
                        .annotation_type = AnnotationType::Skeleton
                }
                "guide-class" => metadata
                    .tasks
                    .iter_mut()
                    .find(|t| t.task_id == fixture.guide_task_id)
                    .unwrap()
                    .class_ids
                    .clear(),
                "task-disabled" => {
                    metadata
                        .tasks
                        .iter_mut()
                        .find(|t| t.task_id == fixture.task_id)
                        .unwrap()
                        .enabled = false
                }
                "role-revoked" => metadata
                    .role_assignments
                    .retain(|r| r.user_id != fixture.annotator),
                _ => unreachable!(),
            }
            fixture.repository.save_dataset(&metadata).await.unwrap();
            let result = run_command(
                command,
                &fixture.repository,
                &fixture.annotator,
                &assignment,
                &annotation,
            )
            .await;
            assert!(result.is_err(), "{command:?} accepted {change}");
            assert_eq!(
                fixture
                    .repository
                    .load_events(&fixture.image_id)
                    .await
                    .unwrap(),
                before,
                "{command:?} appended after {change}"
            );
        }
    }
}
