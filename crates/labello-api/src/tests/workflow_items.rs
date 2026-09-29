#[tokio::test]
async fn background_preparation_discovers_datasets_without_an_assignment_request() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    configure_pixel_task(&app).await;
    upload_test_image(&app, "work.png", &png_bytes(20, 20)).await;
    std::fs::create_dir(temp.path().join("unrelated")).unwrap();
    state.maintain_workflow_prelabels().await.unwrap();
    let repo = state.repo(&"ds".into()).unwrap();
    let metadata = repo.load_dataset().await.unwrap();
    let image = metadata.images.keys().next().unwrap();
    let prepared = repo.load_image_state(image).await.unwrap();
    assert_eq!(prepared.workflow_preparations[&"bounding_box:pixel".into()].status, labello_domain::WorkflowPreparationStatus::Ready);
    assert!(prepared.assignments.is_empty());
    state.maintain_workflow_prelabels().await.unwrap();
    assert_eq!(repo.load_image_state(image).await.unwrap(), prepared);
}

#[tokio::test]
async fn workflow_item_api_enforces_role_display_and_exact_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(ApiState::new(temp.path()));
    create_dataset(&app).await;
    configure_pixel_task(&app).await;
    upload_test_image(&app, "work.png", &png_bytes(20, 20)).await;
    let claim = json!({ "selection": { "taskId": "bounding_box:pixel", "kind": "annotation", "variant": "overview" } });
    let (status, _) = import_json_request(&app, "POST", "/datasets/ds/work-items/claim", "reviewer_2", None, claim.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, claimed) = import_json_request(&app, "POST", "/datasets/ds/work-items/claim", "admin", None, claim).await;
    assert_eq!(status, StatusCode::OK);
    let action = json!({ "assignmentId": claimed["assignmentId"], "imageId": claimed["imageId"], "taskId": claimed["taskId"], "kind": "annotation" });
    let image = claimed["imageId"].as_str().unwrap();
    let assignment = claimed["assignmentId"].as_str().unwrap();
    let submit_uri = format!("/datasets/ds/images/{image}/annotation-batch?assignmentId={assignment}&imageId={image}&taskId=bounding_box:pixel&kind=annotation");
    let submit = json!({ "schemaVersion": 3, "payloads": [], "complete": true });
    let (status, _) = import_json_request(&app, "POST", &submit_uri, "admin", None, submit.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = import_json_request(&app, "POST", "/datasets/ds/work-items/display", "other_annotator", None, action.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, displayed) = import_json_request(&app, "POST", "/datasets/ds/work-items/display", "admin", None, action.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(displayed["workflowSeen"].get(assignment).is_some());
    let draft = json!({ "assignment": action, "edits": { "changes": [], "reason": null }, "expectedSequence": 0 });
    let (status, _) = import_json_request(&app, "POST", "/datasets/ds/work-items/draft", "other_annotator", None, draft.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, saved) = import_json_request(&app, "POST", "/datasets/ds/work-items/draft", "admin", None, draft.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, retry) = import_json_request(&app, "POST", "/datasets/ds/work-items/draft", "admin", None, draft).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, retry);

    let (status, completed) = import_json_request(&app, "POST", &submit_uri, "admin", None, submit.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["taskStates"]["bounding_box:pixel"]["status"], "submitted");
    let (status, retry) = import_json_request(&app, "POST", &submit_uri, "admin", None, submit).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry, completed);
}
