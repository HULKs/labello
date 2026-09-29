async fn global_get(app: &axum::Router, path: &str, user: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder().uri(path);
    if let Some(user) = user { request = request.header("x-test-user-id", user); }
    app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap()
}

#[tokio::test]
async fn global_preferences_are_personal_durable_and_do_not_require_a_dataset() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    assert_eq!(global_get(&app, "/keybindings", None).await.status(), StatusCode::UNAUTHORIZED);
    let mut bindings = labello_domain::KeybindingSet::defaults_for(UserId::from("person"));
    bindings.pan_drag_modifier = labello_domain::PanDragModifier::Alt;
    let save = |user: &str, csrf: Option<&str>| {
        let mut request = Request::builder().method("PUT").uri("/keybindings")
            .header("x-test-user-id", user).header(header::CONTENT_TYPE, "application/json");
        if let Some(csrf) = csrf { request = request.header(crate::csrf::HEADER, csrf); }
        request.body(Body::from(serde_json::to_vec(&bindings).unwrap())).unwrap()
    };
    assert_eq!(app.clone().oneshot(save("person", Some("invalid"))).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.clone().oneshot(save("other", None)).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.clone().oneshot(save("person", None)).await.unwrap().status(), StatusCode::OK);
    let reloaded = router(ApiState::new(temp.path()));
    let response = global_get(&reloaded, "/keybindings", Some("person")).await;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let actual: labello_domain::KeybindingSet = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(actual, bindings);
    let response = global_get(&reloaded, "/keybindings", Some("other")).await;
    let other: labello_domain::KeybindingSet = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(other, labello_domain::KeybindingSet::defaults_for(UserId::from("other")));
}

#[tokio::test]
async fn legacy_shortcuts_require_explicit_choice_and_originals_survive() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let mut old = labello_domain::KeybindingSet::defaults_for(UserId::from("admin"));
    old.pan_drag_modifier = labello_domain::PanDragModifier::Alt;
    repo.save_keybindings(&old).await.unwrap();
    let mut other_metadata = repo.load_dataset_config().await.unwrap();
    other_metadata.dataset_id = DatasetId::from("second");
    other_metadata.name = "Second".into();
    for assignment in &mut other_metadata.role_assignments { assignment.dataset_id = DatasetId::from("second"); }
    let second_repo = state.repo(&DatasetId::from("second")).unwrap();
    second_repo.initialize(other_metadata).await.unwrap();
    let different = labello_domain::KeybindingSet::defaults_for(UserId::from("admin"));
    second_repo.save_keybindings(&different).await.unwrap();
    let original = tokio::fs::read(temp.path().join("ds/users/admin/keybindings.toml")).await.unwrap();
    let response = global_get(&app, "/keybindings/legacy", Some("admin")).await;
    let candidates: Vec<labello_client::LegacyKeybindings> = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].bindings, old);
    assert_eq!(candidates[1].bindings, different);
    assert!(state.preferences.load(&UserId::from("admin")).await.unwrap().is_none());
    let request = Request::builder().method("PUT").uri("/keybindings")
        .header("x-test-user-id", "admin").header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&old).unwrap())).unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::OK);
    let response = global_get(&app, "/datasets/ds/keybindings", Some("admin")).await;
    let actual: labello_domain::KeybindingSet = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(actual, old);
    assert_eq!(tokio::fs::read(temp.path().join("ds/users/admin/keybindings.toml")).await.unwrap(), original);
    let response = global_get(&app, "/keybindings/legacy", Some("admin")).await;
    let candidates: Vec<labello_client::LegacyKeybindings> = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(candidates.is_empty());
}

#[tokio::test]
async fn global_statistics_enforce_dataset_roles_and_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    assert_eq!(global_get(&app, "/stats", None).await.status(), StatusCode::UNAUTHORIZED);
    let response = global_get(&app, "/stats", Some("outsider")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let rows: Vec<labello_client::DatasetStatistics> = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(rows.is_empty());
    let response = global_get(&app, "/stats", Some("admin")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let rows: Vec<labello_client::DatasetStatistics> = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].dataset_id, DatasetId::from("ds"));
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    assert_eq!(rows[0].stats, repo.dataset_stats().await.unwrap());
    let mut metadata = repo.load_dataset_config().await.unwrap();
    metadata.role_assignments.clear();
    repo.save_dataset(&metadata).await.unwrap();
    let response = global_get(&app, "/stats", Some("admin")).await;
    let rows: Vec<labello_client::DatasetStatistics> = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(rows.is_empty(), "bootstrap administration must not grant statistics access");
    tokio::fs::write(temp.path().join("ds/labello.dataset.toml"), "malformed = [").await.unwrap();
    assert_eq!(global_get(&app, "/stats", Some("admin")).await.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
