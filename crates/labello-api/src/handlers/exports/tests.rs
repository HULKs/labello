use super::*;
use axum::{body::to_bytes, http::Request};
use labello_domain::{
    DatasetMetadata, DatasetRoleAssignment, EventLogEntry, EventPayload, ExportClassSelection,
    ExportProfile, ExportSplit, ImageId, ImageRecord, ImagesIndex, TaskOutcome, TaskState,
    TaskStatus, UserAccount, UserId, now,
};
use std::collections::{BTreeMap, BTreeSet};
use tower::ServiceExt;

async fn fixture() -> (
    tempfile::TempDir,
    ApiState,
    Router,
    HeaderMap,
    ExportOptions,
) {
    let root = tempfile::tempdir().unwrap();
    let service = ExportService::new(root.path(), Default::default())
        .await
        .unwrap();
    let state = ApiState::new(root.path()).with_export_service(service);
    let dataset = DatasetId::from("export");
    let repository = state.repo(&dataset).unwrap();
    let mut metadata = DatasetMetadata::new(dataset.clone(), "Export", now());
    metadata.role_assignments.push(DatasetRoleAssignment {
        dataset_id: dataset,
        user_id: "admin".into(),
        roles: BTreeSet::from([DatasetRole::DataAdmin]),
        assigned_at: now(),
        assigned_by: None,
    });
    metadata.label_classes = serde_json::from_value(serde_json::json!([
        {"classId":"person","name":"Person","color":"#ffffff","description":null}
    ]))
    .unwrap();
    metadata.tasks = serde_json::from_value(serde_json::json!([
        {"taskId":"boxes","name":"Boxes","annotationType":"bounding_box","classIds":["person"],
        "instructions":{"title":"Boxes","exampleText":"","exampleImages":[]},"skeleton":null,
        "review":{"workflow":"none","requiredReviews":1,"allowReviewerCorrections":false,"agreementThreshold":null},
        "prelabelConfigIds":[],"enabled":true}
    ])).unwrap();
    repository.initialize(metadata).await.unwrap();
    let original = repository.root().join("images/original.png");
    image::RgbImage::from_pixel(20, 20, image::Rgb([30, 60, 90]))
        .save(&original)
        .unwrap();
    let bytes = std::fs::read(original).unwrap();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let image_id = ImageId::from("image");
    repository
        .save_images_index(&ImagesIndex {
            images_by_hash: BTreeMap::from([(
                hash.clone(),
                ImageRecord {
                    image_id: image_id.clone(),
                    blake3: hash,
                    canonical_path: "images/original.png".into(),
                    known_paths: vec![],
                    duplicate_paths: vec![],
                    file_name: "original.png".into(),
                    byte_size: bytes.len() as u64,
                    width: 20,
                    height: 20,
                    media_type: "image/png".into(),
                    source_memberships: None,
                },
            )]),
            ..ImagesIndex::default()
        })
        .await
        .unwrap();
    let complete = EventLogEntry::new(
        1,
        image_id.clone(),
        "author".into(),
        DatasetRole::Annotator,
        now(),
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: "boxes".into(),
                status: TaskStatus::Completed,
                outcome: Some(TaskOutcome::AnnotationCompleted),
                assigned_to: None,
                completed_by: Some("author".into()),
                completed_at: Some(now()),
                updated_at: now(),
            },
        },
    );
    std::fs::create_dir_all(repository.annotations_dir(&image_id)).unwrap();
    std::fs::write(
        repository.events_path(&image_id),
        serde_json::to_string(&complete).unwrap() + "\n",
    )
    .unwrap();
    let headers = credentials(&state, "admin");
    let options = ExportOptions {
        profile: ExportProfile::UltralyticsYoloDetectV1,
        classes: BTreeSet::from([ExportClassSelection {
            task_id: "boxes".into(),
            class_id: "person".into(),
        }]),
        fallback_split: ExportSplit::Train,
        split_choices: BTreeMap::new(),
    };
    let app = crate::router(state.clone());
    (root, state, app, headers, options)
}

fn credentials(state: &ApiState, user: &str) -> HeaderMap {
    let user_id = UserId::from(user);
    state
        .server_store
        .upsert_user(UserAccount {
            user_id: user_id.clone(),
            display_name: "Test user".into(),
            github_user_id: None,
            github_login: None,
            created_at: now(),
            updated_at: now(),
        })
        .unwrap();
    let session = state.create_session(user_id).unwrap();
    HeaderMap::from_iter([
        (
            header::COOKIE,
            format!("labello_session={}", session.cookie)
                .parse()
                .unwrap(),
        ),
        (
            axum::http::HeaderName::from_static(crate::csrf::HEADER),
            session.csrf.parse().unwrap(),
        ),
    ])
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    headers: &HeaderMap,
    body: Option<serde_json::Value>,
) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    *request.headers_mut().unwrap() = headers.clone();
    if body.is_some() {
        request = request.header(header::CONTENT_TYPE, "application/json");
    }
    app.clone()
        .oneshot(
            request
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn job(response: Response) -> labello_client::ExportJob {
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

async fn settle(app: &Router, headers: &HeaderMap, path: &str) -> labello_client::ExportJob {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let value = job(request(app, "GET", path, headers, None).await).await;
            if !value.phase.is_active() {
                return value;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn export_api_requires_current_admin_and_csrf_and_streams_verified_attachments() {
    let (_root, state, app, admin, options) = fixture().await;
    let path = "/datasets/export/exports";
    let anonymous = HeaderMap::new();
    let member = credentials(&state, "member");
    for headers in [&anonymous, &member] {
        assert_eq!(
            request(
                &app,
                "POST",
                path,
                headers,
                Some(serde_json::to_value(&options).unwrap())
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let mut no_csrf = admin.clone();
    no_csrf.remove(crate::csrf::HEADER);
    assert_eq!(
        request(
            &app,
            "POST",
            path,
            &no_csrf,
            Some(serde_json::to_value(&options).unwrap())
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let created = job(request(
        &app,
        "POST",
        path,
        &admin,
        Some(serde_json::to_value(&options).unwrap()),
    )
    .await)
    .await;
    let path = format!("{path}/{}", created.job_id);
    assert_eq!(
        settle(&app, &admin, &path).await.phase,
        labello_client::ExportPhase::Ready
    );
    for (method, suffix) in [
        ("GET", ""),
        ("POST", "/start"),
        ("POST", "/cancel"),
        ("GET", "/download"),
        ("HEAD", "/download"),
    ] {
        assert_eq!(
            request(&app, method, &format!("{path}{suffix}"), &member, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    job(request(&app, "POST", &format!("{path}/start"), &admin, None).await).await;
    let completed = settle(&app, &admin, &path).await;
    assert_eq!(completed.phase, labello_client::ExportPhase::Succeeded);
    let response = request(&app, "HEAD", &format!("{path}/download"), &admin, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/zip");
    assert!(to_bytes(response.into_body(), 1).await.unwrap().is_empty());
    let response = request(&app, "GET", &format!("{path}/download"), &admin, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment; filename=\"labello-export-")
    );
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(Some(bytes.len() as u64), completed.archive_bytes);
    assert_eq!(
        Some(blake3::hash(&bytes).to_hex().to_string()),
        completed.archive_blake3
    );
    let repo = state.repo(&DatasetId::from("export")).unwrap();
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.role_assignments.clear();
    repo.save_dataset(&metadata).await.unwrap();
    for (method, suffix) in [
        ("GET", ""),
        ("POST", "/start"),
        ("POST", "/cancel"),
        ("GET", "/download"),
        ("HEAD", "/download"),
    ] {
        assert_eq!(
            request(&app, method, &format!("{path}{suffix}"), &admin, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn export_api_validation_is_bounded_and_failures_do_not_include_source_content() {
    let (_root, state, app, admin, mut options) = fixture().await;
    options.classes.clear();
    let response = request(
        &app,
        "POST",
        "/datasets/export/exports",
        &admin,
        Some(serde_json::to_value(&options).unwrap()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body =
        String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec()).unwrap();
    assert!(body.contains("export input is invalid"));
    assert!(!body.contains("original.png"));
    let response = request(
        &app,
        "POST",
        "/datasets/export/exports",
        &admin,
        Some(serde_json::json!({"secret": "x".repeat(1024 * 1024)})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        request(
            &app,
            "GET",
            "/datasets/export/exports/not-a-job",
            &admin,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    // Disabled service advertises unavailability through the same closed client DTO.
    let disabled = crate::router(ApiState::new(state.datasets_root()));
    let response = request(
        &disabled,
        "GET",
        "/datasets/export/exports/capabilities",
        &admin,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let capabilities: labello_client::ExportCapabilities =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert!(!capabilities.available);
}
