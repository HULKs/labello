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

#[test]
fn short_review_availability_feedback_preserves_type_phase_and_canvas_allocation() {
    for revision in [false, true] {
        let api = Rc::new(SpyApi::new());
        seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2, y: 0.2, width: 0.3, height: 0.3,
        }), true);
        let mut harness = loaded_review_harness(api);
        harness.set_size(egui::vec2(320.0, 320.0));
        harness.run_steps(4);
        if revision { enter_test_review_revision(harness.state_mut()); }
        harness.run_steps(4);
        let before = harness.get_by_label("Annotation canvas").rect();
        let bar = harness.get_by_label("Workspace context bar").rect();
        let assignment = harness.state().work.assignment.as_ref().unwrap().assignment_id.clone();
        harness.state_mut().work.availability.loading = true;
        harness.state_mut().work.availability.tasks.clear();
        harness.run_steps(4);
        let after = harness.get_by_label("Annotation canvas").rect();
        assert_eq!(after, before, "availability must not displace required review context: revision={revision}");
        assert_eq!(harness.get_by_label("Workspace context bar").rect(), bar);
        assert!(after.height() >= 44.0);
        assert_review_bar_paints(&harness, "Bounding boxes · Object 1 of 1");
        let details = harness.get_by_label_contains("Review details: Workflow:").rect();
        let spinner = harness.get_by_label("Loading workflow assignment availability").rect();
        assert!(details.contains_rect(spinner), "loading feedback shares the identity line: {spinner:?} in {details:?}");
        assert!(spinner.bottom() <= details.top() + details.height() / 2.0);
        assert_eq!(harness.state().work.assignment.as_ref().unwrap().assignment_id, assignment);
        harness.get_by_label_contains("Review details: Workflow:").focus();
        harness.key_press(egui::Key::Enter);
        harness.run_steps(4);
        assert_eq!(harness.state().work.drawer, Some(Drawer::Inspector));
        harness.key_press(egui::Key::Escape);
        harness.run_steps(4);
        assert!(harness.state().work.drawer.is_none());
    }
}

fn assert_workspace_resize_settles_without_input(mut harness: Harness<'_, LabelloApp>) {
    let phase = harness.state().review_context().map(|context| context.phase);
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.run_steps(6);
    let settled = harness.get_by_label("Annotation canvas").rect();
    for size in [egui::vec2(1440.0, 1000.0), egui::vec2(1288.0, 820.0),
        egui::vec2(600.0, 800.0), egui::vec2(390.0, 844.0),
        egui::vec2(320.0, 568.0), egui::vec2(320.0, 320.0)] {
        harness.set_size(size);
        // Run only requested frames; extra steps would conceal a missing repaint.
        harness.run();
    }
    for size in [egui::vec2(320.0, 568.0), egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0), egui::vec2(1288.0, 820.0),
        egui::vec2(1440.0, 1000.0), egui::vec2(320.0, 320.0)] {
        harness.set_size(size);
        harness.run();
    }
    let resized = harness.get_by_label("Annotation canvas").rect();
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 320.0));
    for button in harness.query_all_by_role(egui::accesskit::Role::Button) {
        let rect = button.rect();
        if rect.top() >= resized.bottom() - 0.5 {
            assert!(viewport.contains_rect(rect), "footer action clipped after resize: {rect:?}");
            assert!(rect.height() >= 44.0, "footer action too short: {rect:?}");
        }
    }
    assert_eq!(resized, settled, "idle resize must settle without pointer input");
    assert_eq!(harness.state().review_context().map(|context| context.phase), phase);
    assert!(resized.height() >= 44.0, "{resized:?}");
}

#[test]
fn workspace_idle_resize_settles_annotation_actions() {
    assert_workspace_resize_settles_without_input(loaded_work_harness(Rc::new(SpyApi::new())));
}

#[test]
fn workspace_idle_resize_settles_review_actions() {
    for revision in [false, true] {
        let api = Rc::new(SpyApi::new());
        seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2, y: 0.2, width: 0.3, height: 0.3,
        }), true);
        let mut harness = loaded_review_harness(api);
        if revision { enter_test_review_revision(harness.state_mut()); }
        assert_workspace_resize_settles_without_input(harness);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn workspace_idle_resize_settles_migration_actions() {
    use crate::inspector_presets::{build, InspectorPreset};
    for (preset, review_index) in [
        (InspectorPreset::MigrationObject, None),
        (InspectorPreset::MigrationFullImage, None),
        (InspectorPreset::MigrationReview, Some(0)),
        (InspectorPreset::MigrationReview, Some(2)),
    ] {
        let mut harness = Harness::builder().with_size(egui::vec2(320.0, 320.0))
            .build_eframe(move |ctx| {
                let mut app = build(preset, &ctx.egui_ctx);
                if let Some(index) = review_index {
                    let task_id = app.work.selected_task_id.clone().unwrap();
                    let state = app.work.current_state.as_mut().unwrap();
                    let target_set_hash = state.migration_target_sets[&task_id].target_set_hash.clone();
                    let state_hash = state.current_migration_state_hash(&task_id).unwrap();
                    state.migration_confirmations.insert(task_id.clone(), labello_domain::MigrationConfirmation {
                        confirmation_hash: labello_domain::migration_confirmation_hash(&target_set_hash, &state_hash).unwrap(),
                        task_id, target_set_hash, state_hash,
                        actor_user_id: UserId::from("annotator"), timestamp: now(),
                    });
                    app.work.migration.review_index = index;
                }
                app
            });
        if let Some(index) = review_index {
            // Initial assignment synchronization chooses the canonical first target.
            // Now exercise the explicit object/final positions in the stable scope.
            if index == 2 {
                let task = harness.state().selected_task().unwrap().clone();
                let user = harness.state().config.user_id.clone();
                let state = harness.state_mut().work.current_state.as_mut().unwrap();
                for (position, target) in state.review_object_targets(&task).unwrap().into_iter().enumerate() {
                    let timestamp = now();
                    state.apply_event(&EventLogEntry::new(
                        state.current_sequence + 1, state.image_id.clone(), user.clone(),
                        DatasetRole::Reviewer, timestamp, EventPayload::ReviewRecorded {
                            review: labello_domain::ReviewRecord {
                                review_id: labello_domain::ReviewId::from(format!("resize-review-{position}")),
                                target, reviewer_user_id: user.clone(),
                                decision: labello_domain::ReviewDecision::Approved, timestamp, comment: None,
                            },
                        },
                    )).unwrap();
                }
            }
            harness.state_mut().work.migration.review_index = index;
            let context = harness.state().review_context().expect("valid migration review context");
            assert_eq!(matches!(context.phase, crate::review_context::ReviewContextPhase::FullImage { .. }), index == 2);
        }
        assert_workspace_resize_settles_without_input(harness);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn workspace_idle_resize_does_not_repaint_forever_when_actions_cannot_fit() {
    let mut harness = Harness::builder().with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|ctx| crate::inspector_presets::build(
            crate::inspector_presets::InspectorPreset::MigrationFullImage, &ctx.egui_ctx));
    harness.run_steps(4);
    harness.set_size(egui::vec2(160.0, 120.0));
    harness.run();
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.run();
    let confirm = harness.get_by_label_contains("Confirm & finish").rect();
    assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 320.0)).contains_rect(confirm));
}
