use super::*;

async fn balanced_repo(
    images: usize,
    window: u64,
) -> (
    tempfile::TempDir,
    DatasetRepository,
    TaskId,
    TaskId,
    Vec<UserId>,
) {
    let (temp, repo, task, users) = annotation_repo(images, &["alice", "bob"]).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    let mut peer = metadata.tasks[0].clone();
    peer.task_id = TaskId::from("bounding_box:peer");
    let peer_id = peer.task_id.clone();
    metadata.tasks.push(peer);
    metadata.imbalance = Some(ImbalanceConfig {
        max_difference: window,
        enforce: true,
    });
    for roles in &mut metadata.role_assignments {
        roles.roles.insert(DatasetRole::Reviewer);
    }
    repo.save_dataset(&metadata).await.unwrap();
    (temp, repo, task, peer_id, users)
}

async fn progress(
    repo: &DatasetRepository,
    image: usize,
    task: &TaskId,
    user: &UserId,
    status: TaskStatus,
) {
    repo.append_payload(
        &ImageId::from(format!("img_{image}")),
        &Actor {
            user_id: user.clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task.clone(),
                outcome: (status == TaskStatus::Completed)
                    .then_some(TaskOutcome::AnnotationCompleted),
                status,
                assigned_to: None,
                completed_by: None,
                completed_at: None,
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn preload_projects_current_and_queued_work_includes_equality_and_reuses_counts() {
    let (_temp, repo, task, peer, users) = balanced_repo(20, 5).await;
    for i in 0..12 {
        progress(&repo, i, &task, &users[0], TaskStatus::Submitted).await;
    }
    for i in 0..10 {
        progress(&repo, i, &peer, &users[0], TaskStatus::Submitted).await;
    }
    let current = repo
        .assign_next_image(&users[0], &task, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    let mut excluded = vec![current.image_id.clone()];
    for _ in 0..2 {
        let queued = repo
            .assign_preloaded_image_excluding(
                &users[0],
                &task,
                AssignmentKind::Annotation,
                &excluded,
            )
            .await
            .unwrap()
            .unwrap();
        excluded.push(queued.image_id);
    }
    repo.reset_image_state_load_count();
    assert!(
        repo.assign_preloaded_image_excluding(
            &users[0],
            &task,
            AssignmentKind::Annotation,
            &excluded
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(
        repo.image_state_load_count(),
        0,
        "blocked prefetch must not scan image states"
    );
    assert_eq!(repo.task_completion_cache.scan_count(), 1);
    assert_eq!(
        repo.task_annotation_counts().await.unwrap()[&task],
        12,
        "reservations are not completion statistics"
    );
    let (_, eligible) = repo
        .preload_queue_policy(&users[0], &AssignmentKind::Annotation)
        .await
        .unwrap();
    assert_eq!(eligible.unwrap().len(), 3);
    repo.complete_assignment(
        &users[0],
        &current.assignment_id,
        &current.image_id,
        &task,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    assert!(
        repo.assign_preloaded_image_excluding(
            &users[0],
            &task,
            AssignmentKind::Annotation,
            &excluded
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(
        repo.task_completion_cache.scan_count(),
        1,
        "completion transfers capacity without rebuilding"
    );
}

#[tokio::test]
async fn concurrent_preloads_have_one_winner_for_the_last_slot() {
    let (_temp, repo, task, _, users) = balanced_repo(4, 1).await;
    let clone = repo.clone();
    let (first, second) = tokio::join!(
        repo.assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[]),
        clone.assign_preloaded_image_excluding(&users[1], &task, AssignmentKind::Annotation, &[]),
    );
    assert_eq!(
        usize::from(first.unwrap().is_some()) + usize::from(second.unwrap().is_some()),
        1
    );
    assert_eq!(
        repo.assignment_progress(&AssignmentKind::Annotation)
            .await
            .unwrap()
            .outstanding(&task),
        1
    );
}

#[tokio::test]
async fn preload_retry_release_expiry_restart_and_config_changes_reconcile_capacity() {
    let (temp, repo, task, _, users) = balanced_repo(6, 2).await;
    let first = repo
        .assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[])
        .await
        .unwrap()
        .unwrap();
    let second = repo
        .assign_preloaded_image_excluding(
            &users[0],
            &task,
            AssignmentKind::Annotation,
            std::slice::from_ref(&first.image_id),
        )
        .await
        .unwrap()
        .unwrap();
    let retried = repo
        .assign_preloaded_image_excluding(
            &users[0],
            &task,
            AssignmentKind::Annotation,
            std::slice::from_ref(&first.image_id),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retried.assignment_id, second.assignment_id);
    let restarted = DatasetRepository::new(temp.path());
    let excluded = [first.image_id.clone(), second.image_id.clone()];
    assert!(
        restarted
            .assign_preloaded_image_excluding(
                &users[1],
                &task,
                AssignmentKind::Annotation,
                &excluded
            )
            .await
            .unwrap()
            .is_none()
    );
    drop(restarted);
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.imbalance.as_mut().unwrap().max_difference = 1;
    repo.save_dataset(&metadata).await.unwrap();
    let (_, eligible) = repo
        .preload_queue_policy(&users[0], &AssignmentKind::Annotation)
        .await
        .unwrap();
    assert_eq!(eligible.unwrap(), vec![first.assignment_id.clone()]);
    repo.release_assignment(
        &users[0],
        &second.assignment_id,
        &second.image_id,
        &task,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    assert!(
        repo.assign_preloaded_image_excluding(
            &users[1],
            &task,
            AssignmentKind::Annotation,
            &excluded
        )
        .await
        .unwrap()
        .is_none()
    );
    expire_assignment(&repo, &first, &users[0]).await;
    assert!(
        repo.assign_preloaded_image_excluding(
            &users[1],
            &task,
            AssignmentKind::Annotation,
            &excluded
        )
        .await
        .unwrap()
        .is_some()
    );
    assert_eq!(repo.task_completion_cache.scan_count(), 1);
}

#[tokio::test]
async fn zero_window_blocks_speculation_without_blocking_foreground_progress() {
    let (_temp, repo, task, _, users) = balanced_repo(4, 0).await;
    assert!(
        repo.assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[])
            .await
            .unwrap()
            .is_none()
    );
    let current = repo
        .assign_next_image(&users[0], &task, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    repo.complete_assignment(
        &users[0],
        &current.assignment_id,
        &current.image_id,
        &task,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn preload_exemptions_and_review_counts_keep_existing_policy() {
    let (_temp, repo, task, peer, users) = balanced_repo(4, 1).await;
    for i in 0..4 {
        progress(&repo, i, &task, &users[0], TaskStatus::Submitted).await;
    }
    let queued = repo
        .assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Review, &[])
        .await
        .unwrap()
        .unwrap();
    assert!(
        repo.assign_preloaded_image_excluding(
            &users[1],
            &task,
            AssignmentKind::Review,
            std::slice::from_ref(&queued.image_id)
        )
        .await
        .unwrap()
        .is_none()
    );
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata
        .tasks
        .iter_mut()
        .find(|candidate| candidate.task_id == peer)
        .unwrap()
        .enabled = false;
    repo.save_dataset(&metadata).await.unwrap();
    let next = repo
        .assign_preloaded_image_excluding(
            &users[1],
            &task,
            AssignmentKind::Review,
            std::slice::from_ref(&queued.image_id),
        )
        .await
        .unwrap()
        .unwrap();
    metadata
        .tasks
        .iter_mut()
        .find(|candidate| candidate.task_id == peer)
        .unwrap()
        .enabled = true;
    metadata.imbalance.as_mut().unwrap().enforce = false;
    repo.save_dataset(&metadata).await.unwrap();
    assert!(
        repo.assign_preloaded_image_excluding(
            &users[0],
            &task,
            AssignmentKind::Review,
            &[queued.image_id, next.image_id]
        )
        .await
        .unwrap()
        .is_some()
    );
}

#[tokio::test]
async fn preload_failed_cache_write_and_cancelled_caller_keep_committed_capacity() {
    let (_temp, repo, task, _, users) = balanced_repo(4, 1).await;
    repo.task_annotation_counts().await.unwrap();
    repo.fail_next_state_cache_write_after_completion();
    assert!(
        repo.assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[])
            .await
            .is_err()
    );
    assert!(
        repo.assign_preloaded_image_excluding(&users[1], &task, AssignmentKind::Annotation, &[])
            .await
            .unwrap()
            .is_none()
    );
    let retry = repo
        .assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[])
        .await
        .unwrap()
        .unwrap();
    repo.release_assignment(
        &users[0],
        &retry.assignment_id,
        &retry.image_id,
        &task,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    let pause = repo.pause_after_next_completion_observation().await;
    let writer_repo = repo.clone();
    let user = users[0].clone();
    let writer_task = task.clone();
    let writer = tokio::spawn(async move {
        writer_repo
            .assign_preloaded_image_excluding(&user, &writer_task, AssignmentKind::Annotation, &[])
            .await
    });
    pause.started.notified().await;
    writer.abort();
    assert!(writer.await.unwrap_err().is_cancelled());
    assert!(
        repo.assign_preloaded_image_excluding(&users[1], &task, AssignmentKind::Annotation, &[])
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(repo.task_completion_cache.scan_count(), 1);
}

#[tokio::test]
async fn preload_retry_at_full_capacity_cannot_create_a_different_reservation() {
    let (_temp, repo, task, _, users) = balanced_repo(4, 1).await;
    let existing = repo
        .assign_preloaded_image_excluding(
            &users[0],
            &task,
            AssignmentKind::Annotation,
            &[ImageId::from("img_0")],
        )
        .await
        .unwrap()
        .unwrap();
    repo.assignment_cursors.lock().clear();
    let retry = repo
        .assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.assignment_id, existing.assignment_id);
    assert_eq!(
        repo.assignment_progress(&AssignmentKind::Annotation)
            .await
            .unwrap()
            .outstanding(&task),
        1
    );
}

#[tokio::test]
async fn preload_interrupted_publication_invalidates_the_derived_projection() {
    let (_temp, repo, task, _, users) = balanced_repo(3, 1).await;
    repo.assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[])
        .await
        .unwrap()
        .unwrap();
    drop(repo.completion_publication());
    assert!(
        repo.assign_preloaded_image_excluding(&users[1], &task, AssignmentKind::Annotation, &[])
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(repo.task_completion_cache.scan_count(), 2);
}

#[tokio::test]
async fn preload_reconciliation_removes_disabled_and_invalid_work_without_balance_enforcement() {
    let (_temp, repo, task, _, users) = balanced_repo(4, 10).await;
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.imbalance = None;
    repo.save_dataset(&metadata).await.unwrap();
    let claim = repo
        .assign_preloaded_image_excluding(&users[0], &task, AssignmentKind::Annotation, &[])
        .await
        .unwrap()
        .unwrap();
    let eligible = repo
        .preload_queue_policy(&users[0], &AssignmentKind::Annotation)
        .await
        .unwrap()
        .1
        .unwrap();
    assert_eq!(eligible, vec![claim.assignment_id]);
    metadata.tasks[0].enabled = false;
    repo.save_dataset(&metadata).await.unwrap();
    assert!(
        repo.preload_queue_policy(&users[0], &AssignmentKind::Annotation)
            .await
            .unwrap()
            .1
            .unwrap()
            .is_empty()
    );
    metadata.tasks[0].enabled = true;
    repo.save_dataset(&metadata).await.unwrap();
    progress(&repo, 0, &task, &users[0], TaskStatus::Submitted).await;
    assert!(
        repo.preload_queue_policy(&users[0], &AssignmentKind::Annotation)
            .await
            .unwrap()
            .1
            .unwrap()
            .is_empty()
    );
}
