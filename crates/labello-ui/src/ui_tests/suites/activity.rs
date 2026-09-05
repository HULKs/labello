fn activity_value(
    app: &LabelloApp,
    at: labello_domain::Timestamp,
    submitted: u64,
) -> labello_client::CurrentUserActivity {
    labello_client::CurrentUserActivity {
        dataset_id: app.config.dataset_id.clone(),
        user_id: app.config.user_id.clone(),
        window: labello_domain::UtcActivityWindow::containing(at),
        sampled_at: at,
        counts: labello_domain::DailyActivityCounts {
            annotation_tasks_submitted: submitted,
            final_task_reviews: 2,
        },
    }
}

fn held_activity_request(app: &mut LabelloApp) -> RequestIdentity {
    app.request_activity();
    app.runtime
        .commands
        .iter()
        .find_map(|command| match command {
            UiCommand::CurrentUserActivity { request, .. } => Some(request.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn activity_refresh_coalesces_and_preserves_assignment_requests_and_values() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.run();
    let app = harness.state_mut();
    let before = app.work.assignment.clone();
    let operation = app.work.active_operation_id;
    let value = activity_value(app, labello_domain::now(), 4);
    app.datasets.activity.value = Some(value.clone());
    let request = held_activity_request(app);
    app.request_activity();
    app.runtime
        .tx
        .send(UiMessage::ActivityVisibilityRegained)
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(
        app.datasets.activity.pending_request,
        Some(request.request_id)
    );
    assert_eq!(app.datasets.activity.value, Some(value.clone()));
    assert_eq!(app.work.assignment, before);
    assert_eq!(app.work.active_operation_id, operation);
    app.runtime
        .tx
        .send(UiMessage::CurrentUserActivityLoaded {
            request: request.clone(),
            result: Err("unavailable".into()),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.datasets.activity.error.is_some());
    assert_eq!(app.datasets.activity.value, Some(value));
    app.runtime
        .commands
        .retain(|command| !matches!(command, UiCommand::CurrentUserActivity { .. }));
    app.refresh_activity_if_due(&egui::Context::default());
    assert!(app.datasets.activity.pending_request.is_none());
    app.datasets.activity.last_attempt = Some(Instant::now() - Duration::from_secs(30));
    app.refresh_activity_if_due(&egui::Context::default());
    assert!(app.datasets.activity.pending_request.is_some());
}

#[test]
fn activity_rejects_old_endpoint_user_dataset_and_day_without_clearing_work() {
    for changed in ["endpoint", "user", "dataset", "day"] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        harness.run();
        let app = harness.state_mut();
        let at: labello_domain::Timestamp = "2026-09-05T23:59:59Z".parse().unwrap();
        let value = activity_value(app, at, 4);
        app.datasets.activity.value = Some(value.clone());
        let request = held_activity_request(app);
        match changed {
            "endpoint" => app.config.api_base_url = "http://new.invalid".into(),
            "user" => app.config.user_id = UserId::from("other"),
            "dataset" => app.config.dataset_id = DatasetId::from("other"),
            "day" => {
                app.datasets.activity.server_clock =
                    Some((at, Instant::now() - Duration::from_secs(2)))
            }
            _ => unreachable!(),
        }
        app.runtime
            .tx
            .send(UiMessage::CurrentUserActivityLoaded {
                request,
                result: Ok(value),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert!(app.work.assignment.is_some());
        if changed == "day" {
            assert!(app.datasets.activity.value.is_none());
        } else if changed != "dataset" {
            assert!(app.datasets.activity.error.is_some());
        }
    }
}

#[test]
fn activity_completion_during_refresh_queues_one_followup_and_dispatch_failure_is_local() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.run();
    let app = harness.state_mut();
    let request = held_activity_request(app);
    app.activity_work_completed();
    app.activity_work_completed();
    app.runtime
        .commands
        .retain(|command| !matches!(command, UiCommand::CurrentUserActivity { .. }));
    let value = activity_value(app, labello_domain::now(), 5);
    app.runtime
        .tx
        .send(UiMessage::CurrentUserActivityLoaded {
            request,
            result: Ok(value),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    let next = app.datasets.activity.pending_request.unwrap();
    assert_eq!(
        app.runtime
            .commands
            .iter()
            .filter(|command| matches!(command, UiCommand::CurrentUserActivity { .. }))
            .count(),
        1
    );
    let request = app
        .runtime
        .commands
        .iter()
        .find(|command| command.request().request_id == next)
        .unwrap()
        .request()
        .clone();
    app.runtime
        .tx
        .send(UiMessage::RequestFailed {
            request,
            error: "dispatch failed".into(),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.datasets.activity.pending_request.is_none());
    assert!(app.datasets.activity.error.is_some());
    assert!(app.work.assignment.is_some());
    assert!(app.datasets.activity.value.is_some());
}

#[test]
fn activity_summary_keeps_complete_accessible_labels_and_primary_actions_at_required_sizes() {
    for (width, height) in [
        (320., 568.),
        (390., 844.),
        (600., 800.),
        (1288., 820.),
        (1440., 1000.),
        (320., 320.),
    ] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        harness.run();
        let app = harness.state_mut();
        app.datasets.activity.value = Some(activity_value(app, labello_domain::now(), 123456));
        app.datasets.activity.last_attempt = Some(Instant::now());
        harness.set_size(egui::vec2(width, height));
        harness.run();
        let label = harness.get_by_label("Annotation tasks submitted today in UTC: 123456. Final task reviews completed today in UTC: 2.");
        assert!(label.rect().right() <= width);
        assert!(label.rect().bottom() <= height);
        assert!(harness.get_by_label("Submit & next").rect().bottom() <= label.rect().top());
    }
}

#[test]
fn activity_loading_zero_failure_retry_and_midnight_have_distinct_accessible_states() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.run();
    let app = harness.state_mut();
    app.datasets.activity.value = None;
    app.datasets.activity.last_attempt = Some(Instant::now());
    harness.run();
    assert!(
        harness
            .query_by_label("Loading activity today in UTC.")
            .is_some()
    );
    harness.state_mut().datasets.activity.error = Some("unavailable".into());
    harness.run();
    assert!(
        harness
            .query_by_label("Activity today in UTC is unavailable. Retry activity.")
            .is_some()
    );
    assert!(harness.get_by_label("Retry").rect().height() >= 44.0);
    click(&mut harness, "Retry");
    step_until(&mut harness, 12, |app| {
        app.datasets.activity.pending_request.is_none()
    });
    assert!(harness.query_by_label("Annotation tasks submitted today in UTC: 0. Final task reviews completed today in UTC: 0.").is_some());
    let app = harness.state_mut();
    let at: labello_domain::Timestamp = "2026-09-05T23:59:59Z".parse().unwrap();
    app.datasets.activity.value = Some(activity_value(app, at, 99));
    app.datasets.activity.server_clock = Some((at, Instant::now() - Duration::from_secs(2)));
    app.refresh_activity_if_due(&egui::Context::default());
    assert!(app.datasets.activity.value.is_none());
    assert!(app.datasets.activity.pending_request.is_some());
}

#[test]
fn activity_visibility_refreshes_once_and_inactive_view_does_not_poll() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.run();
    let app = harness.state_mut();
    app.datasets.activity.last_attempt = Some(Instant::now());
    app.runtime
        .tx
        .send(UiMessage::ActivityVisibilityRegained)
        .unwrap();
    app.runtime
        .tx
        .send(UiMessage::ActivityVisibilityRegained)
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(
        app.runtime
            .commands
            .iter()
            .filter(|command| matches!(command, UiCommand::CurrentUserActivity { .. }))
            .count(),
        1
    );
    app.view = AppView::Setup;
    app.datasets.activity.pending_request = None;
    app.datasets.activity.last_attempt = None;
    app.refresh_activity_if_due(&egui::Context::default());
    assert!(app.datasets.activity.pending_request.is_none());
}

#[test]
fn activity_queue_failure_stays_local_and_releases_loading_ownership() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.run();
    let app = harness.state_mut();
    let request = held_activity_request(app);
    let command = UiCommand::CurrentUserActivity {
        request,
        dataset_id: app.config.dataset_id.clone(),
    };
    app.runtime.error = None;
    while app.runtime.commands.len() < 64 {
        app.runtime.commands.push_back(UiCommand::Stats {
            request: command.request().clone(),
            dataset_id: app.config.dataset_id.clone(),
        });
    }
    assert!(!app.queue_command(command));
    assert!(app.runtime.error.is_none());
    assert!(app.datasets.activity.pending_request.is_none());
    assert!(app.datasets.activity.error.is_some());
    assert!(app.work.assignment.is_some());
}

#[test]
fn activity_response_that_can_have_crossed_midnight_does_not_show_the_old_day() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.run();
    let app = harness.state_mut();
    let request = held_activity_request(app);
    app.datasets.activity.value = None;
    app.datasets.activity.server_clock = None;
    app.datasets.activity.last_attempt = Some(Instant::now() - Duration::from_secs(2));
    let value = activity_value(app, "2026-09-05T23:59:59Z".parse().unwrap(), 99);
    app.accept_activity(request, Ok(value));
    assert!(app.datasets.activity.value.is_none());
    assert!(app.datasets.activity.error.is_some());
}

#[cfg(feature = "inspector-presets")]
#[test]
fn activity_footer_preserves_short_migration_confirmation_with_and_without_counts() {
    for show_activity in [false, true] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 320.0))
            .build_eframe(|ctx| {
                let mut app = crate::inspector_presets::build(
                    crate::inspector_presets::InspectorPreset::MigrationFullImage,
                    &ctx.egui_ctx,
                );
                if show_activity {
                    app.runtime.api = Some(Rc::new(SpyApi::new()));
                    app.datasets.activity.identity = Some((
                        app.config.api_base_url.clone(),
                        app.config.user_id.clone(),
                        app.config.dataset_id.clone(),
                    ));
                    app.datasets.activity.value =
                        Some(activity_value(&app, labello_domain::now(), 123456));
                    app.datasets.activity.last_attempt = Some(Instant::now());
                }
                app
            });
        harness.run_steps(4);
        assert_eq!(harness.state().activity_available(), show_activity);
        let canvas = harness.get_by_label("Annotation canvas").rect();
        assert!(
            canvas.height() >= 44.0,
            "show_activity={show_activity}: {canvas:?}"
        );
        let confirm = harness.get_by_label_contains("Confirm & finish").rect();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 320.0));
        assert!(viewport.contains_rect(confirm));
        assert!(confirm.height() >= 44.0);
        if show_activity {
            let summary = harness.get_by_label("Annotation tasks submitted today in UTC: 123456. Final task reviews completed today in UTC: 2.").rect();
            assert!(viewport.contains_rect(summary));
            assert!(confirm.bottom() <= summary.top());
        }
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn activity_retry_uses_short_migration_overflow_without_obscuring_canvas() {
    for stale in [false, true] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 320.0))
            .build_eframe(|ctx| {
                let mut app = crate::inspector_presets::build(
                    crate::inspector_presets::InspectorPreset::MigrationFullImage,
                    &ctx.egui_ctx,
                );
                app.runtime.api = Some(Rc::new(SpyApi::new()));
                app.datasets.activity.identity = Some((
                    app.config.api_base_url.clone(),
                    app.config.user_id.clone(),
                    app.config.dataset_id.clone(),
                ));
                app.datasets.activity.value =
                    stale.then(|| activity_value(&app, labello_domain::now(), 123456));
                app.datasets.activity.error = Some("unavailable".into());
                app.datasets.activity.last_attempt = Some(Instant::now());
                app
            });
        harness.run_steps(4);
        let canvas = harness.get_by_label("Annotation canvas").rect();
        assert!(canvas.height() >= 44.0, "stale={stale}: {canvas:?}");
        assert!(
            harness
                .get_by_label_contains("Confirm & finish")
                .rect()
                .height()
                >= 44.0
        );
        assert!(
            harness
                .query_by_label_contains(
                    "Retry activity is available in the workspace More actions"
                )
                .is_some()
        );
        assert!(harness.query_by_label("Retry").is_none());
        harness.get_by_label("More").click();
        harness.run_steps(3);
        let retry = harness.get_by_label("Retry activity");
        assert!(retry.rect().height() >= 44.0);
        retry.focus();
        harness.run_steps(2);
        let before = harness.state().datasets.activity.last_attempt;
        let assignment = harness.state().work.assignment.clone();
        harness.key_press(egui::Key::Enter);
        harness.run_steps(3);
        assert_ne!(harness.state().datasets.activity.last_attempt, before);
        assert_eq!(harness.state().work.assignment, assignment);
    }
}
