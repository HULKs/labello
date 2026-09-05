#[test]
fn secondary_workspace_actions_use_available_wide_space() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.set_size(egui::vec2(2200.0, 1000.0));
    harness.run_steps(4);
    assert!(harness.query_by_label_contains("Undo").is_some());
    assert!(harness.query_by_label_contains("Redo").is_some());
    assert!(harness.query_by_label("More actions").is_none());
}

struct OverflowProbe {
    width: f32,
    actions: Vec<crate::panels::WorkspaceAction>,
    widths: Vec<f32>,
    more: f32,
    gap: f32,
    clicked: Vec<crate::panels::WorkspaceCommand>,
}

fn overflow_probe() -> Harness<'static, OverflowProbe> {
    use crate::panels::{WorkspaceAction, WorkspaceCommand};
    use labello_domain::UserAction;
    Harness::builder()
        .with_size(egui::vec2(1600.0, 500.0))
        .build_ui_state(
            |ui, state| {
                ui.spacing_mut().interact_size = egui::Vec2::splat(44.0);
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.set_width(state.width);
                ui.horizontal_wrapped(|ui| {
                    state.widths = state
                        .actions
                        .iter()
                        .map(|action| {
                            let button = egui::Button::new(action.label.as_str())
                                .shortcut_text(action.shortcut.clone());
                            crate::panels::workspace_button_size(ui, &button).x
                        })
                        .collect();
                    state.more = crate::panels::workspace_button_size(
                        ui,
                        &egui::Button::new("More actions"),
                    )
                    .x;
                    state.gap = ui.spacing().item_spacing.x;
                    if let Some(command) = crate::panels::workspace_secondary_actions(
                        ui,
                        &state.actions,
                        "More actions",
                    ) {
                        state.clicked.push(command);
                    }
                });
            },
            OverflowProbe {
                width: 1500.0,
                widths: Vec::new(),
                more: 0.0,
                gap: 0.0,
                clicked: Vec::new(),
                actions: vec![
                    WorkspaceAction {
                        command: WorkspaceCommand::User(UserAction::UndoEdit),
                        label: "Undo".into(),
                        shortcut: "Ctrl+Shift+Z".into(),
                        enabled: true,
                        help: "Undo the last edit.",
                    },
                    WorkspaceAction {
                        command: WorkspaceCommand::User(UserAction::RedoEdit),
                        label: "Redo the deliberately long translated action".into(),
                        shortcut: "Alt+Shift+Backspace".into(),
                        enabled: true,
                        help: "Redo the last undone edit.",
                    },
                    WorkspaceAction {
                        command: WorkspaceCommand::User(UserAction::SaveAnnotations),
                        label: "Save all current annotation changes".into(),
                        shortcut: "Ctrl+S".into(),
                        enabled: false,
                        help: "Save current edits.",
                    },
                ],
            },
        )
}

#[test]
fn workspace_overflow_measures_each_promotion_and_final_trigger_removal() {
    let mut harness = overflow_probe();
    harness.run_steps(3);
    let widths = harness.state().widths.clone();
    let gap = harness.state().gap;
    let more = harness.state().more;
    let thresholds = [
        widths[0] + gap + more,
        widths[0] + widths[1] + 2.0 * gap + more,
        widths.iter().sum::<f32>() + 2.0 * gap,
    ];
    assert!(thresholds.windows(2).all(|pair| pair[0] < pair[1]));
    for (promotion, threshold) in thresholds.into_iter().enumerate() {
        for delta in [-1.0, 0.0, 1.0] {
            let available = threshold + delta;
            harness.state_mut().width = available;
            harness.run_steps(4);
            let expected = promotion + usize::from(delta >= 0.0);
            let labels = ["Undo", "Redo the deliberately", "Save"];
            for (index, label) in labels.into_iter().enumerate() {
                assert_eq!(
                    harness.query_by_label_contains(label).is_some(),
                    index < expected,
                    "width={available} index={index} prefix={expected}"
                );
            }
            assert_eq!(
                harness.query_by_label("More actions").is_some(),
                expected < widths.len()
            );
            assert!(harness.state().clicked.is_empty());
        }
    }
}

#[test]
fn workspace_overflow_moves_focus_to_trigger_and_preserves_command_on_menu_entry() {
    use crate::panels::WorkspaceCommand;
    let mut harness = overflow_probe();
    harness.run_steps(3);
    let redo_id = harness
        .get_by_label_contains("Redo the deliberately")
        .accesskit_node()
        .locate()
        .0;
    harness
        .get_by_label_contains("Redo the deliberately")
        .focus();
    harness.run_steps(2);
    harness.state_mut().width = harness.state().more + 4.0;
    harness.run_steps(4);
    assert!(harness.get_by_label("More actions").is_focused());
    assert!(harness.state().clicked.is_empty());
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let redo = harness.get_by_label_contains("Redo the deliberately");
    assert_eq!(redo.accesskit_node().locate().0, redo_id);
    assert!(redo.is_focused());
    assert!(
        harness
            .get_by_label_contains("Save")
            .accesskit_node()
            .is_disabled()
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert_eq!(
        harness.state().clicked,
        [WorkspaceCommand::User(labello_domain::UserAction::RedoEdit)]
    );
}

#[test]
fn workspace_overflow_remeasures_changed_shortcuts_and_never_reserves_an_empty_menu() {
    let mut harness = overflow_probe();
    harness.run_steps(3);
    harness.state_mut().actions.truncate(1);
    harness.state_mut().actions[0].label = "Go".into();
    harness.state_mut().actions[0].shortcut = "X".into();
    harness.run_steps(3);
    let width = harness.state().widths[0];
    harness.state_mut().width = width;
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("Go").is_some());
    assert!(harness.query_by_label("More actions").is_none());
    harness.state_mut().actions[0].shortcut = "Ctrl+Alt+Shift+Backspace".into();
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("Go").is_none());
    assert!(harness.query_by_label("More actions").is_some());
    harness.state_mut().width = harness.state().widths[0];
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("Go").is_some());
    assert!(harness.query_by_label("More actions").is_none());
}

#[test]
fn workspace_action_measurement_matches_painted_font_icon_and_shortcut() {
    let _harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 500.0))
        .build_ui(|ui| {
            ui.horizontal(|ui| {
                let button = egui::Button::new((
                    egui::Atom::custom(egui::Id::new("measured-icon"), egui::vec2(29.0, 21.0)),
                    egui::RichText::new("A long translated action").size(23.0),
                ))
                .shortcut_text("Ctrl+Alt+Shift+Backspace")
                .min_size(egui::Vec2::splat(44.0))
                .wrap_mode(egui::TextWrapMode::Extend);
                let measured = crate::panels::workspace_button_size(ui, &button);
                let response = ui.add(button);
                assert!(response.rect.width() <= measured.x + 0.01);
                assert!((response.rect.width() - measured.x).abs() <= 0.02);
                assert!((response.rect.height() - measured.y).abs() <= 0.02);
            });
        });
}

#[test]
fn workspace_overflow_dynamic_save_previous_and_loading_keep_one_command_location() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.set_size(egui::vec2(2200.0, 1000.0));
    harness.run_steps(3);
    harness.state_mut().work.previous_assignment = harness.state().work.assignment.clone();
    for dirty in [false, true, false] {
        harness.state_mut().work.save_status = if dirty {
            SaveStatus::Dirty
        } else {
            SaveStatus::Saved
        };
        harness.state_mut().work.last_edit_at = Some(Instant::now());
        harness.run_steps(3);
        let previous = harness
            .query_all_by_label_contains("Previous")
            .filter(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .count();
        assert_eq!(previous, 1);
        assert_eq!(harness.query_by_label("Save").is_some(), dirty);
        assert!(harness.query_by_label("More actions").is_none());
    }
    harness.state_mut().loading.saving = true;
    harness.run_steps(3);
    assert!(
        harness
            .get_by_label_contains("Undo")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        harness
            .get_by_label_contains("Redo")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn workspace_overflow_resizing_keeps_primary_controls_context_and_settled_canvas() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    for width in [2200.0, 1288.0, 600.0, 390.0, 320.0, 600.0, 1288.0, 2200.0] {
        harness.set_size(egui::vec2(width, 568.0));
        harness.run_steps(4);
        let primary = harness.get_by_label("Submit & next").rect();
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 568.0))
                .contains_rect(primary)
        );
        let bar = harness.get_by_label("Workspace context bar").rect();
        let canvas = harness.get_by_label("Annotation canvas").rect();
        assert!(canvas.height() >= 44.0 && canvas.top() >= bar.bottom());
        let more = harness.query_by_label("More actions").is_some();
        harness.run_steps(4);
        assert_eq!(more, harness.query_by_label("More actions").is_some());
        assert_eq!(canvas, harness.get_by_label("Annotation canvas").rect());
    }
}

#[test]
fn workspace_overflow_long_menu_actions_stay_inside_a_short_viewport() {
    let mut harness = overflow_probe();
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.state_mut().width = 200.0;
    harness.state_mut().actions[1].label =
        "A much longer translated redo action that explains what will change".into();
    harness.run_steps(4);
    harness.get_by_label("More actions").click();
    harness.run_steps(4);
    let rect = harness
        .get_by_label_contains("A much longer translated")
        .rect();
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 320.0)).contains_rect(rect),
        "{rect:?}"
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_final_overflow_preserves_primary_confirmation_and_short_canvas() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|ctx| {
            crate::inspector_presets::build(
                crate::inspector_presets::InspectorPreset::MigrationFullImage,
                &ctx.egui_ctx,
            )
        });
    harness.run_steps(4);
    let canvas = harness.get_by_label("Annotation canvas").rect();
    assert!(canvas.height() >= 44.0, "{canvas:?}");
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 320.0))
            .contains_rect(harness.get_by_label_contains("Confirm & finish").rect())
    );
    assert!(harness.query_by_label_contains("Previous object").is_none());
    harness.get_by_label("More").click();
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("Previous object").is_some());
}

#[test]
fn short_review_revision_keeps_mode_in_context_without_a_canvas_caption_row() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2, y: 0.2, width: 0.3, height: 0.3,
    }), true);
    let mut harness = loaded_review_harness(api);
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.run_steps(4);
    let original_canvas = harness.get_by_label("Annotation canvas").rect();
    let original_bar = harness.get_by_label("Workspace context bar").rect();
    enter_test_review_revision(harness.state_mut());
    harness.run_steps(4);
    let canvas = harness.get_by_label("Annotation canvas").rect();
    assert!(canvas.height() >= original_canvas.height() - 0.5,
        "revision mode must use the existing context allocation: before={original_canvas:?} after={canvas:?}");
    assert_eq!(harness.get_by_label("Workspace context bar").rect().height(), original_bar.height());
    let context = harness.state().review_context().unwrap();
    assert!(context.revision_mode);
    let identity = if context.workflow_name == context.class_name {
        context.workflow_name.clone()
    } else { format!("{} · {}", context.workflow_name, context.class_name) };
    assert_review_bar_paints(&harness, &format!("Revising · {identity}"));
    assert_review_bar_paints(&harness, "Bounding boxes · Object 1 of 1");
    let details = harness.get_by_label_contains("Review details: Workflow:");
    assert!(details.accesskit_node().label().unwrap().contains("Decision revision mode; geometry unchanged"));
    assert!(harness.query_by_label("Decision revision; geometry unchanged.").is_none());

    // An invalid target cannot claim that the context bar presented revision details.
    harness.state_mut().work.annotations[0].version += 1;
    harness.run_steps(4);
    assert!(harness.state().review_revision_active());
    assert!(harness.state().review_context().is_none());
    assert!(harness.query_by_label("Decision revision; geometry unchanged.").is_some());
}
