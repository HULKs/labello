#[test]
fn data_saver_and_original_detail_replace_pixels_without_replacing_edits() {
    use crate::image_quality::Representation;
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    assert!(!harness.state().work.quality.data_saver);
    harness.state_mut().create_bbox(BoundingBox {
        x: 0.2,
        y: 0.3,
        width: 0.2,
        height: 0.2,
    });
    let transform = StoredCanvasTransform {
        zoom: 2.0,
        pan_x: 31.0,
        pan_y: -17.0,
    };
    harness.state_mut().work.canvas.restore_transform(transform);
    let assignment = harness.state().work.assignment.clone();
    let annotations = harness.state().work.annotations.clone();
    let selection = harness.state().work.selected_annotation.clone();
    let generation = harness.state().work.edit_generation;
    let history = harness.state().work.undo_stack.len();
    let profile_start = api.state.borrow().preview_profiles.len();
    click(&mut harness, "Data saver");
    step_until(&mut harness, 24, |app| {
        app.work.quality.loading.is_none() && app.work.queue.len() == 2
    });
    assert_eq!(
        harness.state().work.quality.current,
        Representation::DataSaver
    );
    assert!(
        api.state.borrow().preview_profiles[profile_start..]
            .iter()
            .all(|profile| *profile == labello_client::ImagePreviewProfile::DataSaverV1)
    );
    assert_eq!(api.counts().get_image_preview, 0);
    assert_eq!(api.counts().get_original_detail, 0);
    for representation in [Representation::Original, Representation::DataSaver] {
        harness.state_mut().request_representation(representation);
        step_until(&mut harness, 12, |app| app.work.quality.loading.is_none());
        assert_eq!(harness.state().work.quality.current, representation);
        assert_eq!(harness.state().work.assignment, assignment);
        assert_eq!(harness.state().work.annotations, annotations);
        assert_eq!(harness.state().work.selected_annotation, selection);
        assert_eq!(harness.state().work.edit_generation, generation);
        assert_eq!(harness.state().work.undo_stack.len(), history);
        assert_eq!(harness.state().work.canvas.stored_transform(), transform);
        assert_eq!(harness.state().work.save_status, SaveStatus::Dirty);
    }
    assert_eq!(api.counts().get_original_detail, 1);
}

#[test]
fn failed_data_saver_and_original_detail_keep_drafts_and_require_explicit_retry() {
    use crate::image_quality::Representation;
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    harness.state_mut().create_bbox(BoundingBox {
        x: 0.1,
        y: 0.1,
        width: 0.3,
        height: 0.3,
    });
    let annotations = harness.state().work.annotations.clone();
    api.state.borrow_mut().fail_encoded_previews = true;
    harness.state_mut().set_data_saver(true);
    step_until(&mut harness, 20, |app| app.work.quality.error.is_some());
    assert_eq!(api.counts().get_image_preview, 0);
    assert_eq!(api.counts().get_original_detail, 0);
    assert!(harness.query_by_label("Image unavailable").is_some());
    assert!(harness.query_by_label("Retry image").is_some());
    api.state.borrow_mut().fail_original_detail = true;
    click(&mut harness, "Load original detail");
    step_until(&mut harness, 12, |app| app.work.quality.loading.is_none());
    assert_eq!(api.counts().get_original_detail, 1);
    assert!(harness.state().work.quality.error.is_some());
    assert_eq!(harness.state().work.annotations, annotations);
    api.state.borrow_mut().fail_original_detail = false;
    click(&mut harness, "Retry image");
    step_until(&mut harness, 12, |app| app.work.quality.loading.is_none());
    assert_eq!(
        harness.state().work.quality.current,
        Representation::Original
    );
    assert_eq!(api.counts().get_original_detail, 2);
    assert_eq!(harness.state().work.annotations, annotations);
    assert_eq!(harness.state().work.save_status, SaveStatus::Dirty);
}

#[test]
fn superseded_queued_detail_never_fetches_original_and_context_reset_clears_policy() {
    use crate::image_quality::Representation;
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    harness
        .state_mut()
        .request_representation(Representation::Original);
    harness.state_mut().set_data_saver(true);
    step_until(&mut harness, 20, |app| app.work.quality.loading.is_none());
    assert_eq!(api.counts().get_original_detail, 0);
    assert_eq!(
        harness.state().work.quality.current,
        Representation::DataSaver
    );
    harness.state_mut().isolate_browser_workspace();
    assert!(!harness.state().work.quality.data_saver);
    assert_eq!(
        harness.state().work.quality.current,
        Representation::Standard
    );
}

#[test]
fn original_detail_is_temporary_for_one_image_visit() {
    use crate::image_quality::Representation;
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    harness.state_mut().set_data_saver(true);
    step_until(&mut harness, 20, |app| {
        app.work.quality.loading.is_none() && app.work.queue.len() == 2
    });
    harness
        .state_mut()
        .request_representation(Representation::Original);
    step_until(&mut harness, 12, |app| app.work.quality.loading.is_none());
    let original = harness
        .state()
        .work
        .assignment
        .as_ref()
        .unwrap()
        .image_id
        .clone();
    click(&mut harness, "Skip");
    step_until(&mut harness, 20, |app| {
        app.work
            .assignment
            .as_ref()
            .is_some_and(|assignment| assignment.image_id != original)
    });
    assert_eq!(
        harness.state().work.quality.current,
        Representation::DataSaver
    );
    assert!(harness.state().work.quality.data_saver);
    assert_eq!(api.counts().get_original_detail, 1);
}

#[test]
fn image_quality_controls_remain_reachable_in_mobile_and_short_layouts() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    for (width, height) in [
        (320.0, 568.0),
        (390.0, 844.0),
        (600.0, 800.0),
        (1288.0, 820.0),
        (1440.0, 1000.0),
    ] {
        harness.set_size(egui::vec2(width, height));
        harness.run_steps(3);
        let checkbox = harness
            .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Data saver")
            .rect();
        assert!(checkbox.height() >= 43.0);
        assert!(checkbox.left() >= 0.0 && checkbox.right() <= width);
        if width < 1288.0 {
            click(&mut harness, "Image quality");
            assert_control_inside(
                &harness,
                "Load original detail",
                egui::accesskit::Role::Button,
                width,
                height,
            );
            harness.key_press(egui::Key::Escape);
            harness.step();
        } else {
            assert_control_inside(
                &harness,
                "Load original detail",
                egui::accesskit::Role::Button,
                width,
                height,
            );
        }
    }
    harness.set_size(egui::vec2(320.0, 320.0));
    harness.run_steps(3);
    assert!(harness.get_by_label("Annotation canvas").rect().height() >= 80.0);
    click_accesskit_button(&mut harness, "Image quality settings");
    harness.run_steps(3);
    assert!(harness.query_by_label("Data saver").is_some());
    click(&mut harness, "Image quality");
    assert_control_inside(
        &harness,
        "Load original detail",
        egui::accesskit::Role::Button,
        320.0,
        320.0,
    );
}

#[test]
fn representation_replies_cannot_cross_assignment_or_auth_ownership() {
    use crate::app::LoadedImage;
    use crate::image_quality::Representation;
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let app = harness.state_mut();
    let loaded = || LoadedImage {
        representation: Representation::Original,
        assignment: app.work.assignment.clone().unwrap(),
        queued: app.work.current.clone().unwrap(),
        annotations: Vec::new(),
        state: app.work.current_state.clone().unwrap(),
        color_image: None,
    };
    let stale_auth = loaded();
    let mut stale_assignment = loaded();
    stale_assignment.assignment.assignment_id = "superseded-assignment".into();
    let texture = app.work.current_texture.as_ref().unwrap().id();
    app.request_representation(Representation::Original);
    let request = app.runtime.commands.back().unwrap().request().clone();
    let operation_id = request.request_id;
    app.apply_representation(
        &egui::Context::default(),
        operation_id,
        Ok(stale_assignment),
    );
    assert_eq!(app.work.quality.current, Representation::Standard);
    assert_eq!(app.work.current_texture.as_ref().unwrap().id(), texture);
    app.begin_auth_epoch();
    app.runtime
        .tx
        .send(UiMessage::RepresentationLoaded {
            request,
            operation_id,
            result: Box::new(Ok(stale_auth)),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.work.quality.current, Representation::Standard);
    assert_eq!(app.work.current_texture.as_ref().unwrap().id(), texture);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn representation_replies_preserve_review_corrections_and_migration_drafts() {
    use crate::app::LoadedImage;
    use crate::image_quality::Representation;
    use crate::inspector_presets::{self, InspectorPreset};
    let ctx = egui::Context::default();
    for preset in [
        InspectorPreset::ReviewCorrection,
        InspectorPreset::MigrationObject,
    ] {
        let mut app = inspector_presets::build(preset, &ctx);
        app.work.migration.draft_dirty = true;
        let correction = app.work.correction_draft.clone();
        let migration = app.work.migration.clone();
        let annotations = app.work.annotations.clone();
        let assignment = app.work.assignment.clone();
        for representation in [Representation::DataSaver, Representation::Original] {
            app.work.quality.loading = Some(123);
            let loaded = LoadedImage {
                representation,
                assignment: app.work.assignment.clone().unwrap(),
                queued: app.work.current.clone().unwrap(),
                annotations: Vec::new(),
                state: app.work.current_state.clone().unwrap(),
                color_image: Some(egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE])),
            };
            app.apply_representation(&ctx, 123, Ok(loaded));
            assert_eq!(app.work.quality.current, representation);
            assert_eq!(app.work.correction_draft, correction);
            assert_eq!(app.work.migration.draft, migration.draft);
            assert_eq!(app.work.migration.draft_group, migration.draft_group);
            assert_eq!(app.work.migration.cursor, migration.cursor);
            assert_eq!(app.work.migration.active_pass_id, migration.active_pass_id);
            assert!(app.work.migration.draft_dirty);
            assert_eq!(app.work.annotations, annotations);
            assert_eq!(app.work.assignment, assignment);
        }
    }
}

#[test]
fn superseded_image_command_wakes_the_next_queued_transfer_without_input() {
    use crate::image_quality::Representation;
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let app = harness.state_mut();
    app.runtime.commands.clear();
    app.request_representation(Representation::Original);
    app.request_representation(Representation::DataSaver);
    let ctx = egui::Context::default();
    for _ in 0..5 {
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
    }
    let idle = ctx.run_ui(egui::RawInput::default(), |_| {});
    assert!(
        idle.viewport_output[&egui::ViewportId::ROOT].repaint_delay > std::time::Duration::ZERO
    );
    app.runtime.repaint_ctx = Some(ctx.clone());
    let output = ctx.run_ui(egui::RawInput::default(), |_| app.start_next_command());
    assert_eq!(app.runtime.commands.len(), 1);
    assert_eq!(
        output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
        std::time::Duration::ZERO
    );
}
