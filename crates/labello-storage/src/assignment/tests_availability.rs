use super::tests::annotation_repo;
use super::*;

async fn reason(
    repo: &DatasetRepository,
    user: &UserId,
    task: &TaskId,
    kind: AssignmentKind,
) -> Option<WorkflowUnavailableReason> {
    repo.assignment_availability_reasons(user, kind.clone())
        .await
        .unwrap()
        .into_iter()
        .find(|(candidate, _)| *candidate == kind)
        .unwrap()
        .1[task]
}

#[tokio::test]
async fn reasons_distinguish_empty_claimed_mixed_and_finished_and_invalidate_cache() {
    use WorkflowUnavailableReason as R;
    let (_temp, repo, task, users) = annotation_repo(0, &["a", "b"]).await;
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        Some(R::EmptyDataset)
    );
    let (_temp, repo, task, users) = annotation_repo(2, &["a", "b"]).await;
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        None
    );
    let claimed = repo
        .assign_next_image(&users[1], &task, AssignmentKind::Annotation)
        .await
        .unwrap()
        .unwrap();
    // One claim does not make the workflow unavailable while another image is eligible.
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        None
    );
    let actor = Actor {
        user_id: users[0].clone(),
        role: DatasetRole::Annotator,
    };
    let other = if claimed.image_id == ImageId::from("img_0") {
        "img_1"
    } else {
        "img_0"
    };
    let complete = || EventPayload::TaskStateChanged {
        task_state: TaskState {
            task_id: task.clone(),
            status: TaskStatus::Completed,
            outcome: Some(TaskOutcome::AnnotationCompleted),
            assigned_to: None,
            completed_by: Some(users[0].clone()),
            completed_at: Some(labello_domain::now()),
            updated_at: labello_domain::now(),
        },
    };
    repo.append_payload(&ImageId::from(other), &actor, complete())
        .await
        .unwrap();
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        Some(R::Unavailable)
    );
    // The owner's still-active assignment remains resumable.
    assert_eq!(
        reason(&repo, &users[1], &task, AssignmentKind::Annotation).await,
        None
    );
    repo.release_assignment(
        &users[1],
        &claimed.assignment_id,
        &claimed.image_id,
        &task,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    repo.append_payload(&claimed.image_id, &actor, complete())
        .await
        .unwrap();
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        Some(R::AnnotationFinished)
    );
    let scans = repo.assignment_availability_cache.scan_count();
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        Some(R::AnnotationFinished)
    );
    assert_eq!(repo.assignment_availability_cache.scan_count(), scans);
}

#[tokio::test]
async fn reasons_share_review_configuration_role_and_balance_gates() {
    use WorkflowUnavailableReason as R;
    let (_temp, repo, task, users) = annotation_repo(1, &["a"]).await;
    assert!(
        repo.assignment_availability_reasons(&users[0], AssignmentKind::Review)
            .await
            .is_err()
    );
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments[0]
        .roles
        .insert(DatasetRole::Reviewer);
    metadata.tasks[0].review.workflow = ReviewWorkflow::None;
    repo.save_dataset(&metadata).await.unwrap();
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Review).await,
        Some(R::ReviewDisabled)
    );
    metadata.tasks[0].review.workflow = ReviewWorkflow::Approval;
    repo.save_dataset(&metadata).await.unwrap();
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Review).await,
        Some(R::NothingAwaitingReview)
    );
    let mut peer = metadata.tasks[0].clone();
    peer.task_id = TaskId::from("peer");
    metadata.tasks.push(peer);
    metadata.imbalance = Some(labello_domain::ImbalanceConfig {
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
                task_id: task.clone(),
                status: TaskStatus::Submitted,
                outcome: None,
                assigned_to: None,
                completed_by: None,
                completed_at: None,
                updated_at: labello_domain::now(),
            },
        },
    )
    .await
    .unwrap();
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        Some(R::BalanceLimit)
    );
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Review).await,
        None
    );
    metadata.imbalance.as_mut().unwrap().enforce = false;
    repo.save_dataset(&metadata).await.unwrap();
    assert_eq!(
        reason(&repo, &users[0], &task, AssignmentKind::Annotation).await,
        Some(R::AnnotationFinished)
    );
}
