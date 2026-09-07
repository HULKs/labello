async fn activity_response(
    app: &axum::Router,
    path: &str,
    user: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::builder().uri(path);
    if let Some(user) = user {
        request = request.header("x-test-user-id", user);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn activity_counts(app: &axum::Router, user: &str) -> labello_client::CurrentUserActivity {
    // An unsupported user selector cannot override the authenticated session.
    let response =
        activity_response(app, "/datasets/ds/stats/me?userId=someone_else", Some(user)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = to_bytes(response.into_body(), 2048).await.unwrap();
    let value: labello_client::CurrentUserActivity = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value.user_id, UserId::from(user));
    assert_eq!(value.dataset_id, DatasetId::from("ds"));
    assert_eq!(
        value.window,
        labello_domain::UtcActivityWindow::containing(value.sampled_at)
    );
    value
}

#[tokio::test]
async fn current_user_activity_is_authenticated_isolated_and_counts_committed_final_work() {
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let app = router(state.clone());
    create_dataset(&app).await;
    assert_eq!(
        activity_response(&app, "/datasets/ds/stats/me", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (image, task) = prepare_correction_task(&app, false, true, "activity.png").await;
    let submitted = activity_counts(&app, "admin").await;
    assert_eq!(submitted.counts.annotation_tasks_submitted, 1);
    assert_eq!(submitted.counts.final_task_reviews, 0);
    let review = activity_counts(&app, "reviewer_2").await;
    assert_eq!(
        review.counts,
        labello_domain::DailyActivityCounts::default()
    );
    let outsider = activity_response(&app, "/datasets/ds/stats/me", Some("outsider")).await;
    assert!(!outsider.status().is_success());
    let object = post_test_review(
        &app,
        &image,
        "reviewer_2",
        "object-review",
        json!({"targetType":"annotation_version","annotation_id":"ann_1","version":1}),
        "approved",
    )
    .await;
    assert_eq!(object.status(), StatusCode::OK);
    assert_eq!(
        activity_counts(&app, "reviewer_2")
            .await
            .counts
            .final_task_reviews,
        0
    );
    let final_review = post_test_review(
        &app,
        &image,
        "reviewer_2",
        "final-review",
        json!({"targetType":"task","task_id":task}),
        "rejected",
    )
    .await;
    assert_eq!(final_review.status(), StatusCode::OK);
    assert_eq!(
        activity_counts(&app, "reviewer_2")
            .await
            .counts
            .final_task_reviews,
        1
    );
    assert_eq!(
        activity_counts(&app, "admin").await.counts,
        submitted.counts
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
    assert!(
        !activity_response(&app, "/datasets/other/stats/me", Some("reviewer_2"))
            .await
            .status()
            .is_success()
    );
}

#[tokio::test]
async fn current_user_activity_revision_commits_and_retries_remain_one_task() {
    let fixture = api_review_revision_fixture(Some("approved")).await;
    let before = activity_counts(&fixture.app, "reviewer_2").await;
    assert_eq!(before.counts.final_task_reviews, 1);
    let reopened = response_json(
        post_assignment_action(&fixture.app, "reviewer_2", "reopen", &fixture.original).await,
    )
    .await;
    assert_eq!(
        activity_counts(&fixture.app, "reviewer_2").await.counts,
        before.counts
    );
    let replacement = api_review_revision_replacement(&fixture.task_id, "rejected");
    for _ in 0..2 {
        assert_eq!(
            post_api_review_revision(&fixture.app, "reviewer_2", &reopened, replacement.clone())
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            activity_counts(&fixture.app, "reviewer_2").await.counts,
            before.counts
        );
    }
    let restarted = labello_storage::DatasetRepository::new(fixture.repository.root());
    assert_eq!(
        restarted
            .daily_activity(&UserId::from("reviewer_2"), before.window)
            .await
            .unwrap(),
        before.counts
    );
    assert_eq!(
        activity_counts(&fixture.app, "admin")
            .await
            .counts
            .annotation_tasks_submitted,
        1
    );
}

#[tokio::test]
async fn current_user_activity_counts_guided_migration_with_and_without_review() {
    for needs_review in [false, true] {
        let fixture = api_migration_fixture().await;
        if !needs_review {
            let mut metadata = fixture.repository.load_dataset_config().await.unwrap();
            let task = metadata
                .tasks
                .iter_mut()
                .find(|task| task.task_id == fixture.task_id)
                .unwrap();
            task.review.workflow = ReviewWorkflow::None;
            task.review.required_reviews = 0;
            fixture
                .repository
                .save_dataset(&metadata)
                .await
                .unwrap();
        }
        let assignment: Assignment = serde_json::from_value(
            claim_assignment_for_task(
                &fixture.app,
                "annotator",
                "annotation",
                fixture.task_id.as_str(),
            )
            .await,
        )
        .unwrap();
        let mut state = migration_state(&fixture.app, &fixture.image_id, "annotator").await;
        assert_eq!(
            activity_counts(&fixture.app, "annotator")
                .await
                .counts
                .annotation_tasks_submitted,
            0
        );
        for (index, target) in fixture.targets.iter().enumerate() {
            let request = labello_client::SaveMigrationSkeletonRequest {
                assignment_id: assignment.assignment_id.clone(),
                pass_id: None,
                target: migration_expectation(&state, &fixture.task_id, target),
                skeleton: migration_skeleton(0.2 + index as f32 * 0.2),
            };
            state = successful_migration(
                migration_request(
                    &fixture,
                    "annotator",
                    "skeleton",
                    Some(&format!("activity-save-{index}")),
                    &request,
                )
                .await,
            )
            .image_state;
        }
        assert_eq!(
            activity_counts(&fixture.app, "annotator")
                .await
                .counts
                .annotation_tasks_submitted,
            0
        );
        let target_set_hash = state.migration_target_sets[&fixture.task_id]
            .target_set_hash
            .clone();
        let state_hash = state
            .current_migration_state_hash(&fixture.task_id)
            .unwrap();
        let confirmation_hash = migration_confirmation_hash(&target_set_hash, &state_hash).unwrap();
        let confirm = labello_client::ConfirmMigrationRequest {
            assignment_id: assignment.assignment_id.clone(),
            task_id: fixture.task_id.clone(),
            target_set_hash,
            state_hash,
            confirmation_hash,
        };
        for _ in 0..2 {
            let response = successful_migration(
                migration_request(
                    &fixture,
                    "annotator",
                    "confirm",
                    Some("activity-confirm"),
                    &confirm,
                )
                .await,
            );
            assert_eq!(
                response.image_state.task_states[&fixture.task_id].status,
                if needs_review {
                    TaskStatus::Submitted
                } else {
                    TaskStatus::Completed
                }
            );
            assert_eq!(
                activity_counts(&fixture.app, "annotator")
                    .await
                    .counts
                    .annotation_tasks_submitted,
                1
            );
            assert_eq!(
                activity_counts(&fixture.app, "reviewer_1").await.counts,
                labello_domain::DailyActivityCounts::default()
            );
        }
    }
}
