#[cfg(feature = "inspector-presets")]
fn migration_workflow_review(excluded: bool) -> LabelloApp {
    use labello_domain::{WorkflowAssignmentContext, WorkflowItem};
    let mut app = crate::inspector_presets::build(
        crate::inspector_presets::InspectorPreset::MigrationReview,
        &egui::Context::default(),
    );
    let task = app.selected_task().unwrap().clone();
    let assignment = app.work.assignment.clone().unwrap();
    let state = app.work.current_state.as_mut().unwrap();
    let target = state.review_object_targets(&task).unwrap()[usize::from(excluded)].clone();
    state.review_assignment_contexts.clear();
    state.workflow_assignments.insert(
        assignment.assignment_id.clone(),
        WorkflowAssignmentContext {
            item: WorkflowItem::from_review_target(&target),
            task_fingerprint: labello_domain::workflow_task_fingerprint(&task),
            overview_fingerprint: None,
            review_target: Some(target),
            review_exception: false,
            source_assignment_id: None,
        },
    );
    app.install_workflow_item();
    app.work.inspector_panel_collapsed = true;
    app.work.workflow_panel_collapsed = true;
    app
}

#[cfg(feature = "inspector-presets")]
#[test]
fn excluded_workflow_review_submits_a_skeleton_without_legacy_assignment_context() {
    let mut app = migration_workflow_review(true);
    app.begin_new_review_object(Some(("skeleton-right".into(), "group-right".into())));
    let count = app
        .selected_task()
        .unwrap()
        .skeleton
        .as_ref()
        .unwrap()
        .keypoints
        .len();
    for index in 0..count {
        app.place_review_correction_keypoint(NormalizedPoint {
            x: 0.65,
            y: 0.3 + index as f32 * 0.1,
        });
    }
    assert!(app.review_can_reject());
    let task = app.selected_task().unwrap().clone();
    let state = app.work.current_state.as_ref().unwrap();
    let expected_round = state.review_round(&task.task_id).unwrap().clone();
    let expected_fingerprint = state.review_target_fingerprint(&task);
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    assert!(
        app.reject_review_item(),
        "valid correction must queue a request: {:?}",
        app.runtime.error
    );
    let submitted = app.work.review_corrections.submission.as_ref().unwrap();
    assert_eq!(submitted.round, expected_round);
    assert_eq!(submitted.target_fingerprint, expected_fingerprint);
    assert!(matches!(
        &submitted.changes[..],
        [labello_domain::ReviewCorrectionChange::MigrationObject {
            replacement: labello_domain::MigrationReviewCorrection::Skeleton { .. },
            ..
        }]
    ));
    assert!(
        app.runtime
            .commands
            .iter()
            .any(|command| matches!(command, UiCommand::Correction { .. }))
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn excluded_workflow_review_creation_is_in_bottom_bar_at_all_widths() {
    for (width, height) in [
        (320.0, 320.0),
        (320.0, 568.0),
        (390.0, 844.0),
        (600.0, 800.0),
        (844.0, 390.0),
        (1288.0, 820.0),
        (1440.0, 1000.0),
    ] {
        let app = migration_workflow_review(true);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, height))
            .build_eframe(|_| app);
        harness.run();
        let button = harness.get_by_role_and_label(
            egui::accesskit::Role::Button,
            "Create skeleton for excluded object",
        );
        let rect = button.rect();
        assert!(rect.width() >= 44.0 && rect.height() >= 44.0);
        assert!(rect.left() >= 0.0 && rect.right() <= width && rect.bottom() <= height);
        assert!(rect.top() > height / 2.0);
        for other in harness.query_all_by_role(egui::accesskit::Role::Button) {
            if other.rect() != rect {
                assert!(
                    !other.rect().intersects(rect),
                    "creation overlaps another button"
                );
            }
        }
        button.focus();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(harness.state().work.correction_draft.is_some());
        assert!(
            harness
                .query_by_role_and_label(
                    egui::accesskit::Role::Button,
                    "Create skeleton for excluded object"
                )
                .is_none()
        );
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_workflow_review_keeps_the_source_box_read_only_and_uses_it_for_focus() {
    let app = migration_workflow_review(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|_| app);
    harness.run();
    let app = harness.state();
    assert!(
        !app.manual_migration_active(),
        "annotation queue items use the shared review canvas"
    );
    let guide = app.review_source_box().unwrap();
    assert_eq!(
        guide.annotation_id,
        labello_domain::AnnotationId::from("guide-left")
    );
    assert_eq!(app.refocus_annotation().unwrap(), guide);
    assert!(
        app.work
            .annotations
            .iter()
            .all(|a| a.annotation_id != guide.annotation_id)
    );
    assert!(app.work.correction_draft.as_ref().is_some_and(|d| d.annotation_id == labello_domain::AnnotationId::from("skeleton-left")));
    assert!(app.excluded_review_creation_target().is_none());
    assert!(app.work.canvas.current_zoom() > 1.0);
    let app = harness.state_mut();
    app.work
        .current_state
        .as_mut()
        .unwrap()
        .annotations
        .get_mut(&guide.annotation_id)
        .unwrap()
        .last_mut()
        .unwrap()
        .deleted = true;
    assert!(app.review_source_box().is_none());
    assert_eq!(
        app.refocus_annotation().unwrap().annotation_id,
        labello_domain::AnnotationId::from("skeleton-left")
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn excluded_workflow_review_single_keypoint_and_missing_context_keep_corrections() {
    let mut app = migration_workflow_review(true);
    let task_id = app.work.selected_task_id.clone().unwrap();
    let task = app
        .work
        .tasks
        .iter_mut()
        .find(|t| t.task_id == task_id)
        .unwrap();
    let spec = task.skeleton.as_mut().unwrap();
    spec.keypoints.truncate(1);
    spec.edges.clear();
    app.begin_new_review_object(Some(app.excluded_review_creation_target().unwrap()));
    assert!(!app.retain_review_editor());
    assert!(app.runtime.error.is_some());
    assert!(app.work.correction_draft.is_some());
    app.place_review_correction_keypoint(NormalizedPoint { x: 0.65, y: 0.4 });
    assert!(app.review_can_reject());
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    let round = app
        .work
        .current_state
        .as_mut()
        .unwrap()
        .review_rounds
        .remove(&task_id)
        .unwrap();
    assert!(!app.reject_review_item());
    assert!(
        app.runtime
            .error
            .as_ref()
            .unwrap()
            .contains("Review context is unavailable")
    );
    assert_eq!(app.work.review_corrections.changes.len(), 1);
    app.work
        .current_state
        .as_mut()
        .unwrap()
        .review_rounds
        .insert(task_id, round);
    assert!(app.reject_review_item());
    let submitted = app.work.review_corrections.submission.clone().unwrap();
    app.runtime.commands.clear();
    app.runtime.active_requests.clear();
    app.work.active_operation_id = None;
    app.loading.saving = false;
    assert!(app.submit_staged_review_corrections());
    assert_eq!(
        app.work.review_corrections.submission.as_ref(),
        Some(&submitted)
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn excluded_workflow_review_can_still_submit_a_changed_exclusion_reason() {
    let mut app = migration_workflow_review(true);
    app.work.inspector_panel_collapsed = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|_| app);
    harness.run();
    click(&mut harness, "Set exclusion");
    harness.run();
    click(&mut harness, "Insufficient visible features");
    harness.run();
    let app = harness.state_mut();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    assert!(
        app.reject_review_item(),
        "reason correction: changed={}, valid={}, staged={}, error={:?}",
        app.focused_review_changed(),
        app.review_editor_valid(),
        app.work.review_corrections.changes.len(),
        app.runtime.error
    );
    assert!(matches!(
        &app.work
            .review_corrections
            .submission
            .as_ref()
            .unwrap()
            .changes[..],
        [labello_domain::ReviewCorrectionChange::MigrationObject {
            replacement: labello_domain::MigrationReviewCorrection::Exclude {
                reason: labello_domain::MigrationExclusionReason::InsufficientVisibleFeatures,
                ..
            },
            ..
        }]
    ));
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_overview_approval_uses_the_leased_workflow_target() {
    let mut app = migration_workflow_review(false);
    let assignment = app.work.assignment.clone().unwrap();
    let task = app.selected_task().unwrap().clone();
    let state = app.work.current_state.as_mut().unwrap();
    let target = state.review_targets(&task).unwrap().last().unwrap().clone();
    let context = state.workflow_assignments.get_mut(&assignment.assignment_id).unwrap();
    context.item = labello_domain::WorkflowItem::Overview;
    context.review_target = Some(target.clone());
    app.discard_correction();
    app.install_workflow_item();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.runtime.commands.clear();
    assert!(app.review_overview());
    assert!(app.review_can_approve());
    app.work.migration.busy = true;
    app.trigger_migration_review_action(labello_domain::ReviewDecision::Approved);
    assert!(app.runtime.commands.is_empty());
    app.work.migration.busy = false;
    app.confirm_review_item();
    assert!(app.runtime.commands.iter().any(|command| matches!(command,
        UiCommand::Review { review, phase: crate::app::ReviewPhase::FullImage, .. }
            if review.target == target)), "Overview must use the captured target and the item review transaction");
    assert!(!app.runtime.commands.iter().any(|command| matches!(command, UiCommand::Migration { .. })));
}
