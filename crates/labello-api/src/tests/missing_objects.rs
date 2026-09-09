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
async fn missing_object_creation_is_retired_in_normal_and_revision_review() {
    for completed in [false, true] {
        let fixture = api_review_revision_fixture(completed.then_some("approved")).await;
        let response = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
        assert_eq!(response.status(), StatusCode::OK);
        let assignment = response_json(response).await;
        let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
        let events = fixture.repository.load_events(&fixture.image_id).await.unwrap();
        let body = missing_object_body(&fixture, &before);
        assert_eq!(post_missing_objects(&fixture, "reviewer_2", &assignment, &body).await.status(), StatusCode::BAD_REQUEST);
        if completed {
            let mut replacement = api_review_revision_replacement(&fixture.task_id, "rejected");
            replacement["missingObjects"] = body["locations"].clone();
            assert!(post_api_review_revision(&fixture.app, "reviewer_2", &assignment, replacement).await.status().is_client_error());
        }
        assert_eq!(load_test_image_state(&fixture.app, &fixture.image_id).await, before);
        assert_eq!(fixture.repository.load_events(&fixture.image_id).await.unwrap(), events);
    }
}

// Historical event construction is deliberately separate from active HTTP commands.
async fn seed_historical_missing_objects(fixture: &ApiReviewRevisionFixture, assignment: &Value, body: &Value) {
    let mut assignment: Assignment = serde_json::from_value(assignment.clone()).unwrap();
    let submission: labello_domain::MissingObjectRejection = serde_json::from_value(body.clone()).unwrap();
    let state = fixture.repository.load_image_state(&fixture.image_id).await.unwrap();
    let timestamp = now();
    let evidence = state.missing_object_evidence_for_submission(&DatasetId::from("ds"), &assignment.assignment_id, &submission, timestamp).unwrap();
    assignment.status = labello_domain::AssignmentStatus::Completed;
    assignment.updated_at = timestamp;
    let actor = labello_domain::Actor { user_id: UserId::from("reviewer_2"), role: DatasetRole::Reviewer };
    let mut events = fixture.repository.load_events(&fixture.image_id).await.unwrap();
    let mut replay = state;
    for payload in [
        EventPayload::ReviewRecorded { review: submission.review.clone() },
        EventPayload::TaskStateChanged { task_state: labello_domain::TaskState { task_id: TaskId::from(fixture.task_id.clone()), status: TaskStatus::NeedsCorrection, outcome: None, assigned_to: None, completed_by: Some(actor.user_id.clone()), completed_at: Some(timestamp), updated_at: timestamp } },
        EventPayload::AssignmentUpdated { assignment },
        EventPayload::MissingObjectEvidenceRecorded { evidence: Box::new(evidence), submission: Box::new(submission) },
    ] {
        let event = labello_domain::EventLogEntry::new(replay.current_sequence + 1, fixture.image_id.clone(), actor.user_id.clone(), actor.role.clone(), timestamp, payload);
        replay.apply_event(&event).unwrap();
        events.push(event);
    }
    let bytes = events.iter().map(|event| serde_json::to_string(event).unwrap() + "\n").collect::<String>();
    tokio::fs::write(fixture.repository.events_path(&fixture.image_id), bytes).await.unwrap();
    fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap();
}

#[tokio::test]
async fn historical_missing_object_evidence_survives_snapshot_and_offline_wire() {
    let fixture = api_review_revision_fixture(None).await;
    let response = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    let assignment = response_json(response).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    seed_historical_missing_objects(&fixture, &assignment, &missing_object_body(&fixture, &before)).await;
    let state = fixture.repository.rebuild_image_state(&fixture.image_id).await.unwrap();
    assert_eq!(state.missing_object_history(&TaskId::from(fixture.task_id.clone())).len(), 1);
    let bundle = fixture.repository.create_offline_bundle(&UserId::from("admin"), 10, false).await.unwrap();
    let decoded: labello_domain::OfflineBundle = serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert_eq!(decoded.images[0].state, state);
    let snapshot = fixture.repository.create_snapshot().await.unwrap();
    let file = snapshot.files.iter().find(|file| file.path.ends_with("/state.json")).unwrap();
    let saved: ImageState = serde_json::from_slice(&tokio::fs::read(fixture.repository.snapshots_dir().join(&snapshot.snapshot_id).join(&file.path)).await.unwrap()).unwrap();
    assert_eq!(saved, state);
}
#[tokio::test]
async fn missing_object_evidence_wire_replay_and_raw_ingress_preserve_authoritative_transaction() {
    let fixture = api_review_revision_fixture(None).await;
    let response = post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    let assignment = response_json(response).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    let body = missing_object_body(&fixture, &before);
    seed_historical_missing_objects(&fixture, &assignment, &body).await;
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
