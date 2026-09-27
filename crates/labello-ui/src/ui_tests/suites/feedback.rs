#[test]
fn feedback_mandatory_overlay_preserves_work_and_ignores_escape() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    step_until(&mut harness, 20, |app| !app.loading.stats);
    harness.state_mut().runtime.api = None;
    let ctx = harness.ctx.clone();
    harness.state_mut().seed_feedback(&ctx, true);
    let annotations = harness.state().work.annotations.clone();
    let assignment = harness.state().work.assignment.clone();
    harness.run_steps(3);
    assert!(harness.query_by_label("Feedback requires your attention").is_some());
    assert!(harness.query_by_label("Close feedback").is_none());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    harness.key_press(egui::Key::Space);
    harness.run_steps(2);
    assert!(harness.state().feedback.open);
    assert_eq!(harness.state().work.annotations, annotations);
    assert_eq!(harness.state().work.assignment, assignment);
    // Dropping below five never unlocks the latched workflow.
    harness.state_mut().feedback.items.truncate(1);
    harness.run_steps(2);
    assert!(harness.query_by_label("Close feedback").is_none());
    harness.state_mut().feedback.items.clear();
    harness.run_steps(2);
    click(&mut harness, "Close feedback");
    harness.run_steps(2);
    assert!(!harness.state().feedback.open);
    assert_eq!(harness.state().work.annotations, annotations);
}

#[test]
fn feedback_inbox_is_anchored_and_viewing_controls_fit_compact_layouts() {
    for size in [egui::vec2(1440.,1000.), egui::vec2(800.,800.), egui::vec2(390.,844.), egui::vec2(640.,360.)] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        step_until(&mut harness, 20, |app| !app.loading.stats);
        harness.state_mut().runtime.api = None;
        let ctx = harness.ctx.clone();
        harness.state_mut().seed_feedback(&ctx, false);
        harness.set_size(size);
        harness.run_steps(4);
        let button = harness.get_by_label("Open feedback inbox, 5 pending").rect();
        assert!(button.top() >= 0. && button.bottom() <= 56. && button.right() <= size.x);
        assert!(harness.query_by_label("Correction feedback").is_some());
        assert_eq!(harness.state().feedback.items.len(), 5);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(3);
        harness.state_mut().seed_feedback(&ctx, true);
        harness.run_steps(4);
        let next = harness.get_by_label("Next feedback");
        next.focus();
        harness.run_steps(3);
        let next = harness.get_by_label("Next feedback").rect();
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(next), "{size:?}: {next:?}");
    }
}

#[test]
fn feedback_errors_and_stale_replies_cannot_unlock_required_work() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    step_until(&mut harness, 20, |app| !app.loading.stats);
    harness.state_mut().runtime.api = None;
    let ctx = harness.ctx.clone();
    harness.state_mut().seed_feedback(&ctx, true);
    let request = harness.state_mut().request_identity(None);
    harness.state_mut().feedback.pending = Some(request.request_id);
    let mut stale = request.clone();
    stale.request_id += 1;
    harness.state_mut().reduce_feedback(&ctx, UiMessage::Feedback {
        request: stale,
        result: Box::new(Ok(crate::feedback::FeedbackReply::Dismissed(Vec::new()))),
    });
    assert!(harness.state().feedback_required());
    harness.state_mut().reduce_feedback(&ctx, UiMessage::Feedback {
        request,
        result: Box::new(Err(labello_client::ClientError::Api { status: 503, message: "Try again".into() }.into())),
    });
    harness.run_steps(3);
    assert!(harness.state().feedback_required());
    assert!(harness.query_by_label("Retry feedback").is_some());
    assert!(harness.query_by_label("Close feedback").is_none());
    // Clearing one workflow leaves the other workflow's obligation intact.
    harness.state_mut().feedback.items[4].summary.task_id = "another-workflow".into();
    harness.state_mut().feedback.items.drain(..4);
    harness.run_steps(2);
    assert!(harness.state().feedback_required());
    assert!(harness.query_by_label("Close feedback").is_none());
}
