#[test]
fn statistics_overlay_preserves_annotation_and_review_work_and_restores_focus() {
    for review in [false, true] {
        for compact in [false, true] {
            let api = Rc::new(SpyApi::new());
            let mut harness = if review {
                loaded_review_harness(api.clone())
            } else {
                loaded_work_harness(api.clone())
            };
            if compact {
                harness.set_size(egui::vec2(390.0, 844.0));
                harness.step();
            }
            for _ in 0..10 {
                harness.step();
            }
            let assignment = harness.state().work.assignment.clone();
            let annotations = harness.state().work.annotations.clone();
            let selected = harness.state().work.selected_annotation.clone();
            let view = harness.state().view;
            let epoch = harness.state().workspace_epoch;
            let counts = api.counts();
            let transform = harness.state().work.canvas.stored_transform();
            click_application_menu_item(&mut harness, "Statistics");
            step_until(&mut harness, 10, |app| !app.loading.stats);
            assert!(harness.state().navigation.statistics.open);
            let dialog =
                harness.get_by_role_and_label(egui::accesskit::Role::Window, "Dataset statistics");
            assert!(dialog.accesskit_node().is_modal());
            for _ in 0..6 {
                harness.key_press(egui::Key::Tab);
                harness.step();
                assert!(
                    harness.get_by_label("Close statistics").is_focused()
                        || harness.get_by_label("Refresh now").is_focused()
                );
            }
            assert!(
                harness
                    .query_by_label("Switch active assignment?")
                    .is_none()
            );
            for key in [egui::Key::Delete, egui::Key::N, egui::Key::Y] {
                harness.key_press(key);
                harness.step();
            }
            assert_eq!(harness.state().work.annotations, annotations);
            harness.key_press(egui::Key::Escape);
            harness.step();
            harness.step();
            assert!(!harness.state().navigation.statistics.open);
            assert_eq!(harness.state().view, view);
            assert_eq!(harness.state().workspace_epoch, epoch);
            assert_eq!(harness.state().work.assignment, assignment);
            assert_eq!(harness.state().work.selected_annotation, selected);
            assert_eq!(harness.state().work.canvas.stored_transform(), transform);
            assert_eq!(api.counts().release_assignment, counts.release_assignment);
            assert_eq!(api.counts().assign_next_image, counts.assign_next_image);
            assert_eq!(api.counts().annotation_batch, counts.annotation_batch);
            assert_eq!(api.counts().record_review, counts.record_review);
            assert!(
                harness
                    .get_by_role_and_label(egui::accesskit::Role::Button, "Statistics")
                    .is_focused(),
                "review={review}, compact={compact}, invoker={:?}",
                harness.state().navigation.statistics.invoker
            );
        }
    }
}

#[test]
fn statistics_overlay_accepts_inflight_save_without_invalidating_assignment_requests() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let assignment = harness.state().work.assignment.clone().unwrap();
    let state = harness.state().work.current_state.clone().unwrap();
    harness.state_mut().work.active_operation_id = Some(77);
    harness.state_mut().loading.saving = true;
    harness.state_mut().runtime.active_requests.insert(77);
    let request = test_request(harness.state(), 77, Some("demo"));
    harness.state_mut().open_view(AppView::Stats);
    assert!(harness.state().runtime.active_requests.contains(&77));
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::SaveFinished {
            request,
            operation_id: 77,
            assignment_id: assignment.assignment_id,
            edit_generation: 0,
            completed: false,
            result: Box::new(Ok(state)),
        })
        .unwrap();
    harness.step();
    assert!(harness.state().navigation.statistics.open);
    assert!(!harness.state().loading.saving);
    assert_eq!(harness.state().work.active_operation_id, None);
    assert_eq!(harness.state().work.save_status, SaveStatus::Saved);
}

#[test]
fn statistics_overlay_closes_on_invalidated_assignment_or_authentication() {
    for authentication in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut harness = loaded_work_harness(api);
        harness.state_mut().open_view(AppView::Stats);
        if authentication {
            harness.state_mut().begin_auth_epoch();
        } else {
            harness.state_mut().work.assignment = None;
        }
        harness.step();
        assert!(!harness.state().navigation.statistics.open);
        assert!(harness.query_by_label("Dataset statistics").is_none());
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn statistics_overlay_preserves_unsaved_migration_input_and_pending_revisit() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [
        InspectorPreset::MigrationObject,
        InspectorPreset::MigrationFullImage,
    ] {
        let app = inspector_presets::build(preset, &egui::Context::default());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.state_mut().work.migration.draft_dirty = true;
        harness.state_mut().work.migration.draft = Some(SkeletonGeometry {
            keypoints: vec![KeypointAnnotation {
                name: "head".into(),
                state: KeypointState::Visible,
                point: Some(NormalizedPoint { x: 0.4, y: 0.3 }),
            }],
        });
        harness.state_mut().work.migration.pending_revisit_target =
            Some(labello_domain::ObjectGroupId::from("group-left"));
        let draft = harness.state().work.migration.draft.clone();
        let cursor = harness.state().work.migration.cursor.clone();
        let epoch = harness.state().workspace_epoch;
        harness.state_mut().open_view(AppView::Stats);
        harness.step();
        assert!(harness.query_by_label("Dataset statistics").is_some());
        harness.key_press(egui::Key::Escape);
        harness.step();
        assert_eq!(harness.state().work.migration.draft, draft);
        assert_eq!(harness.state().work.migration.cursor, cursor);
        assert!(
            harness
                .state()
                .work
                .migration
                .pending_revisit_target
                .is_some()
        );
        assert!(harness.state().work.migration.draft_dirty);
        assert_eq!(harness.state().workspace_epoch, epoch);
    }
}

#[test]
fn statistics_overlay_remote_states_and_close_fit_supported_viewports() {
    for (width, height) in [
        (320.0, 568.0),
        (390.0, 844.0),
        (600.0, 800.0),
        (1288.0, 820.0),
        (1440.0, 1000.0),
        (320.0, 320.0),
    ] {
        let mut app = LabelloApp::default();
        app.datasets.last_stats_completion = None;
        app.open_statistics();
        app.loading.stats = true;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, height))
            .build_eframe(|_| app);
        for _ in 0..4 {
            harness.step();
        }
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
        let modal = harness
            .get_by_role_and_label(egui::accesskit::Role::Window, "Dataset statistics")
            .rect();
        assert!(
            viewport.expand(1.0).contains_rect(modal),
            "statistics modal outside {width}x{height}: {modal:?}"
        );
        let close = harness.get_by_label("Close statistics").rect();
        assert!(close.height() >= 44.0 && viewport.contains_rect(close));
        assert!(harness.query_by_label("Loading statistics...").is_some());
        harness.state_mut().loading.stats = false;
        harness.state_mut().datasets.stats_error = Some("Service unavailable".into());
        harness.step();
        assert!(harness.query_by_label("Retry statistics").is_some());
        harness.state_mut().datasets.stats = stats(12);
        harness.state_mut().datasets.last_stats_completion = Some(Instant::now());
        harness.step();
        assert!(harness.query_by_label("Metric Images").is_some());
        assert!(
            harness
                .query_by_label("Statistics may be stale. Last refresh failed: Service unavailable")
                .is_some()
        );
        harness.state_mut().datasets.stats_error = None;
        harness.state_mut().loading.stats = true;
        harness.step();
        assert!(harness.query_by_label("Refreshing statistics").is_some());
        assert!(harness.query_by_label("Metric Images").is_some());
        harness.state_mut().loading.stats = false;
        harness.state_mut().datasets.stats = DatasetStats::default();
        harness.step();
        assert!(harness.query_by_label("No enabled tasks").is_some());
        harness.state_mut().runtime.error =
            Some("The workflow request could not complete. ".repeat(12));
        for _ in 0..4 {
            harness.step();
        }
        assert!(
            viewport.expand(1.0).contains_rect(
                harness
                    .get_by_role_and_label(egui::accesskit::Role::Window, "Dataset statistics")
                    .rect()
            )
        );
        for _ in 0..4 {
            harness.step();
        }
        click(&mut harness, "Close statistics");
        assert!(
            !harness.state().navigation.statistics.open,
            "close at {width}x{height}"
        );
    }
}

#[test]
fn statistics_overlay_refresh_coalesces_during_image_load_and_rejects_old_auth_response() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api.clone());
    app.setup.started = true;
    app.datasets.metadata = Some(api.metadata());
    app.loading.image = true;
    app.open_statistics();
    let request = app
        .runtime
        .commands
        .iter()
        .find_map(|command| match command {
            UiCommand::Stats { request, .. } => Some(request.clone()),
            _ => None,
        })
        .expect("stats refresh can run during image loading");
    app.request_stats();
    app.refresh_stats_if_due();
    assert_eq!(
        app.runtime
            .commands
            .iter()
            .filter(|command| matches!(command, UiCommand::Stats { .. }))
            .count(),
        1
    );
    assert!(app.loading.image);
    app.begin_auth_epoch();
    app.runtime
        .tx
        .send(UiMessage::StatsLoaded {
            request,
            result: Ok(stats(99)),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.datasets.stats.total_images, 0);
    assert!(!app.navigation.statistics.open);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn statistics_overlay_keeps_an_unsaved_annotation_and_its_selection() {
    use crate::inspector_presets::{self, InspectorPreset};
    let mut app = inspector_presets::build(InspectorPreset::Annotation, &egui::Context::default());
    app.create_bbox(BoundingBox {
        x: 0.25,
        y: 0.3,
        width: 0.2,
        height: 0.2,
    });
    let annotations = app.work.annotations.clone();
    let selected = app.work.selected_annotation.clone();
    let assignment = app.work.assignment.clone();
    let queue_len = app.work.queue.len();
    assert_eq!(app.work.save_status, SaveStatus::Dirty);
    app.open_view(AppView::Stats);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1288.0, 820.0))
        .build_eframe(|_| app);
    for _ in 0..4 {
        harness.step();
    }
    harness.key_press(egui::Key::Escape);
    for _ in 0..3 {
        harness.step();
    }
    assert!(!harness.state().navigation.statistics.open);
    assert_eq!(harness.state().work.annotations, annotations);
    assert_eq!(harness.state().work.selected_annotation, selected);
    assert_eq!(harness.state().work.assignment, assignment);
    assert_eq!(harness.state().work.queue.len(), queue_len);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn statistics_overlay_cannot_mask_companion_reconciliation_and_resumes_after_cancel() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [egui::vec2(1440.0, 1000.0), egui::vec2(320.0, 320.0)] {
        let mut harness = Harness::builder().with_size(size).build_eframe(|ctx| {
            inspector_presets::build(InspectorPreset::MigrationDiscovery, &ctx.egui_ctx)
        });
        harness.run_steps(3);
        let assignment = harness.state().work.assignment.clone();
        let annotations = harness.state().work.annotations.clone();
        let cursor = harness.state().work.migration.cursor.clone();
        let epoch = harness.state().workspace_epoch;
        harness.state_mut().open_view(AppView::Stats);
        harness
            .state_mut()
            .work
            .migration
            .pending_companion_reconciliation =
            Some(labello_domain::AnnotationId::from("discovered-object-1"));
        harness.run_steps(3);
        assert!(harness.state().navigation.statistics.open);
        assert!(harness.query_by_label("Reconcile companion box?").is_some());
        assert!(harness.query_by_label("Dataset statistics").is_none());
        harness.key_press(egui::Key::ArrowRight);
        harness.run_steps(2);
        assert!(!harness.state().work.migration.busy);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(3);
        assert!(
            harness
                .state()
                .work
                .migration
                .pending_companion_reconciliation
                .is_none()
        );
        assert!(harness.query_by_label("Dataset statistics").is_some());
        assert!(harness.state().navigation.statistics.open);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(3);
        assert!(!harness.state().navigation.statistics.open);
        assert_eq!(harness.state().work.assignment, assignment);
        assert_eq!(harness.state().work.annotations, annotations);
        assert_eq!(harness.state().work.migration.cursor, cursor);
        assert_eq!(harness.state().workspace_epoch, epoch);
    }
}

#[test]
fn statistics_overlay_resizes_using_immediate_repaints_without_waiting_for_refresh() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    harness.set_size(egui::vec2(1440.0, 1000.0));
    harness.state_mut().open_view(AppView::Stats);
    harness.run();
    let assignment = harness.state().work.assignment.clone();
    let annotations = harness.state().work.annotations.clone();
    let epoch = harness.state().workspace_epoch;
    let counts = api.counts();
    for size in [
        egui::vec2(320.0, 320.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0),
        egui::vec2(1288.0, 820.0),
        egui::vec2(1440.0, 1000.0),
    ] {
        harness.set_size(size);
        // Honor immediate repaint requests, without forcing later timer/input frames.
        harness.run();
        let rect = harness.get_by_label("Dataset statistics").rect();
        assert!(
            rect.left() >= -0.5
                && rect.top() >= -0.5
                && rect.right() <= size.x + 0.5
                && rect.bottom() <= size.y + 0.5,
            "statistics did not settle inside {size:?}: {rect:?}"
        );
        assert_control_inside(
            &harness,
            "Close statistics",
            egui::accesskit::Role::Button,
            size.x,
            size.y,
        );
        assert_eq!(harness.state().work.assignment, assignment);
        assert_eq!(harness.state().work.annotations, annotations);
        assert_eq!(harness.state().workspace_epoch, epoch);
    }
    assert_eq!(api.counts().release_assignment, counts.release_assignment);
    assert_eq!(api.counts().record_review, counts.record_review);
}
