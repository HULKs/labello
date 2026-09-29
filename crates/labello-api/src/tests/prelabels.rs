use super::*;
use labello_domain::*;
use labello_storage::prelabel::{InferenceFuture, PrelabelLimits, PrelabelRunner, PrelabelService};

struct Runner(PrelabelExecutionKind);
impl PrelabelRunner for Runner {
    fn inspect(&self, _model: Vec<u8>) -> labello_storage::prelabel::ModelInspectionFuture {
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
        _owner: labello_storage::prelabel::InferenceOwner,
        _: Vec<u8>,
        _: Vec<u8>,
        config: PrelabelConfig,
        task: TaskDefinition,
    ) -> InferenceFuture {
        let execution = self.0.clone();
        Box::pin(async move {
            Ok(PrelabelInferenceResult {
                execution,
                suggestions: vec![PrelabelSuggestion {
                    suggestion_id: "candidate".into(),
                    config_id: config.config_id,
                    task_id: task.task_id,
                    class_id: "pixel".into(),
                    confidence: 0.9,
                    geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                        x: 0.1,
                        y: 0.1,
                        width: 0.4,
                        height: 0.4,
                    }),
                    evidence: None,
                }],
            })
        })
    }
}

#[tokio::test]
async fn model_check_accepts_an_unsaved_filename_and_enforces_admin_csrf_and_path_limits() {
    let fixture = Fixture::new().await;
    let route = "/datasets/ds/prelabel-model-check";
    let request = json!({"location": "model.onnx"});
    assert_eq!(
        fixture
            .request("POST", route, "admin", request.clone())
            .await
            .status(),
        StatusCode::OK
    );
    for actor in ["other_annotator", "outsider"] {
        assert_eq!(
            fixture
                .request("POST", route, actor, request.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    for location in [
        "../model.onnx",
        "/tmp/model.onnx",
        "https://example.com/model.onnx",
    ] {
        assert_eq!(
            fixture
                .request("POST", route, "admin", json!({"location": location}))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let missing = fixture
        .request("POST", route, "admin", json!({"location":"missing.onnx"}))
        .await;
    assert_eq!(missing.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response_json(missing).await;
    assert!(
        !body
            .to_string()
            .contains(fixture._temp.path().to_str().unwrap())
    );
    assert_eq!(
        fixture
            .request("POST", route, "admin", json!({"location":"x".repeat(5000)}))
            .await
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        fixture
            .request(
                "POST",
                "/datasets/missing/prelabel-model-check",
                "admin",
                request.clone()
            )
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let denied = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(route)
                .header("x-test-user-id", "admin")
                .header(crate::csrf::HEADER, "invalid")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(request.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn explicit_model_mapping_rejects_unknown_dataset_classes_and_round_trips() {
    let fixture = Fixture::new().await;
    let mut config = fixture
        .repo
        .load_dataset_config()
        .await
        .unwrap()
        .prelabel_configs
        .remove(0);
    let spec = config.yolo.as_mut().unwrap();
    spec.use_explicit_mapping(80);
    spec.output_name = Some("output0".into());
    spec.class_mappings[0].model_class_id = 32;
    let saved = fixture
        .request(
            "POST",
            "/datasets/ds/prelabels",
            "admin",
            serde_json::to_value(&config).unwrap(),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(
        fixture
            .repo
            .load_dataset_config()
            .await
            .unwrap()
            .prelabel_configs[0],
        config
    );
    config.yolo.as_mut().unwrap().class_mappings[0].class_id = "unknown".into();
    assert_eq!(
        fixture
            .request(
                "POST",
                "/datasets/ds/prelabels",
                "admin",
                serde_json::to_value(config).unwrap()
            )
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
}

struct Fixture {
    _temp: tempfile::TempDir,
    app: axum::Router,
    repo: labello_storage::DatasetRepository,
    service: PrelabelService,
    assignment: Value,
}

#[tokio::test]
async fn workflow_model_selection_rejects_new_multiple_models_but_preserves_legacy_lists() {
    let fixture = Fixture::new().await;
    let mut metadata = fixture.repo.load_dataset_config().await.unwrap();
    let mut second = metadata.prelabel_configs[0].clone();
    second.config_id = "second".into();
    metadata.prelabel_configs.push(second);
    fixture.repo.save_dataset(&metadata).await.unwrap();
    metadata.tasks[0].prelabel_config_ids.push("second".into());
    let update = serde_json::to_value(&metadata).unwrap();
    assert_eq!(
        fixture
            .request("PUT", "/datasets/ds/admin", "admin", update.clone())
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    // Old datasets can retain their ordered list until an admin chooses a replacement.
    fixture.repo.save_dataset(&metadata).await.unwrap();
    assert_eq!(
        fixture
            .request("PUT", "/datasets/ds/admin", "admin", update)
            .await
            .status(),
        StatusCode::OK
    );
    metadata.tasks[0].prelabel_config_ids.reverse();
    assert_eq!(
        fixture
            .request(
                "PUT",
                "/datasets/ds/admin",
                "admin",
                serde_json::to_value(&metadata).unwrap()
            )
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    metadata.tasks[0].prelabel_config_ids.truncate(1);
    assert_eq!(
        fixture
            .request(
                "PUT",
                "/datasets/ds/admin",
                "admin",
                serde_json::to_value(&metadata).unwrap()
            )
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn server_gpu_provenance_survives_acceptance_replay_snapshots_and_offline_wire() {
    for execution in [
        PrelabelExecutionKind::ServerCuda,
        PrelabelExecutionKind::ServerWebGpu,
    ] {
        let fixture = Fixture::with_execution(execution.clone()).await;
        let response = fixture.hints().await;
        assert_eq!(response.execution, Some(execution.clone()));
        let hint = &response.suggestions[0];
        assert_eq!(
            hint.evidence.as_ref().unwrap().provenance.execution,
            execution
        );
        assert_eq!(
            hint.evidence.as_ref().unwrap().provenance.trust,
            PredictionTrust::ServerGenerated
        );
        let saved = fixture
            .save(&fixture.batch("accepted", hint, false, false))
            .await;
        assert_eq!(saved.status(), StatusCode::OK);
        let saved: ImageState = serde_json::from_value(response_json(saved).await).unwrap();
        let mut replayed = ImageState::new(fixture.image());
        for event in fixture.repo.load_events(&fixture.image()).await.unwrap() {
            replayed.apply_event(&event).unwrap();
            assert_eq!(
                serde_json::from_slice::<ImageState>(&serde_json::to_vec(&replayed).unwrap())
                    .unwrap(),
                replayed
            );
        }
        assert_eq!(saved, replayed);
        let snapshot = fixture.repo.create_snapshot().await.unwrap();
        let bytes = fixture
            .repo
            .snapshot_file(
                &snapshot.snapshot_id,
                &format!("annotations/{}/state.json", fixture.image()),
            )
            .await
            .unwrap();
        assert_eq!(serde_json::from_slice::<ImageState>(&bytes).unwrap(), saved);
        let bundle = fixture
            .repo
            .create_offline_bundle(&"admin".into(), 10, false)
            .await
            .unwrap();
        let decoded: OfflineBundle =
            serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert_eq!(decoded.images[0].state, saved);
    }
}
impl Fixture {
    async fn new() -> Self {
        Self::with_execution(PrelabelExecutionKind::ServerCpu).await
    }
    async fn with_execution(execution: PrelabelExecutionKind) -> Self {
        Self::with_configuration(execution, ReviewWorkflow::Approval).await
    }
    async fn with_configuration(execution: PrelabelExecutionKind, review: ReviewWorkflow) -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("models")).unwrap();
        std::fs::write(temp.path().join("models/model.onnx"), b"test runner model").unwrap();
        let service = PrelabelService::new(
            temp.path(),
            &temp.path().join("models"),
            PrelabelLimits::default(),
            std::sync::Arc::new(Runner(execution)),
        )
        .await
        .unwrap();
        let state = ApiState::new(temp.path()).with_prelabel_service(service.clone());
        let app = router(state.clone());
        create_dataset(&app).await;
        configure_pixel_task(&app).await;
        upload_test_image(&app, "prelabel.png", &png_bytes(100, 100)).await;
        let repo = state.repo(&"ds".into()).unwrap().as_ref().clone();
        let mut metadata = repo.load_dataset_config().await.unwrap();
        metadata.tasks[0].review.workflow = review;
        metadata.tasks[0].prelabel_config_ids = vec!["model".into()];
        metadata.prelabel_configs = vec![PrelabelConfig {
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
                class_ids: vec![Some("pixel".into())],
                keypoints: vec![],
                ..Default::default()
            }),
        }];
        repo.save_dataset(&metadata).await.unwrap();
        // Exercise backward-compatible saves for leases opened by an older
        // client. These hints come from an admin batch, never a user route.
        let assignment = claim_assignment(&app, "admin", "annotation").await;
        assert!(!assignment.is_null());
        let prepared = service
            .command(
                &"ds".into(),
                repo.clone(),
                PrelabelAdminCommand::Preflight {
                    mappings: BTreeMap::new(),
                },
            )
            .await
            .unwrap();
        service
            .command(
                &"ds".into(),
                repo.clone(),
                PrelabelAdminCommand::Start {
                    run_id: prepared.runs[0].run_id.clone(),
                },
            )
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let jobs = service.admin_state(&"ds".into()).await.unwrap();
                if jobs
                    .runs
                    .iter()
                    .all(|run| run.phase == PrelabelRunPhase::Completed)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        Self {
            _temp: temp,
            app,
            repo,
            service,
            assignment,
        }
    }
    fn image(&self) -> ImageId {
        self.assignment["imageId"].as_str().unwrap().into()
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        user: &str,
        body: Value,
    ) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("x-test-user-id", user)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn hints(&self) -> PrelabelResponse {
        if let Some(retained) = self
            .service
            .retained_suggestions(
                &"ds".into(),
                &self.repo,
                &self.image(),
                &"bounding_box:pixel".into(),
            )
            .await
            .unwrap()
        {
            return retained.response;
        }
        PrelabelResponse {
            suggestions: Vec::new(),
            generation: self
                .service
                .generation_status(&"ds".into(), &"bounding_box:pixel".into(), &"model".into())
                .await
                .unwrap(),
            execution: None,
            from_batch: true,
            browser_grant: None,
        }
    }
    async fn reset(&self) {
        let response = self
            .request(
                "POST",
                "/datasets/ds/prelabel-management",
                "admin",
                serde_json::to_value(PrelabelAdminCommand::Reset {
                    scope: Default::default(),
                })
                .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    fn batch(
        &self,
        id: &str,
        hint: &PrelabelSuggestion,
        edited: bool,
        complete: bool,
    ) -> labello_client::AnnotationBatchRequest {
        let mut annotation = AnnotationVersion::native(
            id.into(),
            hint.task_id.clone(),
            hint.class_id.clone(),
            AnnotationType::BoundingBox,
            hint.geometry.clone(),
            "admin".into(),
            now(),
        );
        if edited {
            annotation.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.2,
                y: 0.2,
                width: 0.4,
                height: 0.4,
            });
        }
        labello_client::AnnotationBatchRequest {
            payloads: vec![EventPayload::AnnotationVersionCreated {
                annotation,
                previous_version: None,
                reason: None,
            }],
            prelabel_acceptances: BTreeMap::from([(id.into(), hint.evidence.clone().unwrap())]),
            complete,
        }
    }
    async fn save(
        &self,
        batch: &labello_client::AnnotationBatchRequest,
    ) -> axum::response::Response {
        self.save_for(&self.assignment, batch).await
    }
    async fn save_for(
        &self,
        assignment: &Value,
        batch: &labello_client::AnnotationBatchRequest,
    ) -> axum::response::Response {
        let image = assignment["imageId"].as_str().unwrap();
        let path = format!(
            "/datasets/ds/images/{}/annotation-batch?assignmentId={}&imageId={}&taskId=bounding_box%3Apixel&kind=annotation",
            image,
            assignment["assignmentId"].as_str().unwrap(),
            image
        );
        self.request("POST", &path, "admin", serde_json::to_value(batch).unwrap())
            .await
    }
}

#[tokio::test]
async fn managed_prediction_acceptance_survives_reset_and_waits_for_overview_before_review() {
    let f = Fixture::new().await;
    upload_test_image(&f.app, "queued.png", &png_bytes(101, 100)).await;
    f.service
        .synchronize_workflows(&"ds".into(), f.repo.clone(), true)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let jobs = f.service.admin_state(&"ds".into()).await.unwrap();
            if jobs
                .runs
                .iter()
                .all(|run| run.phase == PrelabelRunPhase::Completed)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let selection = |kind: &str, variant: &str| json!({"selection": {"taskId": "bounding_box:pixel", "kind": kind, "variant": variant}});
    let response = f
        .request(
            "POST",
            "/datasets/ds/work-items/claim",
            "admin",
            selection("annotation", "objects"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let object = response_json(response).await;
    assert!(!object.is_null());
    let action = |assignment: &Value| json!({"assignmentId": assignment["assignmentId"], "imageId": assignment["imageId"], "taskId": assignment["taskId"], "kind": assignment["kind"]});
    let response = f
        .request(
            "POST",
            "/datasets/ds/work-items/display",
            "admin",
            action(&object),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let state: ImageState = serde_json::from_value(response_json(response).await).unwrap();
    let hint = state.workflow_preparations[&"bounding_box:pixel".into()].prelabels[0].clone();
    f.reset().await;
    let response = f
        .save_for(&object, &f.batch("queued-object", &hint, false, true))
        .await;
    let status = response.status();
    let value = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let saved: ImageState = serde_json::from_value(value).unwrap();
    assert_eq!(
        saved.task_states[&hint.task_id].status,
        TaskStatus::InProgress
    );
    assert!(saved.workflow_pending_objects(&hint.task_id).is_empty());
    let response = f
        .request(
            "POST",
            "/datasets/ds/work-items/claim",
            "reviewer_2",
            selection("review", "objects"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response_json(response).await.is_null());
    let response = f
        .request(
            "POST",
            "/datasets/ds/work-items/claim",
            "admin",
            selection("annotation", "overview"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let overview = response_json(response).await;
    assert_eq!(overview["imageId"], object["imageId"]);
    let response = f
        .request(
            "POST",
            "/datasets/ds/work-items/display",
            "admin",
            action(&overview),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = f
        .save_for(
            &overview,
            &labello_client::AnnotationBatchRequest {
                payloads: vec![],
                prelabel_acceptances: BTreeMap::new(),
                complete: true,
            },
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let completed: ImageState = serde_json::from_value(response_json(response).await).unwrap();
    assert_eq!(
        completed.task_states[&hint.task_id].status,
        TaskStatus::Submitted
    );
    assert_eq!(
        f.repo
            .rebuild_image_state(&hint.evidence.as_ref().unwrap().provenance.image_id)
            .await
            .unwrap(),
        completed
    );
    let response = f
        .request(
            "POST",
            "/datasets/ds/work-items/claim",
            "reviewer_2",
            selection("review", "objects"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["imageId"], object["imageId"]);
}

#[tokio::test]
async fn exact_prelabel_acceptance_is_human_reviewable_and_replayable_and_retry_survives_reset() {
    let f = Fixture::new().await;
    let hint = f.hints().await.suggestions.remove(0);
    let batch = f.batch("accepted", &hint, false, true);
    let response = f.save(&batch).await;
    let status = response.status();
    let value = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let state: ImageState = serde_json::from_value(value).unwrap();
    let annotation = state.current_annotation(&"accepted".into()).unwrap();
    assert_eq!(
        annotation.revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::AcceptedUnchanged
        }
    );
    let AnnotationOrigin::Prelabel { prelabel } = &annotation.origin else {
        panic!("missing prelabel origin");
    };
    assert_eq!(
        prelabel.provenance,
        hint.evidence.as_ref().unwrap().provenance
    );
    assert_eq!(prelabel.predicted_geometry, hint.geometry);
    assert_eq!(
        state.task_states[&hint.task_id].status,
        TaskStatus::Submitted
    );
    let events = f.repo.load_events(&f.image()).await.unwrap();
    let mut replayed = ImageState::new(f.image());
    for event in &events {
        replayed.apply_event(event).unwrap();
        let round_trip: ImageState =
            serde_json::from_slice(&serde_json::to_vec(&replayed).unwrap()).unwrap();
        assert_eq!(round_trip, replayed);
    }
    assert_eq!(replayed, state);
    f.reset().await;
    let retried = f.save(&batch).await;
    let status = retried.status();
    let value = response_json(retried).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["currentSequence"], state.current_sequence);
    assert_eq!(f.repo.load_events(&f.image()).await.unwrap(), events);
    assert!(
        !claim_assignment(&f.app, "reviewer_2", "review")
            .await
            .is_null()
    );
}

#[tokio::test]
async fn edited_acceptance_keeps_original_prediction_and_duplicate_acceptance_is_rejected() {
    let f = Fixture::new().await;
    let hint = f.hints().await.suggestions.remove(0);
    let response = f.save(&f.batch("edited", &hint, true, false)).await;
    let status = response.status();
    let value = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let state: ImageState = serde_json::from_value(value).unwrap();
    let annotation = state.current_annotation(&"edited".into()).unwrap();
    assert_eq!(
        annotation.revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::Edited
        }
    );
    let AnnotationOrigin::Prelabel { prelabel } = &annotation.origin else {
        panic!("missing prelabel origin");
    };
    assert_eq!(prelabel.predicted_geometry, hint.geometry);
    assert_ne!(annotation.geometry, hint.geometry);
    let response = f.save(&f.batch("duplicate", &hint, false, false)).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        f.repo
            .load_image_state(&f.image())
            .await
            .unwrap()
            .current_annotation(&"duplicate".into())
            .is_none()
    );
}

#[tokio::test]
async fn reset_and_tampering_reject_new_acceptance_without_writing_events() {
    let f = Fixture::new().await;
    let hint = f.hints().await.suggestions.remove(0);
    let events = f.repo.load_events(&f.image()).await.unwrap();
    let mut forged = f.batch("forged", &hint, false, false);
    forged
        .prelabel_acceptances
        .get_mut(&"forged".into())
        .unwrap()
        .provenance
        .model_id = "forged".into();
    assert_eq!(f.save(&forged).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(f.repo.load_events(&f.image()).await.unwrap(), events);
    f.reset().await;
    let events = f.repo.load_events(&f.image()).await.unwrap();
    assert_eq!(
        f.save(&f.batch("stale", &hint, false, false))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert!(f.hints().await.generation.paused);
    assert_eq!(f.repo.load_events(&f.image()).await.unwrap(), events);
}

#[tokio::test]
async fn prelabel_acceptance_rechecks_boxes_committed_after_inference() {
    let f = Fixture::new().await;
    let hint = f.hints().await.suggestions.remove(0);
    let mut manual = f.batch("manual", &hint, false, false);
    manual.prelabel_acceptances.clear();
    assert_eq!(f.save(&manual).await.status(), StatusCode::OK);
    let events = f.repo.load_events(&f.image()).await.unwrap();
    let response = f.save(&f.batch("overlapping", &hint, false, false)).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(f.repo.load_events(&f.image()).await.unwrap(), events);
}

#[tokio::test]
async fn annotators_cannot_select_models_generate_hints_or_download_models() {
    let f = Fixture::new().await;
    for (method, route) in [
        ("POST", "/datasets/ds/prelabel-suggestions"),
        ("GET", "/datasets/ds/prelabel-retained"),
        ("GET", "/datasets/ds/prelabel-generation"),
        ("POST", "/datasets/ds/prelabel-browser-result"),
        ("GET", "/datasets/ds/prelabels/model/model"),
    ] {
        for actor in ["admin", "other_annotator"] {
            assert_eq!(
                f.request(method, route, actor, json!({})).await.status(),
                StatusCode::NOT_FOUND,
                "{method} {route}"
            );
        }
    }
    for actor in ["other_annotator", "outsider"] {
        assert_eq!(
            f.request(
                "POST",
                "/datasets/ds/prelabel-management",
                actor,
                serde_json::to_value(PrelabelAdminCommand::Reset {
                    scope: Default::default()
                })
                .unwrap()
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn accepted_prelabel_survives_later_edits_snapshots_offline_statistics_and_ground_truth_export()
 {
    let f =
        Fixture::with_configuration(PrelabelExecutionKind::ServerCpu, ReviewWorkflow::None).await;
    let hint = f.hints().await.suggestions.remove(0);
    let response = f.save(&f.batch("accepted", &hint, false, false)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let state: ImageState = serde_json::from_value(response_json(response).await).unwrap();
    let original = state.current_annotation(&"accepted".into()).unwrap();
    let mut edited = original.clone();
    edited.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.2,
        width: 0.4,
        height: 0.4,
    });
    let response = f
        .save(&labello_client::AnnotationBatchRequest {
            payloads: vec![EventPayload::AnnotationVersionCreated {
                annotation: edited,
                previous_version: Some(1),
                reason: None,
            }],
            prelabel_acceptances: BTreeMap::new(),
            complete: true,
        })
        .await;
    let status = response.status();
    let value = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let saved: ImageState = serde_json::from_value(value).unwrap();
    let current = saved.current_annotation(&"accepted".into()).unwrap();
    assert_eq!(current.origin, original.origin);
    assert_eq!(current.version, 2);
    assert_eq!(
        current.revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::Edited
        }
    );
    assert_eq!(
        saved.task_states[&hint.task_id].status,
        TaskStatus::Completed
    );

    let snapshot = f.repo.create_snapshot().await.unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .all(|file| !file.path.contains("prelabels/"))
    );
    let bytes = f
        .repo
        .snapshot_file(
            &snapshot.snapshot_id,
            &format!("annotations/{}/state.json", f.image()),
        )
        .await
        .unwrap();
    assert_eq!(serde_json::from_slice::<ImageState>(&bytes).unwrap(), saved);
    let offline = f
        .repo
        .create_offline_bundle(&"admin".into(), 10, false)
        .await
        .unwrap();
    let decoded: OfflineBundle =
        serde_json::from_slice(&serde_json::to_vec(&offline).unwrap()).unwrap();
    assert_eq!(decoded.images[0].state, saved);
    let stats = f.repo.dataset_stats().await.unwrap();
    assert_eq!(stats.provenance.accepted_prelabel_annotations, 1);
    assert_eq!(stats.completed_tasks, 1);
    let mut event = f
        .repo
        .load_events(&f.image())
        .await
        .unwrap()
        .into_iter()
        .find(|event| matches!(event.payload, EventPayload::AnnotationVersionCreated { .. }))
        .unwrap();
    event.schema_version = LEGACY_SCHEMA_VERSION;
    assert!(serde_json::to_value(event).is_err());

    let export = labello_storage::export::ExportService::new(f._temp.path(), Default::default())
        .await
        .unwrap();
    let options = ExportOptions {
        profile: ExportProfile::UltralyticsYoloDetectV1,
        classes: BTreeSet::from([ExportClassSelection {
            task_id: hint.task_id,
            class_id: hint.class_id,
        }]),
        fallback_split: ExportSplit::Train,
        splits: ExportSplit::all(),
        split_choices: BTreeMap::new(),
    };
    let job = export
        .preflight(&"ds".into(), f.repo.clone(), options)
        .await
        .unwrap();
    let settle = || async {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let job = export.job(&"ds".into(), &job.job_id).await.unwrap();
                if !job.phase.is_active() {
                    return job;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    };
    let ready = settle().await;
    assert_eq!(ready.phase, labello_storage::export::ExportPhase::Ready);
    assert_eq!(ready.summary.as_ref().unwrap().objects, 1);
    export.start(&"ds".into(), &job.job_id).await.unwrap();
    assert_eq!(
        settle().await.phase,
        labello_storage::export::ExportPhase::Succeeded
    );
    let (file, _, _permit) = export.download(&"ds".into(), &job.job_id).await.unwrap();
    assert!(file.metadata().unwrap().len() > 0);
}

#[tokio::test]
async fn sessions_report_prelabel_availability_without_requiring_generation() {
    for available in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut state = ApiState::new(temp.path())
            .with_session_cookie_secure(false)
            .with_browser_origins(vec!["https://app.example.com".into()])
            .unwrap()
            .with_local_admin_login(Some("admin".into()));
        if available {
            let models = temp.path().join("models");
            std::fs::create_dir(&models).unwrap();
            state = state.with_prelabel_service(
                PrelabelService::new(
                    temp.path(),
                    &models,
                    PrelabelLimits::default(),
                    std::sync::Arc::new(Runner(PrelabelExecutionKind::ServerCpu)),
                )
                .await
                .unwrap(),
            );
        }
        let app = router(state);
        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/local-admin")
                    .header(header::ORIGIN, "https://app.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert_eq!(response_json(login).await["prelabelAvailable"], available);
        let me = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
        assert_eq!(me.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(me).await["prelabelAvailable"], available);
        let unauthenticated = app
            .oneshot(Request::builder().uri("/me").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    }
}
