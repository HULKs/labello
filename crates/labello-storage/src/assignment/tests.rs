use std::collections::{BTreeMap, BTreeSet};

use labello_domain::{
    AnnotationGeometry, AnnotationOrigin, AnnotationType, AnnotationVersion, BoundingBox, ClassId,
    DatasetId, DatasetMetadata, DatasetRoleAssignment, HumanRevisionKind, ImageRecord, ImagesIndex,
    ImbalanceConfig, ImportCoverage, ImportId, ImportTaskInitialization, KeypointAnnotation,
    KeypointSpec, KeypointState, LabelClass, LegacyAdjudicationDecision, LegacyAdjudicationId,
    LegacyAdjudicationRecord, NormalizedPoint, ReviewConfig, ReviewDecision, ReviewId,
    ReviewRecord, ReviewTarget, ReviewWorkflow, SCHEMA_VERSION, SkeletonGeometry, SkeletonSpec,
    TaskDefinition, TutorialContent, now,
};

use super::*;

#[test]
fn current_reviews_begin_after_the_latest_submission() {
    let image_id = ImageId::from("img_1");
    let task_id = TaskId::from("bounding_box:person");
    let reviewer = UserId::from("reviewer");
    let timestamp = now();
    let task_state = |status| EventPayload::TaskStateChanged {
        task_state: TaskState {
            task_id: task_id.clone(),
            status,
            outcome: None,
            assigned_to: None,
            completed_by: None,
            completed_at: None,
            updated_at: timestamp,
        },
    };
    let review = |review_id: &str| ReviewRecord {
        review_id: ReviewId::from(review_id),
        target: ReviewTarget::Task {
            task_id: task_id.clone(),
        },
        reviewer_user_id: reviewer.clone(),
        decision: ReviewDecision::Approved,
        timestamp,
        comment: None,
    };
    let payloads = [
        task_state(TaskStatus::Submitted),
        EventPayload::ReviewRecorded {
            review: review("old_review"),
        },
        task_state(TaskStatus::NeedsCorrection),
        task_state(TaskStatus::Submitted),
        EventPayload::ReviewRecorded {
            review: review("current_review"),
        },
    ];
    let events = payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            EventLogEntry::new(
                index as u64 + 1,
                image_id.clone(),
                reviewer.clone(),
                DatasetRole::Reviewer,
                timestamp,
                payload,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        current_task_reviews(&events, &task_id),
        vec![review("current_review")]
    );
}

#[tokio::test]
async fn assignment_availability_caches_single_pass_scans_and_invalidates_on_writes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = DatasetRepository::new(temp.path());
    let user_id = UserId::from("annotator");
    let task_specs = [
        ("bounding_box:person", "person", "Person"),
        ("bounding_box:vehicle", "vehicle", "Vehicle"),
        ("bounding_box:ball", "ball", "Ball"),
    ];
    let mut metadata = DatasetMetadata::new(DatasetId::from("ds"), "Dataset", now());
    for (task_id, class_id, name) in task_specs {
        metadata.label_classes.push(LabelClass {
            class_id: ClassId::from(class_id),
            name: name.to_string(),
            color: "#5eead4".to_string(),
            description: None,
        });
        metadata.tasks.push(TaskDefinition {
            task_id: TaskId::from(task_id),
            name: format!("{name} boxes"),
            annotation_type: AnnotationType::BoundingBox,
            class_ids: vec![ClassId::from(class_id)],
            instructions: TutorialContent {
                title: format!("Annotate {name}"),
                example_text: "Draw boxes.".to_string(),
                example_images: Vec::new(),
            },
            skeleton: None,
            review: ReviewConfig::default(),
            prelabel_config_ids: Vec::new(),
            manual_box_guide_migration: None,
            enabled: true,
        });
    }
    metadata.role_assignments.push(DatasetRoleAssignment {
        dataset_id: metadata.dataset_id.clone(),
        user_id: user_id.clone(),
        roles: BTreeSet::from([DatasetRole::Annotator, DatasetRole::Reviewer]),
        assigned_at: now(),
        assigned_by: None,
    });
    repo.initialize(metadata.clone()).await.unwrap();

    let images = (0..4)
        .map(|index| {
            let image_id = ImageId::from(format!("img_{index}"));
            (
                format!("hash_{index}"),
                ImageRecord {
                    image_id,
                    blake3: format!("hash_{index}"),
                    canonical_path: format!("images/{index}.png"),
                    known_paths: vec![format!("images/{index}.png")],
                    duplicate_paths: Vec::new(),
                    file_name: format!("{index}.png"),
                    byte_size: 4,
                    width: 2,
                    height: 2,
                    media_type: "image/png".to_string(),
                    source_memberships: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    repo.save_images_index(&ImagesIndex {
        schema_version: SCHEMA_VERSION,
        image_count: images.len(),
        images_by_hash: images.clone(),
    })
    .await
    .unwrap();

    let timestamp = now();
    let actor = Actor {
        user_id: user_id.clone(),
        role: DatasetRole::Annotator,
    };
    for image in images.values() {
        let payloads = metadata
            .tasks
            .iter()
            .map(|task| EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: task.task_id.clone(),
                    status: TaskStatus::Completed,
                    outcome: Some(TaskOutcome::AnnotationCompleted),
                    assigned_to: None,
                    completed_by: Some(user_id.clone()),
                    completed_at: Some(timestamp),
                    updated_at: timestamp,
                },
            })
            .collect();
        repo.append_payloads_unlocked(&image.image_id, &actor, payloads)
            .await
            .unwrap();
    }

    repo.reset_image_state_load_count();
    let availability = repo
        .assignment_availability(&user_id, AssignmentKind::Annotation)
        .await
        .unwrap();

    assert!(availability.values().all(|available| !available));
    assert_eq!(
        repo.image_state_load_count(),
        images.len() as u64,
        "availability should load each image state once regardless of task count"
    );
    assert_eq!(repo.assignment_availability_cache.scan_count(), 1);

    let cached = repo
        .assignment_availability(&user_id, AssignmentKind::Annotation)
        .await
        .unwrap();
    assert_eq!(cached, availability);
    assert_eq!(repo.image_state_load_count(), images.len() as u64);
    assert_eq!(
        repo.assignment_availability_cache.scan_count(),
        1,
        "an unchanged request should reuse the completed scan"
    );

    let review = repo
        .assignment_availability(&user_id, AssignmentKind::Review)
        .await
        .unwrap();
    assert!(review.values().all(|available| !available));
    assert!(
        repo.assignment_availability(&user_id, AssignmentKind::LegacyAdjudication)
            .await
            .is_err()
    );
    assert_eq!(repo.image_state_load_count(), images.len() as u64);
    assert_eq!(
        repo.assignment_availability_cache.scan_count(),
        1,
        "one authorized kind scan should warm the other work views"
    );

    let first_image = &images.values().next().unwrap().image_id;
    let first_task = &metadata.tasks[0].task_id;
    repo.append_payload(
        first_image,
        &actor,
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: first_task.clone(),
                status: TaskStatus::Completed,
                outcome: Some(TaskOutcome::AnnotationCompleted),
                assigned_to: None,
                completed_by: Some(user_id.clone()),
                completed_at: Some(timestamp),
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();
    repo.reset_image_state_load_count();
    let refreshed = repo
        .assignment_availability(&user_id, AssignmentKind::Annotation)
        .await
        .unwrap();
    assert_eq!(refreshed, availability);
    assert_eq!(repo.image_state_load_count(), images.len() as u64);
    assert_eq!(
        repo.assignment_availability_cache.scan_count(),
        2,
        "event writes must invalidate cached availability"
    );

    repo.save_dataset(&metadata).await.unwrap();
    repo.reset_image_state_load_count();
    let (first, second) = tokio::join!(
        repo.assignment_availability(&user_id, AssignmentKind::Annotation),
        repo.assignment_availability(&user_id, AssignmentKind::Annotation),
    );
    assert_eq!(first.unwrap(), availability);
    assert_eq!(second.unwrap(), availability);
    assert_eq!(repo.image_state_load_count(), images.len() as u64);
    assert_eq!(
        repo.assignment_availability_cache.scan_count(),
        3,
        "concurrent misses should share one scan"
    );
}

#[tokio::test]
async fn historical_roles_and_assignment_kinds_cannot_be_introduced() {
    let (_temp, repo, task_id, users) = annotation_repo(4, &["worker"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[0]
        .roles
        .insert(DatasetRole::LegacyAdjudicator);
    assert!(repo.save_dataset(&metadata).await.is_err());
    repo.reset_image_state_load_count();
    assert!(
        repo.assignment_availability(&users[0], AssignmentKind::LegacyAdjudication)
            .await
            .is_err()
    );
    assert!(
        repo.assign_next_image(&users[0], &task_id, AssignmentKind::LegacyAdjudication)
            .await
            .is_err()
    );
    assert_eq!(repo.image_state_load_count(), 0);
}

#[tokio::test]
async fn review_disabled_tasks_return_no_work_without_loading_images() {
    let (_temp, repo, _task_id, users) = annotation_repo(4, &["reviewer"]).await;
    let user_id = &users[0];
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[0].roles = BTreeSet::from([DatasetRole::Reviewer]);
    metadata.tasks[0].review = ReviewConfig {
        workflow: ReviewWorkflow::None,
        allow_reviewer_corrections: false,
        legacy: None,
    };
    repo.save_dataset(&metadata).await.unwrap();

    repo.reset_image_state_load_count();
    let availability = repo
        .assignment_availability(user_id, AssignmentKind::Review)
        .await
        .unwrap();

    assert!(availability.values().all(|available| !available));
    assert_eq!(repo.image_state_load_count(), 0);
}

#[tokio::test]
async fn pending_review_tasks_do_not_reload_review_history() {
    let (_temp, repo, _task_id, users) = annotation_repo(4, &["reviewer"]).await;
    let user_id = &users[0];
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[0].roles = BTreeSet::from([DatasetRole::Reviewer]);
    metadata.tasks[0].review = ReviewConfig {
        workflow: ReviewWorkflow::Approval,
        allow_reviewer_corrections: false,
        legacy: None,
    };
    repo.save_dataset(&metadata).await.unwrap();

    repo.reset_image_state_load_count();
    repo.reset_event_load_count();
    let availability = repo
        .assignment_availability(user_id, AssignmentKind::Review)
        .await
        .unwrap();

    assert!(availability.values().all(|available| !available));
    assert_eq!(repo.image_state_load_count(), 4);
    assert_eq!(
        repo.event_load_count(),
        4,
        "pending tasks should only read events while validating each state cache"
    );
}

#[tokio::test]
async fn review_history_scan_is_reused_and_rebuilt_after_restart() {
    let (temp, repo, task_id, users) = annotation_repo(4, &["reviewer"]).await;
    let reviewer = &users[0];
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[0].roles = BTreeSet::from([DatasetRole::Reviewer]);
    metadata.tasks[0].review = ReviewConfig {
        workflow: ReviewWorkflow::Approval,
        allow_reviewer_corrections: false,
        legacy: None,
    };
    repo.save_dataset(&metadata).await.unwrap();

    let timestamp = now();
    for index in 0..4 {
        repo.append_payload(
            &ImageId::from(format!("img_{index}")),
            &Actor {
                user_id: reviewer.clone(),
                role: DatasetRole::Reviewer,
            },
            EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: task_id.clone(),
                    status: TaskStatus::Submitted,
                    outcome: None,
                    assigned_to: None,
                    completed_by: Some(reviewer.clone()),
                    completed_at: Some(timestamp),
                    updated_at: timestamp,
                },
            },
        )
        .await
        .unwrap();
    }

    repo.reset_image_state_load_count();
    repo.reset_event_load_count();
    let first = repo
        .assign_next_image(reviewer, &task_id, AssignmentKind::Review)
        .await
        .unwrap()
        .unwrap();
    let cold_state_loads = repo.image_state_load_count();
    let cold_event_loads = repo.event_load_count();
    assert!(
        cold_state_loads >= 4,
        "cold history scan must inspect every image"
    );
    assert!(
        cold_event_loads >= 4,
        "cold history scan must inspect every event log"
    );

    repo.reset_image_state_load_count();
    repo.reset_event_load_count();
    let warm = repo
        .assign_next_image(reviewer, &task_id, AssignmentKind::Review)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(warm.image_id, first.image_id);
    assert!(
        repo.image_state_load_count() < cold_state_loads,
        "warm history should avoid a full image scan"
    );
    assert!(
        repo.event_load_count() < cold_event_loads,
        "warm history should avoid rereading every event log"
    );

    let restarted = DatasetRepository::new(temp.path());
    restarted.reset_image_state_load_count();
    restarted.reset_event_load_count();
    let restored = restarted
        .assign_next_image(reviewer, &task_id, AssignmentKind::Review)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.image_id, first.image_id);
    assert!(restarted.image_state_load_count() >= 4);
    assert!(restarted.event_load_count() >= 4);
}

#[tokio::test]
async fn warm_review_reopen_reads_only_target_from_large_index() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let mut index = repo.load_images_index().await.unwrap();
    for index_number in 0..255 {
        let empty_image_id = ImageId::from(format!("empty_{index_number}"));
        index.images_by_hash.insert(
            format!("empty_hash_{index_number}"),
            ImageRecord {
                image_id: empty_image_id,
                blake3: format!("empty_hash_{index_number}"),
                canonical_path: format!("images/empty_{index_number}.png"),
                known_paths: vec![format!("images/empty_{index_number}.png")],
                duplicate_paths: Vec::new(),
                file_name: format!("empty_{index_number}.png"),
                byte_size: 4,
                width: 2,
                height: 2,
                media_type: "image/png".to_string(),
                source_memberships: None,
            },
        );
    }
    repo.save_images_index(&index).await.unwrap();
    assert_eq!(index.images_by_hash.len(), 256);

    repo.prepare_review_history().await.unwrap();
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &original, ReviewDecision::Approved).await;

    repo.reset_image_state_load_count();
    repo.reset_event_load_count();
    let started = std::time::Instant::now();
    let reopened = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    let elapsed_ms = started.elapsed().as_millis();
    println!(
        "warm review reopen: images=256 state_loads={} event_loads={} elapsed_ms={elapsed_ms}",
        repo.image_state_load_count(),
        repo.event_load_count()
    );
    assert_eq!(reopened.image_id, image_id);
    assert!(repo.image_state_load_count() <= 2);
    assert!(repo.event_load_count() <= 3);
}

#[tokio::test]
async fn review_history_observes_durable_review_when_state_cache_write_fails() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    approve_test_objects(&repo, &original).await;
    repo.fail_next_state_cache_write_after_completion();

    let result = repo
        .record_review_for_assignment(
            &reviewers[0],
            review_context(&original),
            ReviewRecord {
                review_id: ReviewId::generate(),
                target: ReviewTarget::Task {
                    task_id: task_id.clone(),
                },
                reviewer_user_id: reviewers[0].clone(),
                decision: ReviewDecision::Approved,
                timestamp: now(),
                comment: None,
            },
        )
        .await;
    assert!(
        result.is_err(),
        "the injected state-cache failure must surface"
    );

    let reopened = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    assert_eq!(reopened.kind, AssignmentKind::Review);
    assert_eq!(reopened.image_id, image_id);
}

#[tokio::test]
async fn review_history_merges_commit_observed_while_scan_is_paused() {
    let (_temp, repo, image_a, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let image_b = ImageId::from("img_2");
    add_submitted_review_image(&repo, &image_b, &task_id, &annotator).await;

    let first = claim_review(&repo, &image_a, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &first, ReviewDecision::Approved).await;
    let second = claim_review(&repo, &image_b, &task_id, &reviewers[0]).await;

    repo.review_history_cache.invalidate();
    let pause = repo.review_history_cache.pause_before_publish().await;
    let preparing = {
        let repository = repo.clone();
        tokio::spawn(async move { repository.prepare_review_history().await })
    };
    pause.scanned.notified().await;

    finalize_test_review(&repo, &second, ReviewDecision::Approved).await;
    pause.resume.notify_one();
    preparing.await.unwrap().unwrap();

    let error = repo
        .reopen_review_assignment(&reviewers[0], &first.assignment_id, &image_a, &task_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::AssignmentConflict(message)
            if message == "this is no longer the immediately previous review assignment"
    ));
}

#[tokio::test]
async fn review_history_key_guard_serializes_other_image_completion_and_reopen() {
    let (_temp, repo, image_a, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let image_b = ImageId::from("img_2");
    add_submitted_review_image(&repo, &image_b, &task_id, &annotator).await;

    let first = claim_review(&repo, &image_a, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &first, ReviewDecision::Approved).await;
    let second = claim_review(&repo, &image_b, &task_id, &reviewers[0]).await;
    let pause = repo.review_history_cache.pause_after_commit_guards().await;

    let writing_repository = repo.clone();
    let writing_reviewer = reviewers[0].clone();
    let writing_task = task_id.clone();
    let writer = tokio::spawn(async move {
        writing_repository
            .record_review_for_assignment(
                &writing_reviewer,
                review_context(&second),
                ReviewRecord {
                    review_id: ReviewId::generate(),
                    target: ReviewTarget::Task {
                        task_id: writing_task.clone(),
                    },
                    reviewer_user_id: writing_reviewer.clone(),
                    decision: ReviewDecision::Approved,
                    timestamp: now(),
                    comment: None,
                },
            )
            .await
    });
    pause.guards_acquired.notified().await;

    let reopening_repository = repo.clone();
    let reopening_reviewer = reviewers[0].clone();
    let reopening_assignment_id = first.assignment_id.clone();
    let reopening_image = image_a.clone();
    let reopening_task = task_id.clone();
    let mut reopening = tokio::spawn(async move {
        reopening_repository
            .reopen_review_assignment(
                &reopening_reviewer,
                &reopening_assignment_id,
                &reopening_image,
                &reopening_task,
            )
            .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(250), &mut reopening)
            .await
            .is_err(),
        "reopening the same reviewer/task must wait for the other-image publication"
    );

    pause.resume.notify_one();
    writer.await.unwrap().unwrap();
    let error = reopening.await.unwrap().unwrap_err();
    assert!(matches!(
        error,
        StorageError::AssignmentConflict(message)
            if message == "this is no longer the immediately previous review assignment"
    ));
}

#[tokio::test]
async fn retries_return_same_users_active_assignment() {
    let temp = tempfile::tempdir().unwrap();
    let repo = DatasetRepository::new(temp.path());
    let user_id = UserId::from("annotator");
    let task_id = TaskId::from("bounding_box:person");
    let class_id = ClassId::from("person");
    let mut metadata = DatasetMetadata::new(DatasetId::from("ds"), "Dataset", now());
    metadata.label_classes.push(LabelClass {
        class_id: class_id.clone(),
        name: "Person".to_string(),
        color: "#5eead4".to_string(),
        description: None,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: task_id.clone(),
        name: "Person boxes".to_string(),
        annotation_type: AnnotationType::BoundingBox,
        class_ids: vec![class_id],
        instructions: TutorialContent {
            title: "Instructions".to_string(),
            example_text: "Draw boxes.".to_string(),
            example_images: Vec::new(),
        },
        skeleton: None,
        review: ReviewConfig::default(),
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: None,
        enabled: true,
    });
    metadata.role_assignments.push(DatasetRoleAssignment {
        dataset_id: metadata.dataset_id.clone(),
        user_id: user_id.clone(),
        roles: BTreeSet::from([DatasetRole::Annotator]),
        assigned_at: now(),
        assigned_by: None,
    });
    repo.initialize(metadata).await.unwrap();
    let image = ImageRecord {
        image_id: ImageId::from("img_1"),
        blake3: "hash".to_string(),
        canonical_path: "images/one.png".to_string(),
        known_paths: vec!["images/one.png".to_string()],
        duplicate_paths: Vec::new(),
        file_name: "one.png".to_string(),
        byte_size: 4,
        width: 2,
        height: 2,
        media_type: "image/png".to_string(),
        source_memberships: None,
    };
    let second_image = ImageRecord {
        image_id: ImageId::from("img_2"),
        blake3: "hash2".to_string(),
        canonical_path: "images/two.png".to_string(),
        known_paths: vec!["images/two.png".to_string()],
        duplicate_paths: Vec::new(),
        file_name: "two.png".to_string(),
        byte_size: 4,
        width: 2,
        height: 2,
        media_type: "image/png".to_string(),
        source_memberships: None,
    };
    repo.save_images_index(&ImagesIndex {
        schema_version: SCHEMA_VERSION,
        image_count: 2,
        images_by_hash: BTreeMap::from([
            ("hash".to_string(), image),
            ("hash2".to_string(), second_image),
        ]),
    })
    .await
    .unwrap();

    let first = repo
        .assign_next_image(&user_id, &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    let sequence_before_reclaim = repo
        .load_image_state(&first.image_id)
        .await
        .unwrap()
        .current_sequence;
    let reclaimed = repo
        .reclaim_assignment(
            &user_id,
            &first.assignment_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed, first);
    assert_eq!(
        repo.load_image_state(&first.image_id)
            .await
            .unwrap()
            .current_sequence,
        sequence_before_reclaim,
        "exact reclaim must not invalidate a browser draft's base sequence"
    );
    let retry = repo
        .assign_next_image(&user_id, &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(retry.assignment_id, first.assignment_id);
    assert_eq!(retry.image_id, first.image_id);

    repo.complete_assignment(
        &user_id,
        &first.assignment_id,
        &first.image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();

    let next = repo
        .assign_next_image(&user_id, &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.image_id, ImageId::from("img_2"));
    let first_state = repo.load_image_state(&first.image_id).await.unwrap();
    assert_eq!(
        first_state
            .assignments
            .iter()
            .find(|assignment| assignment.assignment_id == first.assignment_id)
            .unwrap()
            .status,
        AssignmentStatus::Completed
    );
}

#[tokio::test]
async fn records_do_not_infer_assignment_completion() {
    let temp = tempfile::tempdir().unwrap();
    let repo = DatasetRepository::new(temp.path());
    repo.initialize(DatasetMetadata::new(
        DatasetId::from("ds"),
        "Dataset",
        now(),
    ))
    .await
    .unwrap();
    let image_id = ImageId::from("img_1");
    let user_id = UserId::from("worker");
    let task_id = TaskId::from("bounding_box:person");

    for (kind, role) in [
        (AssignmentKind::Review, DatasetRole::Reviewer),
        (
            AssignmentKind::LegacyAdjudication,
            DatasetRole::LegacyAdjudicator,
        ),
    ] {
        let actor = Actor {
            user_id: user_id.clone(),
            role,
        };
        let assignment = Assignment {
            assignment_id: AssignmentId::generate(),
            image_id: image_id.clone(),
            task_id: task_id.clone(),
            assigned_to: user_id.clone(),
            kind: kind.clone(),
            status: AssignmentStatus::Active,
            expires_at: Some(lease_expiration(now())),
            created_at: now(),
            updated_at: now(),
        };
        repo.append_payload(
            &image_id,
            &actor,
            EventPayload::AssignmentUpdated {
                assignment: assignment.clone(),
            },
        )
        .await
        .unwrap();
        let payload = match kind {
            AssignmentKind::Review => EventPayload::ReviewRecorded {
                review: ReviewRecord {
                    review_id: ReviewId::generate(),
                    target: ReviewTarget::Task {
                        task_id: task_id.clone(),
                    },
                    reviewer_user_id: user_id.clone(),
                    decision: ReviewDecision::Approved,
                    timestamp: now(),
                    comment: None,
                },
            },
            AssignmentKind::LegacyAdjudication => EventPayload::LegacyAdjudicationRecorded {
                adjudication: LegacyAdjudicationRecord {
                    adjudication_id: LegacyAdjudicationId::from("adj_historical"),
                    task_id: task_id.clone(),
                    annotation_ids: Vec::new(),
                    adjudicator_user_id: user_id.clone(),
                    decision: LegacyAdjudicationDecision::AcceptAnnotation,
                    resolution: "accepted".to_string(),
                    timestamp: now(),
                },
            },
            AssignmentKind::Annotation => unreachable!(),
        };
        repo.append_payload(&image_id, &actor, payload)
            .await
            .unwrap();

        let state = repo.load_image_state(&image_id).await.unwrap();
        assert_eq!(
            state
                .assignments
                .iter()
                .find(|candidate| candidate.assignment_id == assignment.assignment_id)
                .unwrap()
                .status,
            AssignmentStatus::Active
        );
    }
}

#[tokio::test]
async fn one_reviewer_owns_and_completes_the_task() {
    let (temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let first = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    assert!(
        repo.assign_next_image(&reviewers[1], &task_id, AssignmentKind::Review)
            .await
            .unwrap()
            .is_none()
    );
    let completed = finalize_test_review(&repo, &first, ReviewDecision::Approved).await;
    assert_eq!(
        completed.task_states[&task_id].status,
        TaskStatus::Completed
    );
    assert_eq!(
        assignment_status(&completed, &first),
        AssignmentStatus::Completed
    );
    assert!(
        repo.assign_next_image(&reviewers[1], &task_id, AssignmentKind::Review)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        DatasetRepository::new(temp.path())
            .load_image_state(&image_id)
            .await
            .unwrap(),
        completed
    );
    assert_eq!(
        repo.rebuild_image_state(&image_id).await.unwrap(),
        completed
    );
}

#[tokio::test]
async fn release_cancels_assignment_and_makes_image_reclaimable() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let first = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    let released = repo
        .release_assignment(
            &users[0],
            &first.assignment_id,
            &first.image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();
    assert_eq!(released.status, AssignmentStatus::Cancelled);
    let state = repo.load_image_state(&first.image_id).await.unwrap();
    assert_eq!(state.task_states[&task_id].status, TaskStatus::Pending);

    let reclaimed = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.image_id, first.image_id);
    assert_ne!(reclaimed.assignment_id, first.assignment_id);
}

#[tokio::test]
async fn cancelled_assignment_reopens_as_a_fresh_replayable_attempt() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let original = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    repo.release_assignment(
        &users[0],
        &original.assignment_id,
        &original.image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();

    let reopened = repo
        .reopen_assignment(
            &users[0],
            &original.assignment_id,
            &original.image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();
    let retry = repo
        .reopen_assignment(
            &users[0],
            &original.assignment_id,
            &original.image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();

    assert_ne!(reopened.assignment_id, original.assignment_id);
    assert_eq!(retry.assignment_id, reopened.assignment_id);
    let state = repo.rebuild_image_state(&original.image_id).await.unwrap();
    assert_eq!(state.task_states[&task_id].status, TaskStatus::InProgress);
    assert_eq!(
        assignment_status(&state, &original),
        AssignmentStatus::Cancelled
    );
    assert_eq!(
        assignment_status(&state, &reopened),
        AssignmentStatus::Active
    );
}

#[tokio::test]
async fn submitted_assignment_reopens_to_its_previous_annotation_state() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let original = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    repo.complete_assignment(
        &users[0],
        &original.assignment_id,
        &original.image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();

    let reopened = repo
        .reopen_assignment(
            &users[0],
            &original.assignment_id,
            &original.image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();

    let state = repo.rebuild_image_state(&original.image_id).await.unwrap();
    assert_eq!(state.task_states[&task_id].status, TaskStatus::InProgress);
    assert_eq!(
        assignment_status(&state, &original),
        AssignmentStatus::Completed
    );
    assert_eq!(
        assignment_status(&state, &reopened),
        AssignmentStatus::Active
    );
}

#[tokio::test]
async fn reopen_preserves_needs_correction_and_rejects_downstream_review_work() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker", "reviewer"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[1]
        .roles
        .insert(DatasetRole::Reviewer);
    repo.save_dataset(&metadata).await.unwrap();
    let image_id = ImageId::from("img_0");
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task_id.clone(),
                status: TaskStatus::NeedsCorrection,
                outcome: None,
                assigned_to: None,
                completed_by: None,
                completed_at: None,
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();
    let correction = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    repo.complete_assignment(
        &users[0],
        &correction.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    let reopened = repo
        .reopen_assignment(
            &users[0],
            &correction.assignment_id,
            &image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap();
    assert_eq!(
        repo.load_image_state(&image_id).await.unwrap().task_states[&task_id].status,
        TaskStatus::NeedsCorrection
    );

    repo.complete_assignment(
        &users[0],
        &reopened.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    repo.assign_next_image(&users[1], &task_id, AssignmentKind::Review)
        .await
        .unwrap()
        .unwrap();
    let error = repo
        .reopen_assignment(
            &users[0],
            &reopened.assignment_id,
            &image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::AssignmentConflict(_)));
}

#[tokio::test]
async fn concurrent_reopen_retries_share_one_renewed_successor() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let original = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    repo.release_assignment(
        &users[0],
        &original.assignment_id,
        &original.image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    let left_repo = repo.clone();
    let right_repo = repo.clone();
    let left_assignment = original.clone();
    let right_assignment = original.clone();
    let left_task = task_id.clone();
    let right_task = task_id.clone();
    let left_user = users[0].clone();
    let right_user = users[0].clone();

    let (left, right) = tokio::join!(
        async move {
            left_repo
                .reopen_assignment(
                    &left_user,
                    &left_assignment.assignment_id,
                    &left_assignment.image_id,
                    &left_task,
                    AssignmentKind::Annotation,
                )
                .await
                .unwrap()
        },
        async move {
            right_repo
                .reopen_assignment(
                    &right_user,
                    &right_assignment.assignment_id,
                    &right_assignment.image_id,
                    &right_task,
                    AssignmentKind::Annotation,
                )
                .await
                .unwrap()
        }
    );

    assert_eq!(left.assignment_id, right.assignment_id);
    assert!(left.expires_at.is_some());
    assert!(right.expires_at.is_some());
    let state = repo.rebuild_image_state(&original.image_id).await.unwrap();
    assert_eq!(
        state
            .assignments
            .iter()
            .filter(|assignment| assignment.status == AssignmentStatus::Active)
            .count(),
        1
    );
}

#[tokio::test]
async fn released_image_can_be_excluded_from_the_next_claim() {
    let (_temp, repo, task_id, users) = annotation_repo(2, &["worker"]).await;
    let first = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    repo.release_assignment(
        &users[0],
        &first.assignment_id,
        &first.image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();

    let next = repo
        .assign_next_image_excluding(
            &users[0],
            &task_id,
            AssignmentKind::Annotation,
            std::slice::from_ref(&first.image_id),
        )
        .await
        .unwrap()
        .unwrap();

    assert_ne!(next.image_id, first.image_id);
}

#[tokio::test]
async fn claim_retry_renews_the_same_assignment() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let first = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    let retry = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(retry.assignment_id, first.assignment_id);
    assert!(retry.updated_at >= first.updated_at);
    assert!(retry.expires_at.unwrap() >= first.expires_at.unwrap());
    let (_, refreshed) = repo
        .append_for_assignment(
            &users[0],
            AssignmentContext {
                assignment_id: &retry.assignment_id,
                image_id: &retry.image_id,
                task_id: &task_id,
                kind: AssignmentKind::Annotation,
            },
            Vec::new(),
            false,
        )
        .await
        .unwrap();
    assert_eq!(refreshed.assignment_id, first.assignment_id);
    assert!(refreshed.expires_at.unwrap() >= retry.expires_at.unwrap());
    let persisted = repo.load_image_state(&first.image_id).await.unwrap();
    assert_eq!(persisted.assignments[0].expires_at, refreshed.expires_at);
}

#[tokio::test]
async fn claim_retry_resumes_at_the_previous_assignment_cursor() {
    let (_temp, repo, task_id, users) = annotation_repo(4, &["worker"]).await;
    let metadata = repo.load_dataset().await.unwrap();
    let excluded = metadata.images.keys().take(3).cloned().collect::<Vec<_>>();
    let first = repo
        .assign_next_image_excluding(&users[0], &task_id, AssignmentKind::Annotation, &excluded)
        .await
        .unwrap()
        .unwrap();

    repo.reset_image_state_load_count();
    let retry = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(retry.assignment_id, first.assignment_id);
    assert_eq!(repo.image_state_load_count(), 2);
}

#[tokio::test]
async fn expired_annotation_is_cancelled_and_atomically_reclaimed() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["owner", "next", "other"]).await;
    let first = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    expire_assignment(&repo, &first, &users[0]).await;

    let next_repo = repo.clone();
    let other_repo = repo.clone();
    let next_task = task_id.clone();
    let other_task = task_id.clone();
    let next_user = users[1].clone();
    let other_user = users[2].clone();
    let (next, other) = tokio::join!(
        async move {
            next_repo
                .assign_next_image(&next_user, &next_task, AssignmentKind::Annotation)
                .await
                .unwrap()
        },
        async move {
            other_repo
                .assign_next_image(&other_user, &other_task, AssignmentKind::Annotation)
                .await
                .unwrap()
        }
    );
    assert_eq!(
        usize::from(next.is_some()) + usize::from(other.is_some()),
        1
    );
    let reclaimed = next.or(other).unwrap();

    assert_ne!(reclaimed.assignment_id, first.assignment_id);
    assert!(reclaimed.assigned_to == users[1] || reclaimed.assigned_to == users[2]);
    let state = repo.load_image_state(&first.image_id).await.unwrap();
    assert_eq!(state.task_states[&task_id].status, TaskStatus::InProgress);
    assert_eq!(
        state
            .assignments
            .iter()
            .find(|assignment| assignment.assignment_id == first.assignment_id)
            .unwrap()
            .status,
        AssignmentStatus::Cancelled
    );
    let events = repo.load_events(&first.image_id).await.unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventPayload::TaskStateChanged { task_state }
            if task_state.task_id == task_id && task_state.status == TaskStatus::Pending
    )));
}

#[tokio::test]
async fn expired_correction_assignment_preserves_needs_correction() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["owner", "next"]).await;
    let image_id = ImageId::from("img_0");
    let timestamp = now();
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task_id.clone(),
                status: TaskStatus::NeedsCorrection,
                outcome: None,
                assigned_to: None,
                completed_by: None,
                completed_at: None,
                updated_at: timestamp,
            },
        },
    )
    .await
    .unwrap();
    let first = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    expire_assignment(&repo, &first, &users[0]).await;

    let reclaimed = repo
        .assign_next_image(&users[1], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    assert_ne!(reclaimed.assignment_id, first.assignment_id);
    assert_eq!(
        repo.load_image_state(&image_id).await.unwrap().task_states[&task_id].status,
        TaskStatus::NeedsCorrection
    );
}

#[tokio::test]
async fn expired_owner_cannot_complete_assignment() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["owner"]).await;
    let assignment = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    expire_assignment(&repo, &assignment, &users[0]).await;

    let error = repo
        .complete_assignment(
            &users[0],
            &assignment.assignment_id,
            &assignment.image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::AssignmentConflict(_)));
    assert!(error.to_string().contains("expired"));
}

#[tokio::test]
async fn completing_annotation_without_review_completes_task() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].review.workflow = ReviewWorkflow::None;

    repo.save_dataset(&metadata).await.unwrap();
    let assignment = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    repo.complete_assignment(
        &users[0],
        &assignment.assignment_id,
        &assignment.image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();

    let state = repo.load_image_state(&assignment.image_id).await.unwrap();
    assert_eq!(state.task_states[&task_id].status, TaskStatus::Completed);
}

#[tokio::test]
async fn historical_review_configuration_cannot_be_saved() {
    let (_temp, repo, _task_id, _users) = annotation_repo(1, &["worker"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].review.workflow = ReviewWorkflow::LegacyIndependentAgreement;
    assert!(repo.save_dataset(&metadata).await.is_err());
    metadata.tasks[0].review.workflow = ReviewWorkflow::Approval;
    metadata.tasks[0].review.legacy = Some(Box::new(labello_domain::LegacyReviewConfig {
        required_reviews: 2,
        agreement_threshold: None,
    }));
    assert!(repo.save_dataset(&metadata).await.is_err());
}

#[tokio::test]
async fn release_preserves_needs_correction() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let image_id = ImageId::from("img_0");
    let timestamp = now();
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task_id.clone(),
                status: TaskStatus::NeedsCorrection,
                outcome: None,
                assigned_to: None,
                completed_by: None,
                completed_at: None,
                updated_at: timestamp,
            },
        },
    )
    .await
    .unwrap();
    let assignment = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    repo.release_assignment(
        &users[0],
        &assignment.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();

    assert_eq!(
        repo.load_image_state(&image_id).await.unwrap().task_states[&task_id].status,
        TaskStatus::NeedsCorrection
    );
}

#[tokio::test]
async fn exact_assignment_rejects_the_wrong_user() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["owner", "other"]).await;
    let assignment = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();

    let error = repo
        .complete_assignment(
            &users[1],
            &assignment.assignment_id,
            &assignment.image_id,
            &task_id,
            AssignmentKind::Annotation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::Unauthorized(_)));
    assert_eq!(
        repo.load_image_state(&assignment.image_id)
            .await
            .unwrap()
            .assignments[0]
            .status,
        AssignmentStatus::Active
    );
}

#[tokio::test]
async fn concurrent_claims_cannot_take_the_same_annotation_work() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["first", "second"]).await;
    let first_repo = repo.clone();
    let second_repo = repo.clone();
    let first_task = task_id.clone();
    let second_task = task_id.clone();
    let first_user = users[0].clone();
    let second_user = users[1].clone();

    let (first, second) = tokio::join!(
        async move {
            first_repo
                .assign_next_image(&first_user, &first_task, AssignmentKind::Annotation)
                .await
                .unwrap()
        },
        async move {
            second_repo
                .assign_next_image(&second_user, &second_task, AssignmentKind::Annotation)
                .await
                .unwrap()
        }
    );

    assert_eq!(
        usize::from(first.is_some()) + usize::from(second.is_some()),
        1
    );
}

#[tokio::test]
async fn annotation_claims_skip_excluded_and_imported_terminal_tasks() {
    let (_temp, repo, task_id, users) = annotation_repo(3, &["worker"]).await;
    let timestamp = now();
    for (index, (coverage, status)) in [
        (ImportCoverage::Excluded, TaskStatus::Pending),
        (ImportCoverage::Complete, TaskStatus::Completed),
        (ImportCoverage::Complete, TaskStatus::Submitted),
    ]
    .into_iter()
    .enumerate()
    {
        let image_id = ImageId::from(format!("img_{index}"));
        let terminal = matches!(status, TaskStatus::Completed | TaskStatus::Submitted);
        repo.append_payload(
            &image_id,
            &Actor {
                user_id: UserId::from("admin"),
                role: DatasetRole::DataAdmin,
            },
            EventPayload::ImportInitialized {
                import_id: ImportId::from(format!("imp_{index}")),
                annotations: Vec::new(),
                task_initializations: vec![ImportTaskInitialization {
                    task_id: task_id.clone(),
                    coverage,
                    initial_state: TaskState {
                        task_id: task_id.clone(),
                        status,
                        outcome: terminal.then_some(TaskOutcome::ImportedGroundTruth),
                        assigned_to: None,
                        completed_by: terminal.then(|| UserId::from("admin")),
                        completed_at: terminal.then_some(timestamp),
                        updated_at: timestamp,
                    },
                }],
                migration_target_sets: Vec::new(),
            },
        )
        .await
        .unwrap();
        assert!(
            !repo
                .load_image_state(&image_id)
                .await
                .unwrap()
                .assignment_eligible(&task_id)
        );
    }

    assert!(
        repo.assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn claim_rejects_enabled_tasks_without_exactly_one_class() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].class_ids.push(ClassId::from("second"));
    repo.save_dataset(&metadata).await.unwrap();

    let error = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::InvalidAssignment(_)));
}

#[tokio::test]
async fn annotation_batch_is_atomic_and_idempotent() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["worker"]).await;
    let assignment = repo
        .assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    let annotation = |id: &str| AnnotationVersion {
        annotation_id: AnnotationId::from(id),
        version: 1,
        object_group_id: None,
        origin: AnnotationOrigin::native(),
        task_id: task_id.clone(),
        class_id: ClassId::from("person"),
        annotation_type: AnnotationType::BoundingBox,
        revision_source: RevisionSource::Human {
            action: HumanRevisionKind::Authored,
        },
        geometry: AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.1,
            y: 0.1,
            width: 0.4,
            height: 0.4,
        }),
        author_user_id: users[0].clone(),
        created_at: now(),
        updated_at: now(),
        deleted: false,
    };
    let create = |annotation: AnnotationVersion| EventPayload::AnnotationVersionCreated {
        annotation,
        previous_version: None,
        reason: None,
    };
    let context = || AssignmentContext {
        assignment_id: &assignment.assignment_id,
        image_id: &assignment.image_id,
        task_id: &task_id,
        kind: AssignmentKind::Annotation,
    };
    let before = repo.load_image_state(&assignment.image_id).await.unwrap();
    let error = repo
        .apply_annotation_batch(
            &users[0],
            context(),
            vec![
                create(annotation("ann_1")),
                EventPayload::AnnotationDeleted {
                    annotation_id: AnnotationId::from("missing"),
                    version: 1,
                    reason: None,
                },
            ],
            false,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::InvalidAssignment(_)));
    let unchanged = repo.load_image_state(&assignment.image_id).await.unwrap();
    assert_eq!(unchanged.current_sequence, before.current_sequence);
    assert!(unchanged.annotations.is_empty());

    let first_version = annotation("ann_stale_delete");
    let mut second_version = first_version.clone();
    second_version.version = 2;
    second_version.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.2,
        width: 0.3,
        height: 0.3,
    });
    let error = repo
        .apply_annotation_batch(
            &users[0],
            context(),
            vec![
                create(first_version),
                EventPayload::AnnotationVersionCreated {
                    annotation: second_version,
                    previous_version: Some(1),
                    reason: Some("move".to_string()),
                },
                EventPayload::AnnotationDeleted {
                    annotation_id: AnnotationId::from("ann_stale_delete"),
                    version: 1,
                    reason: None,
                },
            ],
            false,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::InvalidAssignment(_)));
    assert_eq!(
        repo.load_image_state(&assignment.image_id)
            .await
            .unwrap()
            .current_sequence,
        before.current_sequence
    );

    let payloads = vec![create(annotation("ann_1")), create(annotation("ann_2"))];
    let saved = repo
        .apply_annotation_batch(&users[0], context(), payloads.clone(), true)
        .await
        .unwrap();
    assert_eq!(saved.active_annotations().count(), 2);
    assert_eq!(saved.assignments[0].status, AssignmentStatus::Completed);

    let retried = repo
        .apply_annotation_batch(&users[0], context(), payloads, true)
        .await
        .unwrap();
    assert_eq!(retried.current_sequence, saved.current_sequence);
    assert_eq!(retried.active_annotations().count(), 2);
}

#[tokio::test]
async fn bbox_correction_resubmits_idempotently_and_updates_quality_stats() {
    let (_temp, repo, image_id, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, true).await;
    let first = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let competing = historical_review_assignment(&repo, &image_id, &task_id, &reviewers[1]).await;
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: reviewers[1].clone(),
            role: DatasetRole::Reviewer,
        },
        EventPayload::ReviewRecorded {
            review: ReviewRecord {
                review_id: ReviewId::from("rev_partial_approval"),
                target: ReviewTarget::Task {
                    task_id: task_id.clone(),
                },
                reviewer_user_id: reviewers[1].clone(),
                decision: ReviewDecision::Approved,
                timestamp: now(),
                comment: None,
            },
        },
    )
    .await
    .unwrap();
    let correction_id = CorrectionId::from("cor_bbox");
    let geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.25,
        width: 0.4,
        height: 0.3,
    });

    let event = correct(
        &repo,
        &image_id,
        &task_id,
        &reviewers[0],
        &first,
        &correction_id,
        1,
        geometry.clone(),
    )
    .await
    .unwrap();
    let event_count = repo.load_events(&image_id).await.unwrap().len();
    let retry = correct(
        &repo,
        &image_id,
        &task_id,
        &reviewers[0],
        &first,
        &correction_id,
        1,
        geometry.clone(),
    )
    .await
    .unwrap();

    assert_eq!(retry.event_id, event.event_id);
    assert_eq!(
        repo.load_events(&image_id).await.unwrap().len(),
        event_count
    );
    assert!(matches!(
        event.payload,
        EventPayload::ReviewCorrectionSubmitted { .. }
    ));
    let state = repo.load_image_state(&image_id).await.unwrap();
    let corrected = state
        .current_annotation(&AnnotationId::from("ann_1"))
        .unwrap();
    assert_eq!(corrected.version, 2);
    assert_eq!(corrected.geometry, geometry);
    assert_eq!(corrected.author_user_id, reviewers[0]);
    assert!(matches!(
        &corrected.revision_source,
        RevisionSource::ReviewerCorrection { correction_id: id } if id == &correction_id
    ));
    assert_eq!(state.task_states[&task_id].status, TaskStatus::Submitted);
    assert_eq!(state.task_states[&task_id].outcome, None);
    assert!(
        state
            .reviews
            .iter()
            .any(|review| review.decision == ReviewDecision::Rejected)
    );
    assert_eq!(
        assignment_status(&state, &first),
        AssignmentStatus::Completed
    );
    assert_eq!(
        assignment_status(&state, &competing),
        AssignmentStatus::Cancelled
    );
    assert_eq!(repo.rebuild_image_state(&image_id).await.unwrap(), state);
    assert!(
        repo.assign_next_image(&annotator, &task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_none()
    );

    let stats = repo.dataset_stats().await.unwrap();
    assert_eq!(stats.completed_tasks, 0);
    assert_eq!(stats.needs_correction_tasks, 0);
    assert_eq!(stats.awaiting_review_tasks, 1);
    assert_eq!(stats.per_task[&task_id].completed, 0);
    assert_eq!(stats.per_task[&task_id].needs_correction, 0);
    let contributors = stats.contributors.unwrap();
    let history = &contributors[&annotator].history;
    assert_eq!(history.iter().map(|day| day.labeled).sum::<usize>(), 1);
    assert_eq!(history.iter().map(|day| day.accepted).sum::<usize>(), 1);
    assert_eq!(history.iter().map(|day| day.rejected).sum::<usize>(), 1);
    for reviewer in reviewers {
        let history = &contributors[&reviewer].history;
        assert_eq!(history.iter().map(|day| day.reviewed).sum::<usize>(), 1);
        assert_eq!(
            history
                .iter()
                .map(|day| day.labeled + day.accepted + day.rejected)
                .sum::<usize>(),
            0
        );
    }
}

#[tokio::test]
async fn skeleton_correction_is_server_versioned_and_legacy_disabled_config_still_allows_correction()
 {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::Skeleton, true).await;
    let assignment = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let unchanged = AnnotationGeometry::Skeleton(SkeletonGeometry {
        keypoints: vec![KeypointAnnotation {
            name: "nose".to_string(),
            state: KeypointState::Visible,
            point: Some(NormalizedPoint { x: 0.5, y: 0.5 }),
        }],
    });
    let unchanged_error = correct(
        &repo,
        &image_id,
        &task_id,
        &reviewers[0],
        &assignment,
        &CorrectionId::from("cor_unchanged"),
        1,
        unchanged,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        unchanged_error,
        StorageError::InvalidCorrection(_)
            | StorageError::AssignmentConflict(_)
            | StorageError::Domain(_)
    ));

    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].skeleton.as_mut().unwrap().allow_hidden = false;
    repo.save_dataset(&metadata).await.unwrap();
    repo.release_assignment(
        &reviewers[0],
        &assignment.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    let assignment = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    for (correction_id, keypoints) in [
        ("cor_missing", Vec::new()),
        (
            "cor_wrong_name",
            vec![KeypointAnnotation {
                name: "ear".to_string(),
                state: KeypointState::Visible,
                point: Some(NormalizedPoint { x: 0.6, y: 0.4 }),
            }],
        ),
        (
            "cor_hidden",
            vec![KeypointAnnotation {
                name: "nose".to_string(),
                state: KeypointState::Hidden,
                point: Some(NormalizedPoint { x: 0.6, y: 0.4 }),
            }],
        ),
        (
            "cor_absent",
            vec![KeypointAnnotation {
                name: "nose".to_string(),
                state: KeypointState::Absent,
                point: None,
            }],
        ),
    ] {
        let error = correct(
            &repo,
            &image_id,
            &task_id,
            &reviewers[0],
            &assignment,
            &CorrectionId::from(correction_id),
            1,
            AnnotationGeometry::Skeleton(SkeletonGeometry { keypoints }),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            StorageError::InvalidCorrection(_)
                | StorageError::AssignmentConflict(_)
                | StorageError::Domain(_)
        ));
    }
    let geometry = AnnotationGeometry::Skeleton(SkeletonGeometry {
        keypoints: vec![KeypointAnnotation {
            name: "nose".to_string(),
            state: KeypointState::Visible,
            point: Some(NormalizedPoint { x: 0.6, y: 0.4 }),
        }],
    });
    let stale = correct(
        &repo,
        &image_id,
        &task_id,
        &reviewers[0],
        &assignment,
        &CorrectionId::from("cor_stale"),
        0,
        geometry.clone(),
    )
    .await
    .unwrap_err();
    assert!(matches!(stale, StorageError::AssignmentConflict(_)));

    correct(
        &repo,
        &image_id,
        &task_id,
        &reviewers[0],
        &assignment,
        &CorrectionId::from("cor_skeleton"),
        1,
        geometry.clone(),
    )
    .await
    .unwrap();
    let state = repo.load_image_state(&image_id).await.unwrap();
    assert_eq!(
        state
            .current_annotation(&AnnotationId::from("ann_1"))
            .unwrap()
            .geometry,
        geometry
    );

    let (_temp, disabled_repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let assignment = claim_review(&disabled_repo, &image_id, &task_id, &reviewers[0]).await;
    correct(
        &disabled_repo,
        &image_id,
        &task_id,
        &reviewers[0],
        &assignment,
        &CorrectionId::from("cor_disabled"),
        1,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.2,
            height: 0.2,
        }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn concurrent_corrections_have_one_winner_and_leave_no_active_review_assignment() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, true).await;
    let first = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let second = first.clone();
    let first_repo = repo.clone();
    let second_repo = repo.clone();
    let first_image = image_id.clone();
    let second_image = image_id.clone();
    let first_task = task_id.clone();
    let second_task = task_id.clone();
    let first_user = reviewers[0].clone();
    let second_user = reviewers[0].clone();
    let geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.3,
        y: 0.3,
        width: 0.2,
        height: 0.2,
    });
    let other_geometry = geometry.clone();

    let (left, right) = tokio::join!(
        async move {
            correct(
                &first_repo,
                &first_image,
                &first_task,
                &first_user,
                &first,
                &CorrectionId::from("cor_first"),
                1,
                geometry,
            )
            .await
        },
        async move {
            correct(
                &second_repo,
                &second_image,
                &second_task,
                &second_user,
                &second,
                &CorrectionId::from("cor_second"),
                1,
                other_geometry,
            )
            .await
        }
    );

    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let state = repo.load_image_state(&image_id).await.unwrap();
    assert_eq!(state.review_correction_submissions.len(), 1);
    assert_eq!(
        state
            .current_annotation(&AnnotationId::from("ann_1"))
            .unwrap()
            .version,
        2
    );
    assert!(state.assignments.iter().all(|assignment| {
        assignment.kind != AssignmentKind::Review || assignment.status != AssignmentStatus::Active
    }));
}

#[tokio::test]
async fn concurrent_final_approvals_cannot_leave_the_task_submitted() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].name.push_str(" changed");
    repo.save_dataset(&metadata).await.unwrap();
    let first = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let second = first.clone();
    let first_repo = repo.clone();
    let second_repo = repo.clone();
    let first_image = image_id.clone();
    let second_image = image_id.clone();
    let first_task = task_id.clone();
    let second_task = task_id.clone();
    let first_user = reviewers[0].clone();
    let second_user = reviewers[0].clone();

    let (left, right) = tokio::join!(
        async move {
            record_task_approval(
                &first_repo,
                &first_image,
                &first_task,
                &first_user,
                &first,
                "rev_first",
            )
            .await
        },
        async move {
            record_task_approval(
                &second_repo,
                &second_image,
                &second_task,
                &second_user,
                &second,
                "rev_second",
            )
            .await
        }
    );

    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let state = repo.load_image_state(&image_id).await.unwrap();
    assert_eq!(state.task_states[&task_id].status, TaskStatus::Completed);
    assert_eq!(
        state.task_states[&task_id].outcome,
        Some(TaskOutcome::Approved)
    );
    assert_eq!(task_approval_count(&state.reviews, &task_id), 1);
    assert!(
        state
            .assignments
            .iter()
            .filter(|assignment| {
                assignment.task_id == task_id && assignment.kind == AssignmentKind::Review
            })
            .all(|assignment| assignment.status == AssignmentStatus::Completed)
    );
}

async fn claim_review(
    repo: &DatasetRepository,
    image_id: &ImageId,
    task_id: &TaskId,
    reviewer: &UserId,
) -> Assignment {
    let assignment = repo
        .assign_next_image(reviewer, task_id, AssignmentKind::Review)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&assignment.image_id, image_id);
    assignment
}

#[tokio::test]
async fn skipped_review_reopens_with_fresh_identity_and_preserves_round() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    repo.release_assignment(
        &reviewers[0],
        &original.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    let before = repo.load_image_state(&image_id).await.unwrap();
    let reopened = repo
        .reopen_assignment(
            &reviewers[0],
            &original.assignment_id,
            &image_id,
            &task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap();
    assert_ne!(reopened.assignment_id, original.assignment_id);
    assert_eq!(reopened.status, AssignmentStatus::Active);
    let after = repo.rebuild_image_state(&image_id).await.unwrap();
    assert_eq!(after.task_states, before.task_states);
    assert_eq!(after.reviews, before.reviews);
}

#[tokio::test]
async fn previous_review_reopens_before_current_release_and_survives_later_release() {
    let (_temp, repo, image_a, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let index = repo.load_images_index().await.unwrap();
    let image_b = ImageId::from("img_2");
    let mut image_index = index;
    image_index.images_by_hash.insert(
        "hash_2".to_string(),
        ImageRecord {
            image_id: image_b.clone(),
            blake3: "hash_2".to_string(),
            canonical_path: "images/two.png".to_string(),
            known_paths: vec!["images/two.png".to_string()],
            duplicate_paths: Vec::new(),
            file_name: "two.png".to_string(),
            byte_size: 4,
            width: 100,
            height: 100,
            media_type: "image/png".to_string(),
            source_memberships: None,
        },
    );
    image_index.image_count = 2;
    repo.save_images_index(&image_index).await.unwrap();
    for event in repo.load_events(&image_a).await.unwrap() {
        repo.append_payload(
            &image_b,
            &Actor {
                user_id: UserId::from("annotator"),
                role: DatasetRole::Annotator,
            },
            event.payload,
        )
        .await
        .unwrap();
    }

    let previous = claim_review(&repo, &image_a, &task_id, &reviewers[0]).await;
    repo.release_assignment(
        &reviewers[0],
        &previous.assignment_id,
        &image_a,
        &task_id,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    let current = repo
        .assign_next_image_excluding(
            &reviewers[0],
            &task_id,
            AssignmentKind::Review,
            std::slice::from_ref(&image_a),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.image_id, image_b);

    let failed = repo
        .reopen_review_assignment(
            &reviewers[0],
            &AssignmentId::from("missing_previous_review"),
            &image_a,
            &task_id,
        )
        .await
        .unwrap_err();
    assert!(failed.to_string().contains("missing"));
    let current_state = repo.load_image_state(&image_b).await.unwrap();
    assert_eq!(
        assignment_status(&current_state, &current),
        AssignmentStatus::Active
    );

    let reopened = repo
        .reopen_review_assignment(&reviewers[0], &previous.assignment_id, &image_a, &task_id)
        .await
        .unwrap();
    assert_ne!(reopened.assignment_id, previous.assignment_id);
    assert_eq!(reopened.image_id, image_a);
    assert_eq!(reopened.status, AssignmentStatus::Active);

    repo.release_assignment(
        &reviewers[0],
        &current.assignment_id,
        &image_b,
        &task_id,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    let reopened_state = repo.load_image_state(&image_a).await.unwrap();
    assert_eq!(
        assignment_status(&reopened_state, &reopened),
        AssignmentStatus::Active
    );
    assert_eq!(
        repo.reclaim_assignment(
            &reviewers[0],
            &reopened.assignment_id,
            &task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap()
        .assignment_id,
        reopened.assignment_id
    );
}

#[tokio::test]
async fn previous_completed_review_ignores_expiry_cleanup_on_another_image() {
    let (_temp, repo, image_a, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let image_b = ImageId::from("img_2");
    let mut image_index = repo.load_images_index().await.unwrap();
    let mut image_b_record = image_index.images_by_hash.values().next().unwrap().clone();
    image_b_record.image_id = image_b.clone();
    image_b_record.blake3 = "hash_2".to_string();
    image_b_record.canonical_path = "images/two.png".to_string();
    image_b_record.known_paths = vec!["images/two.png".to_string()];
    image_b_record.file_name = "two.png".to_string();
    image_index
        .images_by_hash
        .insert("hash_2".to_string(), image_b_record);
    image_index.image_count = 2;
    repo.save_images_index(&image_index).await.unwrap();
    for event in repo.load_events(&image_a).await.unwrap() {
        repo.append_payload(
            &image_b,
            &Actor {
                user_id: annotator.clone(),
                role: DatasetRole::Annotator,
            },
            event.payload,
        )
        .await
        .unwrap();
    }

    let completed = claim_review(&repo, &image_a, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &completed, ReviewDecision::Approved).await;

    let expired = claim_review(&repo, &image_b, &task_id, &reviewers[0]).await;
    expire_assignment(&repo, &expired, &reviewers[0]).await;
    let successor = claim_review(&repo, &image_b, &task_id, &reviewers[0]).await;
    assert_ne!(successor.assignment_id, expired.assignment_id);
    let state_b = repo.load_image_state(&image_b).await.unwrap();
    let cancelled = state_b
        .assignments
        .iter()
        .find(|assignment| assignment.assignment_id == expired.assignment_id)
        .unwrap();
    assert_eq!(cancelled.status, AssignmentStatus::Cancelled);
    assert!(
        cancelled
            .expires_at
            .is_some_and(|expires_at| expires_at < cancelled.updated_at)
    );
    assert!(
        state_b
            .review_finished_sequences
            .contains_key(&expired.assignment_id)
    );
    assert_eq!(
        assignment_status(&state_b, &successor),
        AssignmentStatus::Active
    );

    let reopened = repo
        .reopen_review_assignment(&reviewers[0], &completed.assignment_id, &image_a, &task_id)
        .await;
    assert!(
        reopened.is_ok(),
        "maintenance cancellation on another image must not supersede a completed review: {reopened:?}"
    );

    let (_temp, repo, image_a, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let image_b = ImageId::from("img_2");
    let mut image_index = repo.load_images_index().await.unwrap();
    let mut image_b_record = image_index.images_by_hash.values().next().unwrap().clone();
    image_b_record.image_id = image_b.clone();
    image_b_record.blake3 = "hash_2".to_string();
    image_b_record.canonical_path = "images/two.png".to_string();
    image_b_record.known_paths = vec!["images/two.png".to_string()];
    image_b_record.file_name = "two.png".to_string();
    image_index
        .images_by_hash
        .insert("hash_2".to_string(), image_b_record);
    image_index.image_count = 2;
    repo.save_images_index(&image_index).await.unwrap();
    for event in repo.load_events(&image_a).await.unwrap() {
        repo.append_payload(
            &image_b,
            &Actor {
                user_id: annotator.clone(),
                role: DatasetRole::Annotator,
            },
            event.payload,
        )
        .await
        .unwrap();
    }
    let completed = claim_review(&repo, &image_a, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &completed, ReviewDecision::Approved).await;
    let expired = claim_review(&repo, &image_b, &task_id, &reviewers[0]).await;
    expire_assignment(&repo, &expired, &reviewers[0]).await;
    let successor = claim_review(&repo, &image_b, &task_id, &reviewers[0]).await;
    repo.release_assignment(
        &reviewers[0],
        &successor.assignment_id,
        &image_b,
        &task_id,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    let error = repo
        .reopen_review_assignment(&reviewers[0], &completed.assignment_id, &image_a, &task_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::AssignmentConflict(message)
            if message == "this is no longer the immediately previous review assignment"
    ));
}

#[tokio::test]
async fn previous_review_conflicts_distinguish_context_changes_and_preserve_denial() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    repo.release_assignment(
        &reviewers[0],
        &original.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    let before = repo.load_image_state(&image_id).await.unwrap();
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].name = "Changed review task".to_string();
    repo.save_dataset(&metadata).await.unwrap();
    let error = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::AssignmentConflict(message)
            if message == "previous review task configuration changed"
    ));
    assert_eq!(repo.load_image_state(&image_id).await.unwrap(), before);

    let (_temp, repo, image_id, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    repo.release_assignment(
        &reviewers[0],
        &original.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    let later = claim_review(&repo, &image_id, &task_id, &reviewers[1]).await;
    finalize_test_review(&repo, &later, ReviewDecision::Rejected).await;
    let correction = repo
        .assign_next_image(&annotator, &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    repo.complete_assignment(
        &annotator,
        &correction.assignment_id,
        &image_id,
        &task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    let before = repo.load_image_state(&image_id).await.unwrap();
    let error = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::AssignmentConflict(message) if message == "previous review submission changed"
    ));
    assert_eq!(repo.load_image_state(&image_id).await.unwrap(), before);
}

fn revision_replacements(
    state: &labello_domain::ImageState,
    assignment: &Assignment,
    decision: ReviewDecision,
) -> labello_domain::ReviewRevisionCommit {
    labello_domain::ReviewRevisionCommit {
        missing_objects: Vec::new(),
        reviews: state.review_assignment_contexts[&assignment.assignment_id]
            .targets
            .iter()
            .map(|target| ReviewRecord {
                review_id: ReviewId::generate(),
                target: target.clone(),
                reviewer_user_id: assignment.assigned_to.clone(),
                decision: if matches!(
                    target,
                    ReviewTarget::Task { .. } | ReviewTarget::MigrationConfirmation { .. }
                ) {
                    decision.clone()
                } else {
                    ReviewDecision::Approved
                },
                timestamp: now(),
                comment: None,
            })
            .collect(),
    }
}

fn review_context(assignment: &Assignment) -> AssignmentContext<'_> {
    AssignmentContext {
        assignment_id: &assignment.assignment_id,
        image_id: &assignment.image_id,
        task_id: &assignment.task_id,
        kind: AssignmentKind::Review,
    }
}

async fn finalize_test_review(
    repo: &DatasetRepository,
    assignment: &Assignment,
    decision: ReviewDecision,
) -> labello_domain::ImageState {
    let state = repo.load_image_state(&assignment.image_id).await.unwrap();
    let metadata = repo.load_dataset_config().await.unwrap();
    if decision == ReviewDecision::Rejected {
        // Historical fixture for reopening old decision-only rejections.
        let mut finished = assignment.clone();
        finished.status = AssignmentStatus::Completed;
        let (_, state) = repo
            .append_payloads_with_state_unlocked(
                &assignment.image_id,
                &Actor {
                    user_id: assignment.assigned_to.clone(),
                    role: DatasetRole::Reviewer,
                },
                vec![
                    EventPayload::ReviewRecorded {
                        review: ReviewRecord {
                            review_id: ReviewId::generate(),
                            target: ReviewTarget::Task {
                                task_id: assignment.task_id.clone(),
                            },
                            reviewer_user_id: assignment.assigned_to.clone(),
                            decision,
                            timestamp: now(),
                            comment: None,
                        },
                    },
                    EventPayload::TaskStateChanged {
                        task_state: TaskState {
                            task_id: assignment.task_id.clone(),
                            status: TaskStatus::NeedsCorrection,
                            outcome: None,
                            assigned_to: None,
                            completed_by: None,
                            completed_at: None,
                            updated_at: now(),
                        },
                    },
                    EventPayload::AssignmentUpdated {
                        assignment: finished,
                    },
                ],
            )
            .await
            .unwrap();
        return state;
    }
    for target in state
        .review_object_targets(metadata.task(&assignment.task_id).unwrap())
        .unwrap()
    {
        if state
            .effective_review_for_target(&assignment.task_id, &target, &assignment.assigned_to)
            .is_none()
        {
            repo.record_review_for_assignment(
                &assignment.assigned_to,
                review_context(assignment),
                ReviewRecord {
                    review_id: ReviewId::generate(),
                    target,
                    reviewer_user_id: assignment.assigned_to.clone(),
                    decision: ReviewDecision::Approved,
                    timestamp: now(),
                    comment: None,
                },
            )
            .await
            .unwrap();
        }
    }
    repo.record_review_for_assignment(
        &assignment.assigned_to,
        review_context(assignment),
        ReviewRecord {
            review_id: ReviewId::generate(),
            target: ReviewTarget::Task {
                task_id: assignment.task_id.clone(),
            },
            reviewer_user_id: assignment.assigned_to.clone(),
            decision,
            timestamp: now(),
            comment: None,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn original_submitter_can_reopen_previous_review_and_commit_idempotently() {
    let (_temp, repo, image_id, task_id, annotator, _reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata
        .role_assignments
        .iter_mut()
        .find(|assignment| assignment.user_id == annotator)
        .unwrap()
        .roles
        .insert(DatasetRole::Reviewer);
    repo.save_dataset(&metadata).await.unwrap();

    let original = claim_review(&repo, &image_id, &task_id, &annotator).await;
    let completed = finalize_test_review(&repo, &original, ReviewDecision::Approved).await;
    assert_eq!(
        completed.task_states[&task_id].status,
        TaskStatus::Completed
    );
    assert_eq!(completed.reviews[0].reviewer_user_id, annotator);

    let revision = repo
        .reopen_review_assignment(&annotator, &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    let retry = repo
        .reopen_review_assignment(&annotator, &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    assert_ne!(revision.assignment_id, original.assignment_id);
    assert_eq!(revision, retry);

    let opened = repo.load_image_state(&image_id).await.unwrap();
    let replacement = revision_replacements(&opened, &revision, ReviewDecision::Approved);
    let committed = repo
        .commit_review_revision(&annotator, review_context(&revision), replacement.clone())
        .await
        .unwrap();
    assert_eq!(
        committed.task_states[&task_id].status,
        TaskStatus::Completed
    );
    let events = repo.load_events(&image_id).await.unwrap();
    let retried = repo
        .commit_review_revision(&annotator, review_context(&revision), replacement)
        .await
        .unwrap();
    assert_eq!(retried, committed);
    assert_eq!(repo.load_events(&image_id).await.unwrap(), events);
}

#[tokio::test]
async fn review_revision_requires_corrections_and_resubmits_atomically() {
    let (_temp, repo, image, task, _, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image, &task, &reviewers[0]).await;
    finalize_test_review(&repo, &original, ReviewDecision::Approved).await;
    let revision = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image, &task)
        .await
        .unwrap();
    let before = repo.load_image_state(&image).await.unwrap();
    let replacement = revision_replacements(&before, &revision, ReviewDecision::Rejected);
    assert!(
        repo.commit_review_revision(&reviewers[0], review_context(&revision), replacement)
            .await
            .is_err()
    );
    assert_eq!(repo.load_image_state(&image).await.unwrap(), before);
    let captured = &before.review_assignment_contexts[&revision.assignment_id];
    let submission = labello_domain::ReviewCorrectionSubmission {
        correction_id: CorrectionId::generate(),
        round: captured.round.clone(),
        target_fingerprint: captured.target_fingerprint.clone(),
        reason: None,
        changes: vec![labello_domain::ReviewCorrectionChange::Edit {
            annotation_id: "ann_1".into(),
            expected_version: 1,
            geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.4,
                y: 0.3,
                width: 0.2,
                height: 0.2,
            }),
        }],
    };
    let (first, retry) = tokio::join!(
        repo.submit_review_corrections(
            &reviewers[0],
            review_context(&revision),
            submission.clone()
        ),
        repo.submit_review_corrections(&reviewers[0], review_context(&revision), submission)
    );
    let after = first.unwrap();
    assert_eq!(after, retry.unwrap());
    assert_eq!(after.task_states[&task].status, TaskStatus::Submitted);
    assert_eq!(after.effective_reviews_for_task(&task).count(), 0);
    assert!(
        repo.reopen_review_assignment(&reviewers[0], &revision.assignment_id, &image, &task)
            .await
            .is_err()
    );
    assert_eq!(repo.rebuild_image_state(&image).await.unwrap(), after);
    let fresh = claim_review(&repo, &image, &task, &reviewers[0]).await;
    assert_ne!(fresh.assignment_id, revision.assignment_id);
    let approved = finalize_test_review(&repo, &fresh, ReviewDecision::Approved).await;
    assert_eq!(approved.task_states[&task].status, TaskStatus::Completed);
}

#[tokio::test]
async fn review_revision_rejects_later_work_even_with_equal_event_timestamps() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &original, ReviewDecision::Approved).await;
    let mut state = repo.load_image_state(&image_id).await.unwrap();
    let events = repo.load_events(&image_id).await.unwrap();
    let terminal_timestamp = events.last().unwrap().timestamp;
    let event = EventLogEntry::new(
        0,
        image_id.clone(),
        reviewers[1].clone(),
        DatasetRole::Reviewer,
        terminal_timestamp,
        EventPayload::ReviewRecorded {
            review: ReviewRecord {
                review_id: ReviewId::generate(),
                target: ReviewTarget::AnnotationVersion {
                    annotation_id: AnnotationId::from("ann_1"),
                    version: 1,
                },
                reviewer_user_id: reviewers[1].clone(),
                decision: ReviewDecision::Approved,
                timestamp: terminal_timestamp,
                comment: None,
            },
        },
    );
    repo.append_resequenced_events(&image_id, &mut state, &[event])
        .await
        .unwrap();
    let before = state.clone();
    assert!(
        repo.reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
            .await
            .is_err()
    );
    assert_eq!(repo.load_image_state(&image_id).await.unwrap(), before);
}

#[tokio::test]
async fn review_revision_rejects_expired_later_attempt_and_changed_configuration() {
    let (_temp, repo, image_id, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.tasks[0].name.push_str(" changed");
    repo.save_dataset(&metadata).await.unwrap();
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &original, ReviewDecision::Rejected).await;
    let later = repo
        .assign_next_image(&annotator, &task_id, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    expire_assignment(&repo, &later, &annotator).await;
    assert!(
        repo.reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
            .await
            .is_err()
    );

    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &original, ReviewDecision::Rejected).await;
    let revision = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    let before = repo.load_image_state(&image_id).await.unwrap();
    let replacement = revision_replacements(&before, &revision, ReviewDecision::Approved);
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.tasks[0].name.push_str(" changed");
    repo.save_dataset(&metadata).await.unwrap();
    assert!(
        repo.commit_review_revision(&reviewers[0], review_context(&revision), replacement)
            .await
            .is_err()
    );
    assert_eq!(repo.load_image_state(&image_id).await.unwrap(), before);
}

#[tokio::test]
async fn review_revision_replay_rejects_incomplete_targets_and_supersession_and_upgrades_cache() {
    let (temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let before = finalize_test_review(&repo, &original, ReviewDecision::Approved).await;
    let revision = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    let events = repo.load_events(&image_id).await.unwrap();
    let opening = events.last().unwrap().clone();
    let mut missing_target = opening.clone();
    let EventPayload::ReviewAssignmentOpened { context, .. } = &mut missing_target.payload else {
        panic!("opening");
    };
    context.targets.remove(0);
    let mut projected = before.clone();
    assert!(projected.apply_event(&missing_target).is_err());
    assert_eq!(projected, before);
    let mut missing_supersession = opening.clone();
    let EventPayload::ReviewAssignmentOpened { context, .. } = &mut missing_supersession.payload
    else {
        panic!("opening");
    };
    context.superseded_review_ids.clear();
    assert!(projected.apply_event(&missing_supersession).is_err());
    let encoded = serde_json::to_vec(&opening).unwrap();
    assert_eq!(
        serde_json::from_slice::<EventLogEntry>(&encoded).unwrap(),
        opening
    );
    let mut legacy = opening;
    legacy.schema_version = labello_domain::LEGACY_SCHEMA_VERSION;
    assert!(legacy.validate_shape().is_err());
    assert!(serde_json::to_vec(&legacy).is_err());
    let expected = repo.load_image_state(&image_id).await.unwrap();
    let mut old_cache = serde_json::to_value(&expected).unwrap();
    old_cache
        .as_object_mut()
        .unwrap()
        .remove("reviewProjectionVersion");
    old_cache.as_object_mut().unwrap().remove("reviewRounds");
    crate::fsjson::write_json_atomic(&repo.state_path(&image_id), &old_cache)
        .await
        .unwrap();
    let restarted = DatasetRepository::new(temp.path());
    assert_eq!(
        restarted.load_image_state(&image_id).await.unwrap(),
        expected
    );
    assert!(expected.review_assignment_contexts[&revision.assignment_id].decision_revision);
}

#[tokio::test]
async fn image_scoped_revalidation_refreshes_state_and_rejects_completed_review_work() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let competing = claim_review(&repo, &image_id, &task_id, &reviewers[1]).await;
    let queued = historical_review_assignment(&repo, &image_id, &task_id, &reviewers[0]).await;
    let sequence_before = repo
        .load_image_state(&image_id)
        .await
        .unwrap()
        .current_sequence;

    let (renewed, refreshed) = repo
        .revalidate_assignment_on_image(
            &reviewers[0],
            &queued.assignment_id,
            &image_id,
            &task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(renewed.assignment_id, queued.assignment_id);
    assert!(renewed.updated_at >= queued.updated_at);
    assert_eq!(refreshed.current_sequence, sequence_before + 1);
    assert_eq!(refreshed.active_annotations().count(), 1);
    assert!(refreshed.assignments.contains(&renewed));

    approve_test_objects(&repo, &competing).await;
    repo.record_review_for_assignment(
        &reviewers[1],
        AssignmentContext {
            assignment_id: &competing.assignment_id,
            image_id: &image_id,
            task_id: &task_id,
            kind: AssignmentKind::Review,
        },
        ReviewRecord {
            review_id: ReviewId::from("rev_competing_final"),
            target: ReviewTarget::Task {
                task_id: task_id.clone(),
            },
            reviewer_user_id: reviewers[1].clone(),
            decision: ReviewDecision::Approved,
            timestamp: now(),
            comment: None,
        },
    )
    .await
    .unwrap();

    assert!(
        repo.revalidate_assignment_on_image(
            &reviewers[0],
            &queued.assignment_id,
            &image_id,
            &task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        repo.assign_next_image(&reviewers[0], &task_id, AssignmentKind::Review)
            .await
            .unwrap()
            .is_none(),
        "an active but stale review lease must not be reclaimed before its delayed release"
    );
    assert!(
        repo.release_assignment(
            &reviewers[0],
            &queued.assignment_id,
            &image_id,
            &task_id,
            AssignmentKind::Review
        )
        .await
        .is_err()
    );
    assert_eq!(
        assignment_status(&repo.load_image_state(&image_id).await.unwrap(), &queued),
        AssignmentStatus::Cancelled
    );
}

#[tokio::test]
async fn image_scoped_revalidation_rejects_a_disabled_review_workflow_without_renewal() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let queued = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    let task = metadata
        .tasks
        .iter_mut()
        .find(|task| task.task_id == task_id)
        .unwrap();
    task.review.workflow = ReviewWorkflow::None;

    repo.save_dataset(&metadata).await.unwrap();
    let sequence_before = repo
        .load_image_state(&image_id)
        .await
        .unwrap()
        .current_sequence;

    assert!(
        repo.revalidate_assignment_on_image(
            &reviewers[0],
            &queued.assignment_id,
            &image_id,
            &task_id,
            AssignmentKind::Review,
        )
        .await
        .unwrap()
        .is_none()
    );
    let state = repo.load_image_state(&image_id).await.unwrap();
    assert_eq!(state.current_sequence, sequence_before);
    assert_eq!(
        state
            .assignments
            .iter()
            .find(|assignment| assignment.assignment_id == queued.assignment_id)
            .unwrap(),
        &queued
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "the test helper exposes every correction input varied by its callers"
)]
async fn correct(
    repo: &DatasetRepository,
    image_id: &ImageId,
    task_id: &TaskId,
    reviewer: &UserId,
    assignment: &Assignment,
    correction_id: &CorrectionId,
    expected_version: u32,
    geometry: AnnotationGeometry,
) -> StorageResult<EventLogEntry> {
    repo.correct_review_annotation(
        reviewer,
        AssignmentContext {
            assignment_id: &assignment.assignment_id,
            image_id,
            task_id,
            kind: AssignmentKind::Review,
        },
        correction_id,
        &AnnotationId::from("ann_1"),
        expected_version,
        geometry,
        Some("quality correction".to_string()),
    )
    .await
}

async fn approve_test_objects(repo: &DatasetRepository, assignment: &Assignment) {
    let state = repo.load_image_state(&assignment.image_id).await.unwrap();
    let metadata = repo.load_dataset_config().await.unwrap();
    for target in state
        .review_object_targets(metadata.task(&assignment.task_id).unwrap())
        .unwrap()
    {
        if state
            .effective_review_for_target(&assignment.task_id, &target, &assignment.assigned_to)
            .is_none()
        {
            repo.record_review_for_assignment(
                &assignment.assigned_to,
                review_context(assignment),
                ReviewRecord {
                    review_id: ReviewId::generate(),
                    target,
                    reviewer_user_id: assignment.assigned_to.clone(),
                    decision: ReviewDecision::Approved,
                    timestamp: now(),
                    comment: None,
                },
            )
            .await
            .unwrap();
        }
    }
}

async fn record_task_approval(
    repo: &DatasetRepository,
    image_id: &ImageId,
    task_id: &TaskId,
    reviewer: &UserId,
    assignment: &Assignment,
    review_id: &str,
) -> StorageResult<labello_domain::ImageState> {
    approve_test_objects(repo, assignment).await;
    repo.record_review_for_assignment(
        reviewer,
        AssignmentContext {
            assignment_id: &assignment.assignment_id,
            image_id,
            task_id,
            kind: AssignmentKind::Review,
        },
        ReviewRecord {
            review_id: ReviewId::from(review_id),
            target: ReviewTarget::Task {
                task_id: task_id.clone(),
            },
            reviewer_user_id: reviewer.clone(),
            decision: ReviewDecision::Approved,
            timestamp: now(),
            comment: None,
        },
    )
    .await
}

fn assignment_status(
    state: &labello_domain::ImageState,
    assignment: &Assignment,
) -> AssignmentStatus {
    state
        .assignments
        .iter()
        .find(|candidate| candidate.assignment_id == assignment.assignment_id)
        .unwrap()
        .status
        .clone()
}

async fn correction_repo(
    annotation_type: AnnotationType,
    allow_reviewer_corrections: bool,
) -> (
    tempfile::TempDir,
    DatasetRepository,
    ImageId,
    TaskId,
    UserId,
    [UserId; 2],
) {
    let temp = tempfile::tempdir().unwrap();
    let repo = DatasetRepository::new(temp.path());
    let image_id = ImageId::from("img_1");
    let task_id = TaskId::from(match annotation_type {
        AnnotationType::BoundingBox => "bounding_box:person",
        AnnotationType::Skeleton => "skeleton:person",
    });
    let class_id = ClassId::from("person");
    let annotator = UserId::from("annotator");
    let reviewers = [UserId::from("reviewer_1"), UserId::from("reviewer_2")];
    let mut metadata = DatasetMetadata::new(DatasetId::from("ds"), "Dataset", now());
    metadata.label_classes.push(LabelClass {
        class_id: class_id.clone(),
        name: "Person".to_string(),
        color: "#5eead4".to_string(),
        description: None,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: task_id.clone(),
        name: "Person annotation".to_string(),
        annotation_type: annotation_type.clone(),
        class_ids: vec![class_id.clone()],
        instructions: TutorialContent {
            title: "Instructions".to_string(),
            example_text: "Annotate the person.".to_string(),
            example_images: Vec::new(),
        },
        skeleton: (annotation_type == AnnotationType::Skeleton).then_some(SkeletonSpec {
            keypoints: vec![KeypointSpec {
                name: "nose".to_string(),
                required: true,
            }],
            edges: Vec::new(),
            allow_hidden: true,
            allow_absent: false,
        }),
        review: ReviewConfig {
            workflow: ReviewWorkflow::Approval,
            allow_reviewer_corrections,
            legacy: None,
        },
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: None,
        enabled: true,
    });
    metadata.role_assignments.push(DatasetRoleAssignment {
        dataset_id: metadata.dataset_id.clone(),
        user_id: annotator.clone(),
        roles: BTreeSet::from([DatasetRole::Annotator]),
        assigned_at: now(),
        assigned_by: None,
    });
    metadata
        .role_assignments
        .extend(reviewers.iter().map(|reviewer| DatasetRoleAssignment {
            dataset_id: metadata.dataset_id.clone(),
            user_id: reviewer.clone(),
            roles: BTreeSet::from([DatasetRole::Reviewer]),
            assigned_at: now(),
            assigned_by: None,
        }));
    repo.initialize(metadata).await.unwrap();
    repo.save_images_index(&ImagesIndex {
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
    let timestamp = now();
    let geometry = match annotation_type {
        AnnotationType::BoundingBox => AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.1,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        }),
        AnnotationType::Skeleton => AnnotationGeometry::Skeleton(SkeletonGeometry {
            keypoints: vec![KeypointAnnotation {
                name: "nose".to_string(),
                state: KeypointState::Visible,
                point: Some(NormalizedPoint { x: 0.5, y: 0.5 }),
            }],
        }),
    };
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: annotator.clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::AnnotationVersionCreated {
            annotation: AnnotationVersion {
                annotation_id: AnnotationId::from("ann_1"),
                version: 1,
                object_group_id: None,
                origin: AnnotationOrigin::native(),
                task_id: task_id.clone(),
                class_id,
                annotation_type,
                revision_source: RevisionSource::Human {
                    action: HumanRevisionKind::Authored,
                },
                geometry,
                author_user_id: annotator.clone(),
                created_at: timestamp,
                updated_at: timestamp,
                deleted: false,
            },
            previous_version: None,
            reason: None,
        },
    )
    .await
    .unwrap();
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: annotator.clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task_id.clone(),
                status: TaskStatus::Submitted,
                outcome: None,
                assigned_to: None,
                completed_by: Some(annotator.clone()),
                completed_at: Some(timestamp),
                updated_at: timestamp,
            },
        },
    )
    .await
    .unwrap();
    (temp, repo, image_id, task_id, annotator, reviewers)
}

async fn add_submitted_review_image(
    repo: &DatasetRepository,
    image_id: &ImageId,
    task_id: &TaskId,
    annotator: &UserId,
) {
    let mut index = repo.load_images_index().await.unwrap();
    index.image_count += 1;
    index.images_by_hash.insert(
        format!("hash_{}", image_id.as_str()),
        ImageRecord {
            image_id: image_id.clone(),
            blake3: format!("hash_{}", image_id.as_str()),
            canonical_path: format!("images/{}.png", image_id.as_str()),
            known_paths: vec![format!("images/{}.png", image_id.as_str())],
            duplicate_paths: Vec::new(),
            file_name: format!("{}.png", image_id.as_str()),
            byte_size: 4,
            width: 100,
            height: 100,
            media_type: "image/png".to_string(),
            source_memberships: None,
        },
    );
    repo.save_images_index(&index).await.unwrap();
    let timestamp = now();
    repo.append_payload(
        image_id,
        &Actor {
            user_id: annotator.clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task_id.clone(),
                status: TaskStatus::Submitted,
                outcome: None,
                assigned_to: None,
                completed_by: Some(annotator.clone()),
                completed_at: Some(timestamp),
                updated_at: timestamp,
            },
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn imbalance_blocks_positive_task_at_zero_peer_and_ignores_disabled_peer() {
    let (_temp, repo, first_task_id, users) = annotation_repo(2, &["annotator"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    let mut second_task = metadata.tasks[0].clone();
    second_task.task_id = TaskId::from("bounding_box:second");
    second_task.name = "Second boxes".to_string();
    metadata.tasks.push(second_task.clone());
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: 0,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();
    repo.append_payload(
        &ImageId::from("img_0"),
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: first_task_id.clone(),
                status: TaskStatus::Completed,
                outcome: Some(TaskOutcome::AnnotationCompleted),
                assigned_to: None,
                completed_by: Some(users[0].clone()),
                completed_at: Some(now()),
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();

    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_none(),
        "a positive task must be blocked while an enabled peer is zero"
    );
    assert_eq!(repo.task_completion_cache.scan_count(), 1);

    metadata.tasks[1].enabled = false;
    repo.save_dataset(&metadata).await.unwrap();
    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_some(),
        "a disabled peer must not participate in imbalance"
    );
    assert_eq!(
        repo.task_completion_cache.scan_count(),
        1,
        "enabling or disabling tasks must not reconstruct completion counts"
    );
}

#[tokio::test]
async fn imbalance_allows_two_zero_tasks_and_a_single_enabled_task() {
    let (_temp, repo, first_task_id, users) = annotation_repo(2, &["annotator"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    let mut second_task = metadata.tasks[0].clone();
    second_task.task_id = TaskId::from("bounding_box:second");
    second_task.name = "Second boxes".to_string();
    metadata.tasks.push(second_task);
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: 0,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();

    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_some()
    );

    metadata.tasks[1].enabled = false;
    repo.save_dataset(&metadata).await.unwrap();
    repo.append_payload(
        &ImageId::from("img_0"),
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: first_task_id.clone(),
                status: TaskStatus::Completed,
                outcome: Some(TaskOutcome::AnnotationCompleted),
                assigned_to: None,
                completed_by: Some(users[0].clone()),
                completed_at: Some(now()),
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();
    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_some(),
        "a dataset with one enabled task has no imbalance peer"
    );
}

#[tokio::test]
async fn imbalance_counts_submitted_for_annotation_but_not_review() {
    let (_temp, repo, first_task_id, users) = annotation_repo(2, &["worker"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[0]
        .roles
        .insert(DatasetRole::Reviewer);
    let mut second_task = metadata.tasks[0].clone();
    second_task.task_id = TaskId::from("bounding_box:second");
    second_task.name = "Second boxes".to_string();
    let second_task_id = second_task.task_id.clone();
    metadata.tasks.push(second_task);
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: 0,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();
    repo.append_payload(
        &ImageId::from("img_0"),
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: first_task_id.clone(),
                status: TaskStatus::Submitted,
                outcome: None,
                assigned_to: None,
                completed_by: Some(users[0].clone()),
                completed_at: Some(now()),
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();

    let annotation = repo
        .assignment_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(!annotation[&first_task_id]);
    assert!(annotation[&second_task_id]);

    let review = repo
        .assignment_availability(&users[0], AssignmentKind::Review)
        .await
        .unwrap();
    assert!(review[&first_task_id]);
    assert!(!review[&second_task_id]);
}

#[tokio::test]
async fn imbalance_uses_strict_absolute_window_boundary() {
    let (_temp, repo, first_task_id, users) = annotation_repo(3, &["annotator"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    let mut second_task = metadata.tasks[0].clone();
    second_task.task_id = TaskId::from("bounding_box:second");
    second_task.name = "Second boxes".to_string();
    let second_task_id = second_task.task_id.clone();
    metadata.tasks.push(second_task);
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: 1,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();
    let actor = Actor {
        user_id: users[0].clone(),
        role: DatasetRole::Annotator,
    };
    for (image_id, task_id) in [
        ("img_0", first_task_id.clone()),
        ("img_1", first_task_id.clone()),
        ("img_0", second_task_id),
    ] {
        repo.append_payload(
            &ImageId::from(image_id),
            &actor,
            EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id,
                    status: TaskStatus::Completed,
                    outcome: Some(TaskOutcome::AnnotationCompleted),
                    assigned_to: None,
                    completed_by: Some(users[0].clone()),
                    completed_at: Some(now()),
                    updated_at: now(),
                },
            },
        )
        .await
        .unwrap();
    }

    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_some(),
        "a gap equal to maxDifference is allowed"
    );
    metadata.imbalance.as_mut().unwrap().max_difference = 0;
    repo.save_dataset(&metadata).await.unwrap();
    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_none(),
        "a gap strictly above maxDifference is blocked"
    );
}

#[tokio::test]
async fn absolute_window_matches_availability_queue_claims_and_statistics_across_transitions() {
    let (_temp, repo, first_task_id, users) = annotation_repo(4, &["annotator"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    let mut second_task = metadata.tasks[0].clone();
    second_task.task_id = TaskId::from("bounding_box:second");
    second_task.name = "Second boxes".to_string();
    let second_task_id = second_task.task_id.clone();
    metadata.tasks.push(second_task);
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: 2,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();
    let actor = Actor {
        user_id: users[0].clone(),
        role: DatasetRole::Annotator,
    };
    for image_id in ["img_0", "img_1"] {
        repo.append_payload(
            &ImageId::from(image_id),
            &actor,
            EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: first_task_id.clone(),
                    status: TaskStatus::Completed,
                    outcome: Some(TaskOutcome::AnnotationCompleted),
                    assigned_to: None,
                    completed_by: Some(users[0].clone()),
                    completed_at: Some(now()),
                    updated_at: now(),
                },
            },
        )
        .await
        .unwrap();
    }

    let boundary_availability = repo
        .assignment_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(boundary_availability[&first_task_id]);
    let boundary_stats = repo.dataset_stats().await.unwrap();
    let boundary_balance = boundary_stats.assignment_balance.unwrap();
    assert_eq!(boundary_balance.annotation_counts[&first_task_id], 2);
    assert_eq!(boundary_balance.annotation_counts[&second_task_id], 0);
    assert!(boundary_balance.annotation_blocked_tasks.is_empty());

    let queued = repo
        .assign_next_image_excluding(&users[0], &first_task_id, AssignmentKind::Annotation, &[])
        .await
        .unwrap()
        .expect("the exact absolute-window boundary remains queue-eligible");
    repo.append_payload(
        &queued.image_id,
        &actor,
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: first_task_id.clone(),
                status: TaskStatus::Completed,
                outcome: Some(TaskOutcome::AnnotationCompleted),
                assigned_to: None,
                completed_by: Some(users[0].clone()),
                completed_at: Some(now()),
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();

    let blocked_availability = repo
        .assignment_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(!blocked_availability[&first_task_id]);
    assert!(blocked_availability[&second_task_id]);
    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_none()
    );
    let blocked_stats = repo.dataset_stats().await.unwrap();
    let blocked_balance = blocked_stats.assignment_balance.unwrap();
    assert_eq!(blocked_balance.annotation_counts[&first_task_id], 3);
    assert_eq!(
        blocked_balance.annotation_blocked_tasks,
        BTreeSet::from([first_task_id.clone()])
    );

    repo.append_payload(
        &ImageId::from("img_3"),
        &actor,
        EventPayload::AnnotationVersionCreated {
            annotation: AnnotationVersion {
                annotation_id: AnnotationId::from("disabled_task_annotation"),
                version: 1,
                object_group_id: None,
                origin: AnnotationOrigin::native(),
                task_id: second_task_id,
                class_id: metadata.tasks[1].class_ids[0].clone(),
                annotation_type: AnnotationType::BoundingBox,
                revision_source: RevisionSource::Human {
                    action: HumanRevisionKind::Authored,
                },
                geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                    x: 0.1,
                    y: 0.1,
                    width: 0.2,
                    height: 0.2,
                }),
                author_user_id: users[0].clone(),
                created_at: now(),
                updated_at: now(),
                deleted: false,
            },
            previous_version: None,
            reason: None,
        },
    )
    .await
    .unwrap();
    metadata.tasks[1].enabled = false;
    repo.save_dataset(&metadata).await.unwrap();
    let single_peer_stats = repo.dataset_stats().await.unwrap();
    assert!(
        single_peer_stats
            .assignment_balance
            .unwrap()
            .annotation_blocked_tasks
            .is_empty()
    );
    assert!(
        repo.assign_next_image(&users[0], &first_task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn global_task_imbalance_can_block_review_work() {
    let (_temp, repo, selected_task_id, users) = annotation_repo(2, &["reviewer"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[0].roles = BTreeSet::from([DatasetRole::Reviewer]);
    let mut peer = metadata.tasks[0].clone();
    peer.task_id = TaskId::from("bounding_box:peer");
    peer.name = "Peer boxes".to_string();
    metadata.tasks.push(peer);
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: 0,
        enforce: true,
    });
    repo.save_dataset(&metadata).await.unwrap();
    let timestamp = now();
    for (image_id, status) in [
        ("img_0", TaskStatus::Completed),
        ("img_1", TaskStatus::Submitted),
    ] {
        repo.append_payload(
            &ImageId::from(image_id),
            &Actor {
                user_id: users[0].clone(),
                role: DatasetRole::Reviewer,
            },
            EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: selected_task_id.clone(),
                    outcome: (status == TaskStatus::Completed)
                        .then_some(TaskOutcome::AnnotationCompleted),
                    status,
                    assigned_to: None,
                    completed_by: Some(users[0].clone()),
                    completed_at: Some(timestamp),
                    updated_at: timestamp,
                },
            },
        )
        .await
        .unwrap();
    }

    assert!(
        repo.assign_next_image(&users[0], &selected_task_id, AssignmentKind::Review)
            .await
            .unwrap()
            .is_none(),
        "global enabled-task imbalance intentionally applies to review claims"
    );
}

#[tokio::test]
async fn disabled_imbalance_enforcement_does_not_initialize_projection() {
    let (_temp, repo, task_id, users) = annotation_repo(1, &["annotator"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: 0,
        enforce: false,
    });
    repo.save_dataset(&metadata).await.unwrap();

    assert!(
        repo.assign_next_image(&users[0], &task_id, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(repo.task_completion_cache.scan_count(), 0);
}

async fn annotation_repo(
    image_count: usize,
    user_names: &[&str],
) -> (tempfile::TempDir, DatasetRepository, TaskId, Vec<UserId>) {
    let temp = tempfile::tempdir().unwrap();
    let repo = DatasetRepository::new(temp.path());
    let task_id = TaskId::from("bounding_box:person");
    let class_id = ClassId::from("person");
    let users = user_names
        .iter()
        .map(|user| UserId::from(*user))
        .collect::<Vec<_>>();
    let mut metadata = DatasetMetadata::new(DatasetId::from("ds"), "Dataset", now());
    metadata.label_classes.push(LabelClass {
        class_id: class_id.clone(),
        name: "Person".to_string(),
        color: "#5eead4".to_string(),
        description: None,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: task_id.clone(),
        name: "Person boxes".to_string(),
        annotation_type: AnnotationType::BoundingBox,
        class_ids: vec![class_id],
        instructions: TutorialContent {
            title: "Instructions".to_string(),
            example_text: "Draw boxes.".to_string(),
            example_images: Vec::new(),
        },
        skeleton: None,
        review: ReviewConfig::default(),
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: None,
        enabled: true,
    });
    metadata
        .role_assignments
        .extend(users.iter().map(|user_id| DatasetRoleAssignment {
            dataset_id: metadata.dataset_id.clone(),
            user_id: user_id.clone(),
            roles: BTreeSet::from([DatasetRole::Annotator]),
            assigned_at: now(),
            assigned_by: None,
        }));
    repo.initialize(metadata).await.unwrap();
    repo.save_images_index(&ImagesIndex {
        schema_version: SCHEMA_VERSION,
        image_count,
        images_by_hash: (0..image_count)
            .map(|index| {
                let image_id = ImageId::from(format!("img_{index}"));
                (
                    format!("hash_{index}"),
                    ImageRecord {
                        image_id,
                        blake3: format!("hash_{index}"),
                        canonical_path: format!("images/{index}.png"),
                        known_paths: vec![format!("images/{index}.png")],
                        duplicate_paths: Vec::new(),
                        file_name: format!("{index}.png"),
                        byte_size: 4,
                        width: 2,
                        height: 2,
                        media_type: "image/png".to_string(),
                        source_memberships: None,
                    },
                )
            })
            .collect(),
    })
    .await
    .unwrap();
    (temp, repo, task_id, users)
}

async fn expire_assignment(repo: &DatasetRepository, assignment: &Assignment, user_id: &UserId) {
    let mut expired = assignment.clone();
    expired.expires_at = Some(now() - std::time::Duration::from_secs(1));
    repo.append_payload(
        &assignment.image_id,
        &Actor {
            user_id: user_id.clone(),
            role: role_for_kind(&assignment.kind),
        },
        EventPayload::AssignmentUpdated {
            assignment: expired,
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn review_revision_of_corrected_geometry_preserves_audit_snapshot_and_offline_state() {
    let (_temp, repo, image_id, task_id, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, true).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    let geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.3,
        y: 0.2,
        width: 0.25,
        height: 0.3,
    });
    correct(
        &repo,
        &image_id,
        &task_id,
        &reviewers[0],
        &original,
        &CorrectionId::from("revision_original_correction"),
        1,
        geometry.clone(),
    )
    .await
    .unwrap();
    let corrected = repo.load_image_state(&image_id).await.unwrap();
    assert_eq!(
        corrected.task_states[&task_id].status,
        TaskStatus::Submitted
    );
    assert!(
        repo.reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
            .await
            .is_err()
    );
    let fresh = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &fresh, ReviewDecision::Approved).await;
    let revision = repo
        .reopen_review_assignment(&reviewers[0], &fresh.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    let opened = repo.load_image_state(&image_id).await.unwrap();
    assert_eq!(opened.annotations, corrected.annotations);
    assert!(
        opened.review_assignment_contexts[&revision.assignment_id]
            .targets
            .contains(&ReviewTarget::AnnotationVersion {
                annotation_id: AnnotationId::from("ann_1"),
                version: 2
            })
    );
    assert!(
        correct(
            &repo,
            &image_id,
            &task_id,
            &reviewers[0],
            &revision,
            &CorrectionId::from("revision_forbidden_correction"),
            2,
            geometry
        )
        .await
        .is_err()
    );
    let replacement = revision_replacements(&opened, &revision, ReviewDecision::Approved);
    let committed = repo
        .commit_review_revision(&reviewers[0], review_context(&revision), replacement)
        .await
        .unwrap();
    assert_eq!(
        committed.task_states[&task_id].status,
        TaskStatus::Completed
    );
    assert_eq!(committed.annotations, corrected.annotations);
    assert_eq!(
        committed.reviewer_corrections,
        corrected.reviewer_corrections
    );
    assert_eq!(
        repo.rebuild_image_state(&image_id).await.unwrap(),
        committed
    );
    let bundle = repo
        .create_offline_bundle(&annotator, 10, false)
        .await
        .unwrap();
    let roundtrip: labello_domain::OfflineBundle =
        serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert_eq!(roundtrip.images[0].state, committed);
    let snapshot = repo.create_snapshot().await.unwrap();
    let file = snapshot
        .files
        .iter()
        .find(|file| file.path.ends_with("/state.json"))
        .unwrap();
    let saved: labello_domain::ImageState = serde_json::from_slice(
        &tokio::fs::read(
            repo.snapshots_dir()
                .join(&snapshot.snapshot_id)
                .join(&file.path),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(saved, committed);
}

#[tokio::test]
async fn review_revision_expired_lease_cannot_commit_and_config_publication_waits_for_validation() {
    let (_temp, repo, image_id, task_id, _annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let original = claim_review(&repo, &image_id, &task_id, &reviewers[0]).await;
    finalize_test_review(&repo, &original, ReviewDecision::Approved).await;
    let mut revision = repo
        .reopen_review_assignment(&reviewers[0], &original.assignment_id, &image_id, &task_id)
        .await
        .unwrap();
    let opened = repo.load_image_state(&image_id).await.unwrap();
    let replacement = revision_replacements(&opened, &revision, ReviewDecision::Rejected);
    revision.expires_at = Some(now() - std::time::Duration::from_secs(1));
    repo.append_payload(
        &image_id,
        &Actor {
            user_id: reviewers[0].clone(),
            role: DatasetRole::Reviewer,
        },
        EventPayload::AssignmentUpdated {
            assignment: revision.clone(),
        },
    )
    .await
    .unwrap();
    let expired = repo.load_image_state(&image_id).await.unwrap();
    assert!(
        repo.commit_review_revision(&reviewers[0], review_context(&revision), replacement)
            .await
            .is_err()
    );
    assert_eq!(repo.load_image_state(&image_id).await.unwrap(), expired);
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].name.push_str(" changed");
    let read_guard = repo.review_config_lock.read().await;
    let write = repo.save_dataset(&metadata);
    tokio::pin!(write);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut write)
            .await
            .is_err()
    );
    assert_ne!(
        repo.load_dataset_config().await.unwrap().tasks[0],
        metadata.tasks[0]
    );
    drop(read_guard);
    write.await.unwrap();
    assert_eq!(
        repo.load_dataset_config().await.unwrap().tasks[0],
        metadata.tasks[0]
    );
}

#[tokio::test]
async fn presence_tracks_claim_release_expiry_and_restart_without_renewing_leases() {
    let (temp, repo, task, users) = annotation_repo(3, &["alice", "bob"]).await;
    assert!(repo.active_lease_holders().await.unwrap().is_empty());
    let a = repo
        .assign_next_image(&users[0], &task, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    let b = repo
        .assign_next_image(&users[1], &task, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repo.active_lease_holders().await.unwrap().len(), 2);
    assert_eq!(
        repo.active_lease_holders().await.unwrap()[&users[0]],
        a.expires_at.unwrap()
    );
    repo.release_assignment(
        &users[0],
        &a.assignment_id,
        &a.image_id,
        &task,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    assert_eq!(repo.active_lease_holders().await.unwrap().len(), 1);
    expire_assignment(&repo, &b, &users[1]).await;
    assert!(repo.active_lease_holders().await.unwrap().is_empty());
    assert!(
        DatasetRepository::new(temp.path())
            .active_lease_holders()
            .await
            .unwrap()
            .is_empty()
    );
    let c = repo
        .assign_next_image(&users[1], &task, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        DatasetRepository::new(temp.path())
            .active_lease_holders()
            .await
            .unwrap()[&users[1]],
        c.expires_at.unwrap()
    );
}

#[tokio::test]
async fn correction_rounds_allow_same_reviewer_and_require_all_current_reviews() {
    for annotation_type in [AnnotationType::BoundingBox, AnnotationType::Skeleton] {
        let (_temp, repo, image, task, _, reviewers) =
            correction_repo(annotation_type.clone(), false).await;
        let metadata = repo.load_dataset_config().await.unwrap();
        let mut assignment = claim_review(&repo, &image, &task, &reviewers[0]).await;
        for cycle in 0..2 {
            let before = repo.load_image_state(&image).await.unwrap();
            let captured = before.review_assignment_contexts[&assignment.assignment_id].clone();
            let geometry = match annotation_type {
                AnnotationType::BoundingBox => AnnotationGeometry::BoundingBox(BoundingBox {
                    x: 0.3 + cycle as f32 * 0.1,
                    y: 0.2,
                    width: 0.2,
                    height: 0.2,
                }),
                AnnotationType::Skeleton => AnnotationGeometry::Skeleton(SkeletonGeometry {
                    keypoints: vec![KeypointAnnotation {
                        name: "nose".into(),
                        state: KeypointState::Visible,
                        point: Some(NormalizedPoint {
                            x: 0.3 + cycle as f32 * 0.1,
                            y: 0.2,
                        }),
                    }],
                }),
            };
            let submission = labello_domain::ReviewCorrectionSubmission {
                correction_id: CorrectionId::generate(),
                round: captured.round.clone(),
                target_fingerprint: captured.target_fingerprint.clone(),
                changes: vec![labello_domain::ReviewCorrectionChange::Edit {
                    annotation_id: "ann_1".into(),
                    expected_version: cycle + 1,
                    geometry,
                }],
                reason: None,
            };
            let context = || AssignmentContext {
                assignment_id: &assignment.assignment_id,
                image_id: &image,
                task_id: &task,
                kind: AssignmentKind::Review,
            };
            let mut unchanged = submission.clone();
            unchanged.changes.clear();
            assert!(
                repo.submit_review_corrections(&reviewers[0], context(), unchanged)
                    .await
                    .is_err()
            );
            assert_eq!(repo.load_image_state(&image).await.unwrap(), before);
            if cycle == 0 {
                repo.fail_next_state_cache_write_after_completion();
                assert!(
                    repo.submit_review_corrections(&reviewers[0], context(), submission.clone())
                        .await
                        .is_err()
                );
            }
            let after = repo
                .submit_review_corrections(&reviewers[0], context(), submission.clone())
                .await
                .unwrap();
            assert_eq!(after.task_states[&task].status, TaskStatus::Submitted);
            assert_ne!(after.review_round(&task), Some(&captured.round));
            assert_eq!(after.effective_reviews_for_task(&task).count(), 0);
            assert_eq!(
                repo.submit_review_corrections(&reviewers[0], context(), submission.clone())
                    .await
                    .unwrap(),
                after
            );
            let mut changed_retry = submission.clone();
            changed_retry.reason = Some("different retry".into());
            assert!(
                repo.submit_review_corrections(&reviewers[0], context(), changed_retry)
                    .await
                    .is_err()
            );
            let events = repo.load_events(&image).await.unwrap();
            let receipt = events.iter().find(|event| matches!(&event.payload, EventPayload::ReviewCorrectionSubmitted { submission: saved, .. } if saved.correction_id == submission.correction_id)).unwrap();
            assert_eq!(
                serde_json::from_slice::<EventLogEntry>(&serde_json::to_vec(receipt).unwrap())
                    .unwrap(),
                *receipt
            );
            let mut legacy = receipt.clone();
            legacy.schema_version = labello_domain::LEGACY_SCHEMA_VERSION;
            assert!(serde_json::to_vec(&legacy).is_err());
            let pristine = labello_domain::rebuild_state(
                image.clone(),
                &events[..receipt.event_sequence as usize - 1],
            )
            .unwrap();
            for mutation in 0..5 {
                let mut forged = receipt.clone();
                let EventPayload::ReviewCorrectionSubmitted {
                    submission,
                    review,
                    task_state,
                    ..
                } = &mut forged.payload
                else {
                    unreachable!()
                };
                match mutation {
                    0 => submission.changes.clear(),
                    1 => review.timestamp += std::time::Duration::from_secs(1),
                    2 => task_state.status = TaskStatus::Completed,
                    3 => submission.target_fingerprint = "foreign".into(),
                    _ => {
                        if let labello_domain::ReviewCorrectionChange::Edit {
                            expected_version,
                            ..
                        } = &mut submission.changes[0]
                        {
                            *expected_version = 99;
                        }
                    }
                }
                let mut state = pristine.clone();
                assert!(state.apply_event(&forged).is_err());
                assert_eq!(state, pristine);
            }
            for boundary in 0..=events.len() {
                labello_domain::rebuild_state(image.clone(), &events[..boundary]).unwrap();
            }
            assert_eq!(repo.rebuild_image_state(&image).await.unwrap(), after);
            let next = claim_review(&repo, &image, &task, &reviewers[0]).await;
            assert_ne!(next.assignment_id, assignment.assignment_id);
            assignment = next;
        }
        {
            let reviewer = &reviewers[0];
            let review_assignment = assignment.clone();
            let context = || AssignmentContext {
                assignment_id: &review_assignment.assignment_id,
                image_id: &image,
                task_id: &task,
                kind: AssignmentKind::Review,
            };
            let state = repo.load_image_state(&image).await.unwrap();
            let targets = state.review_targets(&metadata.tasks[0]).unwrap();
            let record = |target| ReviewRecord {
                review_id: ReviewId::generate(),
                target,
                reviewer_user_id: reviewer.clone(),
                decision: ReviewDecision::Approved,
                timestamp: now(),
                comment: None,
            };
            assert!(
                repo.record_review_for_assignment(
                    reviewer,
                    context(),
                    record(targets.last().unwrap().clone())
                )
                .await
                .is_err()
            );
            for target in targets {
                repo.record_review_for_assignment(reviewer, context(), record(target))
                    .await
                    .unwrap();
            }
            let expected = TaskStatus::Completed;
            assert_eq!(
                repo.load_image_state(&image).await.unwrap().task_states[&task].status,
                expected
            );
        }
        assert_eq!(repo.dataset_stats().await.unwrap().completed_tasks, 1);
    }
}

#[tokio::test]
async fn correction_add_remove_is_atomic_and_rejects_stale_or_foreign_ownership() {
    let (_temp, repo, image, task, annotator, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let assignment = claim_review(&repo, &image, &task, &reviewers[0]).await;
    let before = repo.load_image_state(&image).await.unwrap();
    let captured = &before.review_assignment_contexts[&assignment.assignment_id];
    let mut submission = labello_domain::ReviewCorrectionSubmission {
        correction_id: CorrectionId::generate(),
        round: captured.round.clone(),
        target_fingerprint: captured.target_fingerprint.clone(),
        reason: None,
        changes: vec![
            labello_domain::ReviewCorrectionChange::Add {
                annotation_id: "missing_object".into(),
                class_id: "person".into(),
                geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                    x: 0.2,
                    y: 0.2,
                    width: 0.3,
                    height: 0.3,
                }),
            },
            labello_domain::ReviewCorrectionChange::Remove {
                annotation_id: "ann_1".into(),
                expected_version: 2,
            },
        ],
    };
    let context = || AssignmentContext {
        assignment_id: &assignment.assignment_id,
        image_id: &image,
        task_id: &task,
        kind: AssignmentKind::Review,
    };
    assert!(
        repo.submit_review_corrections(&reviewers[0], context(), submission.clone())
            .await
            .is_err()
    );
    assert_eq!(repo.load_image_state(&image).await.unwrap(), before);
    submission.changes[1] = labello_domain::ReviewCorrectionChange::Remove {
        annotation_id: "ann_1".into(),
        expected_version: 1,
    };
    assert!(
        repo.submit_review_corrections(&annotator, context(), submission.clone())
            .await
            .is_err()
    );
    assert!(
        repo.submit_review_corrections(&reviewers[1], context(), submission.clone())
            .await
            .is_err()
    );
    let after = repo
        .submit_review_corrections(&reviewers[0], context(), submission)
        .await
        .unwrap();
    assert!(
        after
            .current_annotation(&AnnotationId::from("ann_1"))
            .unwrap()
            .deleted
    );
    assert_eq!(after.active_annotations().count(), 1);
    assert_eq!(
        after
            .current_annotation(&AnnotationId::from("missing_object"))
            .unwrap()
            .author_user_id,
        reviewers[0]
    );
    assert_eq!(after.task_states[&task].status, TaskStatus::Submitted);
}

#[tokio::test]
async fn historical_review_upgrade_preserves_audit_and_recovers_work_across_restart() {
    for (final_approval, object_approval) in [(false, true), (true, true), (true, false)] {
        let (temp, repo, image, task, _annotator, reviewers) =
            correction_repo(AnnotationType::BoundingBox, false).await;
        let timestamp = now();
        let old_assignment = Assignment {
            assignment_id: AssignmentId::generate(),
            image_id: image.clone(),
            task_id: task.clone(),
            assigned_to: reviewers[0].clone(),
            kind: AssignmentKind::Review,
            status: AssignmentStatus::Active,
            expires_at: Some(timestamp + std::time::Duration::from_secs(600)),
            created_at: timestamp,
            updated_at: timestamp,
        };
        let actor = Actor {
            user_id: reviewers[0].clone(),
            role: DatasetRole::Reviewer,
        };
        repo.append_payload(
            &image,
            &actor,
            EventPayload::AssignmentUpdated {
                assignment: old_assignment.clone(),
            },
        )
        .await
        .unwrap();
        let metadata = repo.load_dataset().await.unwrap();
        let current = repo.load_image_state(&image).await.unwrap();
        for target in current.review_targets(&metadata.tasks[0]).unwrap() {
            if (!final_approval && matches!(target, ReviewTarget::Task { .. }))
                || (!object_approval && !matches!(target, ReviewTarget::Task { .. }))
            {
                continue;
            }
            repo.append_payload(
                &image,
                &actor,
                EventPayload::ReviewRecorded {
                    review: ReviewRecord {
                        review_id: ReviewId::generate(),
                        target,
                        reviewer_user_id: reviewers[0].clone(),
                        decision: ReviewDecision::Approved,
                        timestamp,
                        comment: None,
                    },
                },
            )
            .await
            .unwrap();
        }
        let before = tokio::fs::read(repo.events_path(&image)).await.unwrap();
        let mut config = labello_domain::DatasetConfig::from_metadata(&metadata);
        config.review_policy_version = 0;
        config.tasks[0].review.legacy = Some(Box::new(labello_domain::LegacyReviewConfig {
            required_reviews: 3,
            agreement_threshold: None,
        }));
        config.role_assignments[0]
            .roles
            .insert(DatasetRole::LegacyAdjudicator);
        crate::fstoml::write_toml_atomic(&repo.dataset_path(), &config)
            .await
            .unwrap();
        let restarted = DatasetRepository::new(temp.path());
        let upgraded_metadata = restarted.load_dataset().await.unwrap();
        assert!(upgraded_metadata.tasks[0].review.is_current());
        assert!(
            upgraded_metadata
                .role_assignments
                .iter()
                .all(|roles| !roles.roles.contains(&DatasetRole::LegacyAdjudicator))
        );
        let upgraded = restarted.load_image_state(&image).await.unwrap();
        assert_eq!(
            upgraded.task_states[&task].status,
            if final_approval && object_approval {
                TaskStatus::Completed
            } else {
                TaskStatus::Submitted
            }
        );
        assert_eq!(
            assignment_status(&upgraded, &old_assignment),
            AssignmentStatus::Cancelled
        );
        let after = tokio::fs::read(repo.events_path(&image)).await.unwrap();
        assert!(after.starts_with(&before));
        assert_eq!(
            upgraded,
            restarted.rebuild_image_state(&image).await.unwrap()
        );
        let again = DatasetRepository::new(temp.path());
        assert_eq!(upgraded, again.load_image_state(&image).await.unwrap());
        assert_eq!(
            after,
            tokio::fs::read(repo.events_path(&image)).await.unwrap()
        );
        let next = again
            .assign_next_image(&reviewers[0], &task, AssignmentKind::Review)
            .await
            .unwrap();
        if final_approval && object_approval {
            assert!(next.is_none());
        } else {
            let next = next.unwrap();
            assert_ne!(next.assignment_id, old_assignment.assignment_id);
            let completed = finalize_test_review(&again, &next, ReviewDecision::Approved).await;
            assert_eq!(completed.task_states[&task].status, TaskStatus::Completed);
        }
    }
}

pub(super) async fn historical_review_assignment(
    repo: &DatasetRepository,
    image: &ImageId,
    task: &TaskId,
    user: &UserId,
) -> Assignment {
    let timestamp = now();
    let assignment = Assignment {
        assignment_id: AssignmentId::generate(),
        image_id: image.clone(),
        task_id: task.clone(),
        assigned_to: user.clone(),
        kind: AssignmentKind::Review,
        status: AssignmentStatus::Active,
        expires_at: Some(timestamp + std::time::Duration::from_secs(600)),
        created_at: timestamp,
        updated_at: timestamp,
    };
    repo.append_payload(
        image,
        &Actor {
            user_id: user.clone(),
            role: DatasetRole::Reviewer,
        },
        EventPayload::AssignmentUpdated {
            assignment: assignment.clone(),
        },
    )
    .await
    .unwrap();
    assignment
}

#[tokio::test]
async fn retired_pending_work_recovers_after_interrupted_upgrade_without_new_permissions() {
    let (temp, repo, image, task, annotator, _reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let timestamp = now();
    let mut pending = repo.load_image_state(&image).await.unwrap().task_states[&task].clone();
    pending.status = TaskStatus::LegacyAdjudicationRequired;
    pending.outcome = None;
    let retired_user = UserId::from("retired_worker");
    let actor = Actor {
        user_id: retired_user.clone(),
        role: DatasetRole::LegacyAdjudicator,
    };
    repo.append_payload(
        &image,
        &actor,
        EventPayload::TaskStateChanged {
            task_state: pending,
        },
    )
    .await
    .unwrap();
    let assignment = Assignment {
        assignment_id: AssignmentId::generate(),
        image_id: image.clone(),
        task_id: task.clone(),
        assigned_to: retired_user.clone(),
        kind: AssignmentKind::LegacyAdjudication,
        status: AssignmentStatus::Active,
        expires_at: None,
        created_at: timestamp,
        updated_at: timestamp,
    };
    repo.append_payload(
        &image,
        &actor,
        EventPayload::AssignmentUpdated {
            assignment: assignment.clone(),
        },
    )
    .await
    .unwrap();
    let metadata = repo.load_dataset().await.unwrap();
    let mut config = labello_domain::DatasetConfig::from_metadata(&metadata);
    config.review_policy_version = 0;
    config.tasks[0].review.workflow = ReviewWorkflow::LegacyIndependentAgreement;
    config.tasks[0].review.legacy = Some(Box::new(labello_domain::LegacyReviewConfig {
        required_reviews: 2,
        agreement_threshold: None,
    }));
    config.role_assignments.push(DatasetRoleAssignment {
        dataset_id: config.dataset_id.clone(),
        user_id: retired_user.clone(),
        roles: BTreeSet::from([DatasetRole::LegacyAdjudicator]),
        assigned_at: timestamp,
        assigned_by: None,
    });
    crate::fstoml::write_toml_atomic(&repo.dataset_path(), &config)
        .await
        .unwrap();
    let before = tokio::fs::read(repo.events_path(&image)).await.unwrap();
    // Fail after image publication but before the configuration commit marker.
    tokio::fs::remove_file(repo.schema_path()).await.unwrap();
    tokio::fs::create_dir(repo.schema_path()).await.unwrap();
    assert!(repo.upgrade_review_policy().await.is_err());
    let interrupted = tokio::fs::read(repo.events_path(&image)).await.unwrap();
    assert!(interrupted.starts_with(&before));
    assert!(interrupted.len() > before.len());
    tokio::fs::remove_dir(repo.schema_path()).await.unwrap();
    let resumed = DatasetRepository::new(temp.path());
    let metadata = resumed.load_dataset().await.unwrap();
    assert_eq!(metadata.tasks[0].review.workflow, ReviewWorkflow::Approval);
    assert!(
        metadata
            .role_assignments
            .iter()
            .all(|assignment| assignment.user_id != retired_user)
    );
    let state = resumed.load_image_state(&image).await.unwrap();
    assert_eq!(state.task_states[&task].status, TaskStatus::NeedsCorrection);
    assert_eq!(state.task_states[&task].outcome, None);
    assert_eq!(
        assignment_status(&state, &assignment),
        AssignmentStatus::Cancelled
    );
    assert_eq!(
        interrupted,
        tokio::fs::read(repo.events_path(&image)).await.unwrap()
    );
    assert!(
        resumed
            .assign_next_image(&retired_user, &task, AssignmentKind::Review)
            .await
            .is_err()
    );
    assert!(
        resumed
            .assign_next_image(&annotator, &task, AssignmentKind::Annotation)
            .await
            .unwrap()
            .is_some()
    );
    for boundary in 0..=resumed.load_events(&image).await.unwrap().len() {
        let events = resumed.load_events(&image).await.unwrap();
        labello_domain::rebuild_state(image.clone(), &events[..boundary]).unwrap();
    }
}

#[tokio::test]
async fn review_policy_upgrade_preserves_a_fresh_correction_round() {
    let (temp, repo, image, task, _, reviewers) =
        correction_repo(AnnotationType::BoundingBox, false).await;
    let assignment = claim_review(&repo, &image, &task, &reviewers[0]).await;
    correct(
        &repo,
        &image,
        &task,
        &reviewers[0],
        &assignment,
        &CorrectionId::generate(),
        1,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.3,
            y: 0.3,
            width: 0.2,
            height: 0.2,
        }),
    )
    .await
    .unwrap();
    let before = repo.load_image_state(&image).await.unwrap();
    let events = tokio::fs::read(repo.events_path(&image)).await.unwrap();
    let mut config =
        labello_domain::DatasetConfig::from_metadata(&repo.load_dataset().await.unwrap());
    config.review_policy_version = 0;
    config.tasks[0].review.legacy = Some(Box::new(labello_domain::LegacyReviewConfig {
        required_reviews: 2,
        agreement_threshold: None,
    }));
    crate::fstoml::write_toml_atomic(&repo.dataset_path(), &config)
        .await
        .unwrap();
    let restarted = DatasetRepository::new(temp.path());
    assert!(
        restarted.load_dataset().await.unwrap().tasks[0]
            .review
            .is_current()
    );
    assert_eq!(restarted.load_image_state(&image).await.unwrap(), before);
    assert_eq!(
        tokio::fs::read(repo.events_path(&image)).await.unwrap(),
        events
    );
    let stats = restarted.dataset_stats().await.unwrap();
    assert_eq!(stats.awaiting_review_tasks, 1);
    assert_eq!(stats.completed_tasks, 0);
    assert_eq!(stats.needs_correction_tasks, 0);
    let fresh = claim_review(&restarted, &image, &task, &reviewers[0]).await;
    assert_ne!(fresh.assignment_id, assignment.assignment_id);
    let completed = finalize_test_review(&restarted, &fresh, ReviewDecision::Approved).await;
    assert_eq!(completed.task_states[&task].status, TaskStatus::Completed);
    assert_eq!(restarted.dataset_stats().await.unwrap().completed_tasks, 1);
}
