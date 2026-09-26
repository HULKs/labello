#[test]
fn submit_next_is_the_rightmost_bottom_action_at_supported_widths() {
    for size in [egui::vec2(1440.0, 900.0), egui::vec2(768.0, 1024.0), egui::vec2(390.0, 844.0), egui::vec2(844.0, 390.0)] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        harness.set_size(size);
        harness.run_steps(4);
        let submit = harness.get_by_label("Submit & next").rect();
        let skip = harness.get_by_label("Skip").rect();
        assert!(submit.left() >= skip.right(), "{size:?}: submit {submit:?}, skip {skip:?}");
        if let Some(more) = harness.query_by_label("More actions") { assert!(submit.left() >= more.rect().right()); }
        assert!(size.x - submit.right() < 30.0 && size.y - submit.bottom() < 30.0, "{size:?}: {submit:?}");
    }
}

#[test]
fn placed_annotation_keypoints_can_change_visibility_and_undo_without_reentering_editing() {
    let api = Rc::new(SpyApi::new());
    {
        let mut state = api.state.borrow_mut();
        let task = &mut state.metadata.tasks[0];
        task.annotation_type = AnnotationType::Skeleton;
        task.prelabel_config_ids.clear();
        task.skeleton = Some(SkeletonSpec {
            keypoints: vec![KeypointSpec { name: "center".into(), required: true }],
            edges: vec![], allow_hidden: true, allow_absent: false,
        });
    }
    let mut harness = loaded_work_harness(api.clone());
    let center = harness.get_by_label("Annotation canvas").rect().center();
    click_at(&mut harness, center);
    harness.run_steps(3);
    assert!(harness.state().work.active_skeleton.is_none());
    let id = harness.state().work.selected_annotation.clone().unwrap();
    click_accesskit_button(&mut harness, "center Occluded");
    let geometry = harness.state().work.annotations[0].geometry.clone();
    let AnnotationGeometry::Skeleton(skeleton) = &geometry else { panic!() };
    assert_eq!(skeleton.keypoints[0].state, KeypointState::Hidden);
    assert!(skeleton.keypoints[0].point.is_some());
    assert_eq!(harness.state().work.selected_annotation.as_ref(), Some(&id));
    harness.state_mut().undo();
    let AnnotationGeometry::Skeleton(skeleton) = &harness.state().work.annotations[0].geometry else { panic!() };
    assert_eq!(skeleton.keypoints[0].state, KeypointState::Visible);
    harness.state_mut().redo();
    harness.state_mut().request_save(false);
    step_until(&mut harness, 12, |app| !app.loading.saving);
    assert_eq!(harness.state().work.annotations[0].geometry, geometry);
    let image_id = &harness.state().work.current.as_ref().unwrap().image.image_id;
    assert_eq!(api.image_state(image_id).current_annotation(&id).unwrap().geometry, geometry);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_blank_canvas_starts_missing_pose_and_completed_points_stay_editable() {
    use crate::inspector_presets::{self, InspectorPreset};
    let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
    app.work.inspector_panel_collapsed = true;
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_eframe(|_| app);
    let rect = harness.get_by_label("Annotation canvas").rect();
    let start = rect.center();
    click_at(&mut harness, start);
    assert!(harness.state().work.migration.adding_missing_object);
    assert_eq!(harness.state().work.migration.keypoint_index, 1);
    let count = harness.state().work.migration.draft.as_ref().unwrap().keypoints.len();
    for index in 1..count { click_at(&mut harness, start + egui::vec2(0.0, index as f32 * 25.0)); }
    assert_eq!(harness.state().work.migration.keypoint_index, count);
    let before = harness.state().work.migration.draft.as_ref().unwrap().keypoints[0].point;
    drag_at(&mut harness, start, start - egui::vec2(30.0, 0.0));
    assert_ne!(harness.state().work.migration.draft.as_ref().unwrap().keypoints[0].point, before);
    harness.state_mut().set_migration_keypoint_visibility(0, KeypointState::Hidden);
    assert_eq!(harness.state().work.migration.draft.as_ref().unwrap().keypoints[0].state, KeypointState::Hidden);
    assert!(harness.state().work.migration.draft_dirty);
}

#[test]
fn review_new_box_keeps_editor_for_immediate_move_and_resize() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox { x: 0.1, y: 0.1, width: 0.1, height: 0.1 }), true);
    let mut harness = loaded_review_harness(api);
    harness.state_mut().request_review(labello_domain::ReviewDecision::Approved);
    step_until(&mut harness, 12, |app| app.review_overview() && !app.loading.saving);
    harness.run_steps(3);
    let rect = harness.get_by_label("Annotation canvas").rect();
    let start = rect.center();
    let end = start + egui::vec2(90.0, 70.0);
    drag_at(&mut harness, start, end);
    let draft = harness.state().work.correction_draft.clone().expect("new box remains editable");
    assert_eq!(draft.expected_version, 0);
    drag_at(&mut harness, start + egui::vec2(45.0, 35.0), start + egui::vec2(65.0, 45.0));
    let moved = harness.state().work.correction_draft.as_ref().unwrap().edited_geometry.clone();
    assert_ne!(moved, draft.edited_geometry);
    drag_at(&mut harness, end + egui::vec2(20.0, 10.0), end + egui::vec2(40.0, 30.0));
    assert_ne!(harness.state().work.correction_draft.as_ref().unwrap().edited_geometry, moved);
}
