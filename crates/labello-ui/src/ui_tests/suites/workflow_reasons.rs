fn test_workflow_reason(app: &LabelloApp, text: &str) -> labello_domain::WorkflowReason {
    let assignment = app.work.assignment.as_ref().unwrap();
    labello_domain::WorkflowReason {
        image_id: assignment.image_id.clone(), event_id: labello_domain::EventId::from("reason-event"),
        event_sequence: 1, actor_user_id: app.config.user_id.clone(), timestamp: labello_domain::now(),
        action: labello_domain::WorkflowReasonAction::ReviewerCorrection,
        task_id: Some(assignment.task_id.clone()), annotation_id: None, object_group_id: None,
        text: Some(text.into()), category: None, current_round: true, current_exclusion: false, superseded: false,
    }
}

#[test]
fn workflow_reasons_preserve_each_corrected_object_and_restore_its_input() {
    let mut harness = two_object_review_revision_harness();
    harness.run_steps(3);
    let first_id = harness.state().work.correction_draft.as_ref().unwrap().annotation_id.clone();
    edit_test_review_box(harness.state_mut());
    harness.state_mut().work.correction_draft.as_mut().unwrap().reason = "first explanation".into();
    assert!(harness.state_mut().reject_review_item());
    harness.run_steps(3);
    edit_test_review_box(harness.state_mut());
    harness.state_mut().work.correction_draft.as_mut().unwrap().reason = "second explanation".into();
    assert!(harness.state_mut().reject_review_item());
    harness.run_steps(3);
    let text = harness.state().correction_reason_text();
    assert!(text.contains("first explanation"));
    assert!(text.contains("second explanation"));
    assert!(text.contains(first_id.as_str()));
    harness.state_mut().navigate_review_item(0);
    assert_eq!(harness.state().work.correction_draft.as_ref().unwrap().reason, "first explanation");
    let encoded = serde_json::to_vec(&harness.state().work.review_corrections).unwrap();
    let decoded: crate::review_corrections::ReviewCorrectionsDraft = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.object_reasons.len(), 2);
    harness.state_mut().reset_review_item();
    assert!(!harness.state().correction_reason_text().contains("first explanation"));
    assert!(harness.state().correction_reason_text().contains("second explanation"));
}

#[test]
fn workflow_reasons_reject_oversize_input_without_discarding_or_truncating_it() {
    let mut harness = two_object_review_revision_harness();
    harness.run_steps(3);
    edit_test_review_box(harness.state_mut());
    let long = "é".repeat(1001);
    harness.state_mut().work.correction_draft.as_mut().unwrap().reason = long.clone();
    assert!(!harness.state_mut().retain_review_editor());
    assert_eq!(harness.state().work.correction_draft.as_ref().unwrap().reason, long);
    assert!(harness.state().work.review_corrections.changes.is_empty());
    harness.state_mut().work.correction_draft.as_mut().unwrap().reason = "   ".into();
    assert!(harness.state_mut().retain_review_editor());
    assert!(harness.state().correction_reason_text().is_empty());
}

#[test]
fn workflow_reasons_notice_dismissal_reopen_and_scope_are_explicit() {
    let mut harness = two_object_review_revision_harness();
    let reason = test_workflow_reason(harness.state(), "Synthetic saved explanation");
    harness.state_mut().install_reason_notice(vec![reason.clone(), reason.clone()]);
    harness.run_steps(4);
    assert!(harness.query_by_label("Saved reasons (2)").is_some());
    click(&mut harness, "Dismiss saved reasons");
    harness.run_steps(8);
    assert!(harness.query_by_label("Dismiss saved reasons").is_none());
    harness.state_mut().install_reason_notice(vec![reason.clone()]);
    harness.run_steps(4);
    assert!(harness.query_by_label("Dismiss saved reasons").is_some());
    harness.state_mut().work.selected_task_id = Some(TaskId::from("another-workflow"));
    harness.run_steps(4);
    assert!(harness.query_by_label("Dismiss saved reasons").is_none());
    harness.state_mut().work.selected_task_id = reason.task_id.clone();
    let mut stale = reason.clone();
    stale.image_id = ImageId::from("another-image");
    harness.state_mut().install_reason_notice(vec![stale]);
    // Revision guidance may remain, but another image's reason is never installed.
    harness.run_steps(4);
    assert!(harness.query_by_label("Saved reasons (1)").is_none());
    harness.state_mut().install_reason_notice(vec![reason]);
    harness.state_mut().view = AppView::Annotate;
    harness.run_steps(4);
    assert!(harness.query_by_label("Dismiss saved reasons").is_none());
}

#[test]
fn workflow_reasons_notice_long_text_compact_and_keyboard_dismissal() {
    for size in [egui::vec2(1440.0,1000.0), egui::vec2(390.0,844.0), egui::vec2(320.0,320.0)] {
        let mut harness = two_object_review_revision_harness();
        let reason = test_workflow_reason(harness.state(), &"Synthetic explanation with long text. ".repeat(100));
        harness.state_mut().install_reason_notice(vec![reason]);
        harness.set_size(size);
        harness.run_steps(5);
        let dismiss = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Dismiss saved reasons");
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(dismiss.rect()), "{size:?}");
        assert!(dismiss.rect().height() >= 44.0);
        dismiss.focus();
        harness.key_press(egui::Key::Enter);
        harness.run_steps(3);
        assert!(!harness.state().reason_notice_visible());
    }
}

#[test]
fn workflow_reasons_load_with_annotation_and_review_retry_without_becoming_empty_success() {
    for review in [false, true] {
        let api = Rc::new(SpyApi::new());
        if review {
            seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.2, y: 0.2, width: 0.3, height: 0.3,
            }), true);
        }
        let mut harness = if review { loaded_review_harness(api.clone()) } else { loaded_work_harness(api.clone()) };
        let reason = test_workflow_reason(harness.state(), "Synthetic reason loaded through the assignment owner");
        api.state.borrow_mut().workflow_reasons.insert(reason.image_id.clone(), vec![reason.clone()]);
        api.state.borrow_mut().fail_next_reasons = true;
        harness.state_mut().retry_assignment_load();
        step_until(&mut harness, 16, |app| !app.loading.image);
        assert!(harness.state().runtime.error.as_ref().unwrap().contains("reason history unavailable"));
        harness.state_mut().retry_assignment_load();
        step_until(&mut harness, 16, |app| !app.loading.image && app.runtime.error.is_none());
        harness.run_steps(3);
        assert!(harness.query_by_label("Saved reasons (1)").is_some());
        click(&mut harness, "Dismiss saved reasons");
        harness.state_mut().retry_assignment_load();
        step_until(&mut harness, 16, |app| !app.loading.image);
        harness.run_steps(3);
        assert!(harness.query_by_label("Saved reasons (1)").is_some());
    }
}

#[test]
fn workflow_reasons_failed_submission_retains_the_exact_reason_for_retry() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2, y: 0.2, width: 0.3, height: 0.3,
    }), true);
    let mut harness = loaded_review_harness(api.clone());
    harness.run_steps(3);
    edit_test_review_box(harness.state_mut());
    harness.state_mut().work.correction_draft.as_mut().unwrap().reason = "Preserve on failed submission".into();
    assert!(harness.state_mut().reject_review_item());
    api.fail_next_correction();
    assert!(harness.state_mut().submit_staged_review_corrections());
    step_until(&mut harness, 16, |app| !app.loading.saving);
    let sent = api.state.borrow().last_correction.clone().unwrap();
    assert!(sent.reason.as_ref().unwrap().contains("Preserve on failed submission"));
    assert_eq!(harness.state().work.review_corrections.object_reasons.len(), 1);
    assert_eq!(harness.state().work.review_corrections.submission.as_ref().unwrap().reason, sent.reason);
    assert!(harness.state_mut().submit_staged_review_corrections());
    step_until(&mut harness, 16, |app| !app.loading.saving);
    let retry = api.state.borrow().last_correction.clone().unwrap();
    assert_eq!(retry.reason, sent.reason);
    assert_eq!(retry.correction_id, sent.correction_id);
}

#[test]
fn workflow_reasons_previous_navigation_reopens_notices_in_both_work_views() {
    for review in [false, true] {
        let api = Rc::new(SpyApi::new());
        if review { seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2, y: 0.2, width: 0.3, height: 0.3,
        }), true); }
        let mut harness = if review { loaded_review_harness(api.clone()) } else { loaded_work_harness(api.clone()) };
        let reason = test_workflow_reason(harness.state(), "Explanation on the previous image");
        let original = reason.image_id.clone();
        api.state.borrow_mut().workflow_reasons.insert(original.clone(), vec![reason.clone()]);
        harness.state_mut().install_reason_notice(vec![reason]);
        harness.run_steps(3);
        click(&mut harness, "Dismiss saved reasons");
        step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
        click(&mut harness, "Skip");
        step_until(&mut harness, 16, |app| app.work.assignment.as_ref().is_some_and(|a| a.image_id != original)
            && app.work.previous_assignment.is_some() && !app.loading.saving);
        assert!(!harness.state().reason_notice_visible());
        harness.set_size(egui::vec2(320.0,568.0));
        harness.state_mut().return_to_previous_assignment();
        step_until(&mut harness, 20, |app| app.work.assignment.as_ref().is_some_and(|a| a.image_id == original)
            && !app.loading.image);
        harness.run_steps(3);
        assert!(harness.query_by_label("Saved reasons (1)").is_some());
    }
}

#[test]
fn workflow_reasons_stale_assignment_response_cannot_replace_the_open_notice() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let reason = test_workflow_reason(harness.state(), "Current explanation");
    harness.state_mut().install_reason_notice(vec![reason.clone()]);
    let app = harness.state();
    let mut loaded = crate::live_protocol::LoadedImage {
        reasons: vec![reason], assignment: app.work.assignment.clone().unwrap(),
        queued: app.work.current.clone().unwrap(), annotations: app.work.annotations.clone(),
        state: app.work.current_state.clone().unwrap(), color_image: None,
    };
    loaded.reasons[0].text = Some("Stale response explanation".into());
    app.runtime.tx.send(UiMessage::ImageLoaded {
        request: test_request(app, u64::MAX, Some("demo")), operation_id: u64::MAX,
        assignment: Some(loaded.assignment.clone()), result: Box::new(Ok(Some(loaded))),
    }).unwrap();
    harness.run_steps(5);
    assert!(harness.query_by_label("Current explanation").is_some());
    assert!(harness.query_by_label("Stale response explanation").is_none());
}

#[test]
fn workflow_reasons_resize_keeps_the_short_footer_reachable_and_details_scrollable() {
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0,1000.0))
        .build_eframe(|ctx| crate::inspector_presets::build(crate::inspector_presets::InspectorPreset::WorkflowReasons, &ctx.egui_ctx));
    harness.run_steps(4);
    harness.set_size(egui::vec2(320.0,320.0));
    harness.run_steps(5);
    let dismiss = harness.get_by_label("Dismiss saved reasons");
    let approve = harness.get_by_label("Approve");
    assert!(dismiss.rect().bottom() <= approve.rect().top());
    assert!(harness.query_by_label("Close details").is_none());
    harness.get_by_role_and_label(egui::accesskit::Role::Button, "Saved reasons (2)").focus();
    harness.key_press(egui::Key::Enter);
    harness.run_steps(4);
    let close = harness.get_by_label("Close details");
    assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0,320.0)).contains_rect(close.rect()));
    harness.get_by_label("Earlier synthetic explanation, retained as historical context.").scroll_to_me();
    harness.run_steps(5);
    let historical = harness.get_by_label("Earlier synthetic explanation, retained as historical context.");
    assert!(historical.rect().bottom() <= 320.0);
    click(&mut harness, "Close details");
    harness.run_steps(3);
    assert!(harness.query_by_label("Close details").is_none());
}
