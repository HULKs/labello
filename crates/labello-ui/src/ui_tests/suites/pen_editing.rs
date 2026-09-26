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

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_primary_actions_are_bottom_right_through_placement_and_confirmation() {
    for preset in [InspectorPreset::MigrationObject, InspectorPreset::MigrationFullImage] {
        let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0))
            .build_eframe(|ctx| inspector_presets::build(preset, &ctx.egui_ctx));
        harness.run_steps(4);
        for placing in [false, true] {
            if placing {
                let canvas = harness.get_by_label("Annotation canvas").rect();
                click_at(&mut harness, canvas.left_top() + canvas.size() * 0.1);
                harness.run_steps(4);
                assert!(harness.state().work.migration.draft.is_some());
            }
            for size in [egui::vec2(1440.0, 900.0), egui::vec2(768.0, 1024.0), egui::vec2(390.0, 844.0), egui::vec2(320.0, 568.0), egui::vec2(844.0, 390.0)] {
                harness.set_size(size);
                harness.run_steps(4);
                let compact = LayoutMode::for_width(size.x) == LayoutMode::Compact;
                let label = match (preset, placing, compact) {
                    (InspectorPreset::MigrationObject, _, true) => "Save & next",
                    (InspectorPreset::MigrationObject, _, false) => "Save skeleton & advance",
                    (_, true, true) => "Save object",
                    (_, true, false) => "Save missing object",
                    (_, false, true) => "Confirm & finish",
                    (_, false, false) => "Confirm all guides & finish",
                };
                let primary = harness.get_by_label(label).rect();
                for secondary in ["Skip", "Previous object", "Discard object changes", "Undo last keypoint", "More"] {
                    if let Some(node) = harness.query_by_label(secondary) {
                        assert!(primary.left() >= node.rect().right(),
                            "{preset:?} {size:?} {label}={primary:?} {secondary}={:?}", node.rect());
                    }
                }
                assert!(size.x - primary.right() < 30.0 && size.y - primary.bottom() < 30.0,
                    "{preset:?} {size:?} {label}={primary:?}");
                assert!(primary.right() <= size.x && primary.bottom() <= size.y);
                assert!(primary.top() >= harness.get_by_label("Annotation canvas").rect().bottom());
            }
        }
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
    harness.key_press(egui::Key::H);
    harness.step();
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

#[test]
fn released_keypoint_visibility_toggle_edits_the_point_and_preserves_next_placement() {
    let api = Rc::new(SpyApi::new());
    {
        let mut state = api.state.borrow_mut();
        let task = &mut state.metadata.tasks[0];
        task.annotation_type = AnnotationType::Skeleton;
        task.prelabel_config_ids.clear();
        task.skeleton = Some(SkeletonSpec {
            keypoints: vec![
                KeypointSpec { name: "head".into(), required: true },
                KeypointSpec { name: "tail".into(), required: true },
            ],
            edges: vec![], allow_hidden: true, allow_absent: false,
        });
    }
    let mut harness = loaded_work_harness(api.clone());
    let center = harness.get_by_label("Annotation canvas").rect().center();
    click_at(&mut harness, center);
    harness.run_steps(3);
    let id = harness.state().work.selected_annotation.clone().unwrap();
    let AnnotationGeometry::Skeleton(before) = harness.state().work.annotations[0].geometry.clone() else { panic!() };
    assert!(!harness.state().work.canvas.is_dragging());
    click_accesskit_button(&mut harness, "Set head as occluded");
    let AnnotationGeometry::Skeleton(skeleton) = &harness.state().work.annotations[0].geometry else { panic!() };
    assert_eq!(skeleton.keypoints[0].state, KeypointState::Hidden);
    assert_eq!(skeleton.keypoints[0].point, before.keypoints[0].point);
    assert!(skeleton.keypoints[1].point.is_none());
    assert!(!harness.state().work.next_keypoint_hidden);
    harness.state_mut().undo();
    assert_eq!(harness.state().work.annotations[0].geometry, AnnotationGeometry::Skeleton(before));
    harness.state_mut().redo();
    click_accesskit_button(&mut harness, "Place tail as occluded");
    assert!(harness.state().work.next_keypoint_hidden);
    click_at(&mut harness, center + egui::vec2(70.0, 40.0));
    harness.run_steps(3);
    assert!(harness.state().work.active_skeleton.is_none());
    click_accesskit_button(&mut harness, "Set tail as visible");
    harness.key_press(egui::Key::H);
    harness.step();
    let AnnotationGeometry::Skeleton(skeleton) = &harness.state().work.annotations[0].geometry else { panic!() };
    assert_eq!(skeleton.keypoints[0].state, KeypointState::Hidden);
    assert_eq!(skeleton.keypoints[1].state, KeypointState::Hidden);
    assert!(!harness.state().work.next_keypoint_hidden);
    click_at(&mut harness, center);
    harness.run_steps(3);
    click_accesskit_button(&mut harness, "Set head as visible");
    harness.state_mut().loading.saving = true;
    harness.key_press(egui::Key::H);
    harness.step();
    let AnnotationGeometry::Skeleton(skeleton) = &harness.state().work.annotations[0].geometry else { panic!() };
    assert_eq!(skeleton.keypoints[0].state, KeypointState::Visible);
    harness.state_mut().loading.saving = false;
    harness.state_mut().request_save(false);
    step_until(&mut harness, 12, |app| !app.loading.saving);
    let image_id = &harness.state().work.current.as_ref().unwrap().image.image_id;
    assert_eq!(api.image_state(image_id).current_annotation(&id).unwrap().geometry, harness.state().work.annotations[0].geometry);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_overview_creates_and_selects_objects_on_canvas_preserving_edits_and_retry() {
    use crate::inspector_presets::{self, InspectorPreset};
    for fail_first_save in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
        let task_id = app.work.selected_task_id.clone().unwrap();
        app.work.tasks.iter_mut().find(|task| task.task_id == task_id).unwrap().skeleton = Some(SkeletonSpec {
            keypoints: vec![KeypointSpec { name: "center".into(), required: true }],
            edges: vec![], allow_hidden: true, allow_absent: false,
        });
        api.set_image_state(app.work.current_state.clone().unwrap());
        app.runtime.api = Some(api.clone());
        app.work.inspector_panel_collapsed = true;
        let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_eframe(|_| app);
        harness.run_steps(3);
        assert!(harness.query_by_label_contains("Add missing object").is_none());
        assert!(harness.query_by_label_contains("Edit added").is_none());
        let first = harness.get_by_label("Annotation canvas").rect().center();
        click_at(&mut harness, first);
        harness.key_press(egui::Key::H);
        harness.step();
        let first_draft = harness.state().work.migration.draft.clone().unwrap();
        assert_eq!(first_draft.keypoints[0].state, KeypointState::Hidden);
        api.state.borrow_mut().fail_next_migration = fail_first_save;
        let second = first + egui::vec2(100.0, 80.0);
        click_at(&mut harness, second);
        step_until(&mut harness, 12, |app| !app.work.migration.busy);
        if fail_first_save {
            assert!(harness.state().work.migration.error.is_some());
            assert_eq!(harness.state().work.migration.draft.as_ref(), Some(&first_draft));
            assert!(harness.state().work.migration.pending_overview_intent.is_some());
            harness.state_mut().trigger_migration_primary_action();
            step_until(&mut harness, 12, |app| !app.work.migration.busy);
        }
        assert!(harness.state().work.migration.error.is_none());
        assert_eq!(harness.state().work.migration.keypoint_index, 1);
        assert!(harness.state().work.migration.editing_missing_annotation_id.is_none());
        let first_id = labello_domain::AnnotationId::from("spy-discovered");
        let state = harness.state().work.current_state.as_ref().unwrap();
        assert_eq!(state.current_annotation(&first_id).unwrap().geometry, AnnotationGeometry::Skeleton(first_draft));
        let second_draft = harness.state().work.migration.draft.clone().unwrap();
        assert_ne!(second_draft.keypoints[0].point, state.current_annotation(&first_id).and_then(|annotation| {
            if let AnnotationGeometry::Skeleton(skeleton) = &annotation.geometry { skeleton.keypoints[0].point } else { None }
        }));
        click_at(&mut harness, first);
        step_until(&mut harness, 12, |app| !app.work.migration.busy);
        harness.run_steps(3);
        assert_eq!(harness.state().work.migration.editing_missing_annotation_id.as_ref(), Some(&first_id));
        let second_id = labello_domain::AnnotationId::from("spy-discovered-1");
        assert_eq!(harness.state().work.current_state.as_ref().unwrap().current_annotation(&second_id).unwrap().geometry, AnnotationGeometry::Skeleton(second_draft));
        harness.key_press(egui::Key::H);
        harness.step();
        let moved = first - egui::vec2(30.0, 0.0);
        drag_at(&mut harness, first, moved);
        let edited_first = harness.state().work.migration.draft.clone().unwrap();
        assert_eq!(edited_first.keypoints[0].state, KeypointState::Visible);
        click_at(&mut harness, first + egui::vec2(-100.0, 80.0));
        step_until(&mut harness, 12, |app| !app.work.migration.busy);
        let saved = harness.state().work.current_state.as_ref().unwrap().current_annotation(&first_id).unwrap();
        assert_eq!(saved.version, 2);
        assert_eq!(saved.geometry, AnnotationGeometry::Skeleton(edited_first));
        assert!(harness.state().work.migration.editing_missing_annotation_id.is_none());
        assert_eq!(harness.state().work.migration.keypoint_index, 1);
        assert!(harness.state().work.migration.pending_overview_intent.is_none());
        assert!(matches!(harness.state().work.migration.cursor, Some(labello_domain::MigrationCursor::FullImage)));
        assert_eq!(harness.state().work.assignment.as_ref().unwrap().status, labello_domain::AssignmentStatus::Active);
    }
}
