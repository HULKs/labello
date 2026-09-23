#[cfg(feature = "inspector-presets")]
mod loading_bars {
    use super::*;
    use crate::inspector_presets::{self, InspectorPreset};

    fn presets() -> [InspectorPreset; 14] {
        [
            InspectorPreset::Annotation,
            InspectorPreset::Review,
            InspectorPreset::ReviewCorrection,
            InspectorPreset::MigrationObject,
            InspectorPreset::MigrationSingleOptional,
            InspectorPreset::MigrationExclusion,
            InspectorPreset::MigrationPass,
            InspectorPreset::MigrationFullImage,
            InspectorPreset::MigrationReview,
            InspectorPreset::MigrationDiscovery,
            InspectorPreset::MigrationDiscoveryReview,
            InspectorPreset::MigrationAnnotatedEdit,
            InspectorPreset::MigrationGuideDeleted,
            InspectorPreset::MigrationCompanionAnnotation,
        ]
    }

    fn harness(preset: InspectorPreset, size: egui::Vec2) -> Harness<'static, LabelloApp> {
        let mut app = inspector_presets::build(preset, &egui::Context::default());
        app.work.inspector_panel_collapsed = true;
        app.work.workflow_panel_collapsed = true;
        Harness::builder().with_size(size).build_eframe(|_| app)
    }

    fn controls(h: &Harness<'_, LabelloApp>) -> Vec<(String, egui::Rect)> {
        h.query_all_by_role(egui::accesskit::Role::Button)
            .filter(|n| n.rect().top() >= 56.0)
            .map(|n| {
                (
                    n.accesskit_node().label().unwrap_or_default().to_owned(),
                    n.rect(),
                )
            })
            .collect()
    }

    fn bar_rects(h: &Harness<'_, LabelloApp>) -> [egui::Rect; 2] {
        ["workspace_context", "compact_primary_actions"].map(|id| {
            egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id))
                .unwrap()
                .outer_rect
        })
    }

    #[test]
    fn blank_bars_reserve_loaded_geometry_including_frame_margins() {
        for preset in [
            InspectorPreset::Annotation,
            InspectorPreset::Review,
            InspectorPreset::MigrationObject,
            InspectorPreset::MigrationFullImage,
        ] {
            for size in [
                egui::vec2(390., 844.),
                egui::vec2(320., 320.),
                egui::vec2(1440., 1000.),
            ] {
                let mut h = harness(preset, size);
                h.state_mut().clear_current_image();
                h.state_mut().loading.image = true;
                h.run_steps(4);
                let initial = bar_rects(&h);
                let mut ready = inspector_presets::build(preset, &h.ctx);
                ready.work.inspector_panel_collapsed = true;
                ready.work.workflow_panel_collapsed = true;
                *h.state_mut() = ready;
                h.run_steps(4);
                assert_eq!(initial, bar_rects(&h), "{preset:?} {size:?}");
            }
        }
    }

    #[test]
    fn loading_bars_reflow_long_content_and_large_text_without_changing_controls() {
        for preset in [InspectorPreset::Review, InspectorPreset::MigrationFullImage] {
            let mut h = harness(preset, egui::vec2(390., 844.));
            for task in &mut h.state_mut().work.tasks {
                task.name = "A deliberately long workflow title for loading transitions".into();
            }
            h.ctx.global_style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(36.));
            });
            h.run_steps(4);
            let before = controls(&h);
            let rects = bar_rects(&h);
            h.state_mut().clear_current_image();
            h.state_mut().loading.image = true;
            h.run_steps(4);
            assert_eq!(controls(&h), before);
            assert_eq!(bar_rects(&h), rects);
            h.set_size(egui::vec2(600., 800.));
            h.run_steps(4);
            assert!(!controls(&h).is_empty());
            assert!(h.query_by_label("Loading review target…").is_none());
        }
    }

    #[test]
    fn image_loading_retains_bar_controls_and_geometry_across_workflows() {
        for preset in presets() {
            for (width, height) in [
                (320., 568.),
                (390., 844.),
                (600., 800.),
                (1288., 820.),
                (1440., 1000.),
                (320., 320.),
            ] {
                let mut h = harness(preset, egui::vec2(width, height));
                h.run_steps(4);
                let before = controls(&h);
                assert!(!before.is_empty(), "{preset:?}");
                // The production next-image path clears all assignment and migration state.
                h.state_mut().clear_current_image();
                h.state_mut().loading.image = true;
                h.run_steps(4);
                assert_eq!(controls(&h), before, "{preset:?} at {width}x{height}");
                for node in h
                    .query_all_by_role(egui::accesskit::Role::Button)
                    .filter(|n| n.rect().top() >= 56.0)
                {
                    assert!(
                        node.accesskit_node().is_disabled(),
                        "retained control must be disabled: {:?}",
                        node.accesskit_node().label()
                    );
                }
                assert!(h.query_by_label_contains("Loading review target").is_none());
                assert!(h.query_by_label("Loading assignment...").is_none());
                let command_count = h.state().runtime.commands.len();
                for action in [
                    labello_domain::UserAction::NextImage,
                    labello_domain::UserAction::PreviousImage,
                    labello_domain::UserAction::SelectPreviousObject,
                    labello_domain::UserAction::SaveAnnotations,
                ] {
                    h.state_mut().trigger_user_action(action);
                }
                assert_eq!(h.state().runtime.commands.len(), command_count);
                assert!(h.state().work.assignment.is_none());
            }
        }
    }

    #[test]
    fn first_load_and_changed_scope_have_blank_bars() {
        for preset in presets() {
            let mut app = inspector_presets::build(preset, &egui::Context::default());
            app.clear_current_image();
            app.loading.image = true;
            app.work.inspector_panel_collapsed = true;
            app.work.workflow_panel_collapsed = true;
            let mut h = Harness::builder()
                .with_size(egui::vec2(390., 844.))
                .build_eframe(|_| app);
            h.run_steps(4);
            assert!(
                controls(&h).is_empty(),
                "initial {preset:?}: {:?}",
                controls(&h)
            );
            assert!(h.query_by_label_contains("Loading review target").is_none());
            assert!(h.query_by_label("Loading assignment...").is_none());
        }
        for change in 0..5 {
            let mut h = harness(InspectorPreset::Review, egui::vec2(390., 844.));
            h.run_steps(4);
            h.state_mut().clear_current_image();
            h.state_mut().loading.image = true;
            match change {
                0 => h.state_mut().view = AppView::Annotate,
                1 => h.state_mut().work.selected_task_id = Some(TaskId::from("another-task")),
                2 => h.state_mut().config.dataset_id = DatasetId::from("another-dataset"),
                3 => h.state_mut().auth_epoch += 1,
                _ => h.state_mut().workspace_epoch += 1,
            }
            h.run_steps(4);
            assert!(
                controls(&h).is_empty(),
                "scope change {change}: {:?}",
                controls(&h)
            );
        }
    }

    #[test]
    fn empty_and_failed_loads_drop_retained_bars_and_retry_starts_blank() {
        for failed in [false, true] {
            let mut h = harness(InspectorPreset::Review, egui::vec2(390., 844.));
            h.run_steps(4);
            h.state_mut().clear_current_image();
            h.state_mut().loading.image = true;
            h.run_steps(3);
            assert!(h.query_by_label("Approve").is_some());
            h.state_mut().loading.image = false;
            if failed {
                h.state_mut().runtime.error = Some("Image unavailable".into());
            }
            h.run_steps(3);
            assert!(h.query_by_label("Approve").is_none());
            assert!(
                h.query_by_label_contains("Review details: Workflow:")
                    .is_none()
            );
            h.state_mut().loading.image = true;
            h.run_steps(3);
            assert!(controls(&h).is_empty());
        }
    }
}
