#[test]
fn compact_review_bar_keeps_workflow_type_and_canonical_phase() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.run_steps(4);
    assert!(
        harness
            .query_by_label_contains("Review details: Workflow:")
            .is_some()
    );
}

#[test]
fn review_bar_allocates_type_phase_controls_and_canvas_at_each_viewport() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    for (width, height) in [
        (1440.0, 1000.0),
        (1288.0, 820.0),
        (600.0, 800.0),
        (390.0, 844.0),
        (320.0, 568.0),
        (320.0, 320.0),
        (1288.0, 320.0),
    ] {
        harness.set_size(egui::vec2(width, height));
        harness.run_steps(4);
        let details = harness.get_by_label_contains("Review details: Workflow:");
        let label = details.accesskit_node().label().unwrap().to_string();
        assert!(label.contains("Bounding boxes") && label.contains("Object 1 of 1"));
        let rect = details.rect();
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height))
                .contains_rect(rect),
            "details={rect:?}"
        );
        assert!(rect.height() >= 44.0);
        let bar = harness.get_by_label("Workspace context bar").rect();
        assert!(bar.contains_rect(rect));
        let canvas = harness.get_by_label("Annotation canvas").rect();
        for label in ["Skip", if width >= 1100.0 { "Discard corrections" } else { "Discard" }] {
            let action = harness.get_by_role_and_label(egui::accesskit::Role::Button, label).rect();
            assert!(action.top() >= canvas.bottom(), "{label} must be below the canvas");
            assert!(action.bottom() <= height, "{label} must fit the viewport");
            assert!(action.height() >= 44.0);
        }
        assert!(canvas.height() >= 44.0, "{width}x{height}: {canvas:?}");
        assert!(
            bar.bottom() <= canvas.top() + 0.5,
            "bar={bar:?} canvas={canvas:?}"
        );
        assert_control_inside(
            &harness,
            "Fit",
            egui::accesskit::Role::Button,
            width,
            height,
        );
        if width < 1100.0 {
            let fit = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Fit").rect();
            let refocus = harness.get_by_label_contains("Refocus object").rect();
            assert!((fit.center().y - rect.center().y).abs() < 1.0);
            assert!((refocus.center().y - rect.center().y).abs() < 1.0);
            assert_control_inside(
                &harness,
                "Workflow",
                egui::accesskit::Role::Button,
                width,
                height,
            );
        }
    }
}

#[test]
fn review_bar_details_opens_by_keyboard_and_contains_complete_long_identity() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    let id = harness.state().work.selected_task_id.clone().unwrap();
    harness
        .state_mut()
        .work
        .tasks
        .iter_mut()
        .find(|task| task.task_id == id)
        .unwrap()
        .name =
        "A deliberately very long duplicate workflow name repeated for distinct geometry types"
            .into();
    for class in &mut harness.state_mut().work.classes {
        class.name = "A different long class name that remains available in full details".into();
    }
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.run_steps(4);
    harness
        .get_by_label_contains("Review details: Workflow:")
        .focus();
    harness.key_press(egui::Key::Enter);
    harness.run_steps(4);
    assert_eq!(harness.state().work.drawer, Some(Drawer::Inspector));
    assert!(harness.get_by_label("Close Inspector").is_focused());
    let summary = format!(
        "Active review context: {}",
        harness
            .state()
            .review_context()
            .unwrap()
            .accessible_summary()
    );
    assert!(harness.query_by_label(&summary).is_some());
    assert_control_inside(
        &harness,
        "Close Inspector",
        egui::accesskit::Role::Button,
        320.0,
        320.0,
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().work.drawer.is_none());
    assert!(
        harness
            .query_by_label_contains("Review details: Workflow:")
            .is_some()
    );
    assert!(
        harness
            .get_by_label_contains("Review details: Workflow:")
            .is_focused()
    );
    harness.state_mut().work.review_index = 1;
    harness.state_mut().sync_review_selection();
    harness.run_steps(3);
    harness
        .get_by_label_contains("Review details: Workflow:")
        .focus();
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(3);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        harness
            .get_by_label_contains("Review details: Workflow:")
            .is_focused(),
        "final details must restore focus after Tab and Escape"
    );
}

fn assert_review_bar_paints(harness: &Harness<'_, LabelloApp>, expected: &str) {
    fn text_rect(shape: &egui::epaint::Shape, expected: &str) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| text_rect(shape, expected))
            }
            _ => None,
        }
    }
    let bar = harness.get_by_label("Workspace context bar").rect();
    let found =
        harness.output().shapes.iter().find_map(|shape| {
            text_rect(&shape.shape, expected).map(|rect| (rect, shape.clip_rect))
        });
    let (rect, clip) = found.unwrap_or_else(|| panic!("missing painted context {expected}"));
    assert!(
        bar.expand(0.5).contains_rect(rect),
        "text {expected}: {rect:?}, bar {bar:?}"
    );
    assert!(
        clip.expand(0.5).contains_rect(rect),
        "text {expected} clipped: {rect:?}, clip {clip:?}"
    );
}

#[test]
fn review_bar_tracks_correction_final_loading_and_missing_preview_without_stale_text() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.run_steps(4);
    assert_review_bar_paints(&harness, "Item 1 / 1");
    harness.state_mut().start_correction();
    harness.run_steps(4);
    assert_review_bar_paints(&harness, "Item 1 / 1");
    assert!(harness.get_by_label("Annotation canvas").rect().height() >= 44.0);
    harness.state_mut().discard_correction();
    harness.state_mut().work.review_index = 1;
    harness.state_mut().sync_review_selection();
    harness.run_steps(4);
    assert_review_bar_paints(&harness, "Image overview");
    harness.state_mut().work.current_texture = None;
    harness.run_steps(3);
    assert_review_bar_paints(&harness, "Image overview");
    harness.state_mut().loading.image = true;
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("Review details: Workflow:")
            .is_none()
    );
    assert!(harness.query_by_label("Loading review target…").is_some());
    harness.state_mut().loading.image = false;
    harness.state_mut().work.assignment = None;
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("Review details: Workflow:")
            .is_none()
    );
    assert!(
        harness
            .query_by_label("No active review assignment")
            .is_some()
    );
}

#[test]
fn review_bar_wraps_measured_type_and_phase_when_text_grows() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    harness.set_size(egui::vec2(320.0, 568.0));
    harness.run_steps(3);
    let before = harness
        .get_by_label("Workspace context bar")
        .rect()
        .height();
    let mut style = (*harness.ctx.global_style()).clone();
    style
        .text_styles
        .get_mut(&egui::TextStyle::Body)
        .unwrap()
        .size = 20.0;
    harness.ctx.set_global_style(style);
    harness.run_steps(4);
    assert_review_bar_paints(&harness, "Item 1 / 1");
    assert!(
        harness
            .get_by_label("Workspace context bar")
            .rect()
            .height()
            >= before
    );
    assert!(harness.get_by_label("Annotation canvas").rect().height() >= 44.0);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn review_bar_uses_canonical_migration_context_for_excluded_discovered_and_final_targets() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::review_context::ReviewContextPhase;
    for preset in [
        InspectorPreset::MigrationReview,
        InspectorPreset::MigrationDiscoveryReview,
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 568.0))
            .build_eframe(|ctx| {
                let mut app = inspector_presets::build(preset, &ctx.egui_ctx);
                let task_id = app.work.selected_task_id.clone().unwrap();
                let state = app.work.current_state.as_mut().unwrap();
                let target_set_hash = state.migration_target_sets[&task_id]
                    .target_set_hash
                    .clone();
                let state_hash = state.current_migration_state_hash(&task_id).unwrap();
                state.migration_confirmations.insert(
                    task_id.clone(),
                    labello_domain::MigrationConfirmation {
                        confirmation_hash: labello_domain::migration_confirmation_hash(
                            &target_set_hash,
                            &state_hash,
                        )
                        .unwrap(),
                        task_id,
                        target_set_hash,
                        state_hash,
                        actor_user_id: UserId::from("annotator"),
                        timestamp: now(),
                    },
                );
                app
            });
        harness.run_steps(4);
        let mut observed = Vec::new();
        for step in 0..8 {
            let context = harness.state().review_context().unwrap();
            let full = harness
                .get_by_label_contains("Review details: Workflow:")
                .accesskit_node()
                .label()
                .unwrap()
                .to_string();
            assert!(full.contains(&context.accessible_summary()));
            if matches!(context.phase, ReviewContextPhase::FullImage { .. }) {
                assert_review_bar_paints(&harness, "Image overview");
                assert!(!full.contains("version"));
                observed.push("Final check");
                break;
            }
            assert_review_bar_paints(&harness, &match context.phase { crate::review_context::ReviewContextPhase::Object { number, total, .. } => format!("Item {number} / {total}"), _ => "Image overview".into() });
            let ReviewContextPhase::Object { number, kind, .. } = context.phase else {
                unreachable!()
            };
            observed.push(kind);
            let task = harness.state().selected_task().unwrap().clone();
            let target = harness
                .state()
                .work
                .current_state
                .as_ref()
                .unwrap()
                .review_object_targets(&task)
                .unwrap()[number - 1]
                .clone();
            let user = harness.state().config.user_id.clone();
            let state = harness.state_mut().work.current_state.as_mut().unwrap();
            let timestamp = now();
            state
                .apply_event(&EventLogEntry::new(
                    state.current_sequence + 1,
                    state.image_id.clone(),
                    user.clone(),
                    DatasetRole::Reviewer,
                    timestamp,
                    EventPayload::ReviewRecorded {
                        review: labello_domain::ReviewRecord {
                            review_id: labello_domain::ReviewId::from(format!("bar-review-{step}")),
                            target,
                            reviewer_user_id: user,
                            decision: labello_domain::ReviewDecision::Approved,
                            timestamp,
                            comment: None,
                        },
                    },
                ))
                .unwrap();
            harness.run_steps(4);
        }
        assert_eq!(observed.last(), Some(&"Final check"));
        if preset == InspectorPreset::MigrationReview {
            assert!(observed.contains(&"Migration: Excluded object"));
        } else {
            assert!(observed.contains(&"Migration: Discovered skeleton"));
        }
    }
}

#[test]
fn review_bar_distinguishes_duplicate_workflow_types_and_rejects_stale_task_data() {
    for skeleton in [false, true] {
        let api = Rc::new(SpyApi::new());
        let geometry = if skeleton {
            AnnotationGeometry::Skeleton(SkeletonGeometry {
                keypoints: vec![KeypointAnnotation {
                    name: "legacy_point".into(),
                    state: KeypointState::Absent,
                    point: None,
                }],
            })
        } else {
            AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.2,
                y: 0.2,
                width: 0.3,
                height: 0.3,
            })
        };
        seed_review_annotation(&api, geometry, true);
        let mut harness = loaded_review_harness(api);
        let original = harness.state().work.selected_task_id.clone().unwrap();
        let task = harness
            .state_mut()
            .work
            .tasks
            .iter_mut()
            .find(|task| task.task_id == original)
            .unwrap();
        task.name = "Duplicate task name".into();
        let mut other = task.clone();
        other.task_id = TaskId::from("other-type-task");
        other.annotation_type = if skeleton {
            AnnotationType::BoundingBox
        } else {
            AnnotationType::Skeleton
        };
        let other_id = other.task_id.clone();
        harness.state_mut().work.tasks.push(other);
        harness.set_size(egui::vec2(320.0, 568.0));
        harness.run_steps(3);
        let expected = "Item 1 / 1";
        assert_review_bar_paints(&harness, expected);
        harness.state_mut().work.selected_task_id = Some(other_id);
        harness.run_steps(3);
        assert!(
            harness
                .query_by_label_contains("Review details: Workflow:")
                .is_none()
        );
        assert!(
            harness
                .query_by_label("Review target unavailable")
                .is_some()
        );
        harness.state_mut().work.selected_task_id = Some(original);
        harness.run_steps(3);
        assert_review_bar_paints(&harness, expected);
    }
}

#[test]
fn review_details_indicator_toggles_the_wide_inspector_and_reflects_its_state() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox { x: 0.2, y: 0.2, width: 0.3, height: 0.3 }), true);
    let mut harness = loaded_review_harness(api);
    harness.set_size(egui::vec2(1440.0, 1000.0));
    harness.run_steps(3);
    assert!(harness.state().work.inspector_panel_collapsed);
    for collapsed in [false, true, false, true] {
        harness.get_by_label_contains("Review details: Workflow:").click();
        harness.run_steps(3);
        assert_eq!(harness.state().work.inspector_panel_collapsed, collapsed);
        let node = harness.get_by_label_contains("Review details: Workflow:");
        assert_eq!(node.accesskit_node().toggled(), Some(if collapsed { egui::accesskit::Toggled::False } else { egui::accesskit::Toggled::True }));
        assert_review_bar_paints(&harness, "Item 1 / 1");
    }
}

#[test]
fn mobile_review_footer_stays_visible_across_review_kinds_and_phases() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [InspectorPreset::Review, InspectorPreset::ReviewCorrection, InspectorPreset::MigrationReview, InspectorPreset::MigrationDiscoveryReview] {
        let mut app = inspector_presets::build(preset, &egui::Context::default());
        app.work.previous_assignment = app.work.assignment.clone();
        let mut harness = Harness::builder().with_size(egui::vec2(390.0, 844.0)).build_eframe(|_| app);
        for position in [0, harness.state().review_object_targets().len()] {
            harness.state_mut().navigate_review_item(position);
            for (width, height) in [(320.0, 320.0), (320.0, 568.0), (390.0, 844.0), (844.0, 390.0), (768.0, 1024.0)] {
                harness.set_size(egui::vec2(width, height));
                harness.run_steps(4);
                let canvas = harness.get_by_label("Annotation canvas").rect();
                assert!(canvas.height() >= 44.0, "{preset:?} {width}x{height}: {canvas:?}");
                let mut row_y: Option<f32> = None;
                for label in ["Previous", "Discard", "Skip"] {
                    let rect = harness.get_by_role_and_label(egui::accesskit::Role::Button, label).rect();
                    assert!(rect.top() >= canvas.bottom() && rect.bottom() <= height);
                    assert!(rect.left() >= 0.0 && rect.right() <= width);
                    assert!(rect.height() >= 44.0);
                    if let Some(y) = row_y { assert!((rect.center().y - y).abs() < 1.0); }
                    row_y = Some(rect.center().y);
                }
                let indicator = harness.get_by_label_contains("Review details:").rect();
                let fit = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Fit").rect();
                assert!((indicator.center().y - fit.center().y).abs() < 1.0);
            }
        }
    }
}

#[test]
fn mobile_review_icon_fallback_keeps_large_text_actions_in_their_rows() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_review_harness(api);
    harness.state_mut().work.previous_assignment = harness.state().work.assignment.clone();
    harness.set_size(egui::vec2(390.0, 844.0));
    harness.ctx.global_style_mut(|style| { style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(36.0)); });
    harness.run_steps(4);
    for label in ["Submit review", "Previous", "Discard", "Skip", "Fit"] {
        let rect = harness.get_by_role_and_label(egui::accesskit::Role::Button, label).rect();
        assert!(rect.left() >= 0.0 && rect.right() <= 390.0);
        assert!(rect.height() >= 44.0 && rect.height() < 60.0, "{label}: {rect:?}");
    }
}

#[test]
fn mobile_annotation_and_migration_toolbars_keep_large_text_controls_inline() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [InspectorPreset::Annotation, InspectorPreset::MigrationObject, InspectorPreset::MigrationFullImage] {
        let app = inspector_presets::build(preset, &egui::Context::default());
        let mut harness = Harness::builder().with_size(egui::vec2(320.0, 568.0)).build_eframe(|_| app);
        for font_size in [16.0, 36.0] {
            harness.ctx.global_style_mut(|style| { style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(font_size)); });
            harness.run_steps(4);
            let fit = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Fit").rect();
            for label in ["Pan", "Workflow", "Inspector"] {
                let rect = harness.get_by_role_and_label(egui::accesskit::Role::Button, label).rect();
                assert!((rect.center().y - fit.center().y).abs() < 1.0, "{preset:?}: {label} {rect:?} {fit:?}");
                assert!(rect.left() >= 0.0 && rect.right() <= 320.0, "{preset:?}: {label} {rect:?}");
            }
            let primary = harness.get_by_label_contains(match preset {
                InspectorPreset::Annotation => "Submit & next",
                InspectorPreset::MigrationObject => "Save & next",
                _ => "Confirm & finish",
            }).rect();
            assert!(primary.left() >= 0.0 && primary.right() <= 320.0, "{preset:?}: {primary:?}");
            assert!(primary.height() >= 44.0 && primary.height() < 60.0);
        }
    }
}

#[test]
fn added_migration_review_item_removal_lives_in_the_footer() {
    use crate::inspector_presets::{self, InspectorPreset};
    for (width, height) in [(320.0, 320.0), (390.0, 844.0), (1440.0, 1000.0)] {
        let mut app = inspector_presets::build(InspectorPreset::MigrationDiscoveryReview, &egui::Context::default());
        app.work.previous_assignment = app.work.assignment.clone();
        app.work.inspector_panel_collapsed = width < 1000.0;
        let mut harness = Harness::builder().with_size(egui::vec2(width, height)).build_eframe(|_| app);
        harness.run_steps(3);
        let remove = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Remove item").rect();
        let skip = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Skip").rect();
        let canvas = harness.get_by_label("Annotation canvas").rect();
        assert!((remove.center().y - skip.center().y).abs() < 1.0);
        assert!(remove.top() >= canvas.bottom() && remove.bottom() <= height);
        assert!(remove.left() >= 0.0 && remove.right() <= width && remove.height() >= 44.0);
        harness.state_mut().work.migration.busy = true;
        harness.run_steps(2);
        assert!(harness.get_by_label("Remove item").accesskit_node().is_disabled());
        harness.state_mut().work.migration.busy = false;
        harness.run_steps(2);
        harness.get_by_label("Remove item").click();
        harness.run_steps(3);
        assert!(harness.state().work.review_corrections.changes.iter().any(|change| matches!(change,
            labello_domain::ReviewCorrectionChange::Remove { annotation_id, expected_version: 1 }
            if annotation_id == &labello_domain::AnnotationId::from("discovered-object-1"))));
        let overview = harness.state().review_object_targets().len();
        harness.state_mut().navigate_review_item(overview);
        harness.run_steps(3);
        assert!(harness.query_by_label("Remove item").is_none());
    }
    let app = inspector_presets::build(InspectorPreset::MigrationReview, &egui::Context::default());
    assert!(app.migration_review_removal().is_none());
}
