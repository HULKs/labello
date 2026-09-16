#[tokio::test]
async fn workflow_reasons_read_persisted_history_with_dataset_authorization() {
    let fixture = api_review_revision_fixture(Some("approved")).await;
    let actor = Actor { user_id: UserId::from("reviewer_2"), role: DatasetRole::Reviewer };
    for (id, text) in [("context-1", "First synthetic explanation"), ("context-2", "Second synthetic explanation")] {
        fixture.repository.append_payload(&fixture.image_id, &actor, EventPayload::ReviewRecorded {
            review: labello_domain::ReviewRecord {
                review_id: labello_domain::ReviewId::from(id),
                target: labello_domain::ReviewTarget::Task { task_id: TaskId::from(fixture.task_id.clone()) },
                reviewer_user_id: actor.user_id.clone(), decision: ReviewDecision::Approved,
                timestamp: now(), comment: Some(text.into()),
            },
        }).await.unwrap();
    }
    let before = fixture.repository.load_events(&fixture.image_id).await.unwrap();
    for (user, expected) in [(None, StatusCode::UNAUTHORIZED), (Some("stranger"), StatusCode::UNAUTHORIZED),
        (Some("reviewer_2"), StatusCode::OK), (Some("admin"), StatusCode::OK)] {
        let mut request = Request::builder().uri(format!("/datasets/ds/images/{}/reasons", fixture.image_id));
        if let Some(user) = user { request = request.header("x-test-user-id", user); }
        let response = fixture.app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let reasons: Vec<labello_domain::WorkflowReason> = serde_json::from_value(response_json(response).await).unwrap();
            assert_eq!(reasons.len(), 2);
            assert!(reasons.iter().all(|reason| reason.review_decision == Some(ReviewDecision::Approved)));
            assert_eq!(reasons[0].text.as_deref(), Some("First synthetic explanation"));
            assert_eq!(reasons[1].text.as_deref(), Some("Second synthetic explanation"));
            assert!(reasons.iter().all(|reason| reason.image_id == fixture.image_id));
        }
    }
    assert_eq!(fixture.repository.load_events(&fixture.image_id).await.unwrap(), before);
    let missing = fixture.app.clone().oneshot(Request::builder()
        .uri("/datasets/ds/images/missing-image/reasons").header("x-test-user-id", "admin")
        .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
