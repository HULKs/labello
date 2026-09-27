fn completion_key(harness: &mut Harness<'static, LabelloApp>, key: egui::Key, pressed: bool) {
    harness.event(egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false, // egui derives repeat from the held-key state.
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(3);
}

#[test]
fn completion_input_review_hold_requires_release_between_items() {
    for key in [egui::Key::Space, egui::Key::Enter, egui::Key::Y] {
        let mut harness = two_object_review_revision_harness();
        if key == egui::Key::Enter {
            harness.state_mut().work.keybindings.bindings.insert(
                labello_domain::UserAction::NextImage,
                labello_domain::KeyChord::new("Enter"),
            );
        }
        harness.run_steps(3);
        completion_key(&mut harness, key, true);
        assert_eq!(harness.state().review_position(), 1);
        for _ in 0..4 {
            completion_key(&mut harness, key, true);
            assert_eq!(harness.state().review_position(), 1, "held {key:?}");
            assert!(harness.state().work.review_revision_commit.is_none());
        }
        completion_key(&mut harness, key, false);
        completion_key(&mut harness, key, true);
        assert!(harness.state().review_overview());
    }
}

#[test]
fn completion_input_blocked_press_does_not_submit_when_enabled() {
    for blocked in ["loading", "saving", "drawer", "invalid", "view"] {
        let mut harness = two_object_review_revision_harness();
        match blocked {
            "loading" => harness.state_mut().loading.image = true,
            "saving" => harness.state_mut().loading.saving = true,
            "drawer" => harness.state_mut().work.drawer = Some(Drawer::Inspector),
            "view" => harness.state_mut().view = AppView::Setup,
            "invalid" => { harness.state_mut().navigate_review_item(2); },
            _ => unreachable!(),
        }
        completion_key(&mut harness, egui::Key::Space, true);
        harness.state_mut().view = AppView::Review;
        harness.state_mut().loading.image = false;
        harness.state_mut().loading.saving = false;
        harness.state_mut().work.drawer = None;
        harness.state_mut().navigate_review_item(0);
        completion_key(&mut harness, egui::Key::Space, true);
        assert_eq!(harness.state().review_position(), 0, "{blocked}");
        completion_key(&mut harness, egui::Key::Space, false);
        completion_key(&mut harness, egui::Key::Space, true);
        assert_eq!(harness.state().review_position(), 1, "{blocked}");
    }
}

#[test]
fn completion_input_annotation_hold_cannot_complete_next_image() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    completion_key(&mut harness, egui::Key::Space, true);
    step_until(&mut harness, 20, |app| !app.loading.saving && !app.loading.image && app.work.pending_transition.is_none());
    assert_eq!(api.counts().complete_assignment, 1);
    for _ in 0..4 {
        completion_key(&mut harness, egui::Key::Space, true);
        assert_eq!(api.counts().complete_assignment, 1);
    }
    completion_key(&mut harness, egui::Key::Space, false);
    completion_key(&mut harness, egui::Key::Space, true);
    step_until(&mut harness, 20, |_| api.counts().complete_assignment == 2);
}

#[test]
fn completion_input_prelabel_hold_accepts_one_suggestion() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_prelabel_work_harness(api.clone());
    let mut second = harness.state().work.current.as_ref().unwrap().prelabels[0].clone();
    second.suggestion_id = "held-key-second-suggestion".into();
    second.geometry = AnnotationGeometry::BoundingBox(BoundingBox { x: 0.7, y: 0.6, width: 0.15, height: 0.2 });
    harness.state_mut().work.current.as_mut().unwrap().prelabels.push(second);
    harness.state_mut().sync_prelabel_review();
    let before = harness.state().pending_prelabel_objects().len();
    assert!(before > 1);
    completion_key(&mut harness, egui::Key::A, true);
    let after = harness.state().pending_prelabel_objects().len();
    assert_eq!(after, before - 1);
    completion_key(&mut harness, egui::Key::A, true);
    assert_eq!(harness.state().pending_prelabel_objects().len(), after);
    assert_eq!(api.counts().complete_assignment, 0);
}

#[test]
fn completion_input_focused_button_and_shortcut_activate_once() {
    for key in [egui::Key::Enter, egui::Key::Space] {
        let mut harness = two_object_review_revision_harness();
        harness.run_steps(3);
        harness.get_by_role_and_label(egui::accesskit::Role::Button, "Approve").focus();
        harness.step();
        completion_key(&mut harness, key, true);
        assert_eq!(harness.state().review_position(), 1);
        harness.get_by_role_and_label(egui::accesskit::Role::Button, "Approve").focus();
        harness.step();
        completion_key(&mut harness, key, true);
        assert_eq!(harness.state().review_position(), 1);
        completion_key(&mut harness, key, false);
        completion_key(&mut harness, key, true);
        assert!(harness.state().review_overview());
    }
}

#[test]
fn completion_input_correction_hold_cannot_decide_next_item() {
    for key in [egui::Key::Space, egui::Key::N] {
        let mut harness = two_object_review_revision_harness();
        harness.run_steps(3);
        edit_test_review_box(harness.state_mut());
        completion_key(&mut harness, key, true);
        assert_eq!(harness.state().review_position(), 1);
        edit_test_review_box(harness.state_mut());
        completion_key(&mut harness, key, true);
        assert_eq!(harness.state().review_position(), 1);
        assert_eq!(harness.state().work.review_corrections.changes.len(), 1);
        completion_key(&mut harness, key, false);
        completion_key(&mut harness, key, true);
        assert!(harness.state().review_overview());
        assert_eq!(harness.state().work.review_corrections.changes.len(), 2);
        completion_key(&mut harness, key, true);
        assert!(harness.state().work.review_corrections.submission.is_none());
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn completion_input_migration_completion_requires_fresh_press() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [InspectorPreset::MigrationObject, InspectorPreset::MigrationFullImage, InspectorPreset::MigrationReview] {
        let api = Rc::new(SpyApi::new());
        let mut app = inspector_presets::build(preset, &egui::Context::default());
        if preset == InspectorPreset::MigrationObject {
            let task = app.selected_task().unwrap().clone();
            let labello_domain::MigrationCursor::Object { object_group_id, .. } = app.work.migration.cursor.clone().unwrap() else { panic!("object cursor"); };
            let mut draft = crate::manual_migration::ManualMigrationState::empty_skeleton(
                task.skeleton.unwrap().keypoints.into_iter().map(|point| point.name),
            );
            for point in &mut draft.keypoints {
                point.point = Some(NormalizedPoint { x: 0.5, y: 0.5 });
                point.state = KeypointState::Visible;
            }
            app.work.migration.keypoint_index = draft.keypoints.len();
            app.work.migration.draft = Some(draft);
            app.work.migration.draft_group = Some(object_group_id);
            app.work.migration.draft_dirty = true;
        }
        api.set_image_state(app.work.current_state.clone().unwrap());
        app.runtime.api = Some(api.clone());
        let mut harness = Harness::builder().with_size(egui::vec2(390.0, 844.0)).build_eframe(|_| app);
        harness.run_steps(3);
        let result = labello_client::ManualMigrationCommandResult {
            image_state: harness.state().work.current_state.clone().unwrap(),
            cursor: harness.state().work.migration.cursor.clone(),
            progress: Default::default(), active_pass: None, confirmation: None,
            assignment: None, annotation_id: None,
        };
        api.respond_to_next_migration_with(result.clone());
        completion_key(&mut harness, egui::Key::Space, true);
        step_until(&mut harness, 12, |app| !app.work.migration.busy);
        assert_eq!(api.counts().migration_commands, 1, "{preset:?}");
        completion_key(&mut harness, egui::Key::Space, true);
        assert_eq!(api.counts().migration_commands, 1, "{preset:?}");
        // A successful save clears the draft; restore valid work for the next decision.
        if preset == InspectorPreset::MigrationObject {
            let app = harness.state_mut();
            for point in &mut app.work.migration.draft.as_mut().unwrap().keypoints {
                point.point = Some(NormalizedPoint { x: 0.5, y: 0.5 });
                point.state = KeypointState::Visible;
            }
            app.work.migration.keypoint_index = app.work.migration.draft.as_ref().unwrap().keypoints.len();
            app.work.migration.draft_dirty = true;
        }
        api.respond_to_next_migration_with(result);
        completion_key(&mut harness, egui::Key::Space, true);
        assert_eq!(api.counts().migration_commands, 1, "{preset:?}");
        completion_key(&mut harness, egui::Key::Space, false);
        completion_key(&mut harness, egui::Key::Space, true);
        harness.run_steps(8);
        assert_eq!(api.counts().migration_commands, 2, "{preset:?}");
    }
}

#[test]
fn completion_input_pointer_hold_activates_one_review_item() {
    let mut harness = two_object_review_revision_harness();
    harness.run_steps(3);
    let pos = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Approve").rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    harness.step();
    assert_eq!(harness.state().review_position(), 0);
    harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    harness.run_steps(6);
    assert_eq!(harness.state().review_position(), 1);
}

#[test]
fn completion_input_modified_binding_and_unrelated_navigation() {
    let mut harness = two_object_review_revision_harness();
    let mut chord = labello_domain::KeyChord::new("Enter");
    chord.ctrl = true;
    harness.state_mut().work.keybindings.bindings.insert(labello_domain::UserAction::NextImage, chord);
    harness.run_steps(3);
    for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::COMMAND] {
        harness.state_mut().navigate_review_item(0);
        for _ in 0..3 {
            harness.event(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers });
            harness.run_steps(3);
            assert_eq!(harness.state().review_position(), 1);
        }
        completion_key(&mut harness, egui::Key::Enter, false);
    }
    // Even Space can still repeat when explicitly bound to a navigation action.
    harness.state_mut().work.keybindings.bindings.insert(labello_domain::UserAction::ZoomIn, labello_domain::KeyChord::new("Space"));
    let before = harness.state().work.canvas.current_zoom();
    completion_key(&mut harness, egui::Key::Space, true);
    let first = harness.state().work.canvas.current_zoom();
    completion_key(&mut harness, egui::Key::Space, true);
    assert!(first > before);
    assert!(harness.state().work.canvas.current_zoom() > first);
}

#[test]
fn completion_input_text_focus_retains_repeated_spaces() {
    let mut harness = two_object_review_revision_harness();
    harness.state_mut().work.inspector_panel_collapsed = false;
    edit_test_review_box(harness.state_mut());
    harness.state_mut().reject_review_item();
    harness.state_mut().navigate_review_item(2);
    harness.run_steps(3);
    harness.get_by_role_and_label(egui::accesskit::Role::MultilineTextInput, "Reason (optional, whole submission)").focus();
    harness.step();
    for _ in 0..3 {
        harness.event(egui::Event::Text(" ".into()));
        completion_key(&mut harness, egui::Key::Space, true);
    }
    assert_eq!(harness.state().work.review_corrections.reason, "   ");
    assert!(harness.state().work.review_revision_commit.is_none());
}

#[test]
fn completion_input_failed_submission_does_not_retry_while_held() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    api.fail_next_batch();
    completion_key(&mut harness, egui::Key::Space, true);
    step_until(&mut harness, 12, |app| !app.loading.saving && app.work.save_status == SaveStatus::Retry);
    completion_key(&mut harness, egui::Key::Space, true);
    assert_eq!(api.counts().complete_assignment, 0);
    assert_eq!(harness.state().work.save_status, SaveStatus::Retry);
    completion_key(&mut harness, egui::Key::Space, false);
    completion_key(&mut harness, egui::Key::Space, true);
    step_until(&mut harness, 12, |_| api.counts().complete_assignment == 1);
}
