#[cfg(feature = "inspector-presets")]
#[test]
fn review_delete_uses_the_configured_key_and_only_removes_the_selected_addition() {
    for (binding, key) in [
        ("Delete", egui::Key::Delete),
        ("Backspace", egui::Key::Backspace),
    ] {
        for mut harness in [
            keypoint_review_overview(1),
            keypoint_review_overview(2),
            migration_keypoint_review_overview(),
        ] {
            harness.state_mut().work.keybindings.bindings.insert(
                labello_domain::UserAction::DeleteAnnotation,
                labello_domain::KeyChord::new(binding),
            );
            let persisted = harness.state().work.current_state.clone();
            let count = harness
                .state()
                .selected_task()
                .unwrap()
                .skeleton
                .as_ref()
                .unwrap()
                .keypoints
                .len();
            let first = harness.get_by_label("Annotation canvas").rect().center()
                + egui::vec2(-50.0, -120.0);
            for object in 0..3 {
                for point in 0..count {
                    click_at(
                        &mut harness,
                        first + egui::vec2(object as f32 * 50.0, point as f32 * 50.0),
                    );
                    harness.run_steps(2);
                }
            }
            let changes = harness.state().work.review_corrections.changes.clone();
            assert_eq!(changes.len(), 3);
            harness.input_mut().time = Some(harness.ctx.input(|input| input.time) + 1.0);
            click_at(&mut harness, first + egui::vec2(50.0, 0.0));
            harness.run_steps(2);
            assert!(harness.state().work.correction_draft.is_some());
            if key != egui::Key::Delete {
                harness.key_press(egui::Key::Delete);
                harness.step();
                assert!(harness.state().work.review_corrections.changes == changes);
            }
            harness.key_press(key);
            harness.run_steps(2);
            assert!(
                harness.state().work.review_corrections.changes
                    == [changes[0].clone(), changes[2].clone()]
            );
            assert!(harness.state().work.correction_draft.is_none());
            assert!(harness.state().work.selected_annotation.is_none());
            harness.key_press(key);
            harness.step();
            assert_eq!(harness.state().work.review_corrections.changes.len(), 2);
            harness.state_mut().navigate_review_item(0);
            harness.run_steps(2);
            let editor = harness.state().work.correction_draft.clone();
            harness.key_press(key);
            harness.step();
            assert!(harness.state().work.correction_draft == editor);
            assert_eq!(harness.state().work.review_corrections.changes.len(), 2);
            assert!(harness.state().work.current_state == persisted);
        }
    }
}

#[test]
fn review_delete_respects_busy_states_dragging_and_text_focus() {
    let mut harness = keypoint_review_overview(1);
    let point = harness.get_by_label("Annotation canvas").rect().center() + egui::vec2(80.0, 50.0);
    click_at(&mut harness, point);
    harness.input_mut().time = Some(harness.ctx.input(|input| input.time) + 1.0);
    click_at(&mut harness, point);
    harness.run_steps(2);
    for blocked in ["saving", "image", "migration", "transition"] {
        match blocked {
            "saving" => harness.state_mut().loading.saving = true,
            "image" => harness.state_mut().loading.image = true,
            "migration" => harness.state_mut().work.migration.busy = true,
            "transition" => {
                harness.state_mut().work.pending_transition =
                    Some(crate::app::PendingTransition::View(AppView::Setup))
            }
            _ => unreachable!(),
        }
        harness.key_press(egui::Key::Delete);
        harness.step();
        assert_eq!(
            harness.state().work.review_corrections.changes.len(),
            1,
            "{blocked}"
        );
        assert!(harness.state().work.correction_draft.is_some(), "{blocked}");
        harness.state_mut().loading.saving = false;
        harness.state_mut().loading.image = false;
        harness.state_mut().work.migration.busy = false;
        harness.state_mut().work.pending_transition = None;
        harness.run_steps(2);
    }
    harness.drag_at(point);
    harness.step();
    assert!(harness.state().work.canvas.is_dragging());
    harness.key_press(egui::Key::Delete);
    harness.step();
    assert_eq!(harness.state().work.review_corrections.changes.len(), 1);
    assert!(harness.state().work.correction_draft.is_some());
    harness.drop_at(point);
    harness.run_steps(2);
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.run_steps(2);
    harness
        .get_by_role_and_label(
            egui::accesskit::Role::MultilineTextInput,
            "Reason (optional)",
        )
        .focus();
    harness.step();
    assert!(harness.ctx.text_edit_focused());
    harness.key_press(egui::Key::Delete);
    harness.step();
    assert_eq!(harness.state().work.review_corrections.changes.len(), 1);
    harness.state_mut().work.inspector_panel_collapsed = true;
    harness.ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    harness.run_steps(2);
    harness.key_press(egui::Key::Delete);
    harness.run_steps(2);
    assert!(harness.state().work.review_corrections.changes.is_empty());
    assert!(harness.state().work.correction_draft.is_none());
}
