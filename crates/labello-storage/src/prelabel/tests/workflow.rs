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

#[tokio::test]
async fn managed_preparation_uses_configured_overlap_filtering() {
    for (threshold, offset) in [(0.9, 0.05), (0.1, 0.2)] {
        let f = queue_fixture(Runner::default()).await;
        let mut metadata = f.repo.load_dataset().await.unwrap();
        metadata.prelabel_configs[0]
            .output_processing
            .suppress_overlaps_iou = Some(threshold);
        f.repo.save_dataset(&metadata).await.unwrap();
        let first = f.hints().await.suggestions[0].clone();

        let mut second = first.clone();
        second.suggestion_id = "second-hint".into();
        second.confidence = 0.8;
        if let AnnotationGeometry::BoundingBox(bounds) = &mut second.geometry {
            bounds.x += offset;
        }
        let provenance = &first.evidence.as_ref().unwrap().provenance;
        let control = f.service.lock(&f.dataset).await.unwrap();
        let item = WorkItem {
            image_id: provenance.image_id.clone(),
            image_hash: provenance.image_hash.clone(),
            task_id: provenance.task_id.clone(),
            config_id: provenance.config_id.clone(),
            config_digest: provenance.config_digest.clone(),
            model_digest: provenance.model_digest.clone(),
            generation: PrelabelService::generation(
                &control,
                &provenance.task_id,
                &provenance.config_id,
            ),
            outcome: PrelabelItemOutcome::Pending,
        };
        let hints = predictions::certify(
            &f.dataset,
            &control,
            &item,
            &metadata.prelabel_configs[0],
            metadata.task(&item.task_id).unwrap(),
            ImageDimensions {
                width: 100,
                height: 100,
            },
            vec![second, first],
            PrelabelExecutionKind::ServerCpu,
        )
        .unwrap();
        drop(control);
        let expected =
            filter_prelabels(&hints, &[], &metadata.prelabel_configs[0].output_processing);
        let task = metadata.task(&TaskId::from("boxes")).unwrap();
        f.repo
            .prepare_workflow_predictions(
                &"fresh".into(),
                &task.task_id,
                workflow_prelabel_digest(&metadata, task),
                None,
                WorkflowPreparationStatus::Ready,
                hints,
            )
            .await
            .unwrap();
        let state = f.repo.load_image_state(&"fresh".into()).await.unwrap();
        assert_eq!(
            state.workflow_preparations[&task.task_id].prelabels,
            expected
        );
    }
}

#[tokio::test]
async fn managed_quotas_never_publish_unretained_predictions_and_retry_after_restart() {
    let probe = queue_fixture(Runner::default()).await;
    let hints = probe.hints().await.suggestions;
    let compact = serde_json::to_vec(&hints).unwrap().len() + 128;
    let total = (serde_json::to_vec_pretty(&hints).unwrap().len() + 128).max(compact) as u64;
    for (limits, retained) in [
        (
            PrelabelLimits {
                max_result_bytes: 1,
                ..Default::default()
            },
            0,
        ),
        (
            PrelabelLimits {
                max_retained_results: 1,
                ..Default::default()
            },
            1,
        ),
        (
            PrelabelLimits {
                max_result_bytes: compact,
                max_total_result_bytes: total,
                ..Default::default()
            },
            1,
        ),
    ] {
        let mut f = queue_fixture(Runner::default()).await;
        f.service = PrelabelService::new(
            f.temp.path(),
            &f.temp.path().join("models"),
            limits.clone(),
            Arc::new(f.runner.clone()),
        )
        .await
        .unwrap();
        f.service
            .synchronize_workflows(&f.dataset, f.repo.clone(), true)
            .await
            .unwrap();
        let admin = f.finish().await;
        assert_eq!(admin.runs[0].phase, PrelabelRunPhase::Interrupted);
        let run_id = admin.runs[0].run_id.clone();
        for restart in [false, true] {
            if restart {
                f.service.shutdown().await;
                f.service = PrelabelService::new(
                    f.temp.path(),
                    &f.temp.path().join("models"),
                    limits.clone(),
                    Arc::new(f.runner.clone()),
                )
                .await
                .unwrap();
                f.service
                    .synchronize_workflows(&f.dataset, f.repo.clone(), true)
                    .await
                    .unwrap();
            }
            let cached = f.service.lock(&f.dataset).await.unwrap().results.len();
            assert_eq!(cached, retained);
            let mut ready = 0;
            for image in ["fresh", "active", "correction"] {
                let state = f.repo.rebuild_image_state(&image.into()).await.unwrap();
                let prep = &state.workflow_preparations[&TaskId::from("boxes")];
                if prep.status == WorkflowPreparationStatus::Ready && !prep.prelabels.is_empty() {
                    ready += 1;
                } else {
                    assert!(
                        prep.prelabels.is_empty(),
                        "unretained predictions must never become queue sources"
                    );
                }
            }
            assert_eq!(
                ready, cached,
                "every published prediction must have passed retention limits"
            );
            let availability = f
                .repo
                .workflow_availability(&"worker".into(), AssignmentKind::Annotation)
                .await
                .unwrap();
            assert_eq!(
                availability
                    .iter()
                    .any(|a| a.selection.variant == WorkflowVariant::Objects && a.available),
                retained > 0
            );
        }
        f.service.shutdown().await;
        f.service = PrelabelService::new(
            f.temp.path(),
            &f.temp.path().join("models"),
            PrelabelLimits::default(),
            Arc::new(f.runner.clone()),
        )
        .await
        .unwrap();
        f.command(PrelabelAdminCommand::Retry {
            run_id: run_id.clone(),
        })
        .await;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let state = f.service.admin_state(&f.dataset).await.unwrap();
                let run = state.runs.iter().find(|run| run.run_id == run_id).unwrap();
                if run.phase != PrelabelRunPhase::Running {
                    assert_eq!(run.generated, 3);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        f.service.shutdown().await;
    }
}

#[tokio::test]
async fn managed_restart_reuses_retained_pending_results_across_publication_boundary() {
    for published in [false, true] {
        let mut f = queue_fixture(Runner::default()).await;
        f.service
            .synchronize_workflows(&f.dataset, f.repo.clone(), true)
            .await
            .unwrap();
        assert_eq!(f.finish().await.runs[0].generated, 3);
        let calls = f.runner.calls.load(Ordering::SeqCst);
        // Reconstruct process loss after durable result retention, either before
        // or after queue publication, but before recording the item outcome.
        let run_id = {
            let mut control = f.service.lock(&f.dataset).await.unwrap();
            if !published {
                for item in &control.runs[0].items {
                    f.service
                        .publish_managed(&f.repo, item, WorkflowPreparationStatus::Pending, vec![])
                        .await
                        .unwrap();
                }
            }
            let run = &mut control.runs[0];
            run.summary.phase = PrelabelRunPhase::Running;
            for item in &mut run.items {
                item.outcome = PrelabelItemOutcome::Pending;
            }
            runs::update_counts(run);
            let id = run.summary.run_id.clone();
            f.service.persist(&f.dataset, &control).await.unwrap();
            id
        };
        f.service = PrelabelService::new(
            f.temp.path(),
            &f.temp.path().join("models"),
            PrelabelLimits::default(),
            Arc::new(f.runner.clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            f.service.admin_state(&f.dataset).await.unwrap().runs[0].phase,
            PrelabelRunPhase::Interrupted
        );
        f.command(PrelabelAdminCommand::Retry { run_id }).await;
        assert_eq!(f.finish().await.runs[0].generated, 3);
        assert_eq!(
            f.runner.calls.load(Ordering::SeqCst),
            calls,
            "retained results must not require inference again"
        );
        for image in ["fresh", "active", "correction"] {
            let state = f.repo.rebuild_image_state(&image.into()).await.unwrap();
            let preparation = &state.workflow_preparations[&TaskId::from("boxes")];
            assert_eq!(preparation.status, WorkflowPreparationStatus::Ready);
            assert_eq!(preparation.prelabels.len(), 1);
        }
    }
}
