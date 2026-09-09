#[tokio::test]
async fn presence_requires_auth_and_lists_cross_dataset_leases_without_granting_access() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    assert_eq!(activity_response(&app, "/presence", None).await.status(), StatusCode::UNAUTHORIZED);
    let (image, task) = prepare_correction_task(&app, false, true, "presence.png").await;
    let task = TaskId::from(task);
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let assignment = repo.assign_next_image(&UserId::from("reviewer_2"), &task, labello_domain::AssignmentKind::Review).await.unwrap().unwrap();
    let other = state.repo(&DatasetId::from("other")).unwrap();
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.dataset_id = DatasetId::from("other");
    metadata.name = "Other project".into();
    for roles in &mut metadata.role_assignments {
        roles.dataset_id = metadata.dataset_id.clone();
        if roles.user_id == UserId::from("reviewer_2") { roles.roles.insert(DatasetRole::Annotator); }
    }
    other.initialize(metadata).await.unwrap();
    other.save_images_index(&repo.load_images_index().await.unwrap()).await.unwrap();
    other.assign_next_image(&UserId::from("reviewer_2"), &task, labello_domain::AssignmentKind::Annotation).await.unwrap().unwrap();
    let response = activity_response(&app, "/presence?userId=reviewer_2", Some("outsider")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    let value: labello_client::ServerPresence = serde_json::from_slice(&bytes).unwrap();
    let user = value.users.iter().find(|u| u.user_id == UserId::from("reviewer_2")).unwrap();
    assert_eq!(value.users.iter().filter(|u| u.user_id == UserId::from("reviewer_2")).count(), 1);
    assert_eq!(user.datasets.len(), 2);
    assert_eq!(user.github_login, None);
    assert_eq!(user.datasets[1].name, "Other project");
    assert_eq!(user.datasets[0].dataset_id, DatasetId::from("ds"));
    assert_eq!(user.datasets[0].name, repo.load_dataset_config().await.unwrap().name);
    assert!(!String::from_utf8_lossy(&bytes).contains(assignment.assignment_id.as_str()));
    assert!(!activity_response(&app, "/datasets/ds/stats", Some("outsider")).await.status().is_success());
    state.server_store.upsert_user(UserAccount {
        user_id: UserId::from("reviewer_2"),
        display_name: "Different display name".into(),
        github_user_id: Some("42".into()),
        github_login: Some("octocat".into()),
        created_at: now(), updated_at: now(),
    }).unwrap();
    let session = state.create_session(UserId::from("reviewer_2")).unwrap();
    let self_response = app.clone().oneshot(Request::builder().uri("/presence")
        .header(header::COOKIE, format!("labello_session={}", session.cookie))
        .body(Body::empty()).unwrap()).await.unwrap();
    let self_value: labello_client::ServerPresence = serde_json::from_slice(&to_bytes(self_response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(self_value.users.len(), 1, "the only active user is the requester");
    assert_eq!(self_value.users[0].user_id, UserId::from("reviewer_2"));
    assert_eq!(self_value.users[0].github_login.as_deref(), Some("octocat"));
    assert_eq!(self_value.users[0].presence_name(), "@octocat");
    assert_eq!(self_value.users[0].datasets, user.datasets);
    repo.release_assignment(&UserId::from("reviewer_2"), &assignment.assignment_id, &image, &task, labello_domain::AssignmentKind::Review).await.unwrap();
    let response = activity_response(&app, "/presence", Some("outsider")).await;
    let value: labello_client::ServerPresence = serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    let user = value.users.iter().find(|u| u.user_id == UserId::from("reviewer_2")).unwrap();
    assert_eq!(user.datasets.len(), 1);
    assert_eq!(user.datasets[0].dataset_id, DatasetId::from("other"));
}
