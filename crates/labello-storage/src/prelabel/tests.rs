use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Runner {
    calls: AtomicUsize,
    execution: Option<PrelabelExecutionKind>,
    failures: AtomicUsize,
    empty: bool,
    pause: Option<Arc<tokio::sync::Notify>>,
    started: tokio::sync::Notify,
    owners: std::sync::Mutex<Vec<InferenceOwner>>,
    released: std::sync::Mutex<Vec<InferenceOwner>>,
}
impl PrelabelRunner for Arc<Runner> {
    fn release(&self, owner: &InferenceOwner) {
        self.released.lock().unwrap().push(owner.clone());
    }

    fn inspect(&self, _model: Vec<u8>) -> crate::prelabel::ModelInspectionFuture {
        Box::pin(async {
            Ok(PrelabelModelInspection {
                model_digest: "a".repeat(64),
                inputs: vec![],
                input_size: Some(320),
                outputs: vec![],
                problem: None,
            })
        })
    }

    fn infer(
        &self,
        owner: InferenceOwner,
        _: Vec<u8>,
        _: Vec<u8>,
        config: PrelabelConfig,
        task: TaskDefinition,
    ) -> InferenceFuture {
        self.owners.lock().unwrap().push(owner);
        let runner = self.clone();
        Box::pin(async move {
            runner.calls.fetch_add(1, Ordering::SeqCst);
            runner.started.notify_one();
            if let Some(pause) = &runner.pause {
                pause.notified().await;
            }
            if runner
                .failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(PrelabelFailure::Inference);
            }
            if runner.empty {
                return Ok(PrelabelInferenceResult {
                    execution: runner
                        .execution
                        .clone()
                        .unwrap_or(PrelabelExecutionKind::ServerCpu),
                    suggestions: vec![],
                });
            }
            Ok(PrelabelInferenceResult {
                execution: runner
                    .execution
                    .clone()
                    .unwrap_or(PrelabelExecutionKind::ServerCpu),
                suggestions: vec![PrelabelSuggestion {
                    suggestion_id: "untrusted-id".into(),
                    config_id: config.config_id,
                    task_id: task.task_id,
                    class_id: "person".into(),
                    confidence: 0.9,
                    geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                        x: 0.1,
                        y: 0.1,
                        width: 0.3,
                        height: 0.3,
                    }),
                    evidence: None,
                }],
            })
        })
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    repo: DatasetRepository,
    service: PrelabelService,
    runner: Arc<Runner>,
    dataset: DatasetId,
}

#[tokio::test]
async fn batch_cache_retains_gpu_execution_for_predictions_and_empty_results() {
    for (execution, empty) in [
        (PrelabelExecutionKind::ServerCuda, false),
        (PrelabelExecutionKind::ServerWebGpu, true),
    ] {
        let fixture = Fixture::new(Runner {
            execution: Some(execution.clone()),
            empty,
            ..Default::default()
        })
        .await;
        let ready = fixture
            .command(PrelabelAdminCommand::Preflight {
                mappings: BTreeMap::new(),
            })
            .await;
        fixture
            .command(PrelabelAdminCommand::Start {
                run_id: ready.runs[0].run_id.clone(),
            })
            .await;
        fixture.finish().await;
        let hints = fixture.hints().await;
        assert!(hints.from_batch);
        assert_eq!(hints.execution, Some(execution.clone()));
        assert_eq!(hints.suggestions.is_empty(), empty);
        for hint in hints.suggestions {
            assert_eq!(
                hint.evidence.as_ref().unwrap().provenance.execution,
                execution
            );
            assert_eq!(
                hint.evidence.as_ref().unwrap().provenance.trust,
                PredictionTrust::ServerGenerated
            );
        }
    }
}

#[tokio::test]
async fn model_inspection_uses_managed_files_and_shared_worker_limit_without_writing_hints() {
    let fixture = Fixture::new(Runner::default()).await;
    let before = fixture.service.admin_state(&fixture.dataset).await.unwrap();
    let metadata = fixture.repo.load_dataset_config().await.unwrap();
    let location = &metadata.prelabel_configs[0].model.location;
    assert!(fixture.service.inspect_model(location).await.is_ok());
    let permit = fixture.service.inner.workers.interactive().await.unwrap();
    let mut waiting = std::pin::pin!(fixture.service.inspect_model(location));
    use std::future::Future;
    assert!(
        waiting
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
            .is_pending()
    );
    drop(permit);
    assert!(waiting.await.is_ok());
    assert_eq!(
        fixture.service.admin_state(&fixture.dataset).await.unwrap(),
        before
    );
    assert_eq!(fixture.runner.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .service
            .inspect_model("missing.onnx")
            .await
            .unwrap_err(),
        PrelabelFailure::ModelUnavailable
    );
    assert_eq!(
        fixture
            .service
            .inspect_model("../model.onnx")
            .await
            .unwrap_err(),
        PrelabelFailure::Invalid
    );
}

#[tokio::test]
async fn checked_mapping_rejects_replaced_model_and_config_round_trips_in_toml() {
    let fixture = Fixture::new(Runner::default()).await;
    let mut metadata = fixture.repo.load_dataset_config().await.unwrap();
    let bytes = fixture
        .service
        .model(&metadata.prelabel_configs[0])
        .await
        .unwrap();
    let config = &mut metadata.prelabel_configs[0];
    let spec = config.yolo.as_mut().unwrap();
    spec.use_explicit_mapping(80);
    spec.output_name = Some("predictions".into());
    spec.model_digest = Some(blake3::hash(&bytes).to_hex().to_string());
    fixture.repo.save_dataset(&metadata).await.unwrap();
    let loaded = fixture.repo.load_dataset_config().await.unwrap();
    assert_eq!(loaded.prelabel_configs, metadata.prelabel_configs);
    assert!(
        fixture
            .service
            .model(&loaded.prelabel_configs[0])
            .await
            .is_ok()
    );
    std::fs::write(
        fixture
            .temp
            .path()
            .join("models")
            .join(&loaded.prelabel_configs[0].model.location),
        b"replaced model",
    )
    .unwrap();
    assert_eq!(
        fixture
            .service
            .model(&loaded.prelabel_configs[0])
            .await
            .unwrap_err(),
        PrelabelFailure::Stale
    );
}

#[tokio::test]
async fn result_write_failure_keeps_work_pending_and_retry_publishes_once() {
    let fixture = Fixture::new(Runner::default()).await;
    let prepared = fixture
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await;
    let run_id = prepared.runs[0].run_id.clone();
    let key = {
        let control = fixture.service.lock(&fixture.dataset).await.unwrap();
        predictions::result_key(&control.runs[0].items[0]).unwrap()
    };
    let path = fixture
        .service
        .directory(&fixture.dataset)
        .unwrap()
        .join(format!("result-{key}.json"));
    std::fs::create_dir(&path).unwrap();
    fixture
        .command(PrelabelAdminCommand::Start {
            run_id: run_id.clone(),
        })
        .await;
    let interrupted = fixture.finish().await;
    assert_eq!(interrupted.runs[0].phase, PrelabelRunPhase::Interrupted);
    assert_eq!(interrupted.runs[0].pending, 6);
    assert_eq!(interrupted.runs[0].generated, 0);
    assert!(
        fixture
            .service
            .lock(&fixture.dataset)
            .await
            .unwrap()
            .results
            .is_empty()
    );
    std::fs::remove_dir(path).unwrap();
    fixture
        .command(PrelabelAdminCommand::Retry { run_id })
        .await;
    assert_eq!(fixture.finish().await.runs[0].generated, 6);
    assert_eq!(
        fixture
            .service
            .lock(&fixture.dataset)
            .await
            .unwrap()
            .results
            .len(),
        6
    );
    assert!(
        fixture
            .repo
            .load_events(&"fresh".into())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn start_requires_fresh_preflight_when_new_eligible_work_would_be_omitted() {
    let fixture = Fixture::new(Runner::default()).await;
    let prepared = fixture
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await;
    let mut metadata = fixture.repo.load_dataset_config().await.unwrap();
    let mut task = metadata.tasks[0].clone();
    task.task_id = "new-workflow".into();
    metadata.tasks.push(task);
    fixture.repo.save_dataset(&metadata).await.unwrap();
    let result = fixture
        .service
        .command(
            &fixture.dataset,
            fixture.repo.clone(),
            PrelabelAdminCommand::Start {
                run_id: prepared.runs[0].run_id.clone(),
            },
        )
        .await;
    assert_eq!(result, Err(PrelabelFailure::NotReady));
    assert_eq!(fixture.runner.calls.load(Ordering::SeqCst), 0);
    let refreshed = fixture
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await;
    assert_eq!(refreshed.runs[0].total, 11);
}

#[tokio::test]
async fn quota_interruption_retains_successes_for_retry_after_operator_increases_limit() {
    let mut fixture = Fixture::new(Runner::default()).await;
    Arc::get_mut(&mut fixture.service.inner)
        .unwrap()
        .limits
        .max_retained_results = 1;
    let prepared = fixture
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await;
    let run_id = prepared.runs[0].run_id.clone();
    fixture
        .command(PrelabelAdminCommand::Start {
            run_id: run_id.clone(),
        })
        .await;
    let interrupted = fixture.finish().await;
    assert_eq!(interrupted.runs[0].phase, PrelabelRunPhase::Interrupted);
    assert_eq!(interrupted.runs[0].generated, 1);
    assert_eq!(interrupted.runs[0].pending, 5);
    fixture.service = PrelabelService::new(
        fixture.temp.path(),
        &fixture.temp.path().join("models"),
        PrelabelLimits::default(),
        Arc::new(fixture.runner.clone()),
    )
    .await
    .unwrap();
    fixture
        .command(PrelabelAdminCommand::Retry { run_id })
        .await;
    assert_eq!(fixture.finish().await.runs[0].generated, 6);
    assert_eq!(fixture.runner.calls.load(Ordering::SeqCst), 7);
}
impl Fixture {
    async fn new(runner: Runner) -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("models")).unwrap();
        std::fs::write(
            temp.path().join("models/model.onnx"),
            b"synthetic model for orchestration tests",
        )
        .unwrap();
        let dataset = DatasetId::from("dataset");
        let repo = DatasetRepository::new(temp.path().join(dataset.as_str()));
        let mut metadata = DatasetMetadata::new(dataset.clone(), "Dataset", now());
        metadata.label_classes.push(LabelClass {
            class_id: "person".into(),
            name: "Person".into(),
            color: "#ffffff".into(),
            description: None,
        });
        for id in ["boxes", "other"] {
            metadata.tasks.push(TaskDefinition {
                task_id: id.into(),
                name: id.into(),
                annotation_type: AnnotationType::BoundingBox,
                class_ids: vec!["person".into()],
                instructions: TutorialContent {
                    title: "".into(),
                    example_text: "".into(),
                    example_images: vec![],
                },
                skeleton: None,
                review: ReviewConfig::default(),
                prelabel_config_ids: vec!["model".into()],
                manual_box_guide_migration: None,
                enabled: true,
            });
        }
        metadata.prelabel_configs.push(PrelabelConfig {
            config_id: "model".into(),
            name: "Model".into(),
            model: ModelSpec {
                model_id: "yolo".into(),
                display_name: "YOLO".into(),
                version: Some("1".into()),
                location: "model.onnx".into(),
            },
            execution: PrelabelExecution::ServerSide { command: vec![] },
            output_processing: OutputProcessing {
                confidence_threshold: 0.25,
                suppress_overlaps_iou: None,
            },
            available_to_annotators: true,
            yolo: Some(YoloModelSpec {
                input_size: 320,
                class_ids: vec![Some("person".into()), None],
                keypoints: vec![],
                ..Default::default()
            }),
        });
        repo.initialize(metadata).await.unwrap();
        let mut index = ImagesIndex::default();
        for (number, name) in ["fresh", "active", "correction", "submitted", "completed"]
            .into_iter()
            .enumerate()
        {
            let bytes = vec![number as u8 + 1];
            let hash = blake3::hash(&bytes).to_hex().to_string();
            let path = format!("images/{name}.png");
            std::fs::write(repo.root().join(&path), bytes).unwrap();
            index.images_by_hash.insert(
                hash.clone(),
                ImageRecord {
                    image_id: name.into(),
                    blake3: hash,
                    canonical_path: path.clone(),
                    known_paths: vec![path],
                    duplicate_paths: vec![],
                    file_name: format!("{name}.png"),
                    byte_size: 1,
                    width: 100,
                    height: 100,
                    media_type: "image/png".into(),
                    source_memberships: None,
                },
            );
        }
        index.image_count = index.images_by_hash.len();
        repo.save_images_index(&index).await.unwrap();
        for (image, status) in [
            ("active", TaskStatus::InProgress),
            ("correction", TaskStatus::NeedsCorrection),
            ("submitted", TaskStatus::Submitted),
            ("completed", TaskStatus::Completed),
        ] {
            for task in ["boxes", "other"] {
                let mut state = TaskState::new(task.into(), now());
                state.status = status.clone();
                let previous = repo.load_events(&image.into()).await.unwrap().len() as u64;
                repo.append_events_atomic(
                    &image.into(),
                    &[EventLogEntry::new(
                        previous + 1,
                        image.into(),
                        "admin".into(),
                        DatasetRole::DataAdmin,
                        now(),
                        EventPayload::TaskStateChanged { task_state: state },
                    )],
                )
                .await
                .unwrap();
            }
        }
        let runner = Arc::new(runner);
        let service = PrelabelService::new(
            temp.path(),
            &temp.path().join("models"),
            PrelabelLimits::default(),
            Arc::new(runner.clone()),
        )
        .await
        .unwrap();
        Self {
            temp,
            repo,
            service,
            runner,
            dataset,
        }
    }
    async fn command(&self, command: PrelabelAdminCommand) -> PrelabelAdminState {
        self.service
            .command(&self.dataset, self.repo.clone(), command)
            .await
            .unwrap()
    }
    async fn hints(&self) -> PrelabelResponse {
        self.service
            .suggestions(
                &self.dataset,
                &self.repo,
                &"annotator".into(),
                &"fresh".into(),
                &"boxes".into(),
                &"model".into(),
            )
            .await
            .unwrap()
    }
    async fn finish(&self) -> PrelabelAdminState {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let state = self.service.admin_state(&self.dataset).await.unwrap();
                if state.runs[0].phase != PrelabelRunPhase::Running {
                    return state;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn batch_selects_remaining_work_and_reuses_results_without_claims_or_events() {
    let fixture = Fixture::new(Runner::default()).await;
    let before = fixture
        .repo
        .load_image_state(&"active".into())
        .await
        .unwrap();
    let preflight = fixture
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await;
    assert_eq!(preflight.runs[0].total, 6); // Missing, InProgress and NeedsCorrection in both tasks.
    assert!(preflight.runs[0].blockers.is_empty());
    fixture
        .command(PrelabelAdminCommand::Start {
            run_id: preflight.runs[0].run_id.clone(),
        })
        .await;
    let completed = fixture.finish().await;
    assert_eq!(completed.runs[0].generated, 6);
    assert_eq!(completed.runs[0].phase, PrelabelRunPhase::Completed);
    let owner = InferenceOwner::Batch {
        dataset: fixture.dataset.clone(),
        run: preflight.runs[0].run_id.clone(),
    };
    assert!(
        fixture
            .runner
            .owners
            .lock()
            .unwrap()
            .iter()
            .all(|seen| seen == &owner)
    );
    assert!(fixture.runner.released.lock().unwrap().contains(&owner));
    assert_eq!(
        fixture
            .repo
            .load_image_state(&"active".into())
            .await
            .unwrap(),
        before
    );
    assert!(
        fixture
            .repo
            .load_events(&"fresh".into())
            .await
            .unwrap()
            .is_empty()
    );
    let hints = fixture.hints().await;
    assert!(hints.from_batch);
    assert_eq!(fixture.runner.calls.load(Ordering::SeqCst), 6);
    let evidence = hints.suggestions[0].evidence.as_ref().unwrap();
    assert_eq!(evidence.provenance.trust, PredictionTrust::ServerGenerated);
    assert_eq!(evidence.provenance.model_id, "yolo");
    assert_eq!(evidence.provenance.processing.iou_threshold(), 0.5);
    let prepared = fixture
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await;
    assert_eq!(prepared.runs[0].reusable, 6);
    assert_eq!(prepared.runs[0].ineligible, 4);
    fixture
        .command(PrelabelAdminCommand::Start {
            run_id: prepared.runs[0].run_id.clone(),
        })
        .await;
    assert_eq!(fixture.finish().await.runs[0].generated, 6);
    assert_eq!(fixture.runner.calls.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn reset_is_scoped_idempotent_and_invalidates_evidence_without_touching_configuration() {
    let f = Fixture::new(Runner::default()).await;
    let hints = f.hints().await;
    let config = f.repo.load_dataset_config().await.unwrap();
    let proofs = BTreeMap::from([(
        "accepted".into(),
        hints.suggestions[0].evidence.clone().unwrap(),
    )]);
    drop(
        f.service
            .authorize_acceptance(&f.dataset, &f.repo, &"fresh".into(), &proofs)
            .await
            .unwrap(),
    );
    let scope = PrelabelScope {
        task_id: Some("boxes".into()),
        config_id: None,
    };
    f.command(PrelabelAdminCommand::Reset {
        scope: scope.clone(),
    })
    .await;
    assert!(matches!(
        f.service
            .authorize_acceptance(&f.dataset, &f.repo, &"fresh".into(), &proofs)
            .await,
        Err(PrelabelFailure::Stale)
    ));
    let paused = f.hints().await;
    assert!(paused.generation.paused && paused.suggestions.is_empty());
    f.command(PrelabelAdminCommand::Reset {
        scope: scope.clone(),
    })
    .await;
    assert_eq!(f.hints().await.generation, paused.generation);
    assert!(
        !f.service
            .generation_status(&f.dataset, &"other".into(), &"model".into())
            .await
            .unwrap()
            .paused
    );
    assert_eq!(f.repo.load_dataset_config().await.unwrap(), config);
    assert_eq!(f.runner.calls.load(Ordering::SeqCst), 1);
    f.command(PrelabelAdminCommand::Resume { scope }).await;
    assert!(!f.hints().await.generation.paused);
    assert!(matches!(
        f.service
            .authorize_acceptance(&f.dataset, &f.repo, &"fresh".into(), &proofs)
            .await,
        Err(PrelabelFailure::Stale)
    ));
}

#[tokio::test]
async fn in_flight_interactive_generation_cannot_publish_after_reset() {
    let release = Arc::new(tokio::sync::Notify::new());
    let f = Fixture::new(Runner {
        pause: Some(release.clone()),
        ..Default::default()
    })
    .await;
    let service = f.service.clone();
    let repo = f.repo.clone();
    let dataset = f.dataset.clone();
    let request = tokio::spawn(async move {
        service
            .suggestions(
                &dataset,
                &repo,
                &"annotator".into(),
                &"fresh".into(),
                &"boxes".into(),
                &"model".into(),
            )
            .await
    });
    f.runner.started.notified().await;
    f.command(PrelabelAdminCommand::Reset {
        scope: Default::default(),
    })
    .await;
    release.notify_one();
    assert_eq!(request.await.unwrap().unwrap_err(), PrelabelFailure::Stale);
    assert_eq!(
        f.command(PrelabelAdminCommand::Resume {
            scope: Default::default()
        })
        .await
        .retained_results,
        0
    );
}

#[tokio::test]
async fn browser_claims_are_bound_and_disclosed_and_tampered_acceptance_is_rejected() {
    let f = Fixture::new(Runner::default()).await;
    let mut metadata = f.repo.load_dataset_config().await.unwrap();
    metadata.prelabel_configs[0].execution = PrelabelExecution::BrowserLocal {
        acceleration: BrowserAcceleration::WebGpuPreferred,
    };
    f.repo.save_dataset(&metadata).await.unwrap();
    let grant = f.hints().await.browser_grant.unwrap();
    let config = metadata.prelabel_configs[0].clone();
    let task = metadata.tasks[0].clone();
    let candidates = f
        .runner
        .infer(
            InferenceOwner::Interactive {
                dataset: "test".into(),
                user: "test".into(),
            },
            vec![],
            vec![],
            config,
            task,
        )
        .await
        .unwrap();
    let request = BrowserPrelabelResult {
        grant: grant.clone(),
        execution: PrelabelExecutionKind::BrowserCpu,
        suggestions: candidates.suggestions,
    };
    let response = f
        .service
        .certify_browser(&f.dataset, &f.repo, request.clone())
        .await
        .unwrap();
    let mut proof = response.suggestions[0].evidence.clone().unwrap();
    assert_eq!(proof.provenance.trust, PredictionTrust::BrowserReported);
    proof.provenance.confidence = 0.5;
    assert!(matches!(
        f.service
            .authorize_acceptance(
                &f.dataset,
                &f.repo,
                &"fresh".into(),
                &BTreeMap::from([("new".into(), proof)])
            )
            .await,
        Err(PrelabelFailure::Invalid)
    ));
    for execution in [
        PrelabelExecutionKind::ServerCpu,
        PrelabelExecutionKind::ServerCuda,
        PrelabelExecutionKind::ServerWebGpu,
    ] {
        let mut forged = request.clone();
        forged.execution = execution;
        assert_eq!(
            f.service
                .certify_browser(&f.dataset, &f.repo, forged)
                .await
                .unwrap_err(),
            PrelabelFailure::Invalid
        );
    }
    f.command(PrelabelAdminCommand::Reset {
        scope: Default::default(),
    })
    .await;
    assert_eq!(
        f.service
            .certify_browser(&f.dataset, &f.repo, request)
            .await
            .unwrap_err(),
        PrelabelFailure::Stale
    );
}

#[tokio::test]
async fn missing_mapping_blocks_start_and_model_changes_invalidate_acceptance() {
    let f = Fixture::new(Runner::default()).await;
    let hints = f.hints().await;
    std::fs::write(
        f.temp.path().join("models/model.onnx"),
        b"different model bytes",
    )
    .unwrap();
    assert!(matches!(
        f.service
            .authorize_acceptance(
                &f.dataset,
                &f.repo,
                &"fresh".into(),
                &BTreeMap::from([("new".into(), hints.suggestions[0].evidence.clone().unwrap())])
            )
            .await,
        Err(PrelabelFailure::Stale)
    ));
    let state = f
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::from([("boxes".into(), "missing".into())]),
        })
        .await;
    assert_eq!(state.runs[0].blockers.len(), 1);
    assert_eq!(
        f.service
            .command(
                &f.dataset,
                f.repo.clone(),
                PrelabelAdminCommand::Start {
                    run_id: state.runs[0].run_id.clone()
                }
            )
            .await
            .unwrap_err(),
        PrelabelFailure::NotReady
    );
}

#[tokio::test]
async fn restart_preserves_successful_results_and_exposes_interrupted_work_for_retry() {
    let f = Fixture::new(Runner::default()).await;
    let run = f
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await
        .runs[0]
        .run_id
        .clone();
    // This is the durable boundary before a worker starts; simulate process loss there.
    {
        let mut control = f.service.lock(&f.dataset).await.unwrap();
        control.runs[0].summary.phase = PrelabelRunPhase::Running;
        f.service.persist(&f.dataset, &control).await.unwrap();
    }
    let restarted = PrelabelService::new(
        f.temp.path(),
        &f.temp.path().join("models"),
        PrelabelLimits::default(),
        Arc::new(f.runner.clone()),
    )
    .await
    .unwrap();
    assert_eq!(
        restarted.admin_state(&f.dataset).await.unwrap().runs[0].phase,
        PrelabelRunPhase::Interrupted
    );
    restarted
        .command(
            &f.dataset,
            f.repo.clone(),
            PrelabelAdminCommand::Retry { run_id: run },
        )
        .await
        .unwrap();
    let restarted_fixture = Fixture {
        service: restarted,
        ..f
    };
    assert_eq!(restarted_fixture.finish().await.runs[0].generated, 6);
    assert!(restarted_fixture.hints().await.from_batch);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn model_paths_reject_traversal_and_symlinks() {
    let f = Fixture::new(Runner::default()).await;
    let mut config = f
        .repo
        .load_dataset_config()
        .await
        .unwrap()
        .prelabel_configs
        .remove(0);
    config.model.location = "../secret.onnx".into();
    assert_eq!(
        f.service.model(&config).await.unwrap_err(),
        PrelabelFailure::Invalid
    );
    std::os::unix::fs::symlink("model.onnx", f.temp.path().join("models/link.onnx")).unwrap();
    config.model.location = "link.onnx".into();
    assert_eq!(
        f.service.model(&config).await.unwrap_err(),
        PrelabelFailure::ModelUnavailable
    );
}

#[tokio::test]
async fn partial_failure_retries_only_failed_items_and_empty_results_are_reused() {
    let f = Fixture::new(Runner {
        failures: AtomicUsize::new(2),
        ..Default::default()
    })
    .await;
    let run_id = f
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await
        .runs[0]
        .run_id
        .clone();
    f.command(PrelabelAdminCommand::Start {
        run_id: run_id.clone(),
    })
    .await;
    let state = f.finish().await;
    assert_eq!((state.runs[0].generated, state.runs[0].failed), (4, 2));
    f.command(PrelabelAdminCommand::Retry { run_id }).await;
    assert_eq!(f.finish().await.runs[0].generated, 6);
    assert_eq!(f.runner.calls.load(Ordering::SeqCst), 8);

    let f = Fixture::new(Runner {
        empty: true,
        ..Default::default()
    })
    .await;
    let run_id = f
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await
        .runs[0]
        .run_id
        .clone();
    f.command(PrelabelAdminCommand::Start { run_id }).await;
    assert_eq!(f.finish().await.runs[0].empty, 6);
    let hints = f.hints().await;
    assert!(hints.from_batch && hints.suggestions.is_empty());
    assert_eq!(f.runner.calls.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn cancel_stops_in_flight_batch_without_publishing_or_completing_work() {
    let release = Arc::new(tokio::sync::Notify::new());
    let f = Fixture::new(Runner {
        pause: Some(release.clone()),
        ..Default::default()
    })
    .await;
    let run_id = f
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await
        .runs[0]
        .run_id
        .clone();
    f.command(PrelabelAdminCommand::Start {
        run_id: run_id.clone(),
    })
    .await;
    f.runner.started.notified().await;
    let state = f
        .command(PrelabelAdminCommand::Cancel {
            run_id: run_id.clone(),
        })
        .await;
    assert_eq!(state.runs[0].phase, PrelabelRunPhase::Cancelled);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while f.service.inner.running.lock().await.contains_key(&run_id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        f.runner
            .released
            .lock()
            .unwrap()
            .contains(&InferenceOwner::Batch {
                dataset: f.dataset.clone(),
                run: run_id.clone()
            })
    );
    release.notify_one();
    let state = f.service.admin_state(&f.dataset).await.unwrap();
    assert_eq!(state.retained_results, 0);
    assert_eq!(state.runs[0].pending, 6);
    assert!(
        f.repo
            .load_events(&"fresh".into())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn reset_waits_for_authorized_annotation_commit_guard() {
    let f = Fixture::new(Runner::default()).await;
    let hint = f.hints().await.suggestions.remove(0);
    let guard = f
        .service
        .authorize_acceptance(
            &f.dataset,
            &f.repo,
            &"fresh".into(),
            &BTreeMap::from([("accepted".into(), hint.evidence.unwrap())]),
        )
        .await
        .unwrap();
    let reset = f.service.command(
        &f.dataset,
        f.repo.clone(),
        PrelabelAdminCommand::Reset {
            scope: Default::default(),
        },
    );
    tokio::pin!(reset);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut reset)
            .await
            .is_err()
    );
    drop(guard);
    assert!(!reset.await.unwrap().paused_scopes.is_empty());
}

#[tokio::test]
async fn batch_rechecks_completed_work_and_excludes_disabled_or_import_excluded_workflows() {
    let f = Fixture::new(Runner::default()).await;
    let mut metadata = f.repo.load_dataset_config().await.unwrap();
    metadata.tasks[1].enabled = false;
    metadata.tasks[1].prelabel_config_ids.clear();
    f.repo.save_dataset(&metadata).await.unwrap();
    f.repo
        .append_events_atomic(
            &"fresh".into(),
            &[EventLogEntry::new(
                1,
                "fresh".into(),
                "admin".into(),
                DatasetRole::DataAdmin,
                now(),
                EventPayload::ImportInitialized {
                    import_id: "import".into(),
                    annotations: vec![],
                    migration_target_sets: vec![],
                    task_initializations: vec![ImportTaskInitialization {
                        task_id: "boxes".into(),
                        coverage: ImportCoverage::Excluded,
                        initial_state: TaskState::new("boxes".into(), now()),
                    }],
                },
            )],
        )
        .await
        .unwrap();
    let state = f
        .command(PrelabelAdminCommand::Preflight {
            mappings: BTreeMap::new(),
        })
        .await;
    assert_eq!(state.runs[0].total, 2);
    assert!(state.runs[0].blockers.is_empty());
    let mut completed = TaskState::new("boxes".into(), now());
    completed.status = TaskStatus::Completed;
    let sequence = f.repo.load_events(&"active".into()).await.unwrap().len() as u64 + 1;
    f.repo
        .append_events_atomic(
            &"active".into(),
            &[EventLogEntry::new(
                sequence,
                "active".into(),
                "admin".into(),
                DatasetRole::DataAdmin,
                now(),
                EventPayload::TaskStateChanged {
                    task_state: completed,
                },
            )],
        )
        .await
        .unwrap();
    f.command(PrelabelAdminCommand::Start {
        run_id: state.runs[0].run_id.clone(),
    })
    .await;
    let state = f.finish().await;
    assert_eq!((state.runs[0].generated, state.runs[0].skipped), (1, 1));
    assert_eq!(f.runner.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn interactive_worker_owner_uses_authenticated_user_and_dataset() {
    let f = Fixture::new(Runner::default()).await;
    f.hints().await;
    f.service
        .suggestions(
            &f.dataset,
            &f.repo,
            &"second-user".into(),
            &"fresh".into(),
            &"boxes".into(),
            &"model".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        *f.runner.owners.lock().unwrap(),
        vec![
            InferenceOwner::Interactive {
                dataset: f.dataset.clone(),
                user: "annotator".into()
            },
            InferenceOwner::Interactive {
                dataset: f.dataset.clone(),
                user: "second-user".into()
            },
        ]
    );
}
