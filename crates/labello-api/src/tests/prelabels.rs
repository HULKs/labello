use super::*;
use labello_domain::*;
use labello_storage::prelabel::{InferenceFuture, PrelabelLimits, PrelabelRunner, PrelabelService};

struct Runner;
impl PrelabelRunner for Runner {
    fn infer(
        &self,
        _: Vec<u8>,
        _: Vec<u8>,
        config: PrelabelConfig,
        task: TaskDefinition,
    ) -> InferenceFuture {
        Box::pin(async move {
            Ok(vec![PrelabelSuggestion {
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
            }])
        })
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    app: axum::Router,
    repo: labello_storage::DatasetRepository,
    assignment: Value,
}
impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("models")).unwrap();
        std::fs::write(temp.path().join("models/model.onnx"), b"test runner model").unwrap();
        let service = PrelabelService::new(
            temp.path(),
            &temp.path().join("models"),
            PrelabelLimits::default(),
            std::sync::Arc::new(Runner),
        )
        .await
        .unwrap();
        let app = router(ApiState::new(temp.path()).with_prelabel_service(service));
        create_dataset(&app).await;
        configure_pixel_task(&app).await;
        upload_test_image(&app, "prelabel.png", &png_bytes(100, 100)).await;
        let repo = labello_storage::DatasetRepository::new(temp.path().join("ds"));
        let mut metadata = repo.load_dataset_config().await.unwrap();
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
            }),
        }];
        repo.save_dataset(&metadata).await.unwrap();
        let assignment = claim_assignment(&app, "admin", "annotation").await;
        Self {
            _temp: temp,
            app,
            repo,
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
        let response = self.request("POST", "/datasets/ds/prelabel-suggestions", "admin", json!({"imageId": self.image(), "taskId": "bounding_box:pixel", "configId": "model"})).await;
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_value(response_json(response).await).unwrap()
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
        let path = format!(
            "/datasets/ds/images/{}/annotation-batch?assignmentId={}&imageId={}&taskId=bounding_box%3Apixel&kind=annotation",
            self.image(),
            self.assignment["assignmentId"].as_str().unwrap(),
            self.image()
        );
        self.request("POST", &path, "admin", serde_json::to_value(batch).unwrap())
            .await
    }
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
    f.reset().await;
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
async fn prelabel_routes_enforce_roles_csrf_and_dataset_boundaries() {
    let f = Fixture::new().await;
    let query = json!({"imageId": f.image(), "taskId": "bounding_box:pixel", "configId": "model"});
    for user in ["reviewer_2", "outsider"] {
        assert_eq!(
            f.request(
                "POST",
                "/datasets/ds/prelabel-suggestions",
                user,
                query.clone()
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED,
            "{user}"
        );
    }
    for method in ["GET", "POST"] {
        let command = serde_json::to_value(PrelabelAdminCommand::Reset {
            scope: Default::default(),
        })
        .unwrap();
        assert_eq!(
            f.request(
                method,
                "/datasets/ds/prelabel-management",
                "other_annotator",
                command
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        f.request(
            "GET",
            "/datasets/ds/prelabels/model/model",
            "outsider",
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let model = f
        .request(
            "GET",
            "/datasets/ds/prelabels/model/model",
            "other_annotator",
            Value::Null,
        )
        .await;
    assert_eq!(model.status(), StatusCode::OK);
    assert_eq!(model.headers()[header::CACHE_CONTROL], "private, no-store");
    let response = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/datasets/ds/prelabel-suggestions")
                .header("x-test-user-id", "admin")
                .header(crate::csrf::HEADER, "invalid")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(query.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.request(
            "POST",
            "/datasets/missing/prelabel-suggestions",
            "admin",
            query
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn accepted_prelabel_survives_later_edits_snapshots_offline_statistics_and_ground_truth_export()
 {
    let f = Fixture::new().await;
    let mut metadata = f.repo.load_dataset_config().await.unwrap();
    metadata.tasks[0].review.workflow = ReviewWorkflow::None;
    f.repo.save_dataset(&metadata).await.unwrap();
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
