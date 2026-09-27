use super::*;

async fn call(
    app: &axum::Router,
    user: Option<&str>,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(user) = user {
        request = request.header("x-test-user-id", user);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn feedback_threshold_latches_across_restart_and_blocks_all_labeling() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let (image, task) = prepare_correction_task(&app, false, true, "synthetic.png").await;
    let threshold = format!(
        "/datasets/ds/tasks/{}/feedback-threshold",
        urlencoding::encode(&task)
    );
    assert_eq!(
        call(&app, Some("admin"), "GET", &threshold, Value::Null)
            .await
            .1["threshold"],
        5
    );
    assert_eq!(
        call(
            &app,
            Some("reviewer_2"),
            "PUT",
            &threshold,
            json!({"threshold":1})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "PUT",
            &threshold,
            json!({"threshold":0})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut events = Vec::new();
    for version in 1..=5 {
        let assignment = claim_assignment_for_task(&app, "reviewer_2", "review", &task).await;
        let body = json!({"correctionId":format!("feedback-{version}"), "annotationId":"ann_1", "expectedVersion":version,
            "geometry":{"type":"bounding_box","geometry":{"x":0.1 + version as f32 * 0.02,"y":0.2,"width":0.3,"height":0.3}}});
        let response =
            post_test_correction(&app, &image, "reviewer_2", &assignment, body.clone()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let event: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        events.push(event["eventId"].as_str().unwrap().to_owned());
        assert_eq!(
            post_test_correction(&app, &image, "reviewer_2", &assignment, body)
                .await
                .status(),
            StatusCode::OK
        );
        let (status, inbox) = call(&app, Some("admin"), "GET", "/feedback", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(inbox.as_array().unwrap().len(), version as usize);
        assert!(
            inbox
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["summary"]["mandatory"] == (version == 5))
        );
    }
    assert_eq!(
        call(&app, Some("reviewer_2"), "GET", "/feedback", Value::Null)
            .await
            .1,
        json!([])
    );
    assert_eq!(
        call(&app, None, "GET", "/feedback", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    // Every labeling ingress is gated, even if an alternate client selects another dataset.
    for path in [
        "/datasets/ds/images/next",
        "/datasets/another/images/next",
        "/datasets/ds/offline-sync",
        "/datasets/ds/assignments/complete",
        "/datasets/ds/images/img/annotation-batch",
        "/datasets/ds/images/img/events",
        "/datasets/ds/images/img/migration/confirm",
    ] {
        assert_eq!(
            call(&app, Some("admin"), "POST", path, json!({})).await.0,
            StatusCode::CONFLICT,
            "{path}"
        );
    }
    let first = &events[0];
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "POST",
            &format!("/feedback/{first}/dismiss"),
            json!({"viewed":false})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "POST",
            &format!("/feedback/{first}/dismiss"),
            json!({"viewed":true})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            Some("reviewer_2"),
            "GET",
            &format!("/feedback/{first}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, detail) = call(
        &app,
        Some("admin"),
        "GET",
        &format!("/feedback/{first}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["feedback"]["before"][0]["version"], 1);
    assert_eq!(detail["feedback"]["after"][0]["version"], 2); // not latest version 6
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "POST",
            &format!("/feedback/{first}/dismiss"),
            json!({"viewed":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    drop(app);
    drop(state);
    let app = router(ApiState::new(temp.path()));
    let inbox = call(&app, Some("admin"), "GET", "/feedback", Value::Null)
        .await
        .1;
    assert_eq!(inbox.as_array().unwrap().len(), 4);
    assert!(
        inbox
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["summary"]["mandatory"] == true)
    );
    for event in &events[1..] {
        assert_eq!(
            call(
                &app,
                Some("admin"),
                "GET",
                &format!("/feedback/{event}"),
                Value::Null
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                Some("admin"),
                "POST",
                &format!("/feedback/{event}/dismiss"),
                json!({"viewed":true})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .1,
        json!([])
    );
    assert_ne!(
        call(
            &app,
            Some("admin"),
            "POST",
            "/datasets/ds/images/next",
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn feedback_optional_dismissal_is_durable_and_missing_image_releases_gate() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let (image, task) = prepare_correction_task(&app, true, true, "synthetic.png").await;
    for version in 1..=2 {
        let assignment = claim_assignment_for_task(&app, "reviewer_2", "review", &task).await;
        let response = post_test_correction(&app, &image, "reviewer_2", &assignment, json!({"correctionId":format!("pose-feedback-{version}"), "annotationId":"ann_1", "expectedVersion":version,
          "geometry":{"type":"skeleton","geometry":{"keypoints":[{"name":"nose","state":"hidden","point":{"x":0.2 + version as f32 * 0.1,"y":0.4}}]}}})).await;
        assert_eq!(response.status(), StatusCode::OK);
        let inbox = call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .1;
        let event = inbox[0]["summary"]["eventId"].as_str().unwrap();
        if version == 1 {
            assert_eq!(
                call(
                    &app,
                    Some("admin"),
                    "POST",
                    &format!("/feedback/{event}/dismiss"),
                    json!({"viewed":false})
                )
                .await
                .0,
                StatusCode::OK
            );
        }
    }
    let threshold = format!(
        "/datasets/ds/tasks/{}/feedback-threshold",
        urlencoding::encode(&task)
    );
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "PUT",
            &threshold,
            json!({"threshold":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .1[0]["summary"]["mandatory"],
        true
    );
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let record = repo.load_image_record(&image).await.unwrap();
    tokio::fs::remove_file(repo.image_path(&record.canonical_path).unwrap())
        .await
        .unwrap();
    assert_eq!(
        call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .1,
        json!([])
    );
    drop(app);
    drop(state);
    assert_eq!(
        call(
            &router(ApiState::new(temp.path())),
            Some("admin"),
            "GET",
            "/feedback",
            Value::Null
        )
        .await
        .1,
        json!([])
    );
}

#[tokio::test]
async fn feedback_activation_excludes_history_and_revocation_clears_required_access() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let (image, task) = prepare_correction_task(&app, false, true, "synthetic.png").await;
    let assignment = claim_assignment_for_task(&app, "reviewer_2", "review", &task).await;
    let response = post_test_correction(
        &app,
        &image,
        "reviewer_2",
        &assignment,
        json!({"correctionId":"historical-correction", "annotationId":"ann_1", "expectedVersion":1,
        "geometry":{"type":"bounding_box","geometry":{"x":0.2,"y":0.2,"width":0.3,"height":0.3}}}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    drop(app);
    drop(state);
    // Model a pre-feature dataset: history exists but feedback control has never
    // been activated. Only disposable test control data is removed.
    tokio::fs::remove_file(temp.path().join(".labello-server/feedback-v1.json"))
        .await
        .unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    assert_eq!(
        call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .1,
        json!([])
    );
    let threshold = format!(
        "/datasets/ds/tasks/{}/feedback-threshold",
        urlencoding::encode(&task)
    );
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "PUT",
            &threshold,
            json!({"threshold":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    let assignment = claim_assignment_for_task(&app, "reviewer_2", "review", &task).await;
    assert_eq!(
        post_test_correction(
            &app,
            &image,
            "reviewer_2",
            &assignment,
            json!({"correctionId":"new-correction", "annotationId":"ann_1", "expectedVersion":2,
        "geometry":{"type":"bounding_box","geometry":{"x":0.3,"y":0.2,"width":0.3,"height":0.3}}})
        )
        .await
        .status(),
        StatusCode::OK
    );
    // Raising the setting before the recipient reads their inbox cannot unlatch it.
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "PUT",
            &threshold,
            json!({"threshold":100})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .1[0]["summary"]["mandatory"],
        true
    );
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata
        .role_assignments
        .retain(|role| role.user_id != UserId::from("admin"));
    repo.save_dataset(&metadata).await.unwrap();
    assert_eq!(
        call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .1,
        json!([])
    );
}

#[tokio::test]
async fn feedback_unreadable_journal_fails_closed_without_mutating_annotations() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let (image, _) = prepare_correction_task(&app, false, true, "synthetic.png").await;
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let before = repo.load_events(&image).await.unwrap();
    tokio::fs::write(
        temp.path().join(".labello-server/feedback-v1.json"),
        b"interrupted external replacement",
    )
    .await
    .unwrap();
    assert_eq!(
        call(&app, Some("admin"), "GET", "/feedback", Value::Null)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        call(
            &app,
            Some("admin"),
            "POST",
            "/datasets/ds/assignments/complete",
            json!({})
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(repo.load_events(&image).await.unwrap(), before);
}

#[tokio::test]
async fn feedback_multiple_datasets_remain_blocked_until_both_workflows_clear() {
    let temp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let (_, task) = prepare_correction_task(&app, false, true, "first.png").await;
    // Assemble two independent synthetic datasets before producing any feedback.
    let other_state = ApiState::new(other.path());
    let other_app = router(other_state.clone());
    create_dataset(&other_app).await;
    prepare_correction_task(&other_app, false, true, "second.png").await;
    let repo = other_state.repo(&DatasetId::from("ds")).unwrap();
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.dataset_id = DatasetId::from("second");
    for role in &mut metadata.role_assignments {
        role.dataset_id = metadata.dataset_id.clone();
    }
    repo.save_dataset(&metadata).await.unwrap();
    tokio::fs::rename(other.path().join("ds"), temp.path().join("second"))
        .await
        .unwrap();
    for dataset in ["ds", "second"] {
        let threshold = format!(
            "/datasets/{dataset}/tasks/{}/feedback-threshold",
            urlencoding::encode(&task)
        );
        assert_eq!(
            call(
                &app,
                Some("admin"),
                "PUT",
                &threshold,
                json!({"threshold":1})
            )
            .await
            .0,
            StatusCode::OK
        );
        let (status, assignment) = call(
            &app,
            Some("reviewer_2"),
            "POST",
            &format!("/datasets/{dataset}/images/next"),
            json!({"kind":"review", "taskId":task}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let path = format!(
            "/datasets/{dataset}/images/{}/corrections?assignmentId={}&imageId={}&taskId={}&kind=review",
            assignment["imageId"].as_str().unwrap(),
            assignment["assignmentId"].as_str().unwrap(),
            assignment["imageId"].as_str().unwrap(),
            urlencoding::encode(&task)
        );
        assert_eq!(call(&app, Some("reviewer_2"), "POST", &path, json!({"correctionId":format!("correction-{dataset}"), "annotationId":"ann_1", "expectedVersion":1, "geometry":{"type":"bounding_box","geometry":{"x":0.2,"y":0.2,"width":0.3,"height":0.3}}})).await.0, StatusCode::OK);
    }
    let (_, inbox) = call(&app, Some("admin"), "GET", "/feedback", Value::Null).await;
    assert_eq!(inbox.as_array().unwrap().len(), 2);
    assert!(
        inbox
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["summary"]["mandatory"] == true)
    );
    for (index, item) in inbox.as_array().unwrap().iter().enumerate() {
        let event = item["summary"]["eventId"].as_str().unwrap();
        assert_eq!(
            call(
                &app,
                Some("admin"),
                "GET",
                &format!("/feedback/{event}"),
                Value::Null
            )
            .await
            .0,
            StatusCode::OK
        );
        // Concurrent retries serialize with the root gate and acknowledge once.
        let path = format!("/feedback/{event}/dismiss");
        let (a, b) = tokio::join!(
            call(&app, Some("admin"), "POST", &path, json!({"viewed":true})),
            call(&app, Some("admin"), "POST", &path, json!({"viewed":true}))
        );
        assert_eq!((a.0, b.0), (StatusCode::OK, StatusCode::OK));
        let status = call(
            &app,
            Some("admin"),
            "POST",
            "/datasets/ds/images/next",
            json!({}),
        )
        .await
        .0;
        if index == 0 {
            assert_eq!(status, StatusCode::CONFLICT);
        } else {
            assert_ne!(status, StatusCode::CONFLICT);
        }
    }
}
