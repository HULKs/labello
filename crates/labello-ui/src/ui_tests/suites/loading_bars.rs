#[cfg(feature = "inspector-presets")]
mod loading_bars {
    use super::*;
    use crate::inspector_presets::{self, InspectorPreset};

    fn presets() -> [InspectorPreset; 17] {
        [
            InspectorPreset::Annotation,
            InspectorPreset::Review,
            InspectorPreset::ReviewCorrection,
            InspectorPreset::OverlayAnnotation,
            InspectorPreset::OverlayReview,
            InspectorPreset::OverlayCorrection,
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
    fn delayed_navigation_does_not_dim_images_or_show_save_markers() {
        for review in [false, true] {
            let api = Rc::new(SpyApi::new());
            let mut h = if review { loaded_review_harness(api) } else { loaded_work_harness(api) };
            h.run_steps(5);
            let texture = h.state().work.current_texture.as_ref().unwrap().id();
            let scheduled = Rc::new(RefCell::new(Vec::new()));
            let tasks = scheduled.clone();
            h.state_mut().set_native_task_spawner(move |task| tasks.borrow_mut().push(task));
            h.state_mut().skip_assignment();
            h.run_steps(3);
            assert!(!scheduled.borrow().is_empty());
            assert!(h.state().loading.saving);
            let task = h.state().work.selected_task_id.as_ref().unwrap();
            assert_eq!(h.state().workflow_marker_reason(task),
                Some(crate::panels::WorkflowMarkerReason::Transition));
            let meshes: Vec<_> = h.output().shapes.iter().filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh),
                _ => None,
            }).collect();
            assert!(!meshes.is_empty());
            assert!(meshes.iter().all(|mesh| mesh.vertices.iter().all(|vertex| vertex.color == egui::Color32::WHITE)));
            let other = labello_domain::TaskId::from("bounding_box:vehicle");
            h.state_mut().work.availability.tasks.insert(other.clone(), false);
            h.state_mut().work.availability.reasons.insert(other.clone(), labello_domain::WorkflowUnavailableReason::BalanceLimit);
            assert_eq!(h.state().workflow_marker_reason(&other),
                Some(crate::panels::WorkflowMarkerReason::Transition));
            assert_eq!(h.state().work.availability.reasons.get(&other),
                Some(&labello_domain::WorkflowUnavailableReason::BalanceLimit));
            let commands = h.state().runtime.commands.len();
            h.state_mut().trigger_user_action(labello_domain::UserAction::NextImage);
            assert_eq!(h.state().runtime.commands.len(), commands);
        }
    }

    #[test]
    fn loaded_admin_and_export_refresh_without_loading_feedback() {
        let api = Rc::new(SpyApi::new());
        let mut h = loaded_admin_harness(api);
        let scheduled = Rc::new(RefCell::new(Vec::new()));
        let tasks = scheduled.clone();
        h.state_mut().set_native_task_spawner(move |task| tasks.borrow_mut().push(task));
        h.state_mut().request_admin_dataset();
        h.run_steps(3);
        assert!(h.state().admin.refreshing);
        assert!(h.query_by_label("Admin changes saved").is_some());
        assert!(h.query_by_label_contains("Saving or refreshing").is_none());
        let mut h = harness(InspectorPreset::ExportReady, egui::vec2(1440., 1000.));
        h.run_steps(3);
        h.state_mut().admin.export.pending = Some((42, crate::export_flow::ExportAction::Load));
        h.run_steps(3);
        assert!(h.query_by_label("Refreshing export data...").is_none());
        assert!(h.query_by_label("Loading export capabilities and history...").is_none());
    }

    #[test]
    fn open_inspector_drawer_keeps_disabled_controls_during_image_load() {
        let mut h = harness(InspectorPreset::Review, egui::vec2(390., 844.));
        h.state_mut().work.drawer = Some(Drawer::Inspector);
        h.run_steps(4);
        let reset = h.get_by_label("Reset item").rect();
        h.state_mut().retire_current_image();
        h.state_mut().loading.image = true;
        h.run_steps(4);
        assert_eq!(h.get_by_label("Reset item").rect(), reset);
        assert!(h.get_by_label("Reset item").accesskit_node().is_disabled());
        assert!(h.query_by_label("Loading review target…").is_none());
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.state().work.drawer.is_none());
    }

    #[test]
    fn retired_image_stays_visible_without_assignment_ownership() {
        for preset in presets() {
            let mut h = harness(preset, egui::vec2(1440., 1000.));
            h.state_mut().work.inspector_panel_collapsed = false;
            h.state_mut().work.workflow_panel_collapsed = false;
            h.run_steps(4);
            let image = h.state().work.current.as_ref().unwrap().image.image_id.clone();
            let canvas = h.get_by_label("Annotation canvas").rect();
            let inspector_controls: Vec<_> = controls(&h).into_iter().filter(|(_, rect)| rect.left() > canvas.right()).collect();
            h.state_mut().retire_current_image();
            h.state_mut().loading.image = true;
            h.run_steps(4);
            assert!(h.state().work.retired_image, "{preset:?}");
            assert!(h.state().work.assignment.is_none(), "{preset:?}");
            assert_eq!(h.state().work.current.as_ref().unwrap().image.image_id, image);
            assert_eq!(h.get_by_label("Annotation canvas").rect(), canvas, "{preset:?}");
            let retained_controls: Vec<_> = controls(&h).into_iter().filter(|(_, rect)| rect.left() > canvas.right()).collect();
            assert_eq!(retained_controls, inspector_controls, "inspector controls for {preset:?}");
            assert!(h.query_by_label_contains("Loading review target").is_none(), "{preset:?}");
            assert!(h.query_by_label("Loading assignment image").is_none(), "{preset:?}");
            let commands = h.state().runtime.commands.len();
            for action in [labello_domain::UserAction::NextImage, labello_domain::UserAction::PreviousImage,
                labello_domain::UserAction::SaveAnnotations, labello_domain::UserAction::DeleteAnnotation] {
                h.state_mut().trigger_user_action(action);
            }
            assert_eq!(h.state().runtime.commands.len(), commands);
            h.state_mut().loading.image = false;
            h.state_mut().runtime.error = Some("Synthetic image failure".into());
            h.run_steps(4);
            assert!(!h.state().work.retired_image);
            assert!(h.state().work.current.is_none());
            assert!(h.query_by_label("Annotation canvas").is_none());
        }
    }

    #[test]
    fn retained_image_is_cleared_on_scope_change_and_empty_result() {
        for change in 0..9 {
            let mut h = harness(InspectorPreset::Review, egui::vec2(390., 844.));
            h.run_steps(4);
            h.state_mut().retire_current_image();
            h.state_mut().loading.image = true;
            h.run_steps(3);
            match change {
                0 => h.state_mut().view = AppView::Annotate,
                1 => h.state_mut().work.selected_task_id = Some(TaskId::from("other")),
                2 => h.state_mut().config.dataset_id = DatasetId::from("other"),
                3 => h.state_mut().auth_epoch += 1,
                4 => h.state_mut().workspace_epoch += 1,
                5 => h.state_mut().loading.session = true,
                6 => h.state_mut().loading.logout = true,
                7 => h.state_mut().loading.dataset = true,
                _ => h.state_mut().loading.image = false,
            }
            h.run_steps(3);
            assert!(h.state().work.current.is_none(), "scope {change}");
            assert!(!h.state().work.retired_image);
        }
    }

    #[test]
    fn subsequent_image_load_keeps_inspector_context_visible() {
        let mut h = harness(InspectorPreset::Review, egui::vec2(1440., 1000.));
        h.state_mut().work.inspector_panel_collapsed = false;
        h.run_steps(4);
        let context = h.get_by_label_contains("Active review context:").accesskit_node().label().unwrap().to_owned();
        h.state_mut().loading.image = true;
        h.run_steps(4);
        assert!(h.query_by_label("Loading review target…").is_none());
        assert!(h.query_by_label(&context).is_some());
    }

    #[test]
    fn global_header_stays_visible_and_stable_during_loads() {
        for preset in [
            InspectorPreset::Annotation,
            InspectorPreset::Review,
            InspectorPreset::MigrationFullImage,
            InspectorPreset::DatasetInspection,
        ] {
            for size in [egui::vec2(390., 844.), egui::vec2(1440., 1000.)] {
                let mut h = harness(preset, size);
                h.run_steps(4);
                let header_controls = |h: &Harness<'_, LabelloApp>| {
                    h.query_all_by_role(egui::accesskit::Role::Button)
                        .filter(|n| n.rect().top() < 56.0)
                        .map(|n| {
                            (
                                n.accesskit_node().label().unwrap_or_default().to_owned(),
                                n.rect(),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                let before = header_controls(&h);
                assert!(!before.is_empty(), "{preset:?}");
                h.state_mut().clear_current_image();
                for phase in 0..4 {
                    h.state_mut().loading.image = phase == 0;
                    h.state_mut().loading.dataset = phase == 1;
                    h.state_mut().loading.session = phase == 2;
                    h.state_mut().loading.logout = phase == 3;
                    h.run_steps(4);
                    assert_eq!(header_controls(&h), before, "{preset:?} {size:?} {phase}");
                }
            }
        }
        let mut initial = harness(InspectorPreset::ReviewInitialLoad, egui::vec2(390., 844.));
        initial.run_steps(4);
        assert!(initial.query_by_label("Open navigation").is_some());
        assert!(controls(&initial).is_empty());
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
                let texture = h.state().work.current_texture.as_ref().unwrap().id();
                h.state_mut().retire_current_image();
                h.state_mut().loading.image = true;
                h.run_steps(4);
                assert_eq!(controls(&h), before, "{preset:?} at {width}x{height}");
                let image_meshes: Vec<_> = h.output().shapes.iter().filter_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh),
                    _ => None,
                }).collect();
                assert!(!image_meshes.is_empty(), "{preset:?} at {width}x{height}");
                assert!(image_meshes.iter().all(|mesh| mesh.vertices.iter().all(|vertex| vertex.color == egui::Color32::WHITE)),
                    "retained image opacity for {preset:?} at {width}x{height}");
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
            assert!(h.query_by_label("Loading assignment image").is_none());
            let task = h.state().work.selected_task_id.as_ref().unwrap();
            assert_eq!(h.state().workflow_marker_reason(task), Some(crate::panels::WorkflowMarkerReason::ImageLoading));
        }
    }
}
