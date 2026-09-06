async fn request_preview(
    app: &axum::Router,
    image_id: &ImageId,
    user: Option<&str>,
    suffix: &str,
) -> axum::response::Response {
    let mut request = Request::builder().uri(format!("/datasets/ds/images/{image_id}/{suffix}"));
    if let Some(user) = user {
        request = request.header("x-test-user-id", user);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn encoded_preview_profiles_are_authorized_on_cold_and_warm_reads() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let png = png_bytes(17, 11);
    let image_id = ImageId::from_blake3_hex(blake3::hash(&png).to_hex().as_ref());
    upload_test_image(&app, "private-upload.png", &png).await;
    let raw = request_preview(&app, &image_id, Some("admin"), "preview?max=1600").await;
    assert_eq!(raw.status(), StatusCode::OK);
    let rgba = to_bytes(raw.into_body(), 1024 * 1024).await.unwrap();
    for profile in ["standard_v1", "data_saver_v1"] {
        for _ in 0..2 {
            let preview = request_preview(
                &app,
                &image_id,
                Some("admin"),
                &format!("encoded-preview?profile={profile}"),
            )
            .await;
            assert_eq!(preview.status(), StatusCode::OK);
            assert_eq!(preview.headers()[header::CONTENT_TYPE], "image/webp");
            assert_eq!(
                preview.headers()[header::CACHE_CONTROL],
                "private, no-store"
            );
            assert_eq!(preview.headers()["x-preview-profile"], profile);
            assert_eq!(preview.headers()["x-image-width"], "17");
            assert_eq!(preview.headers()["x-original-height"], "11");
            let body = to_bytes(preview.into_body(), 16 * 1024 * 1024)
                .await
                .unwrap();
            let decoded = image::load_from_memory_with_format(&body, image::ImageFormat::WebP)
                .unwrap()
                .to_rgba8();
            assert_eq!(decoded.dimensions(), (17, 11));
            if profile == "standard_v1" {
                assert_eq!(decoded.as_raw(), rgba.as_ref());
            }
            for user in [None, Some("intruder")] {
                let denied = request_preview(
                    &app,
                    &image_id,
                    user,
                    &format!("encoded-preview?profile={profile}"),
                )
                .await;
                assert!(matches!(
                    denied.status(),
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
                ));
                let body = to_bytes(denied.into_body(), 1024 * 1024).await.unwrap();
                assert!(!String::from_utf8_lossy(&body).contains("private-upload"));
                assert!(!body.starts_with(b"RIFF"));
            }
        }
    }
    let invalid = request_preview(
        &app,
        &image_id,
        Some("admin"),
        "encoded-preview?profile=arbitrary_9999",
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let record = repo.load_image_record(&image_id).await.unwrap();
    std::fs::remove_file(repo.root().join(record.canonical_path)).unwrap();
    let missing = request_preview(&app, &image_id, Some("admin"), "encoded-preview").await;
    assert_eq!(missing.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn encoded_and_legacy_preview_apply_identical_source_limits() {
    let temp = tempfile::tempdir().unwrap();
    let cache = labello_storage::PreviewCache::new(
        temp.path().join("cache"),
        labello_storage::PreviewConfig {
            max_pixels: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let app = router(ApiState::new(temp.path().join("datasets")).with_preview_cache(cache));
    create_dataset(&app).await;
    let png = png_bytes(17, 11);
    let image_id = ImageId::from_blake3_hex(blake3::hash(&png).to_hex().as_ref());
    upload_test_image(&app, "bounded.png", &png).await;
    for suffix in ["encoded-preview", "preview?max=1600"] {
        let response = request_preview(&app, &image_id, Some("admin"), suffix).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("bounded.png"));
    }
}
