use super::*;
use crate::assignment::AssignmentContext;

fn context(assignment: &Assignment) -> AssignmentContext<'_> {
    AssignmentContext {
        assignment_id: &assignment.assignment_id,
        image_id: &assignment.image_id,
        task_id: &assignment.task_id,
        kind: assignment.kind.clone(),
    }
}

async fn queue_fixture(runner: Runner) -> Fixture {
    let fixture = Fixture::new(runner).await;
    let mut metadata = fixture.repo.load_dataset_config().await.unwrap();
    metadata.tasks[1].enabled = false;
    metadata.role_assignments.push(DatasetRoleAssignment {
        user_id: UserId::from("worker"),
        roles: [DatasetRole::Annotator].into(),
        dataset_id: fixture.dataset.clone(),
        assigned_at: now(),
        assigned_by: Some(UserId::from("admin")),
    });
    fixture.repo.save_dataset(&metadata).await.unwrap();
    fixture
}

#[tokio::test]
async fn managed_preparation_continues_across_bounded_runs() {
    let mut f = queue_fixture(Runner::default()).await;
    f.service = PrelabelService::new(
        f.temp.path(),
        &f.temp.path().join("models"),
        PrelabelLimits {
            max_work_items: 1,
            ..Default::default()
        },
        Arc::new(f.runner.clone()),
    )
    .await
    .unwrap();
    for expected in 1..=3 {
        f.service
            .synchronize_workflows(&f.dataset, f.repo.clone(), true)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let state = f.service.admin_state(&f.dataset).await.unwrap();
                if state
                    .runs
                    .iter()
                    .all(|run| run.phase == PrelabelRunPhase::Completed)
                {
                    assert_eq!(state.runs.len(), expected);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(f.runner.calls.load(Ordering::SeqCst), 3);
    for image in ["fresh", "active", "correction"] {
        let state = f.repo.load_image_state(&image.into()).await.unwrap();
        assert_eq!(
            state.workflow_preparations[&"boxes".into()].status,
            WorkflowPreparationStatus::Ready
        );
    }
}

#[tokio::test]
async fn cancelled_managed_work_waits_for_admin_retry() {
    let pause = Arc::new(tokio::sync::Notify::new());
    let f = queue_fixture(Runner {
        pause: Some(pause),
        ..Default::default()
    })
    .await;
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    f.runner.started.notified().await;
    let run_id = f.service.admin_state(&f.dataset).await.unwrap().runs[0]
        .run_id
        .clone();
    f.command(PrelabelAdminCommand::Cancel { run_id }).await;
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    let admin = f.service.admin_state(&f.dataset).await.unwrap();
    assert_eq!(admin.runs.len(), 1);
    assert_eq!(admin.runs[0].phase, PrelabelRunPhase::Cancelled);
    for image in ["fresh", "active", "correction"] {
        let state = f.repo.load_image_state(&image.into()).await.unwrap();
        assert_eq!(
            state.workflow_preparations[&"boxes".into()].status,
            WorkflowPreparationStatus::Failed
        );
    }
    f.service.shutdown().await;
}

#[tokio::test]
async fn managed_predictions_refresh_unseen_claims_and_preserve_seen_partial_sources() {
    let f = queue_fixture(Runner::default()).await;
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    assert_eq!(f.finish().await.runs[0].generated, 3);
    let user = UserId::from("worker");
    let selection = WorkflowSelection {
        task_id: TaskId::from("boxes"),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let first = f
        .repo
        .claim_workflow_item(&user, &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let state = f
        .repo
        .display_workflow_item(&user, context(&first))
        .await
        .unwrap();
    let source = state.workflow_preparations[&selection.task_id].prelabels[0].clone();
    let excluded = WorkflowItemRef {
        image_id: first.image_id.clone(),
        item: state.workflow_assignments[&first.assignment_id]
            .item
            .clone(),
    };
    f.repo
        .save_workflow_draft(&user, context(&first), source.geometry.clone(), 0)
        .await
        .unwrap();
    let unseen = f
        .repo
        .claim_workflow_item(&user, &selection, &[excluded])
        .await
        .unwrap()
        .unwrap();
    let old_unseen = f
        .repo
        .load_image_state(&unseen.image_id)
        .await
        .unwrap()
        .workflow_preparations[&selection.task_id]
        .prelabels[0]
        .clone();
    let mut metadata = f.repo.load_dataset_config().await.unwrap();
    metadata.prelabel_configs[0].model.version = Some("2".into());
    f.repo.save_dataset(&metadata).await.unwrap();
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    f.finish().await;
    let seen = f.repo.load_image_state(&first.image_id).await.unwrap();
    assert_eq!(
        seen.workflow_preparations[&selection.task_id].prelabels,
        vec![source.clone()]
    );
    assert_eq!(
        seen.assignments
            .iter()
            .find(|a| a.assignment_id == first.assignment_id)
            .unwrap()
            .status,
        AssignmentStatus::Active
    );
    let refreshed = f.repo.load_image_state(&unseen.image_id).await.unwrap();
    assert_eq!(
        refreshed
            .assignments
            .iter()
            .find(|a| a.assignment_id == unseen.assignment_id)
            .unwrap()
            .status,
        AssignmentStatus::Cancelled
    );
    assert_ne!(
        refreshed.workflow_preparations[&selection.task_id].prelabels[0].suggestion_id,
        old_unseen.suggestion_id
    );
    assert!(
        f.repo
            .display_workflow_item(&user, context(&unseen))
            .await
            .is_err()
    );
    let guard = f
        .service
        .authorize_acceptance(
            &f.dataset,
            &f.repo,
            &first.image_id,
            &BTreeMap::from([(
                AnnotationId::from("accepted"),
                source.evidence.clone().unwrap(),
            )]),
        )
        .await
        .unwrap();
    drop(guard);
    metadata.prelabel_configs[0].available_to_annotators = false;
    f.repo.save_dataset(&metadata).await.unwrap();
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    let seen = f.repo.load_image_state(&first.image_id).await.unwrap();
    assert_eq!(
        seen.workflow_preparations[&selection.task_id].prelabels,
        vec![source]
    );
    let unseen = f.repo.load_image_state(&unseen.image_id).await.unwrap();
    assert!(
        unseen.workflow_preparations[&selection.task_id]
            .objects
            .is_empty()
    );
    assert!(
        seen.workflow_object_draft(
            &selection.task_id,
            &seen.workflow_assignments[&first.assignment_id].item
        )
        .is_some()
    );
    assert_eq!(
        f.repo.rebuild_image_state(&first.image_id).await.unwrap(),
        seen
    );
}

#[tokio::test]
async fn managed_pose_predictions_execute_on_the_server_and_split_skeleton_objects() {
    let f = queue_fixture(Runner::default()).await;
    let mut metadata = f.repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].annotation_type = AnnotationType::Skeleton;
    metadata.tasks[0].skeleton = Some(SkeletonSpec {
        keypoints: vec![KeypointSpec {
            name: "nose".into(),
            required: true,
        }],
        edges: vec![],
        allow_hidden: false,
        allow_absent: false,
    });
    metadata.prelabel_configs[0]
        .yolo
        .as_mut()
        .unwrap()
        .keypoints = vec!["nose".into()];
    f.repo.save_dataset(&metadata).await.unwrap();
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    assert_eq!(f.finish().await.runs[0].generated, 3);
    let item = f
        .repo
        .claim_workflow_item(
            &UserId::from("worker"),
            &WorkflowSelection {
                task_id: TaskId::from("boxes"),
                kind: AssignmentKind::Annotation,
                variant: WorkflowVariant::Objects,
            },
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    let state = f.repo.load_image_state(&item.image_id).await.unwrap();
    let source = &state.workflow_preparations[&TaskId::from("boxes")].prelabels[0];
    assert!(matches!(source.geometry, AnnotationGeometry::Skeleton(_)));
    assert_eq!(
        source.evidence.as_ref().unwrap().provenance.execution,
        PrelabelExecutionKind::ServerCpu
    );
    assert!(
        f.runner
            .owners
            .lock()
            .unwrap()
            .iter()
            .all(|owner| matches!(owner, InferenceOwner::Batch { .. }))
    );
}

#[tokio::test]
async fn late_model_results_cannot_interrupt_a_displayed_overview() {
    let pause = Arc::new(tokio::sync::Notify::new());
    let f = queue_fixture(Runner {
        pause: Some(pause.clone()),
        ..Default::default()
    })
    .await;
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    f.runner.started.notified().await;
    let mut metadata = f.repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].prelabel_config_ids.clear();
    f.repo.save_dataset(&metadata).await.unwrap();
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    let selection = WorkflowSelection {
        task_id: TaskId::from("boxes"),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    let item = f
        .repo
        .claim_workflow_item(&UserId::from("worker"), &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let before = f
        .repo
        .display_workflow_item(&UserId::from("worker"), context(&item))
        .await
        .unwrap();
    pause.notify_waiters();
    assert_eq!(f.finish().await.runs[0].skipped, 3);
    assert_eq!(
        f.repo.load_image_state(&item.image_id).await.unwrap(),
        before
    );
    metadata.tasks[0].prelabel_config_ids = vec![PrelabelConfigId::from("model")];
    f.repo.save_dataset(&metadata).await.unwrap();
    f.service
        .synchronize_workflows(&f.dataset, f.repo.clone(), true)
        .await
        .unwrap();
    assert_eq!(
        f.repo.load_image_state(&item.image_id).await.unwrap(),
        before
    );
    f.service.shutdown().await;
}
