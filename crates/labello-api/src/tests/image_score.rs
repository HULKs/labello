async fn image_score_response(
    app: &axum::Router,
    image: &ImageId,
    user: Option<&str>,
    after: u64,
    through: u64,
) -> axum::response::Response {
    activity_response(
        app,
        &format!("/datasets/ds/images/{image}/score?afterSequence={after}&throughSequence={through}&userId=admin"),
        user,
    ).await
}

async fn image_score_value(
    app: &axum::Router,
    image: &ImageId,
    user: &str,
    after: u64,
    through: u64,
) -> i64 {
    let response = image_score_response(app, image, Some(user), after, through).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = to_bytes(response.into_body(), 2048).await.unwrap();
    serde_json::from_slice::<labello_client::ImageScore>(&bytes)
        .unwrap()
        .hundredths
}

#[tokio::test]
async fn image_score_is_authenticated_and_attributes_only_the_requested_user_image_and_window() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let (image, task) = prepare_correction_task(&app, false, true, "score.png").await;
    let before = load_test_image_state(&app, &image).await["currentSequence"]
        .as_u64()
        .unwrap();
    assert_eq!(
        image_score_response(&app, &image, None, 0, before)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        !image_score_response(&app, &image, Some("outsider"), 0, before)
            .await
            .status()
            .is_success()
    );
    assert_eq!(
        image_score_value(&app, &image, "admin", 0, before).await,
        3200
    );
    assert_eq!(
        image_score_value(&app, &image, "reviewer_2", 0, before).await,
        0
    );
    assert_eq!(
        image_score_response(&app, &image, Some("admin"), before + 1, before)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        image_score_response(&app, &image, Some("admin"), 0, before + 1)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        image_score_response(&app, &ImageId::from("missing"), Some("admin"), 0, 0)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let malformed =
        format!("/datasets/ds/images/{image}/score?afterSequence=-1&throughSequence={before}");
    assert_eq!(
        activity_response(&app, &malformed, Some("admin"))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );

    let object = post_test_review(
        &app,
        &image,
        "reviewer_2",
        "scored-object",
        json!({"targetType":"annotation_version","annotation_id":"ann_1","version":1}),
        "approved",
    )
    .await;
    assert_eq!(object.status(), StatusCode::OK);
    let object_sequence = response_json(object).await["currentSequence"]
        .as_u64()
        .unwrap();
    let final_review = post_test_review(
        &app,
        &image,
        "reviewer_2",
        "score-final",
        json!({"targetType":"task","task_id":task}),
        "approved",
    )
    .await;
    assert_eq!(final_review.status(), StatusCode::OK);
    let through = response_json(final_review).await["currentSequence"]
        .as_u64()
        .unwrap();
    assert_eq!(
        image_score_value(&app, &image, "reviewer_2", before, through).await,
        2400
    );
    assert_eq!(
        image_score_value(&app, &image, "reviewer_2", object_sequence, through).await,
        0
    );
    assert_eq!(
        image_score_value(&app, &image, "reviewer_2", before, through).await,
        2400
    );
    assert_eq!(
        image_score_value(&app, &image, "admin", before, through).await,
        0
    );
    assert_eq!(
        image_score_value(&app, &image, "admin", through, through).await,
        0
    );

    let png = png_bytes(7, 3);
    let other_image = ImageId::from_blake3_hex(blake3::hash(&png).to_hex().as_ref());
    upload_test_image(&app, "unscored.png", &png).await;
    let other_sequence = load_test_image_state(&app, &other_image).await["currentSequence"]
        .as_u64()
        .unwrap();
    assert_eq!(
        image_score_value(&app, &other_image, "admin", 0, other_sequence).await,
        0
    );
    assert_eq!(
        image_score_value(&app, &other_image, "reviewer_2", 0, other_sequence).await,
        0
    );
    let restarted = router(ApiState::new(temp.path()));
    assert_eq!(
        image_score_value(&restarted, &image, "reviewer_2", before, through).await,
        2400
    );

    let other = state.repo(&DatasetId::from("other")).unwrap();
    other
        .initialize(DatasetMetadata::new(
            DatasetId::from("other"),
            "Other",
            now(),
        ))
        .await
        .unwrap();
    let path =
        format!("/datasets/other/images/{image}/score?afterSequence=0&throughSequence={through}");
    assert!(
        !activity_response(&app, &path, Some("reviewer_2"))
            .await
            .status()
            .is_success()
    );
}
