#[test]
fn overlapping_boxes_are_absent_from_review_sequence_context_and_keyboard_selection() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox { x: 0.2, y: 0.2, width: 0.3, height: 0.3 }), true);
    let mut harness = loaded_review_harness(api);
    {
        let app = harness.state_mut();
        let mut duplicate = app.work.annotations[0].clone();
        duplicate.annotation_id = "zz_duplicate".into();
        app.work.annotations.push(duplicate.clone());
        let state = app.work.current_state.as_mut().unwrap();
        state.bounding_box_visibility = Some(Default::default());
        state.annotations.insert(duplicate.annotation_id.clone(), vec![duplicate]);
    }
    for size in [egui::vec2(1440.0, 1000.0), egui::vec2(390.0, 844.0), egui::vec2(320.0, 320.0)] {
        harness.set_size(size);
        harness.run_steps(4);
        assert_eq!(harness.state().work.annotations.len(), 2);
        assert_eq!(harness.state().annotation_objects().len(), 1);
        assert_eq!(harness.state().review_object_targets().len(), 1);
        assert!(harness.get_by_label_contains("Review details: Workflow:").accesskit_node().label().unwrap().contains("Object 1 of 1"));
    }
    let app = harness.state_mut();
    app.work.review_index = 1;
    app.sync_review_selection();
    assert!(app.review_overview());
    assert!(app.current_review_annotation().is_none());
    app.view = AppView::Annotate;
    app.work.selected_annotation = Some("zz_duplicate".into());
    app.sync_prelabel_review();
    assert_ne!(app.work.selected_annotation, Some("zz_duplicate".into()));
    app.trigger_user_action(labello_domain::UserAction::SelectNextObject);
    assert_ne!(app.work.selected_annotation, Some("zz_duplicate".into()));
    assert_eq!(app.work.annotations.len(), 2);
}

#[test]
fn overlapping_boxes_recompute_after_draft_geometry_changes_without_deleting_drafts() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox { x: 0.1, y: 0.1, width: 0.2, height: 0.2 }), true);
    let mut harness = loaded_review_harness(api);
    let app = harness.state_mut();
    app.view = AppView::Annotate;
    app.work.current_state.as_mut().unwrap().bounding_box_visibility = Some(Default::default());
    let mut duplicate = app.work.annotations[0].clone();
    duplicate.annotation_id = "zz_duplicate".into();
    app.work.annotations.push(duplicate);
    assert_eq!(app.annotation_objects().len(), 1);
    app.work.annotations[0].geometry = AnnotationGeometry::BoundingBox(BoundingBox { x: 0.7, y: 0.7, width: 0.2, height: 0.2 });
    assert_eq!(app.annotation_objects().len(), 2);
    assert!(app.work.annotations.iter().all(|annotation| !annotation.deleted));
    app.work.current_state.as_mut().unwrap().bounding_box_visibility.as_mut().unwrap().iou_threshold = 1.0;
    app.work.annotations[0].geometry = app.work.annotations[1].geometry.clone();
    assert_eq!(app.annotation_objects().len(), 2);
}

#[test]
fn overlapping_boxes_threshold_control_is_labelled_and_bounded() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_admin_harness(api);
    harness.state_mut().admin.section = AdminSection::Automation;
    for size in [egui::vec2(1440.0, 1000.0), egui::vec2(390.0, 844.0)] {
        harness.set_size(size);
        harness.run_steps(4);
        let control = harness.get_by_role_and_label(egui::accesskit::Role::SpinButton, "IoU threshold");
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(control.rect()));
    }
    assert_eq!(harness.state().datasets.admin_config.as_ref().unwrap().bounding_box_visibility.iou_threshold, 0.9);
}

#[test]
fn overlapping_boxes_do_not_require_hidden_companion_guides() {
    let (api, id) = source_guide_api();
    let mut harness = loaded_work_harness(api);
    let app = harness.state_mut();
    let mut retained = app.work.annotations.iter().find(|a| a.annotation_id == id).unwrap().clone();
    retained.annotation_id = "000_retained".into();
    retained.revision_source = RevisionSource::Human { action: labello_domain::HumanRevisionKind::Authored };
    app.work.annotations.push(retained.clone());
    let state = app.work.current_state.as_mut().unwrap();
    state.bounding_box_visibility = Some(Default::default());
    state.annotations.insert(retained.annotation_id.clone(), vec![retained]);
    app.sync_prelabel_review();
    let hidden = app.work.annotations.iter().find(|a| a.annotation_id == id).unwrap();
    assert!(!app.companion_needs_box(hidden));
    assert!(!app.advance_companion_guide());
    assert!(app.work.current_state.as_ref().unwrap().current_annotation(&"source".into()).is_some());
}

#[test]
fn overlapping_boxes_hide_cross_workflow_prelabels_without_consuming_hints() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_prelabel_work_harness(api);
    let app = harness.state_mut();
    let item = app.pending_prelabel_objects()[0].clone();
    let mut retained = item.annotation.clone();
    retained.annotation_id = "000_retained".into();
    retained.task_id = "other-box-workflow".into();
    app.work.annotations.push(retained.clone());
    let state = app.work.current_state.as_mut().unwrap();
    state.bounding_box_visibility = Some(Default::default());
    state.annotations.insert(retained.annotation_id.clone(), vec![retained]);
    app.sync_prelabel_review();
    assert!(!app.visible_prelabels().iter().any(|hint| hint.suggestion_id == item.suggestion.suggestion_id));
    assert!(!app.pending_prelabel_objects().iter().any(|pending| pending.annotation.annotation_id == item.annotation.annotation_id));
    assert!(app.work.current.as_ref().unwrap().prelabels.iter().any(|hint| hint.suggestion_id == item.suggestion.suggestion_id));
    assert!(!app.work.accepted_prelabels.contains(&item.suggestion.suggestion_id));
}
