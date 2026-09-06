#[tokio::test]
async fn missing_object_rejection_is_atomic_and_exact_retry_preserves_one_evidence_set() {
    let fixture = api_review_revision_fixture(None).await;
    let reopened = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    assert_eq!(reopened.status(), StatusCode::OK);
    let assignment = response_json(reopened).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    let body = json!({
        "review": {"reviewId": "missing-review", "target": {"targetType": "task", "task_id": fixture.task_id},
            "reviewerUserId": "reviewer_2", "decision": "rejected", "timestamp": now().to_rfc3339(), "comment": null},
        "round": before["reviewRounds"][&fixture.task_id],
        "locations": [{"markerId": 1, "classId": "pixel", "position": {"x": 0.7, "y": 0.4}}]
    });
    let uri = format!("/datasets/ds/images/{}/missing-object-rejections?assignmentId={}&imageId={}&taskId={}&kind=review",
        fixture.image_id, assignment["assignmentId"].as_str().unwrap(), fixture.image_id, urlencoding::encode(&fixture.task_id));
    for _ in 0..2 {
        let response = fixture.app.clone().oneshot(Request::builder().method("POST").uri(&uri)
            .header("x-test-user-id", "reviewer_2").header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let state = response_json(response).await;
        assert_eq!(state["taskStates"][&fixture.task_id]["status"], "needs_correction");
        assert_eq!(state["missingObjectEvidence"]["missing-review"]["locations"], body["locations"]);
        assert_eq!(state["reviews"].as_array().unwrap().len(), 2);
    }
    let after = load_test_image_state(&fixture.app, &fixture.image_id).await;
    assert_eq!(after["annotations"], before["annotations"]);
    let events = fixture.repository.load_events(&fixture.image_id).await.unwrap();
    let mut replay = ImageState::new(fixture.image_id.clone());
    for event in &events { replay.apply_event(event).unwrap(); }
    assert_eq!(replay, serde_json::from_value::<ImageState>(after).unwrap());
}

fn missing_object_body(fixture: &ApiReviewRevisionFixture, state: &Value) -> Value {
    json!({
        "review": {"reviewId": "missing-final", "target": {"targetType": "task", "task_id": fixture.task_id},
            "reviewerUserId": "reviewer_2", "decision": "rejected", "timestamp": now().to_rfc3339(), "comment": null},
        "round": state["reviewRounds"][&fixture.task_id],
        "locations": [{"markerId": 1, "classId": "pixel", "position": {"x": 0.7, "y": 0.4}}]
    })
}

async fn post_missing_objects(fixture: &ApiReviewRevisionFixture, user: &str, assignment: &Value, body: &Value) -> axum::response::Response {
    let uri = format!("/datasets/ds/images/{}/missing-object-rejections?assignmentId={}&imageId={}&taskId={}&kind=review",
        fixture.image_id, assignment["assignmentId"].as_str().unwrap(), fixture.image_id, urlencoding::encode(&fixture.task_id));
    fixture.app.clone().oneshot(Request::builder().method("POST").uri(uri)
        .header("x-test-user-id", user).header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string())).unwrap()).await.unwrap()
}

#[tokio::test]
async fn missing_object_rejection_rejects_invalid_stale_foreign_and_object_phase_without_appending() {
    let fixture = api_review_revision_fixture(None).await;
    let reopened = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    let assignment = response_json(reopened).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    let original = missing_object_body(&fixture, &before);
    let before_events = fixture.repository.load_events(&fixture.image_id).await.unwrap();
    let mut cases = Vec::new();
    for point in [json!({"x": -0.01, "y": 0.5}), json!({"x": 0.5, "y": 1.01})] {
        let mut body = original.clone(); body["locations"][0]["position"] = point;
        cases.push((body, StatusCode::BAD_REQUEST));
    }
    let mut body = original.clone(); body["locations"] = json!([]); cases.push((body, StatusCode::BAD_REQUEST));
    let mut body = original.clone(); body["locations"][0]["classId"] = json!("foreign"); cases.push((body, StatusCode::BAD_REQUEST));
    let mut body = original.clone(); body["locations"][0]["markerId"] = json!(0); cases.push((body, StatusCode::BAD_REQUEST));
    let mut body = original.clone(); body["locations"] = json!([body["locations"][0].clone(), body["locations"][0].clone()]); cases.push((body, StatusCode::BAD_REQUEST));
    let mut body = original.clone(); body["locations"] = Value::Array((1..=65).map(|id| json!({"markerId":id,"classId":"pixel","position":{"x":0.5,"y":0.5}})).collect()); cases.push((body, StatusCode::BAD_REQUEST));
    let mut body = original.clone(); body["review"]["decision"] = json!("approved"); cases.push((body, StatusCode::CONFLICT));
    let mut body = original.clone(); body["review"]["target"] = json!({"targetType":"annotation_version","annotation_id":"ann_1","version":1}); cases.push((body, StatusCode::CONFLICT));
    let mut body = original.clone(); body["round"]["eventSequence"] = json!(999); cases.push((body, StatusCode::CONFLICT));
    let mut body = original.clone(); body["review"]["target"]["task_id"] = json!("foreign"); cases.push((body, StatusCode::CONFLICT));
    for (body, expected) in cases {
        let response = post_missing_objects(&fixture, "reviewer_2", &assignment, &body).await;
        assert_eq!(response.status(), expected, "{}", response_json(response).await);
        assert_eq!(load_test_image_state(&fixture.app, &fixture.image_id).await, before);
        assert_eq!(fixture.repository.load_events(&fixture.image_id).await.unwrap(), before_events);
    }
    let response = post_missing_objects(&fixture, "admin", &assignment, &original).await;
    assert!(response.status().is_client_error());
    let response = post_missing_objects(&fixture, "annotator", &assignment, &original).await;
    assert!(response.status().is_client_error());
    let mut foreign = assignment.clone(); foreign["assignmentId"] = json!("foreign");
    assert!(post_missing_objects(&fixture, "reviewer_2", &foreign, &original).await.status().is_client_error());
    let temp = tempfile::tempdir().unwrap();
    let api_state = ApiState::new(temp.path());
    let repository = api_state.repo(&DatasetId::from("ds")).unwrap().as_ref().clone();
    let app = router(api_state);
    create_dataset(&app).await;
    let (image_id, task_id) = prepare_correction_task(&app, false, false, "object-phase.png").await;
    let assignment = claim_assignment(&app, "reviewer_2", "review").await;
    let fresh = ApiReviewRevisionFixture { _temp: temp, app, repository, image_id, task_id, original: assignment.clone() };
    let before = load_test_image_state(&fresh.app, &fresh.image_id).await;
    let body = missing_object_body(&fresh, &before);
    assert_eq!(post_missing_objects(&fresh, "reviewer_2", &assignment, &body).await.status(), StatusCode::CONFLICT);
    assert_eq!(load_test_image_state(&fresh.app, &fresh.image_id).await, before);
}

#[tokio::test]
async fn missing_object_revision_keeps_history_and_supersession_clears_active_guidance() {
    let fixture = api_review_revision_fixture(Some("approved")).await;
    let response = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    let assignment = response_json(response).await;
    let mut replacement = api_review_revision_replacement(&fixture.task_id, "rejected");
    replacement["reviews"][0]["decision"] = json!("approved");
    replacement["missingObjects"] = json!([{"markerId":1,"classId":"pixel","position":{"x":0.2,"y":0.8}}]);
    let mut invalid = replacement.clone(); invalid["reviews"][1]["decision"] = json!("approved");
    assert_eq!(post_api_review_revision(&fixture.app, "reviewer_2", &assignment, invalid).await.status(), StatusCode::CONFLICT);
    for _ in 0..2 {
        let response = post_api_review_revision(&fixture.app, "reviewer_2", &assignment, replacement.clone()).await;
        assert_eq!(response.status(), StatusCode::OK, "{}", response_json(response).await);
    }
    let state = fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap();
    let task_id = TaskId::from(fixture.task_id.clone());
    assert_eq!(state.missing_object_history(&task_id).len(), 1);
    assert_eq!(state.active_missing_object_evidence(&task_id).unwrap().review_id.as_str(), "revision_final");
    assert_eq!(state.reviews.len(), 3);
    let response = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &assignment).await;
    assert_eq!(response.status(), StatusCode::OK);
    let revised = response_json(response).await;
    let mut approve = api_review_revision_replacement(&fixture.task_id, "approved");
    approve["reviews"][0]["reviewId"] = json!("second-object");
    approve["reviews"][1]["reviewId"] = json!("second-final");
    let response = post_api_review_revision(&fixture.app, "reviewer_2", &revised, approve).await;
    assert_eq!(response.status(), StatusCode::OK);
    let final_state = fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap();
    assert!(final_state.active_missing_object_evidence(&task_id).is_none());
    assert_eq!(final_state.missing_object_history(&task_id).len(), 1);
    assert_eq!(final_state.annotations, state.annotations);
    assert_eq!(final_state.missing_object_evidence, state.missing_object_evidence);
}

#[tokio::test]
async fn missing_object_guidance_survives_correction_and_snapshot_but_resubmission_starts_new_round() {
    let fixture = api_review_revision_fixture(None).await;
    let response = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    let assignment = response_json(response).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    let body = missing_object_body(&fixture, &before);
    let (first, retry) = tokio::join!(
        post_missing_objects(&fixture, "reviewer_2", &assignment, &body),
        post_missing_objects(&fixture, "reviewer_2", &assignment, &body));
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(retry.status(), StatusCode::OK);
    let rejected = fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap();
    let task = TaskId::from(fixture.task_id.clone());
    assert!(rejected.active_missing_object_evidence(&task).is_some());
    let bundle = fixture.repository.create_offline_bundle(&UserId::from("admin"), 10, false).await.unwrap();
    let roundtrip: labello_domain::OfflineBundle = serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert_eq!(roundtrip.images[0].state, rejected);
    let snapshot = fixture.repository.create_snapshot().await.unwrap();
    let file = snapshot.files.iter().find(|file| file.path.ends_with("/state.json")).unwrap();
    let saved: ImageState = serde_json::from_slice(&tokio::fs::read(fixture.repository.snapshots_dir().join(&snapshot.snapshot_id).join(&file.path)).await.unwrap()).unwrap();
    assert_eq!(saved, rejected);
    let correction = fixture.repository.assign_next_image(&UserId::from("admin"), &task, labello_domain::AssignmentKind::Annotation).await.unwrap().unwrap();
    let correcting = fixture.repository.load_image_state(&fixture.image_id).await.unwrap();
    assert!(correcting.active_missing_object_evidence(&task).is_some());
    fixture.repository.complete_assignment(&UserId::from("admin"), &correction.assignment_id, &fixture.image_id, &task, labello_domain::AssignmentKind::Annotation).await.unwrap();
    let submitted = fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap();
    assert!(submitted.active_missing_object_evidence(&task).is_none());
    assert_eq!(submitted.missing_object_history(&task).len(),1);
    assert_ne!(submitted.review_round(&task), rejected.review_round(&task));
    let later = claim_assignment(&fixture.app, "reviewer_2", "review").await;
    assert_eq!(later["imageId"], fixture.image_id.as_str());
    let response = post_test_review(&fixture.app, &fixture.image_id, "reviewer_2", "later-empty-rejection",
        json!({"targetType":"task","task_id":fixture.task_id}), "rejected").await;
    assert_eq!(response.status(), StatusCode::OK);
    let later_state = fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap();
    assert!(later_state.active_missing_object_evidence(&task).is_none());
    assert_eq!(later_state.missing_object_history(&task).len(),1);
    assert_eq!(later_state.annotations, rejected.annotations);
}

#[tokio::test]
async fn missing_object_evidence_wire_replay_and_raw_ingress_preserve_authoritative_transaction() {
    let fixture = api_review_revision_fixture(None).await;
    let response = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    let assignment = response_json(response).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    let body = missing_object_body(&fixture, &before);
    assert_eq!(post_missing_objects(&fixture,"reviewer_2",&assignment,&body).await.status(),StatusCode::OK);
    let events = fixture.repository.load_events(&fixture.image_id).await.unwrap();
    let event = events.iter().find(|event| matches!(event.payload,EventPayload::MissingObjectEvidenceRecorded {..})).unwrap();
    assert_eq!(serde_json::from_slice::<labello_domain::EventLogEntry>(&serde_json::to_vec(event).unwrap()).unwrap(),*event);
    let mut legacy = event.clone(); legacy.schema_version = labello_domain::LEGACY_SCHEMA_VERSION;
    assert!(legacy.validate_shape().is_err());
    assert!(serde_json::to_vec(&legacy).is_err());
    let mut replay = ImageState::new(fixture.image_id.clone());
    for current in events.iter().take_while(|current| current.event_id != event.event_id) { replay.apply_event(current).unwrap(); }
    let pristine = replay.clone();
    for mutation in 0..4 {
        let mut forged = event.clone();
        let EventPayload::MissingObjectEvidenceRecorded { evidence, .. } = &mut forged.payload else { unreachable!() };
        match mutation { 0 => evidence.timestamp += chrono::Duration::seconds(1),
            1 => evidence.task_id = TaskId::from("foreign"),
            2 => evidence.locations[0].position.x = -1.0,
            _ => evidence.assignment_id = labello_domain::AssignmentId::from("foreign") }
        assert!(replay.apply_event(&forged).is_err());
        assert_eq!(replay,pristine);
    }
    let persisted = fixture.repository.load_image_state(&fixture.image_id).await.unwrap();
    let payload = serde_json::to_value(&event.payload).unwrap();
    let query = format!("assignmentId={}&imageId={}&taskId={}&kind=annotation",assignment["assignmentId"].as_str().unwrap(),fixture.image_id,urlencoding::encode(&fixture.task_id));
    for (uri,body) in [
        (format!("/datasets/ds/images/{}/events?{query}",fixture.image_id),json!({"payload":payload})),
        (format!("/datasets/ds/images/{}/annotation-batch?{query}",fixture.image_id),json!({"payloads":[payload],"complete":false})),
        (format!("/datasets/ds/images/{}/admin/events",fixture.image_id),json!({"payload":payload}))] {
        let response = fixture.app.clone().oneshot(Request::builder().method("POST").uri(uri).header("x-test-user-id","admin")
            .header(header::CONTENT_TYPE,"application/json").body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::BAD_REQUEST);
        assert!(response_json(response).await["error"].as_str().unwrap().contains("dedicated"));
    }
    assert_eq!(fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap(),persisted);
    assert_eq!(fixture.repository.load_events(&fixture.image_id).await.unwrap(),events);
}
