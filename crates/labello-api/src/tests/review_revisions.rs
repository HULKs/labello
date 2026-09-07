struct ApiReviewRevisionFixture {
    _temp: tempfile::TempDir,
    app: axum::Router,
    repository: labello_storage::DatasetRepository,
    image_id: ImageId,
    task_id: String,
    original: Value,
}

async fn api_review_revision_fixture(final_decision: Option<&str>) -> ApiReviewRevisionFixture {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let repository = state.repo(&DatasetId::from("ds")).unwrap().as_ref().clone();
    let app = router(state);
    create_dataset(&app).await;
    let (image_id, task_id) = prepare_correction_task(&app, false, false, "revision.png").await;
    // The author is admin. reviewer_2 has only the review role.
    let original = claim_assignment(&app, "reviewer_2", "review").await;
    assert_eq!(original["imageId"], image_id.as_str());
    let response = if let Some(decision) = final_decision {
        post_test_review(
            &app,
            &image_id,
            "reviewer_2",
            "review_revision_original",
            json!({"targetType": "task", "task_id": task_id}),
            decision,
        )
        .await
    } else {
        let object = post_test_review(
            &app,
            &image_id,
            "reviewer_2",
            "review_revision_preserved_object",
            json!({"targetType": "annotation_version", "annotation_id": "ann_1", "version": 1}),
            "approved",
        )
        .await;
        assert_eq!(object.status(), StatusCode::OK);
        post_assignment_action(&app, "reviewer_2", "release", &original).await
    };
    assert_eq!(response.status(), StatusCode::OK);
    ApiReviewRevisionFixture {
        _temp: temp,
        app,
        repository,
        image_id,
        task_id,
        original,
    }
}

fn api_review_revision_replacement(task_id: &str, decision: &str) -> Value {
    let timestamp = now().to_rfc3339();
    json!({"reviews": [
        {"reviewId": "revision_object", "target": {"targetType": "annotation_version", "annotation_id": "ann_1", "version": 1},
         "reviewerUserId": "reviewer_2", "decision": decision, "timestamp": timestamp, "comment": null},
        {"reviewId": "revision_final", "target": {"targetType": "task", "task_id": task_id},
         "reviewerUserId": "reviewer_2", "decision": decision, "timestamp": timestamp, "comment": null}
    ]})
}

async fn post_api_review_revision(
    app: &axum::Router,
    user: &str,
    assignment: &Value,
    replacement: Value,
) -> axum::response::Response {
    let query = format!(
        "assignmentId={}&imageId={}&taskId={}&kind={}",
        assignment["assignmentId"].as_str().unwrap(),
        assignment["imageId"].as_str().unwrap(),
        urlencoding::encode(assignment["taskId"].as_str().unwrap()),
        assignment["kind"].as_str().unwrap()
    );
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/datasets/ds/images/{}/review-revisions?{query}",
                    assignment["imageId"].as_str().unwrap()
                ))
                .header("x-test-user-id", user)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(replacement.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn review_reopen_http_revalidates_role_owner_and_task_and_retries_fresh_identity() {
    let fixture = api_review_revision_fixture(None).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    for user in ["other_annotator", "admin"] {
        let response =
            post_assignment_action(&fixture.app, user, "reopen", &fixture.original).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let mut wrong_task = fixture.original.clone();
    wrong_task["taskId"] = json!("bounding_box:other");
    assert_eq!(
        post_assignment_action(&fixture.app, "reviewer_2", "reopen", &wrong_task)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        load_test_image_state(&fixture.app, &fixture.image_id).await,
        before
    );

    let response =
        post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    assert_eq!(response.status(), StatusCode::OK);
    let reopened = response_json(response).await;
    assert_ne!(reopened["assignmentId"], fixture.original["assignmentId"]);
    assert_eq!(reopened["status"], "active");
    assert_eq!(reopened["kind"], "review");
    assert_eq!(reopened["assignedTo"], "reviewer_2");
    assert_eq!(reopened["imageId"], fixture.original["imageId"]);
    assert_eq!(reopened["taskId"], fixture.original["taskId"]);
    assert!(!reopened["expiresAt"].is_null());
    let opened_state = load_test_image_state(&fixture.app, &fixture.image_id).await;
    assert_eq!(opened_state["reviews"], before["reviews"]);
    assert_eq!(opened_state["taskStates"], before["taskStates"]);
    assert!(opened_state["assignments"].as_array().unwrap().iter().any(
        |assignment| assignment["assignmentId"] == fixture.original["assignmentId"]
            && assignment["status"] == "cancelled"
    ));
    let retry =
        post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    assert_eq!(retry.status(), StatusCode::OK);
    assert_eq!(response_json(retry).await, reopened);
    assert_eq!(
        load_test_image_state(&fixture.app, &fixture.image_id).await,
        opened_state
    );
}

#[tokio::test]
async fn completed_review_http_stages_without_retraction_and_commits_both_reversals_idempotently() {
    for (initial, replacement_decision, expected_status) in [
        ("approved", "rejected", "needs_correction"),
        ("rejected", "approved", "completed"),
    ] {
        let fixture = api_review_revision_fixture(Some(initial)).await;
        let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
        let response =
            post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
        assert_eq!(response.status(), StatusCode::OK);
        let reopened = response_json(response).await;
        let opened = load_test_image_state(&fixture.app, &fixture.image_id).await;
        assert_eq!(opened["taskStates"], before["taskStates"]);
        assert_eq!(opened["reviews"], before["reviews"]);
        assert_eq!(opened["annotations"], before["annotations"]);
        assert_ne!(reopened["assignmentId"], fixture.original["assignmentId"]);

        let replacement = api_review_revision_replacement(&fixture.task_id, replacement_decision);
        let committed =
            post_api_review_revision(&fixture.app, "reviewer_2", &reopened, replacement.clone())
                .await;
        assert_eq!(committed.status(), StatusCode::OK);
        let committed = response_json(committed).await;
        assert_eq!(
            committed["taskStates"][&fixture.task_id]["status"],
            expected_status
        );
        assert_eq!(committed["annotations"], before["annotations"]);
        assert_eq!(
            committed["reviews"].as_array().unwrap().len(),
            before["reviews"].as_array().unwrap().len() + 2
        );
        assert!(
            committed["reviews"]
                .as_array()
                .unwrap()
                .contains(&before["reviews"][0])
        );
        assert!(committed["assignments"].as_array().unwrap().iter().any(
            |assignment| assignment["assignmentId"] == reopened["assignmentId"]
                && assignment["status"] == "completed"
        ));

        let persisted: ImageState = serde_json::from_value(committed.clone()).unwrap();
        let target = labello_domain::ReviewTarget::Task {
            task_id: TaskId::from(fixture.task_id.clone()),
        };
        let effective = persisted
            .effective_review_for_target(
                &TaskId::from(fixture.task_id.clone()),
                &target,
                &UserId::from("reviewer_2"),
            )
            .unwrap();
        assert_eq!(
            effective.review_id,
            labello_domain::ReviewId::from("revision_final")
        );
        assert!(
            persisted
                .superseded_review_ids
                .contains(&labello_domain::ReviewId::from("review_revision_original"))
        );
        let events = fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event.payload,
                    EventPayload::ReviewRevisionCommitted { .. }
                ))
                .count(),
            1
        );

        let retry =
            post_api_review_revision(&fixture.app, "reviewer_2", &reopened, replacement.clone())
                .await;
        assert_eq!(retry.status(), StatusCode::OK);
        assert_eq!(response_json(retry).await, committed);
        assert_eq!(
            fixture
                .repository
                .load_events(&fixture.image_id)
                .await
                .unwrap()
                .len(),
            events.len()
        );
        let mut changed_retry = replacement;
        changed_retry["reviews"][0]["comment"] = json!("different retry");
        assert_eq!(
            post_api_review_revision(&fixture.app, "reviewer_2", &reopened, changed_retry)
                .await
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            load_test_image_state(&fixture.app, &fixture.image_id).await,
            committed
        );
    }
}

#[tokio::test]
async fn review_revision_http_rejects_spoofed_roles_context_and_invalid_batches_atomically() {
    let fixture = api_review_revision_fixture(Some("approved")).await;
    let response =
        post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    assert_eq!(response.status(), StatusCode::OK);
    let reopened = response_json(response).await;
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    let replacement = api_review_revision_replacement(&fixture.task_id, "approved");
    // admin has the role but does not own the lease; other_annotator lacks the role.
    for user in ["admin", "other_annotator"] {
        let mut forged = replacement.clone();
        for review in forged["reviews"].as_array_mut().unwrap() {
            review["reviewerUserId"] = json!(user);
        }
        assert_eq!(
            post_api_review_revision(&fixture.app, user, &reopened, forged)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let mut spoofed_reviewer = replacement.clone();
    spoofed_reviewer["reviews"][0]["reviewerUserId"] = json!("admin");
    assert_eq!(
        post_api_review_revision(&fixture.app, "reviewer_2", &reopened, spoofed_reviewer)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut wrong_kind = reopened.clone();
    wrong_kind["kind"] = json!("annotation");
    assert_eq!(
        post_api_review_revision(&fixture.app, "reviewer_2", &wrong_kind, replacement.clone())
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut wrong_task = reopened.clone();
    wrong_task["taskId"] = json!("bounding_box:other");
    assert_eq!(
        post_api_review_revision(&fixture.app, "reviewer_2", &wrong_task, replacement.clone())
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post_api_review_revision(
            &fixture.app,
            "reviewer_2",
            &reopened,
            json!({"reviews": []})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let mut stale = replacement.clone();
    stale["reviews"][0]["target"]["version"] = json!(99);
    let mut foreign = replacement.clone();
    foreign["reviews"][0]["target"] =
        json!({"targetType": "task", "task_id": "bounding_box:other"});
    for invalid in [stale, foreign] {
        assert_eq!(
            post_api_review_revision(&fixture.app, "reviewer_2", &reopened, invalid)
                .await
                .status(),
            StatusCode::CONFLICT
        );
    }
    let mut duplicate = replacement;
    let mut repeated = duplicate["reviews"][0].clone();
    repeated["reviewId"] = json!("duplicate_target_with_distinct_id");
    duplicate["reviews"]
        .as_array_mut()
        .unwrap()
        .insert(1, repeated);
    assert_eq!(
        post_api_review_revision(&fixture.app, "reviewer_2", &reopened, duplicate)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        load_test_image_state(&fixture.app, &fixture.image_id).await,
        before
    );
}

#[tokio::test]
async fn server_owned_review_events_are_rejected_by_raw_http_ingresses() {
    let fixture = api_review_revision_fixture(Some("approved")).await;
    let response =
        post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await;
    assert_eq!(response.status(), StatusCode::OK);
    let reopened = response_json(response).await;
    let committed = post_api_review_revision(
        &fixture.app,
        "reviewer_2",
        &reopened,
        api_review_revision_replacement(&fixture.task_id, "rejected"),
    )
    .await;
    assert_eq!(committed.status(), StatusCode::OK);
    let before = load_test_image_state(&fixture.app, &fixture.image_id).await;
    let events = fixture
        .repository
        .load_events(&fixture.image_id)
        .await
        .unwrap();
    let query = format!(
        "assignmentId={}&imageId={}&taskId={}&kind=annotation",
        reopened["assignmentId"].as_str().unwrap(),
        fixture.image_id,
        urlencoding::encode(&fixture.task_id)
    );
    let mut kinds = BTreeSet::new();
    for event in events.iter().filter(|event| {
        matches!(
            event.payload,
            EventPayload::ReviewAssignmentOpened { .. }
                | EventPayload::ReviewAssignmentFinished { .. }
                | EventPayload::ReviewRevisionCommitted { .. }
        )
    }) {
        let payload = serde_json::to_value(&event.payload).unwrap();
        if !kinds.insert(payload["kind"].as_str().unwrap().to_owned()) {
            continue;
        }
        for (uri, body) in [
            (
                format!("/datasets/ds/images/{}/events?{query}", fixture.image_id),
                json!({"payload": payload}),
            ),
            (
                format!(
                    "/datasets/ds/images/{}/annotation-batch?{query}",
                    fixture.image_id
                ),
                json!({"payloads": [payload], "complete": false}),
            ),
            (
                format!("/datasets/ds/images/{}/admin/events", fixture.image_id),
                json!({"payload": payload}),
            ),
        ] {
            let response = fixture
                .app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(uri)
                        .header("x-test-user-id", "admin")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = response_json(response).await;
            assert!(body["error"].as_str().unwrap().contains("dedicated"));
        }
    }
    assert_eq!(kinds.len(), 3);
    assert_eq!(
        load_test_image_state(&fixture.app, &fixture.image_id).await,
        before
    );
    assert_eq!(
        fixture
            .repository
            .load_events(&fixture.image_id)
            .await
            .unwrap()
            .len(),
        events.len()
    );
}
