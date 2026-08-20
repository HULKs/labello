#[tokio::test]
async fn deployment_readiness_reports_bounded_non_mutating_checks() {
    let temp = tempfile::tempdir().unwrap();
    let app = production_router(ApiState::new(temp.path()));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/deployment/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = response_json(response).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["service"], "labello");
    assert_eq!(body["persistence"], "ok");
    assert_eq!(body["authentication"], "ok");
    assert!(body["releaseTag"].is_string());
    assert!(body["sourceCommit"].is_string());
    assert_eq!(body["schemaVersion"], SCHEMA_VERSION);
    assert!(std::fs::read_dir(temp.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn deployment_readiness_fails_closed_for_missing_data_root() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    let app = production_router(ApiState::new(&missing));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/deployment/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response_json(response).await;
    assert_eq!(body["ok"], false);
    assert_eq!(body["persistence"], "failed");
    assert_eq!(body["authentication"], "ok");
    assert!(!missing.exists());
}

#[tokio::test]
async fn deployment_readiness_fails_closed_for_corrupt_authentication_state() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".labello-server")).unwrap();
    std::fs::write(temp.path().join(".labello-server/auth.json"), b"not-json").unwrap();
    let app = production_router(ApiState::new(temp.path()));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/deployment/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response_json(response).await;
    assert_eq!(body["ok"], false);
    assert_eq!(body["persistence"], "ok");
    assert_eq!(body["authentication"], "failed");
    assert!(!body.to_string().contains("not-json"));
}

#[tokio::test]
async fn deployment_readiness_detects_authentication_corruption_after_startup() {
    let temp = tempfile::tempdir().unwrap();
    let app = production_router(ApiState::new(temp.path()));
    std::fs::create_dir_all(temp.path().join(".labello-server")).unwrap();
    std::fs::write(temp.path().join(".labello-server/auth.json"), b"not-json").unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/deployment/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response_json(response).await;
    assert_eq!(body["authentication"], "failed");
    assert!(!body.to_string().contains("not-json"));
}
