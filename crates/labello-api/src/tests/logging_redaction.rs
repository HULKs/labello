#[tokio::test]
async fn responses_receive_and_propagate_request_ids() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let first_id = first.headers()["x-request-id"].to_str().unwrap();
    uuid::Uuid::parse_str(first_id).unwrap();

    let second = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let second_id = second.headers()["x-request-id"].to_str().unwrap();
    uuid::Uuid::parse_str(second_id).unwrap();
    assert_ne!(first_id, second_id);

    let supplied = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("x-request-id", "test-request-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(supplied.headers()["x-request-id"], "test-request-id");
}

#[tokio::test]
async fn internal_errors_are_sanitized_and_correlated() {
    let temp = tempfile::tempdir().unwrap();
    let auth_dir = temp.path().join(".labello-server");
    std::fs::create_dir_all(&auth_dir).unwrap();
    std::fs::write(
        auth_dir.join("auth.json"),
        r#"{"private":"do-not-expose-this-sentinel""#,
    )
    .unwrap();
    let response = router(ApiState::new(temp.path()))
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(header::COOKIE, "labello_session=invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let request_id = response.headers()["x-request-id"].to_str().unwrap();
    uuid::Uuid::parse_str(request_id).unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body, json!({ "error": "internal server error" }));
    let body = body.to_string();
    assert!(!body.contains("do-not-expose-this-sentinel"));
    assert!(!body.contains(temp.path().to_string_lossy().as_ref()));
}

#[derive(Clone, Default)]
struct LogCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn captured_request(
    app: axum::Router,
    request: Request<Body>,
    filter: &str,
) -> (axum::response::Response, Vec<Value>) {
    use tracing::instrument::WithSubscriber;
    static CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _capture_guard = CAPTURE_LOCK.lock().await;
    type Capture = (tracing::Dispatch, LogCapture);
    static DEFAULT: std::sync::OnceLock<Capture> = std::sync::OnceLock::new();
    static DEBUG: std::sync::OnceLock<Capture> = std::sync::OnceLock::new();
    let slot = if filter == "labello_api=debug" {
        &DEBUG
    } else {
        &DEFAULT
    };
    let (dispatch, capture) = slot.get_or_init(|| {
        let capture = LogCapture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_current_span(true)
            .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
            .with_writer(move || writer.clone())
            .finish();
        let dispatch = tracing::Dispatch::new(subscriber);
        if filter != "labello_api=debug" {
            tracing::dispatcher::set_global_default(dispatch.clone()).unwrap();
        }
        (dispatch, capture)
    });
    // A process-wide default keeps callsite registration stable while other API
    // tests run. Only this request's correlated records belong to its capture.
    capture.0.lock().unwrap().clear();
    let response = app
        .oneshot(request)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    let logs = capture.0.lock().unwrap();
    let events = std::str::from_utf8(&logs)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| {
            event["span"]["request_id"] == response.headers()["x-request-id"].to_str().unwrap()
        })
        .collect();
    (response, events)
}

fn assert_failure_logs(
    response: &axum::response::Response,
    events: &[Value],
    expected: Option<(&str, &str)>,
    route: &str,
) {
    let completions: Vec<_> = events
        .iter()
        .filter(|event| event["fields"]["event"] == "http.request.completed")
        .collect();
    assert_eq!(completions.len(), 1, "{events:?}");
    assert_eq!(completions[0]["level"], "INFO");
    assert_eq!(
        completions[0]["fields"]["status"],
        response.status().as_u16()
    );
    let diagnostics: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                event["fields"]["event"].as_str(),
                Some("api.error" | "api.request.rejected" | "authorization.denied" | "auth.denied")
            )
        })
        .collect();
    assert_eq!(
        diagnostics.len(),
        usize::from(expected.is_some()),
        "{events:?}"
    );
    if let Some((level, category)) = expected {
        assert_eq!(diagnostics[0]["level"], level);
        assert_eq!(diagnostics[0]["fields"]["error_kind"], category);
        assert_eq!(
            diagnostics[0]["fields"]["status"],
            response.status().as_u16()
        );
    }
    for event in completions.into_iter().chain(diagnostics) {
        assert_eq!(
            event["span"]["request_id"],
            response.headers()["x-request-id"].to_str().unwrap()
        );
        assert_eq!(event["span"]["route"], route);
    }
    let logs = serde_json::to_string(events).unwrap();
    for prohibited in [
        "private-sentinel",
        "sentinel.invalid",
        "upload-name.png",
        "secret-source",
        "9876.54321",
    ] {
        assert!(!logs.contains(prohibited), "prohibited value in logs");
    }
}

#[tokio::test]
async fn default_filter_captures_one_redacted_diagnostic_for_all_failure_owners() {
    let filter = "labello_server=info,labello_api=info,labello_storage=info";
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;
    let cases = [
        (
            "POST",
            "/datasets?private-sentinel=secret-source",
            Some("admin"),
            Some("application/json"),
            r#"{"datasetId":"ds","name":"private-sentinel","adminUserId":"admin"}"#,
            StatusCode::CONFLICT,
            Some(("INFO", "storage_already_exists")),
            "/datasets",
        ),
        (
            "POST",
            "/datasets",
            Some("admin"),
            Some("application/json"),
            r#"{"datasetId":"../private-sentinel","name":"upload-name.png","adminUserId":"admin"}"#,
            StatusCode::BAD_REQUEST,
            Some(("INFO", "invalid_id")),
            "/datasets",
        ),
        (
            "POST",
            "/datasets",
            None,
            Some("application/json"),
            r#"{"private-sentinel":9876.54321,"#,
            StatusCode::BAD_REQUEST,
            Some(("INFO", "bad_request")),
            "/datasets",
        ),
        (
            "POST",
            "/datasets",
            None,
            None,
            "private-sentinel",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Some(("INFO", "unsupported_media_type")),
            "/datasets",
        ),
        (
            "GET",
            "/datasets/ds/admin?private-sentinel=secret-source",
            Some("outsider"),
            None,
            "",
            StatusCode::UNAUTHORIZED,
            Some(("WARN", "unauthorized")),
            "/datasets/{dataset_id}/admin",
        ),
        (
            "POST",
            "/imports/imp_test/files/register",
            None,
            Some("application/json"),
            "",
            StatusCode::PAYLOAD_TOO_LARGE,
            Some(("WARN", "payload_too_large")),
            "/imports/{import_id}/files/register",
        ),
        (
            "GET",
            "/private-sentinel/upload-name.png?secret-source=9876.54321",
            None,
            None,
            "",
            StatusCode::NOT_FOUND,
            Some(("INFO", "not_found")),
            "<unmatched>",
        ),
        (
            "DELETE",
            "/health",
            None,
            None,
            "",
            StatusCode::METHOD_NOT_ALLOWED,
            Some(("INFO", "method_not_allowed")),
            "/health",
        ),
        (
            "GET",
            "/me?private-sentinel=secret-source",
            None,
            None,
            "",
            StatusCode::UNAUTHORIZED,
            None,
            "/me",
        ),
        (
            "GET",
            "/health",
            None,
            None,
            "",
            StatusCode::OK,
            None,
            "/health",
        ),
    ];
    for (method, uri, user, content_type, body, status, diagnostic, route) in cases {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, "Bearer private-sentinel")
            .header("idempotency-key", "private-sentinel");
        if let Some(user) = user {
            builder = builder.header("x-test-user-id", user);
        }
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            builder = builder.header(header::CONTENT_LENGTH, 8 * 1024 * 1024 + 1);
        }
        let body = if status == StatusCode::PAYLOAD_TOO_LARGE {
            " ".repeat(8 * 1024 * 1024 + 1)
        } else {
            body.to_string()
        };
        let (response, events) =
            captured_request(app.clone(), builder.body(Body::from(body)).unwrap(), filter).await;
        assert_eq!(response.status(), status, "{method} {route}");
        assert_failure_logs(&response, &events, diagnostic, route);
        if user == Some("outsider") {
            let event = events
                .iter()
                .find(|event| event["fields"]["event"] == "api.request.rejected")
                .unwrap();
            assert_eq!(event["span"]["user_id"], "outsider");
            assert_eq!(event["span"]["dataset_id"], "ds");
        }
    }
    // CSRF middleware rejects before extractors or handlers, using the same owner.
    let (response, events) = captured_request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/datasets")
            .header(header::COOKIE, "labello_session=private-sentinel")
            .header("x-csrf-token", "private-sentinel")
            .header(header::ORIGIN, "https://sentinel.invalid")
            .body(Body::from("private-sentinel"))
            .unwrap(),
        filter,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_failure_logs(
        &response,
        &events,
        Some(("WARN", "unauthorized")),
        "/datasets",
    );
    let (response, events) = captured_request(
        app,
        Request::builder()
            .uri("/me")
            .header(header::COOKIE, "labello_session=private-sentinel")
            .body(Body::empty())
            .unwrap(),
        "labello_api=debug",
    )
    .await;
    assert_failure_logs(&response, &events, Some(("DEBUG", "unauthorized")), "/me");

    let broken = tempfile::tempdir().unwrap();
    std::fs::create_dir(broken.path().join(".labello-server")).unwrap();
    std::fs::write(
        broken.path().join(".labello-server/auth.json"),
        "private-sentinel",
    )
    .unwrap();
    let app = production_router(ApiState::new(broken.path()));
    let (response, events) = captured_request(
        app,
        Request::builder()
            .uri("/me")
            .header(header::COOKIE, "labello_session=private-sentinel")
            .body(Body::empty())
            .unwrap(),
        filter,
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_failure_logs(&response, &events, Some(("ERROR", "internal")), "/me");
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains(broken.path().to_str().unwrap())
    );
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body, json!({"error":"internal server error"}));
}

#[tokio::test]
async fn unsafe_request_ids_are_replaced_before_logging_and_extension_methods_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    for value in [
        "https://sentinel.invalid/private-sentinel",
        "",
        &"x".repeat(129),
    ] {
        let (response, events) = captured_request(
            production_router(ApiState::new(temp.path())),
            Request::builder()
                .method("private-sentinel")
                .uri("/health")
                .header("x-request-id", value)
                .body(Body::empty())
                .unwrap(),
            "labello_server=info,labello_api=info,labello_storage=info",
        )
        .await;
        uuid::Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).unwrap();
        assert_failure_logs(
            &response,
            &events,
            Some(("INFO", "method_not_allowed")),
            "/health",
        );
        assert!(
            events
                .iter()
                .all(|event| event["span"]["method"] == "<other>")
        );
    }
}

#[tokio::test]
async fn storage_failure_diagnostics_never_format_error_details() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;
    std::fs::write(
        temp.path().join("ds/labello.dataset.toml"),
        "private-sentinel = [9876.54321",
    )
    .unwrap();
    let (response, events) = captured_request(
        app,
        Request::builder()
            .uri("/datasets/ds/admin")
            .header("x-test-user-id", "admin")
            .body(Body::empty())
            .unwrap(),
        "labello_server=info,labello_api=info,labello_storage=info",
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_failure_logs(
        &response,
        &events,
        Some(("ERROR", "storage_toml_decode")),
        "/datasets/{dataset_id}/admin",
    );
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains(temp.path().to_str().unwrap())
    );
}

#[tokio::test]
async fn diagnostic_overrides_preserve_public_responses_and_dependency_errors_are_redacted() {
    use crate::error::ApiError;
    use axum::{response::IntoResponse, routing::get};
    let app = axum::Router::new()
        .route(
            "/legacy-quota",
            get(|| async {
                ApiError::ResourceLimit(Box::new(ApiError::Internal("private-sentinel".into())))
            }),
        )
        .route(
            "/quota",
            get(|| async {
                ApiError::ResourceLimit(Box::new(ApiError::Conflict("capacity unavailable".into())))
            }),
        )
        .route(
            "/owner",
            get(|| async {
                ApiError::HiddenDenial(Box::new(ApiError::NotFound("import job".into())))
            }),
        )
        .route(
            "/dependency",
            get(|| async {
                // URL-bearing builder errors exercise redaction without network access.
                let error = reqwest::Client::new()
                    .get("http://[private-sentinel")
                    .build()
                    .unwrap_err();
                ApiError::Http(error)
            }),
        )
        .route(
            "/unavailable",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE.into_response() }),
        )
        .route(
            "/rate",
            get(|| async { StatusCode::TOO_MANY_REQUESTS.into_response() }),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(tower_http::request_id::SetRequestIdLayer::x_request_id(
                    tower_http::request_id::MakeRequestUuid,
                ))
                .layer(axum::middleware::from_fn(crate::logging::observe_response))
                .layer(tower_http::request_id::PropagateRequestIdLayer::x_request_id()),
        );
    for (route, status, level, category, message) in [
        (
            "/legacy-quota",
            StatusCode::INTERNAL_SERVER_ERROR,
            "WARN",
            "resource_limit",
            Some("internal server error"),
        ),
        (
            "/quota",
            StatusCode::CONFLICT,
            "WARN",
            "resource_limit",
            Some("conflict: capacity unavailable"),
        ),
        (
            "/owner",
            StatusCode::NOT_FOUND,
            "WARN",
            "forbidden",
            Some("not found: import job"),
        ),
        (
            "/dependency",
            StatusCode::INTERNAL_SERVER_ERROR,
            "ERROR",
            "http_client",
            Some("internal server error"),
        ),
        (
            "/unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "ERROR",
            "dependency_unavailable",
            None,
        ),
        (
            "/rate",
            StatusCode::TOO_MANY_REQUESTS,
            "WARN",
            "rate_limit",
            None,
        ),
    ] {
        let (response, events) = captured_request(
            app.clone(),
            Request::builder().uri(route).body(Body::empty()).unwrap(),
            "labello_server=info,labello_api=info,labello_storage=info",
        )
        .await;
        assert_eq!(response.status(), status);
        assert_failure_logs(&response, &events, Some((level, category)), route);
        if let Some(message) = message {
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            assert_eq!(body, json!({"error":message}));
        }
    }
}

#[tokio::test]
async fn annotation_batch_resource_limit_is_visible_without_changing_public_status() {
    let temp = tempfile::tempdir().unwrap();
    let request = labello_client::AnnotationBatchRequest {
        payloads: vec![
            EventPayload::AnnotationDeleted {
                annotation_id: AnnotationId::from("private-sentinel"),
                version: 1,
                reason: Some("private-sentinel".into()),
            };
            10_001
        ],
        complete: false,
    };
    let (response, events) = captured_request(production_router(ApiState::new(temp.path())),
        Request::builder().method("POST")
            .uri("/datasets/ds/images/img/annotation-batch?assignmentId=asn&imageId=img&taskId=box&kind=annotation")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&request).unwrap())).unwrap(),
        "labello_server=info,labello_api=info,labello_storage=info").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_failure_logs(
        &response,
        &events,
        Some(("WARN", "resource_limit")),
        "/datasets/{dataset_id}/images/{image_id}/annotation-batch",
    );
}

#[tokio::test]
async fn ingestion_recovery_uses_categories_without_decoder_text_or_filenames() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;
    std::fs::create_dir_all(temp.path().join("ds/images")).unwrap();
    std::fs::write(
        temp.path().join("ds/images/private-sentinel.png"),
        "private-sentinel",
    )
    .unwrap();
    let (response, events) = captured_request(
        app,
        Request::builder()
            .method("POST")
            .uri("/datasets/ds/ingest")
            .header("x-test-user-id", "admin")
            .body(Body::empty())
            .unwrap(),
        "labello_server=info,labello_api=info,labello_storage=info",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_failure_logs(&response, &events, None, "/datasets/{dataset_id}/ingest");
    let recovery = events
        .iter()
        .find(|event| event["fields"]["event"] == "ingest.image.unreadable")
        .unwrap();
    assert_eq!(recovery["level"], "WARN");
    assert_eq!(recovery["fields"]["error_kind"], "storage_image");
    assert!(recovery["fields"].get("diagnostic").is_none());
}
