#[test]
fn annotation_and_review_load_prefetch_and_retry_only_data_saver() {
    for review in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut harness = if review {
            loaded_review_harness(api.clone())
        } else {
            loaded_work_harness(api.clone())
        };
        step_until(&mut harness, 20, |app| app.work.queue.len() == 2);
        let before = api.counts().get_encoded_image_preview;
        assert!(before >= 3);
        harness.state_mut().retry_assignment_load();
        step_until(&mut harness, 20, |app| !app.loading.image);
        assert!(api.counts().get_encoded_image_preview > before);
        assert!(api.state.borrow().preview_profiles.iter().all(|profile|
            *profile == labello_client::ImagePreviewProfile::DataSaverV1));
        assert_eq!(api.counts().get_image_preview, 0);
        assert_eq!(api.counts().get_original_detail, 0);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn working_views_and_settings_have_no_image_quality_selection() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [InspectorPreset::Annotation, InspectorPreset::Review,
        InspectorPreset::ReviewCorrection, InspectorPreset::MigrationObject] {
        for (width, height) in viewport_sizes().into_iter().chain([(320.0, 320.0)]) {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .build_eframe(|ctx| inspector_presets::build(preset, &ctx.egui_ctx));
            harness.run();
            for settings in [false, true] {
                if settings {
                    harness.state_mut().open_shortcut_settings();
                    harness.run();
                    assert!(harness.query_by_label("Keyboard shortcuts").is_some());
                }
                for label in ["Data saver", "Data saver active", "Image quality",
                    "Image quality settings", "Load original detail", "Use selected preview",
                    "Original detail", "Standard detail"] {
                    assert!(harness.query_by_label(label).is_none(), "{preset:?} {width}x{height}: {label}");
                }
            }
        }
    }
}
