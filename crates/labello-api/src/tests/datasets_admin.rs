#[tokio::test]
async fn creates_dataset_and_requires_authentication() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/datasets")
                .header("x-test-user-id", "admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "datasetId": "ds",
                        "name": "Dataset",
                        "adminUserId": "admin"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("x-request-id"));

    let create_without_identity = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/datasets")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "datasetId": "ds2",
                        "name": "Dataset 2",
                        "adminUserId": "admin"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_without_identity.status(), StatusCode::UNAUTHORIZED);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/datasets/ds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = app
        .oneshot(
            Request::builder()
                .uri("/datasets/ds")
                .header("x-test-user-id", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
    let body = to_bytes(authorized.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["datasetId"], "ds");
    assert!(value["roleAssignments"].is_array());
}

#[tokio::test]
async fn rejects_unsafe_dataset_ids_and_existing_datasets() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;

    let duplicate = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/datasets")
                .header("x-test-user-id", "admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "datasetId": "ds",
                        "name": "Replacement",
                        "adminUserId": "admin"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    let unsafe_id = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/datasets")
                .header("x-test-user-id", "admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "datasetId": "../escape",
                        "name": "Escape",
                        "adminUserId": "admin"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsafe_id.status(), StatusCode::BAD_REQUEST);
    assert!(!temp.path().parent().unwrap().join("escape").exists());

    let unsafe_image_id = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/images/bad%5Cid/record")
                .header("x-test-user-id", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsafe_image_id.status(), StatusCode::BAD_REQUEST);

    let unsafe_user_id = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/keybindings")
                .header("x-test-user-id", "../escape")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsafe_user_id.status(), StatusCode::BAD_REQUEST);

    let existing = app
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/admin")
                .header("x-test-user-id", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(existing.into_body(), usize::MAX).await.unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(metadata["name"], "Dataset");
}

#[tokio::test]
async fn protects_admin_dataset_config() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/datasets")
                .header("x-test-user-id", "admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "datasetId": "ds",
                        "name": "Dataset",
                        "adminUserId": "admin"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);

    let non_admin = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/admin")
                .header("x-test-user-id", "intruder")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(non_admin.status(), StatusCode::UNAUTHORIZED);

    let admin = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/admin")
                .header("x-test-user-id", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin.status(), StatusCode::OK);
    let body = to_bytes(admin.into_body(), usize::MAX).await.unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["imageRoots"], json!(["images"]));

    value["name"] = json!("Updated Dataset");
    value["imageRoots"] = json!(["images", "imports/batch-1"]);
    let update = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/datasets/ds/admin")
                .header("x-test-user-id", "admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "name": value["name"],
                        "imageRoots": value["imageRoots"],
                        "labelClasses": value["labelClasses"],
                        "tasks": value["tasks"],
                        "roleAssignments": value["roleAssignments"],
                        "imbalance": value["imbalance"],
                        "prelabelConfigs": value["prelabelConfigs"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update.status(), StatusCode::OK);

    let mut duplicate_roles = value["roleAssignments"].as_array().unwrap().clone();
    duplicate_roles.push(duplicate_roles[0].clone());
    let duplicate = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/datasets/ds/admin")
                .header("x-test-user-id", "admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "name": value["name"],
                        "imageRoots": value["imageRoots"],
                        "labelClasses": value["labelClasses"],
                        "tasks": value["tasks"],
                        "roleAssignments": duplicate_roles,
                        "imbalance": value["imbalance"],
                        "prelabelConfigs": value["prelabelConfigs"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn config_endpoints_do_not_parse_image_index() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;
    tokio::fs::write(
        temp.path().join("ds").join("images-index.json"),
        b"not json",
    )
    .await
    .unwrap();

    for uri in [
        "/datasets/ds",
        "/datasets/ds/admin",
        "/datasets/ds/keybindings",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("x-test-user-id", "admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
    }
}

#[tokio::test]
async fn data_admin_lists_discovered_users_and_assigns_roles() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;
    let discover = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/me")
                .header("x-test-user-id", "worker")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(discover.status(), StatusCode::OK);

    let assigned = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/datasets/ds/roles")
                .header("x-test-user-id", "admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "userId": "worker", "roles": ["annotator", "reviewer"] }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(assigned.status(), StatusCode::OK);

    let users = app
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/users")
                .header("x-test-user-id", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(users.status(), StatusCode::OK);
    let body = to_bytes(users.into_body(), usize::MAX).await.unwrap();
    let users: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    let worker = users
        .iter()
        .find(|user| user["account"]["userId"] == "worker")
        .unwrap();
    assert_eq!(worker["roles"], json!(["annotator", "reviewer"]));
}

#[tokio::test]
async fn data_admin_explores_images_with_bounded_pagination_and_filters() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;
    configure_pixel_task(&app).await;
    upload_test_image(&app, "alpha.png", &png_bytes(2, 2)).await;
    upload_test_image(&app, "beta.png", &png_bytes(3, 2)).await;

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/images")
                .header("x-test-user-id", "intruder")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let first_page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/images?page=1&pageSize=1&status=pending&taskId=bounding_box%3Apixel")
                .header("x-test-user-id", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_page.status(), StatusCode::OK);
    let body = to_bytes(first_page.into_body(), usize::MAX).await.unwrap();
    let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["totalItems"], 2);
    assert_eq!(page["totalPages"], 2);
    assert_eq!(
        page["items"][0]["taskStatuses"]["bounding_box:pixel"],
        "pending"
    );

    let searched = app
        .oneshot(
            Request::builder()
                .uri("/datasets/ds/images?pageSize=500&search=BETA")
                .header("x-test-user-id", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(searched.status(), StatusCode::OK);
    let body = to_bytes(searched.into_body(), usize::MAX).await.unwrap();
    let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(page["pageSize"], 100);
    assert_eq!(page["totalItems"], 1);
    assert_eq!(page["items"][0]["image"]["fileName"], "beta.png");
}

#[tokio::test]
async fn inspector_browsing_and_return_permissions_are_separate() {
    let temp = tempfile::tempdir().unwrap();
    let api = ApiState::new(temp.path());
    let app = router(api.clone());
    create_dataset(&app).await;
    configure_pixel_task(&app).await;
    upload_test_image(&app, "inspected.png", &png_bytes(3, 2)).await;
    let repo = api.repo(&DatasetId::from("ds")).unwrap();
    let metadata = repo.load_dataset().await.unwrap();
    let image = metadata.images.values().next().unwrap().image_id.clone();
    let task = TaskId::from("bounding_box:pixel");
    repo.append_payload(
        &image,
        &Actor {
            user_id: UserId::from("admin"),
            role: DatasetRole::Annotator,
        },
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task.clone(),
                status: TaskStatus::Completed,
                outcome: Some(TaskOutcome::Approved),
                assigned_to: None,
                completed_by: Some(UserId::from("admin")),
                completed_at: Some(now()),
                updated_at: now(),
            },
        },
    )
    .await
    .unwrap();
    let before = repo.load_events(&image).await.unwrap();
    for user in ["admin", "other_annotator", "reviewer_2"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/datasets/ds/images?status=completed&pageSize=1")
                    .header("x-test-user-id", user)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["totalItems"], 1);
    }
    assert_eq!(repo.load_events(&image).await.unwrap(), before);
    let request = labello_domain::ReturnToReviewRequest {
        request_id: labello_domain::EventId::generate(),
        expected_sequence: before.len() as u64,
        task_ids: vec![task.clone()],
        reason: "Please inspect again".into(),
    };
    let uri = format!("/datasets/ds/images/{image}/return-to-review");
    for user in ["other_annotator", "intruder"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .header("x-test-user-id", user)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&uri)
                .header("x-test-user-id", "reviewer_2")
                .header("x-csrf-token", "invalid")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(repo.load_events(&image).await.unwrap(), before);
    for user in ["reviewer_2", "admin"] {
        let state = repo.load_image_state(&image).await.unwrap();
        let mut body = request.clone();
        body.expected_sequence = state.current_sequence;
        body.request_id = labello_domain::EventId::generate();
        let mut blank = body.clone();
        blank.reason = "  ".into();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .header("x-test-user-id", user)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&blank).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&uri)
                        .header("x-test-user-id", user)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        let after = repo.load_image_state(&image).await.unwrap();
        assert_eq!(after.current_sequence, state.current_sequence + 1);
        assert_eq!(after.task_states[&task].status, TaskStatus::Submitted);
        let mut task_state = after.task_states[&task].clone();
        task_state.status = TaskStatus::Completed;
        repo.append_payload(
            &image,
            &Actor {
                user_id: UserId::from(user),
                role: DatasetRole::Reviewer,
            },
            EventPayload::TaskStateChanged { task_state },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn unfiltered_gallery_loads_only_requested_page_states() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    upload_test_image(&app, "alpha.png", &png_bytes(2, 2)).await;
    upload_test_image(&app, "beta.png", &png_bytes(3, 2)).await;
    let repo = state.repo(&"ds".into()).unwrap();
    let index = repo.load_images_index().await.unwrap();
    let other = index.images_by_hash.values().find(|r| r.file_name == "beta.png").unwrap();
    // Reading off-page history is both unnecessary work and an unrelated failure.
    tokio::fs::create_dir_all(repo.events_path(&other.image_id).parent().unwrap()).await.unwrap();
    tokio::fs::write(repo.events_path(&other.image_id), b"invalid event\n").await.unwrap();
    for (query, expected) in [("page=1&pageSize=1", StatusCode::OK), ("page=2&pageSize=1", StatusCode::INTERNAL_SERVER_ERROR), ("page=1&pageSize=1&status=pending", StatusCode::INTERNAL_SERVER_ERROR)] {
        let response = app.clone().oneshot(Request::builder()
            .uri(format!("/datasets/ds/images?{query}"))
            .header("x-test-user-id", "admin").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(page["totalItems"], 2);
            assert_eq!(page["items"][0]["image"]["fileName"], "alpha.png");
        }
    }
}

async fn create_with_schema(app: &axum::Router, id: &str, source: Option<&str>, actor: &str) -> axum::response::Response {
    app.clone().oneshot(Request::builder().method("POST").uri("/datasets")
        .header("x-test-user-id", actor)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"datasetId": id, "name": "New dataset", "adminUserId": actor,
            "schemaSourceDatasetId": source}).to_string())).unwrap()).await.unwrap()
}

#[tokio::test]
async fn schema_copy_preserves_definitions_and_isolates_dataset_data() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    configure_pixel_task(&app).await;
    let source_repo = state.repo(&DatasetId::from("ds")).unwrap();
    let mut source = source_repo.load_dataset().await.unwrap();
    source.tasks[0].instructions.example_images = vec!["private/example.png".into()];
    source.tasks[0].prelabel_config_ids = vec!["source-model".into()];
    source.image_roots = vec!["source-images".into()];
    let mut skeleton = source.tasks[0].clone();
    skeleton.task_id = "pose:pixel".into();
    skeleton.annotation_type = AnnotationType::Skeleton;
    skeleton.skeleton = Some(SkeletonSpec {
        keypoints: vec![KeypointSpec { name: "tail".into(), required: false }, KeypointSpec { name: "nose".into(), required: true }],
        edges: vec![labello_domain::SkeletonEdge { from: "tail".into(), to: "nose".into() }],
        allow_hidden: true, allow_absent: true,
    });
    skeleton.manual_box_guide_migration = Some(ManualBoxGuideMigration {
        guide_task_id: source.tasks[0].task_id.clone(),
        cardinality: MigrationCardinality::ExactlyOne,
        sequence: MigrationSequence::ImportedSpatialOrderV1,
        allow_exclusion: true,
    });
    source.tasks.push(skeleton);
    source_repo.save_dataset(&source).await.unwrap();
    let response = create_with_schema(&app, "copied", Some("ds"), "admin").await;
    assert_eq!(response.status(), StatusCode::OK);
    let copied_repo = state.repo(&DatasetId::from("copied")).unwrap();
    let copied = copied_repo.load_dataset().await.unwrap();
    assert_eq!(copied.label_classes, source.label_classes);
    let mut expected_tasks = source.tasks.clone();
    for task in &mut expected_tasks { task.instructions.example_images.clear(); task.prelabel_config_ids.clear(); }
    assert_eq!(copied.tasks, expected_tasks);
    assert_eq!(copied.image_roots, ["images"]);
    assert!(copied.images.is_empty());
    assert!(copied.migration_history.is_empty());
    assert!(copied.prelabel_configs.is_empty());
    assert!(copied.imbalance.is_none());
    assert_eq!(copied.role_assignments.len(), 1);
    assert_eq!(copied.role_assignments[0].dataset_id, DatasetId::from("copied"));
    assert_eq!(source_repo.load_dataset().await.unwrap(), source);
    let mut edited = copied.clone();
    edited.label_classes[0].name = "Independent".into();
    copied_repo.save_dataset(&edited).await.unwrap();
    assert_eq!(source_repo.load_dataset().await.unwrap(), source);
    let blank = create_with_schema(&app, "blank", None, "admin").await;
    assert_eq!(blank.status(), StatusCode::OK);
    assert!(state.repo(&DatasetId::from("blank")).unwrap().load_dataset().await.unwrap().tasks.is_empty());
}

#[tokio::test]
async fn schema_copy_rejects_unavailable_unauthorized_and_invalid_sources_without_creation() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    configure_pixel_task(&app).await;
    for (destination, source, actor, expected) in [
        ("missing-copy", "missing", "admin", StatusCode::NOT_FOUND),
        ("unsafe-copy", "../ds", "admin", StatusCode::BAD_REQUEST),
        ("non-bootstrap", "ds", "reviewer_2", StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(create_with_schema(&app, destination, Some(source), actor).await.status(), expected);
        assert!(!temp.path().join(destination).exists());
    }
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let mut source = repo.load_dataset_config().await.unwrap();
    source.role_assignments.retain(|assignment| assignment.user_id != UserId::from("admin"));
    repo.save_dataset(&source).await.unwrap();
    assert_eq!(create_with_schema(&app, "denied-copy", Some("ds"), "admin").await.status(), StatusCode::UNAUTHORIZED);
    assert!(!temp.path().join("denied-copy").exists());
    source.role_assignments.push(DatasetRoleAssignment { dataset_id: "ds".into(), user_id: "admin".into(), roles: BTreeSet::from([DatasetRole::DataAdmin]), assigned_at: now(), assigned_by: None });
    source.tasks[0].class_ids = vec!["missing-class".into()];
    repo.save_dataset(&source).await.unwrap();
    assert_eq!(create_with_schema(&app, "invalid-copy", Some("ds"), "admin").await.status(), StatusCode::BAD_REQUEST);
    assert!(!temp.path().join("invalid-copy").exists());
}
