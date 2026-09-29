#[cfg(feature = "inspector-presets")]
#[test]
fn active_migration_discards_stale_availability_without_rechecking() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api);

    let unavailable_task = TaskId::from("bounding_box:person_cleanup");
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.tasks = app
        .work
        .tasks
        .iter()
        .map(|task| (task.task_id.clone(), task.task_id != unavailable_task))
        .collect();
    app.work.availability.resolved = true;
    assert_eq!(
        app.workflow_availability(&unavailable_task),
        Some(false)
    );

    app.request_assignment_availability();
    let availability_request = take_assignment_availability_request(&mut app);
    app.request_exclude_migration_target(labello_domain::ObjectGroupId::from("group-left"));
    assert!(app.work.migration.busy);
    assert!(app.work.availability.refresh_after_load);
    assert_eq!(app.workflow_availability(&unavailable_task), None);
    assert_eq!(
        app.displayed_workflow_availability(&unavailable_task),
        Some(false)
    );

    deliver_assignment_availability(&mut app, availability_request, true);

    assert!(!app.work.availability.loading);
    assert!(!app.work.availability.resolved);
    assert_eq!(
        app.displayed_workflow_availability(&unavailable_task),
        Some(false),
        "discarding the stale response must retain the last known picker state"
    );
    assert!(
        app.runtime
            .commands
            .iter()
            .all(|command| !matches!(command, UiCommand::AssignmentAvailability { .. }))
    );

    app.work.availability.last_attempt = Some(Instant::now() - Duration::from_secs(31));
    app.refresh_assignment_availability_if_due();
    assert!(
        app.runtime
            .commands
            .iter()
            .all(|command| !matches!(command, UiCommand::AssignmentAvailability { .. }))
    );

    app.work.migration.busy = false;
    assert!(app.manual_migration_active());
    app.refresh_assignment_availability_if_due();
    assert!(
        app.runtime
            .commands
            .iter()
            .all(|command| !matches!(command, UiCommand::AssignmentAvailability { .. }))
    );

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|_| app);
    harness.step();
    harness.get_by_role_and_label(
        egui::accesskit::Role::Button,
        "Person: Bounding box annotation · Choose workflow",
    ).click();
    harness.run_steps(2);
    let unavailable_workflow = harness.get_by_role_and_label(
        egui::accesskit::Role::Button,
        "Person: Bounding box annotation · Imported person bounding-box cleanup",
    );
    assert!(
        unavailable_workflow.accesskit_node().is_disabled(),
        "the picker must keep the last known unavailable workflow disabled during migration"
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn workflow_controls_keep_their_identity_when_a_loaded_image_enables_migration_actions() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    let loaded_state = app.work.current_state.take().unwrap();
    assert!(!app.manual_migration_active());

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|_| app);
    harness.step();
    let label = harness.state().workflow_entry_label(&harness.state().selected_workflow().unwrap(), (harness.state().view == AppView::Annotate).then_some(crate::panels::WorkflowActivity::Migration));
    let loading_id = harness
        .get_by_role_and_label(egui::accesskit::Role::Button, &label)
        .accesskit_node()
        .locate()
        .0;

    harness.state_mut().work.current_state = Some(loaded_state);
    harness.step();
    assert!(harness.state().manual_migration_active());
    let loaded_id = harness
        .get_by_role_and_label(egui::accesskit::Role::Button, &label)
        .accesskit_node()
        .locate()
        .0;

    assert_eq!(
        loaded_id, loading_id,
        "loading a migration image must not replace the workflow controls"
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn mutable_migration_spy_preserves_failure_and_durable_reload_progression() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    let image_id = app.work.current.as_ref().unwrap().image.image_id.clone();
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();

    api.fail_next_migration();
    harness
        .state_mut()
        .request_exclude_migration_target(labello_domain::ObjectGroupId::from("group-left"));
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert!(
        harness
            .state()
            .work.migration
            .error
            .as_deref()
            .is_some_and(|error| error.contains("migration command failed")),
        "counts={:?} migration_error={:?} runtime_error={:?}",
        api.counts(),
        harness.state().work.migration.error,
        harness.state().runtime.error,
    );
    assert!(matches!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &labello_domain::ObjectGroupId::from("group-left")
    ));

    harness
        .state_mut()
        .request_exclude_migration_target(labello_domain::ObjectGroupId::from("group-left"));
    harness.step();
    step_until(&mut harness, 8, |app| {
        matches!(
            app.work.migration.cursor,
            Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
                if object_group_id == &labello_domain::ObjectGroupId::from("group-right")
        )
    });
    assert_eq!(api.counts().migration_commands, 2);

    let durable = api.image_state(&image_id);
    let mut reloaded =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    reloaded.work.current_state = Some(durable.clone());
    reloaded.work.annotations = durable.active_annotations().cloned().collect();
    reloaded.work.migration = Default::default();
    let mut reload_harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|_| reloaded);
    reload_harness.step();
    assert!(matches!(
        reload_harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &labello_domain::ObjectGroupId::from("group-right")
    ));
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_previous_object_navigation_immediately_revisits_for_editing() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    let image_id = app.work.current.as_ref().unwrap().image.image_id.clone();
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();

    click_accesskit_button(&mut harness, "Previous object");
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 1);
    assert!(harness.state().work.migration.inspected_group_id.is_none());
    assert!(matches!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &labello_domain::ObjectGroupId::from("group-right")
    ));
    assert!(api.image_state(&image_id).migration_dependencies
        [&labello_domain::TaskId::from("skeleton:person")]
        .contains_key(&labello_domain::ObjectGroupId::from("group-right")));

    harness.key_press(egui::Key::ArrowUp);
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 2);
    assert!(matches!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &labello_domain::ObjectGroupId::from("group-left")
    ));
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_previous_object_edit_confirms_before_discarding_unsaved_input() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    harness.state_mut().work.migration.draft_dirty = true;

    click_accesskit_button(&mut harness, "Previous object");
    harness.step();
    assert!(
        harness
            .query_by_label("Discard current migration draft?")
            .is_some()
    );
    assert_eq!(api.counts().migration_commands, 0);

    click_accesskit_button(&mut harness, "Discard draft and edit object");
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 1);
    assert!(matches!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &labello_domain::ObjectGroupId::from("group-right")
    ));
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_full_image_can_add_an_object_missing_from_the_import() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    app.work.inspector_panel_collapsed = true;
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.step();

    assert!(harness.query_by_label("Add missing object").is_none());
    assert!(harness.query_by_label_contains("Edit added").is_none());

    let canvas = harness.get_by_label("Annotation canvas").rect();
    click_at(&mut harness, canvas.center());
    let moved_first = canvas.center() + egui::vec2(36.0, -18.0);
    drag_at(&mut harness, canvas.center(), moved_first);
    assert_eq!(harness.state().work.migration.keypoint_index, 1);
    assert!(
        harness.state().work.migration.draft.as_ref().unwrap().keypoints[0]
            .point
            .is_some_and(|point| point.x > 0.5 && point.y < 0.5)
    );

    let keypoint_count = {
        let draft = harness.state_mut().work.migration.draft.as_mut().unwrap();
        for keypoint in draft.keypoints.iter_mut().skip(1) {
            keypoint.point = Some(labello_domain::NormalizedPoint { x: 0.5, y: 0.5 });
            keypoint.state = labello_domain::KeypointState::Visible;
        }
        draft.keypoints.len()
    };
    harness.state_mut().work.migration.keypoint_index = keypoint_count;
    harness.state_mut().work.migration.draft_dirty = true;
    harness.step();
    assert!(
        !harness
            .query_by_label_contains("Save missing object")
            .unwrap()
            .accesskit_node()
            .is_disabled()
    );

    click_accesskit_button(&mut harness, "Save missing object");
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 1);
    assert!(!harness.state().work.migration.adding_missing_object);
    assert!(matches!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::FullImage)
    ));
    let task_id = harness.state().work.selected_task_id.as_ref().unwrap();
    assert_eq!(
        harness
            .state()
            .work
            .current_state
            .as_ref()
            .unwrap()
            .active_annotations()
            .filter(|annotation| {
                annotation.task_id == *task_id
                    && annotation.object_group_id.is_none()
                    && annotation.annotation_type == labello_domain::AnnotationType::Skeleton
            })
            .count(),
        1
    );
    assert!(harness.query_by_label("Submit").is_some());
    harness.set_size(egui::vec2(390.0, 667.0));
    harness.step();
    assert!(harness.query_by_label("Submit").is_some());
    assert!(harness.query_by_label("Edit added").is_none());
    let center = harness.get_by_label("Annotation canvas").rect().center();
    click_at(&mut harness, center);
    harness.step();
    assert!(harness.state().work.migration.adding_missing_object);
    assert_eq!(
        harness
            .state()
            .work
            .migration
            .editing_missing_annotation_id
            .as_ref(),
        Some(&labello_domain::AnnotationId::from("spy-discovered"))
    );
    harness.set_size(egui::vec2(1440.0, 900.0));
    harness.step();
    assert!(
        harness
            .query_by_label_contains("Save object changes")
            .unwrap()
            .accesskit_node()
            .is_disabled()
    );
    let edited_point = labello_domain::NormalizedPoint { x: 0.7, y: 0.6 };
    harness.state_mut().work.migration.draft.as_mut().unwrap().keypoints[0].point =
        Some(edited_point);
    harness.state_mut().work.migration.draft_dirty = true;
    harness.step();
    click_accesskit_button(&mut harness, "Save object changes");
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 2);
    assert!(!harness.state().work.migration.adding_missing_object);
    let edited = harness
        .state()
        .work
        .current_state
        .as_ref()
        .unwrap()
        .current_annotation(&labello_domain::AnnotationId::from("spy-discovered"))
        .unwrap();
    assert_eq!(edited.version, 2);
    assert!(
        matches!(
            &edited.geometry,
            labello_domain::AnnotationGeometry::Skeleton(skeleton)
                if skeleton.keypoints[0].point == Some(edited_point)
        ),
        "{:?}",
        edited.geometry
    );
    assert!(
        harness
            .query_by_label("Add missing object")
            .is_none()
    );
    let center = harness.get_by_label("Annotation canvas").rect().center();
    click_at(&mut harness, center);
    harness.step();
    assert!(harness.query_by_label("Remove added object").is_some());
    harness.key_press(egui::Key::Delete);
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 3);
    assert!(!harness.state().work.migration.adding_missing_object);
    assert!(harness.state().work.migration.draft.is_none());
    assert!(harness.query_by_label("Discard object changes").is_none());
    assert!(
        harness
            .state()
            .work
            .current_state
            .as_ref()
            .unwrap()
            .current_annotation(&labello_domain::AnnotationId::from("spy-discovered"))
            .unwrap()
            .deleted
    );
    assert!(harness.query_by_label("Edit added object 1").is_none());
    assert!(
        harness
            .query_by_label("Add missing object")
            .is_none()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn missing_object_uses_its_own_zero_position_explanation() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    let task_id = app.work.selected_task_id.clone().unwrap();
    app.work
        .tasks
        .iter_mut()
        .find(|task| task.task_id == task_id)
        .unwrap()
        .skeleton = Some(labello_domain::SkeletonSpec {
        keypoints: vec![labello_domain::KeypointSpec {
            name: "center".to_string(),
            required: false,
        }],
        edges: Vec::new(),
        allow_hidden: true,
        allow_absent: true,
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|_| app);
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.step();

    harness.key_press(egui::Key::M);
    harness.step();
    let not_present = harness
        .query_all_by_label_contains("Mark center as not present")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap();
    assert!(not_present.accesskit_node().is_disabled());
    assert!(
        harness
            .query_by_label(
                "At least one keypoint position is required to add an object."
            )
            .is_some()
    );
    harness.key_press(egui::Key::N);
    harness.step();
    assert_eq!(harness.state().work.migration.keypoint_index, 0);
    assert!(
        harness
            .query_by_label_contains("Save missing object")
            .unwrap()
            .accesskit_node()
            .is_disabled()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn clicking_a_pending_box_confirms_the_current_skeleton_and_selects_the_clicked_target() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::manual_migration::ManualMigrationState;

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    let task = app.selected_task().unwrap().clone();
    let group_id = match app.work.migration.cursor.as_ref().unwrap() {
        labello_domain::MigrationCursor::Object {
            object_group_id, ..
        } => object_group_id.clone(),
        labello_domain::MigrationCursor::FullImage => panic!("expected object cursor"),
    };
    let mut draft = ManualMigrationState::empty_skeleton(
        task.skeleton
            .unwrap()
            .keypoints
            .into_iter()
            .map(|point| point.name),
    );
    for keypoint in &mut draft.keypoints {
        keypoint.point = Some(labello_domain::NormalizedPoint { x: 0.25, y: 0.3 });
        keypoint.state = labello_domain::KeypointState::Visible;
    }
    app.work.migration.keypoint_index = draft.keypoints.len();
    app.work.migration.draft = Some(draft);
    app.work.migration.draft_group = Some(group_id);
    app.work.migration.draft_dirty = true;
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    assert!(harness.state().work.canvas.zoom() > 1.0);
    harness.state_mut().work.canvas.fit_view();
    harness.step();
    let availability_checks = api.counts().assignment_availability;

    let canvas = harness.get_by_label("Annotation canvas").rect();
    let image_aspect = 1280.0 / 800.0;
    let image_size = if canvas.width() / canvas.height() > image_aspect {
        egui::vec2(canvas.height() * image_aspect, canvas.height())
    } else {
        egui::vec2(canvas.width(), canvas.width() / image_aspect)
    };
    let image = egui::Rect::from_center_size(canvas.center(), image_size);
    let pending_box_center = egui::pos2(
        image.left() + image.width() * 0.73,
        image.top() + image.height() * 0.49,
    );
    harness.event(egui::Event::PointerMoved(pending_box_center));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: pending_box_center,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: pending_box_center,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    let clicked_group_id = labello_domain::ObjectGroupId::from("group-right");
    step_until(&mut harness, 8, |app| {
        app.work.migration.inspected_group_id.as_ref() == Some(&clicked_group_id)
    });
    assert_eq!(
        harness.state().work.migration.inspected_group_id,
        Some(clicked_group_id.clone())
    );
    assert!(harness.state().work.migration.busy);
    step_until(&mut harness, 8, |app| !app.work.migration.busy);

    assert_eq!(api.counts().migration_commands, 2);
    assert!(matches!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &clicked_group_id
    ));
    assert_eq!(
        harness.state().work.migration.draft_group,
        Some(clicked_group_id)
    );
    assert!(
        harness
            .state()
            .work
            .migration
            .draft
            .as_ref()
            .is_some_and(|draft| draft.keypoints.iter().all(|keypoint| keypoint.point.is_none()))
    );
    assert!(!harness.state().work.migration.draft_dirty);
    assert_eq!(
        api.counts().assignment_availability,
        availability_checks
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn clicking_an_unmigrated_box_skips_an_empty_current_skeleton() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    let task_id = app.selected_task().unwrap().task_id.clone();
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    assert_eq!(harness.state().work.migration.keypoint_index, 0);
    harness.state_mut().work.canvas.fit_view();
    harness.step();

    let canvas = harness.get_by_label("Annotation canvas").rect();
    let image_aspect = 1280.0 / 800.0;
    let image_size = if canvas.width() / canvas.height() > image_aspect {
        egui::vec2(canvas.height() * image_aspect, canvas.height())
    } else {
        egui::vec2(canvas.width(), canvas.width() / image_aspect)
    };
    let image = egui::Rect::from_center_size(canvas.center(), image_size);
    let pending_box_center = egui::pos2(
        image.left() + image.width() * 0.73,
        image.top() + image.height() * 0.49,
    );
    click_at(&mut harness, pending_box_center);
    step_until(&mut harness, 8, |app| {
        matches!(
            app.work.migration.cursor,
            Some(labello_domain::MigrationCursor::Object {
                ref object_group_id,
                ..
            }) if object_group_id == &labello_domain::ObjectGroupId::from("group-right")
        )
    });

    assert_eq!(api.counts().migration_commands, 1);
    assert!(matches!(
        &harness.state().work.current_state.as_ref().unwrap().migration_dispositions[&task_id]
            [&labello_domain::ObjectGroupId::from("group-left")]
            .status,
        labello_domain::MigrationDispositionStatus::Excluded { exclusion }
            if exclusion.reason == labello_domain::MigrationExclusionReason::NoValidSkeleton
    ));
}

#[cfg(feature = "inspector-presets")]
#[test]
fn clicking_a_skipped_box_immediately_revisits_it_for_editing() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    let task_id = app.selected_task().unwrap().task_id.clone();
    let skipped_group_id = labello_domain::ObjectGroupId::from("group-right");
    let image_id = app.work.current.as_ref().unwrap().image.image_id.clone();
    let labello_domain::MigrationDispositionStatus::Excluded { exclusion } = &mut app
        .work
        .current_state
        .as_mut()
        .unwrap()
        .migration_dispositions
        .get_mut(&task_id)
        .unwrap()
        .get_mut(&skipped_group_id)
        .unwrap()
        .status
    else {
        panic!("expected excluded migration target");
    };
    exclusion.reason = labello_domain::MigrationExclusionReason::NoValidSkeleton;
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    let canvas = harness.get_by_label("Annotation canvas").rect();
    let image_aspect = 1280.0 / 800.0;
    let image_size = if canvas.width() / canvas.height() > image_aspect {
        egui::vec2(canvas.height() * image_aspect, canvas.height())
    } else {
        egui::vec2(canvas.width(), canvas.width() / image_aspect)
    };
    let image = egui::Rect::from_center_size(canvas.center(), image_size);
    let skipped_box_center = egui::pos2(
        image.left() + image.width() * 0.73,
        image.top() + image.height() * 0.49,
    );
    click_at(&mut harness, skipped_box_center);
    step_until(&mut harness, 8, |app| !app.work.migration.busy);

    assert_eq!(api.counts().migration_commands, 1);
    assert!(matches!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &skipped_group_id
    ));
    assert!(api.image_state(&image_id).migration_dependencies[&task_id]
        .contains_key(&skipped_group_id));
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_skip_requires_confirmation_before_discarding_a_local_draft() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|_| app);
    harness.step();
    harness.state_mut().work.migration.draft_dirty = true;

    click_accesskit_button(&mut harness, "Skip");
    harness.step();
    assert!(harness.query_by_label("Unsaved migration draft").is_some());
    assert!(
        harness
            .query_by_label("Discard draft and switch")
            .is_some()
    );
    assert!(harness.query_by_label("Submit and switch").is_none());
    assert_eq!(api.counts().release_assignment, 0);

    click_accesskit_button(&mut harness, "Cancel");
    harness.step();
    assert!(harness.state().work.pending_transition.is_none());
    assert!(harness.state().work.migration.draft_dirty);
    assert_eq!(api.counts().release_assignment, 0);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_draft_supports_undo_and_delete() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|ctx| {
            inspector_presets::build(InspectorPreset::MigrationObject, &ctx.egui_ctx)
        });
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.step();

    let place_first_keypoint = |app: &mut LabelloApp| {
        let draft = app.work.migration.draft.as_mut().unwrap();
        draft.keypoints[0].point = Some(labello_domain::NormalizedPoint { x: 0.5, y: 0.5 });
        draft.keypoints[0].state = labello_domain::KeypointState::Visible;
        app.work.migration.keypoint_index = 1;
    };

    let task_id = harness.state().work.selected_task_id.clone().unwrap();
    let guide_id = harness
        .state()
        .work.current_state
        .as_ref()
        .unwrap()
        .migration_target_sets[&task_id]
        .targets[0]
        .guide_annotation_id
        .clone();
    let guide_before = harness
        .state()
        .work.current_state
        .as_ref()
        .unwrap()
        .current_annotation(&guide_id)
        .unwrap()
        .clone();

    place_first_keypoint(harness.state_mut());
    harness.state_mut().work.migration.next_hidden = true;
    harness.state_mut().work.canvas.fit_view();
    harness.step();
    let canvas = harness.get_by_label("Annotation canvas").rect();
    drag_at(
        &mut harness,
        canvas.center(),
        canvas.center() + egui::vec2(32.0, -16.0),
    );
    assert!(
        harness.state().work.migration.draft.as_ref().unwrap().keypoints[0]
            .point
            .is_some_and(|point| point.x > 0.5 && point.y < 0.5)
    );
    click_accesskit_button(&mut harness, "Undo last keypoint");
    assert_eq!(harness.state().work.migration.keypoint_index, 0);
    assert!(
        harness.state().work.migration.draft.as_ref().unwrap().keypoints[0]
            .point
            .is_none()
    );
    assert!(!harness.state().work.migration.next_hidden);
    assert_eq!(
        harness
            .state()
            .work.current_state
            .as_ref()
            .unwrap()
            .current_annotation(&guide_id),
        Some(&guide_before)
    );

    place_first_keypoint(harness.state_mut());
    harness.step();
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    harness.step();
    assert_eq!(harness.state().work.migration.keypoint_index, 0);
    assert!(
        harness.state().work.migration.draft.as_ref().unwrap().keypoints[0]
            .point
            .is_none()
    );

    place_first_keypoint(harness.state_mut());
    harness.step();
    harness.key_press(egui::Key::Delete);
    harness.step();
    assert_eq!(harness.state().work.migration.keypoint_index, 0);
    assert!(
        harness.state().work.migration.draft.as_ref().unwrap().keypoints[0]
            .point
            .is_none()
    );

    place_first_keypoint(harness.state_mut());
    harness
        .state_mut()
        .work.current_state
        .as_mut()
        .unwrap()
        .annotations
        .get_mut(&guide_id)
        .unwrap()
        .last_mut()
        .unwrap()
        .deleted = true;
    harness.step();
    assert!(
        harness
            .query_all_by_label_contains("Undo last keypoint")
            .next()
            .unwrap()
            .accesskit_node()
            .is_disabled()
    );
    harness.key_press(egui::Key::Delete);
    harness.step();
    assert_eq!(harness.state().work.migration.keypoint_index, 1);
    assert!(
        harness.state().work.migration.draft.as_ref().unwrap().keypoints[0]
            .point
            .is_some()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_confirmation_promotes_prepared_assignment_without_blocking_reload() {
    use crate::app::LoadedImage;
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::queue::QueuedImage;

    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    api.set_image_state(app.work.current_state.clone().unwrap());
    api.complete_next_migration_with(app.work.assignment.clone().unwrap());
    let next_image_id = ImageId::from("img_prepared_migration");
    let next_assignment = Assignment {
        assignment_id: AssignmentId::generate(),
        image_id: next_image_id.clone(),
        task_id: app.work.selected_task_id.clone().unwrap(),
        assigned_to: app.config.user_id.clone(),
        kind: AssignmentKind::Annotation,
        status: AssignmentStatus::Active,
        expires_at: Some(now() + chrono::Duration::minutes(5)),
        created_at: now(),
        updated_at: now(),
    };
    app.work.queue.clear();
    assert!(app.work.queue.push_prepared(LoadedImage {
            prepared_until: None,
            review_submitters: Vec::new(),
            reasons: Vec::new(),
        assignment: next_assignment,
        queued: QueuedImage {
            image: image_record(next_image_id.as_str(), "prepared-migration.png", 640, 480),
            prelabels: Vec::new(),
        },
        annotations: Vec::new(),
        state: ImageState::new(next_image_id.clone()),
        color_image: None,
    }));
    api.set_no_assignment(true);
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    let previews_before = api.counts().get_encoded_image_preview;
    let availability_checks_before = api.counts().assignment_availability;

    harness.key_press(egui::Key::Space);
    harness.step();
    step_until(&mut harness, 8, |app| {
        !app.work.migration.busy
            && app
                .work.assignment
                .as_ref()
                .is_some_and(|assignment| assignment.image_id == next_image_id)
    });
    assert_eq!(api.counts().migration_commands, 1);
    assert_eq!(api.counts().get_encoded_image_preview, previews_before);
    assert_eq!(api.counts().release_assignment, 0);
    assert!(!harness.state().loading.image);
    assert!(
        harness
            .state()
            .work
            .previous_assignment
            .as_ref()
            .is_some_and(|assignment| assignment.status == AssignmentStatus::Completed)
    );
    step_until(&mut harness, 8, |_| {
        api.counts().assignment_availability > availability_checks_before
    });
    assert_eq!(api.counts().migration_commands, 1);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_review_approval_promotes_cached_work_without_refetching_image_data() {
    use crate::app::LoadedImage;
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::queue::QueuedImage;

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationReview, &egui::Context::default());
    api.set_image_state(app.work.current_state.clone().unwrap());
    api.complete_next_migration_with(app.work.assignment.clone().unwrap());
    let next_image_id = ImageId::from("img_cached_migration_review");
    let next_assignment = Assignment {
        assignment_id: AssignmentId::generate(),
        image_id: next_image_id.clone(),
        task_id: app.work.selected_task_id.clone().unwrap(),
        assigned_to: app.config.user_id.clone(),
        kind: AssignmentKind::Review,
        status: AssignmentStatus::Active,
        expires_at: Some(now() + chrono::Duration::minutes(5)),
        created_at: now(),
        updated_at: now(),
    };
    api.add_active_assignment(next_assignment.clone());
    app.work.queue.clear();
    assert!(app.work.queue.push_prepared(LoadedImage {
            prepared_until: None,
            review_submitters: Vec::new(),
            reasons: Vec::new(),
        assignment: next_assignment,
        queued: QueuedImage {
            image: image_record(
                next_image_id.as_str(),
                "cached-migration-review.png",
                640,
                480,
            ),
            prelabels: Vec::new(),
        },
        annotations: Vec::new(),
        state: ImageState::new(next_image_id.clone()),
        color_image: Some(egui::ColorImage::from_rgba_unmultiplied(
            [1, 1],
            &[24, 48, 72, 255],
        )),
    }));
    api.set_no_assignment(true);
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    let counts_before = api.counts();

    harness.key_press(egui::Key::Y);
    harness.step();
    step_until(&mut harness, 10, |app| {
        !app.work.migration.busy
            && app
                .work
                .assignment
                .as_ref()
                .is_some_and(|assignment| assignment.image_id == next_image_id)
    });

    assert_eq!(api.counts().migration_commands, 1);
    assert_eq!(api.counts().revalidate_assignment, 1);
    assert_eq!(
        api.counts().get_image_record,
        counts_before.get_image_record
    );
    assert_eq!(
        api.counts().get_encoded_image_preview,
        counts_before.get_encoded_image_preview
    );
    assert!(harness.state().work.current_texture.is_some());
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_primary_actions_stay_visible_without_the_inspector_drawer() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut object = Harness::builder()
        .with_size(egui::vec2(390.0, 667.0))
        .build_eframe(|ctx| {
            inspector_presets::build(InspectorPreset::MigrationObject, &ctx.egui_ctx)
        });
    object.step();
    assert!(object.query_by_label_contains("Save & next").is_some());
    assert!(object.query_by_label("Workflow").is_some());
    assert!(object.query_by_label("Inspector").is_some());
    assert!(object.query_by_label("Controls").is_none());
    assert!(object.state().work.drawer.is_none());
    let primary = object.get_by_label_contains("Save & next").rect();
    let workflow = object.get_by_label("Workflow").rect();
    let inspector = object.get_by_label("Inspector").rect();
    let context = object.get_by_label("Workspace context bar").rect();
    assert!(workflow.width() <= 44.5, "{workflow:?}");
    assert!(inspector.width() <= 44.5, "{inspector:?}");
    assert!(
        workflow.top() >= context.top() && workflow.bottom() <= context.bottom(),
        "context={context:?} workflow={workflow:?}"
    );
    assert!(inspector.top() >= context.top() && inspector.bottom() <= context.bottom());
    assert!(
        object.get_by_label("Annotation canvas").rect().bottom() <= primary.top(),
        "the canvas must stop above the compact migration action bar"
    );

    let canvas = object.get_by_label("Annotation canvas").rect();
    click_at(&mut object, canvas.center());
    object.step();
    for width in [400.0, 390.0, 360.0, 320.0] {
        object.set_size(egui::vec2(width, 667.0));
        object.step();
        let canvas = object.get_by_label("Annotation canvas").rect();
        let primary = object.get_by_label_contains("Save & next").rect();
        assert!(
            canvas.bottom() <= primary.top(),
            "width={width} canvas={canvas:?} primary={primary:?}"
        );
        for label in ["Undo last keypoint", "Skip"] {
            let action = object.query_by_label_contains(label).or_else(|| object.query_by_label("More")).unwrap().rect();
            assert!(
                canvas.bottom() <= action.top(),
                "width={width} canvas={canvas:?} {label}={action:?}"
            );
        }
        let context = object.get_by_label("Workspace context bar").rect();
        for label in ["Workflow", "Inspector"] {
            let action = object.get_by_label(label).rect();
            assert!(
                action.top() >= context.top() && action.bottom() <= context.bottom(),
                "width={width} context={context:?} {label}={action:?}"
            );
        }
    }
    click_accesskit_button(&mut object, "Undo last keypoint");
    assert_eq!(object.state().work.migration.keypoint_index, 0);
    object.set_size(egui::vec2(320.0, 568.0));
    object.step();
    let primary = object.get_by_label_contains("Save & next").rect();
    let canvas = object.get_by_label("Annotation canvas").rect();
    assert!(
        canvas.bottom() <= primary.top(),
        "canvas={canvas:?} primary={primary:?}"
    );
    object.set_size(egui::vec2(390.0, 667.0));
    object.step();

    click(&mut object, "Open navigation");
    assert!(
        object
            .query_by_role_and_label(egui::accesskit::Role::Window, "Application navigation")
            .is_some()
    );
    assert!(object.query_by_label("Workflow panel").is_none());
    assert!(object.query_by_label("Inspector panel").is_none());
    object.key_press(egui::Key::Escape);
    object.step();

    click_accesskit_button(&mut object, "Workflow");
    assert_eq!(object.state().work.drawer, Some(Drawer::Workflow));
    object.key_press(egui::Key::W);
    object.step();
    assert!(object.state().work.drawer.is_none());

    let mut roomy_compact = Harness::builder()
        .with_size(egui::vec2(570.0, 667.0))
        .build_eframe(|ctx| {
            inspector_presets::build(InspectorPreset::MigrationObject, &ctx.egui_ctx)
        });
    roomy_compact.step();
    let workflow = roomy_compact.get_by_label("Workflow").rect();
    let inspector = roomy_compact.get_by_label("Inspector").rect();
    let context = roomy_compact
        .get_by_label("Workspace context bar")
        .rect();
    assert!(workflow.width() >= 44.0, "{workflow:?}");
    assert!(inspector.width() >= 44.0, "{inspector:?}");
    assert!(workflow.top() >= context.top() && workflow.bottom() <= context.bottom());
    assert!(inspector.top() >= context.top() && inspector.bottom() <= context.bottom());
    let canvas = roomy_compact.get_by_label("Annotation canvas").rect();
    click_at(&mut roomy_compact, canvas.center());
    roomy_compact.step();
    let undo = roomy_compact.get_by_label("Undo last keypoint").rect();
    let canvas = roomy_compact.get_by_label("Annotation canvas").rect();
    assert!(
        canvas.bottom() <= undo.top(),
        "canvas={canvas:?} undo={undo:?}"
    );

    let mut medium = Harness::builder()
        .with_size(egui::vec2(600.0, 667.0))
        .build_eframe(|ctx| {
            inspector_presets::build(InspectorPreset::MigrationObject, &ctx.egui_ctx)
        });
    medium.step();
    assert!(medium.query_by_label("Workflow").is_some());
    assert!(medium.query_by_label("Inspector").is_some());
    assert!(medium.query_by_label("Migration controls").is_none());
    assert!(medium.get_by_label("Workflow").rect().width() <= 44.5);
    assert!(medium.get_by_label("Inspector").rect().width() <= 44.5);
    let canvas = medium.get_by_label("Annotation canvas").rect();
    let first_row = medium.get_by_label_contains("Save skeleton & advance").rect();
    let inspector = medium.get_by_label("Inspector").rect();
    let context = medium.get_by_label("Workspace context bar").rect();
    assert!(
        canvas.bottom() <= first_row.top(),
        "canvas={canvas:?} first_row={first_row:?}"
    );
    assert!(
        inspector.top() >= context.top() && inspector.bottom() <= context.bottom(),
        "context control is clipped: context={context:?} inspector={inspector:?}"
    );

    click_accesskit_button(&mut object, "Inspector");
    assert_eq!(object.state().work.drawer, Some(Drawer::Inspector));
    object.step();
    assert_eq!(
        object
            .query_all_by_label_contains("Save & next")
            .filter(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .count(),
        1,
        "opening the drawer must not duplicate the primary migration action"
    );
    assert!(
        object
            .query_by_label("Exclude object")
            .is_some()
    );
    assert!(object.query_by_label("Reason (required, this object)").is_some());
    assert!(
        object
            .query_by_label_contains("Not present” applies")
            .is_some()
    );
    let visible = object.get_by_role_and_label(
        egui::accesskit::Role::Button,
        "Place head as visible",
    );
    let occluded = object.get_by_role_and_label(
        egui::accesskit::Role::Button,
        "Place head as occluded",
    );
    assert_eq!(
        visible.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    assert_eq!(
        occluded.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    assert!(visible.rect().height() >= 44.0);
    assert!(occluded.rect().height() >= 44.0);
    click_accesskit_button(&mut object, "Place head as occluded");
    assert!(object.state().work.migration.next_hidden);
    object.key_press(egui::Key::I);
    object.step();
    assert!(object.state().work.drawer.is_none());

    object.set_size(egui::vec2(260.0, 667.0));
    object.step();
    let workflow = object.get_by_label("Workflow").rect();
    let inspector = object.get_by_label("Inspector").rect();
    let context = object.get_by_label("Workspace context bar").rect();
    assert!(workflow.width() <= 44.5, "{workflow:?}");
    assert!(inspector.width() <= 44.5, "{inspector:?}");
    assert!((workflow.top() - inspector.top()).abs() <= 1.0);
    assert!(
        workflow.top() >= context.top()
            && workflow.bottom() <= context.bottom()
            && inspector.bottom() <= context.bottom(),
        "collapsed panel controls must remain in the context bar: \
         context={context:?} workflow={workflow:?} inspector={inspector:?}"
    );

    let mut full_image = Harness::builder()
        .with_size(egui::vec2(390.0, 667.0))
        .build_eframe(|ctx| {
            inspector_presets::build(InspectorPreset::MigrationFullImage, &ctx.egui_ctx)
        });
    full_image.step();
    assert!(
        full_image
            .query_by_label_contains("Submit")
            .is_some()
    );
    assert!(
        full_image
            .query_by_label("Add missing object")
            .is_none()
    );
    assert!(full_image.query_by_label("Workflow").is_some());
    assert!(full_image.query_by_label("Inspector").is_some());
    assert!(full_image.query_by_label("Controls").is_none());
    assert!(full_image.state().work.drawer.is_none());

    let mut wide_full_image = Harness::builder()
        .with_size(egui::vec2(1318.0, 900.0))
        .build_eframe(|ctx| {
            let mut app =
                inspector_presets::build(InspectorPreset::MigrationFullImage, &ctx.egui_ctx);
            app.work.inspector_panel_collapsed = true;
            app
        });
    wide_full_image.step();
    let action = wide_full_image.get_by_label_contains("Submit").rect();
    assert!(
        action.right() <= 1318.0 && action.bottom() <= 900.0,
        "confirmation must remain fully visible at the narrowest wide desktop size: {action:?}"
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn single_optional_migration_separates_not_present_from_object_exclusion() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|ctx| {
            inspector_presets::build(
                InspectorPreset::MigrationSingleOptional,
                &ctx.egui_ctx,
            )
        });
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.step();

    let visible = harness.get_by_role_and_label(
        egui::accesskit::Role::Button,
        "Place center as visible",
    );
    let occluded = harness.get_by_role_and_label(
        egui::accesskit::Role::Button,
        "Place center as occluded",
    );
    assert_eq!(
        visible.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    assert_eq!(
        occluded.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    assert!(visible.rect().height() >= 44.0);
    assert!(occluded.rect().height() >= 44.0);
    assert!(
        harness
            .query_by_label("Visible: click the exact position.")
            .is_some()
    );

    let not_present = harness
        .query_all_by_label_contains("Mark center as not present")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap();
    assert!(not_present.accesskit_node().is_disabled());
    harness
        .state_mut()
        .trigger_user_action(labello_domain::UserAction::MarkKeypointAbsent);
    assert_eq!(harness.state().work.migration.keypoint_index, 0);
    assert!(
        harness
            .query_by_label(
                "At least one keypoint position is required. If none can be placed, use Exclude object below.",
            )
            .is_some()
    );
    assert!(
        harness
            .get_by_label_contains("Save skeleton & advance")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        harness
            .query_by_label("Exclude object")
            .is_some()
    );
    assert!(harness.query_by_label("Reason (required, this object)").is_some());
    assert!(
        harness
            .query_by_label_contains("Not present” applies")
            .is_some()
    );

    harness.key_press(egui::Key::H);
    harness.step();
    assert!(harness.state().work.migration.next_hidden);
    assert!(
        harness
            .query_by_label("Occluded: click the estimated position.")
            .is_some()
    );
    harness.key_press(egui::Key::H);
    harness.step();
    assert!(!harness.state().work.migration.next_hidden);
    click_accesskit_button(&mut harness, "Place center as occluded");
    let canvas = harness.get_by_label("Annotation canvas").rect();
    click_at(&mut harness, canvas.center());
    let draft = harness.state().work.migration.draft.as_ref().unwrap();
    assert_eq!(draft.keypoints[0].state, labello_domain::KeypointState::Hidden);
    assert!(draft.keypoints[0].point.is_some());
    assert_eq!(harness.state().work.migration.keypoint_index, 1);
    assert!(!harness.state().work.migration.next_hidden);
    assert!(!harness.state().work.migration.busy);

    assert!(
        harness
            .query_by_label("Exclude object & advance")
            .is_some()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_decision_summary_counts_positioned_and_not_present_keypoints() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|ctx| {
            inspector_presets::build(InspectorPreset::MigrationObject, &ctx.egui_ctx)
    });
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.step();
    let draft = harness
        .state_mut()
        .work
        .migration
        .draft
        .as_mut()
        .unwrap();
    draft.keypoints[0].point = Some(labello_domain::NormalizedPoint { x: 0.5, y: 0.5 });
    draft.keypoints[0].state = labello_domain::KeypointState::Visible;
    harness.state_mut().work.migration.keypoint_index = 1;
    harness.state_mut().work.migration.draft_dirty = true;
    harness.step();

    let first_not_present = harness
        .query_all_by_label_contains("Mark left_hand as not present")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap();
    assert!(!first_not_present.accesskit_node().is_disabled());

    for index in 0..4 {
        harness.key_press(egui::Key::N);
        harness.step();
        let draft = harness.state().work.migration.draft.as_ref().unwrap();
        assert_eq!(harness.state().work.migration.keypoint_index, index + 2);
        assert_eq!(
            draft
                .keypoints
                .iter()
                .filter(|keypoint| keypoint.point.is_some())
                .count(),
            1
        );
        assert_eq!(
            draft
                .keypoints
                .iter()
                .take(harness.state().work.migration.keypoint_index)
                .filter(|keypoint| {
                    keypoint.state == labello_domain::KeypointState::Absent
                        && keypoint.point.is_none()
                })
                .count(),
            index + 1
        );
    }
    harness.step();
    assert!(
        !harness
            .get_by_label_contains("Save skeleton & advance")
            .accesskit_node()
            .is_disabled()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_refocus_is_touch_sized_and_uses_its_configured_shortcut() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    app.work.keybindings.bindings.insert(
        labello_domain::UserAction::RefocusObject,
        labello_domain::KeyChord::new("F"),
    );
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|_| app);
    harness.step();

    let refocus = harness.get_by_label("Refocus object F");
    let context = harness.get_by_label("Workspace context bar").rect();
    assert!(refocus.rect().height() >= 44.0);
    assert!(refocus.rect().top() >= context.top() && refocus.rect().bottom() <= context.bottom());

    harness.state_mut().work.canvas.fit_view();
    harness.step();
    assert_eq!(harness.state().work.canvas.zoom(), 1.0);
    harness.key_press(egui::Key::F);
    harness.step();
    assert!(harness.state().work.canvas.zoom() > 1.0);

    harness.state_mut().work.canvas.fit_view();
    harness.step();
    click_accesskit_button(&mut harness, "Refocus object F");
    harness.step();
    assert!(harness.state().work.canvas.zoom() > 1.0);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_save_uses_the_contextual_submit_shortcut() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::manual_migration::ManualMigrationState;

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    app.work.keybindings.bindings.insert(
        labello_domain::UserAction::NextImage,
        labello_domain::KeyChord::new("ArrowRight"),
    );
    let task = app.selected_task().unwrap().clone();
    let group_id = match app.work.migration.cursor.as_ref().unwrap() {
        labello_domain::MigrationCursor::Object {
            object_group_id, ..
        } => object_group_id.clone(),
        labello_domain::MigrationCursor::FullImage => panic!("expected object cursor"),
    };
    let mut draft = ManualMigrationState::empty_skeleton(
        task.skeleton
            .unwrap()
            .keypoints
            .into_iter()
            .map(|point| point.name),
    );
    for keypoint in &mut draft.keypoints {
        keypoint.point = Some(labello_domain::NormalizedPoint { x: 0.5, y: 0.5 });
        keypoint.state = labello_domain::KeypointState::Visible;
    }
    app.work.migration.keypoint_index = draft.keypoints.len();
    app.work.migration.draft = Some(draft);
    app.work.migration.draft_group = Some(group_id);
    app.work.migration.draft_dirty = true;
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());

    let mut harness = Harness::builder()
        .with_size(egui::vec2(390.0, 667.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    assert!(
        !harness
            .get_by_label_contains("Save & next")
            .accesskit_node()
            .is_disabled()
    );

    harness.key_press(egui::Key::ArrowRight);
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 1);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_review_refocus_restores_the_active_guide_view() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut app =
        inspector_presets::build(InspectorPreset::MigrationReview, &egui::Context::default());
    app.work.keybindings.bindings.insert(
        labello_domain::UserAction::RefocusObject,
        labello_domain::KeyChord::new("F"),
    );
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|_| app);
    harness.step();

    assert!(harness.state().work.canvas.current_zoom() > 1.0);
    let refocus = harness.get_by_label("Refocus object F");
    let context = harness.get_by_label("Workspace context bar").rect();
    assert!(refocus.rect().height() >= 44.0);
    assert!(refocus.rect().top() >= context.top() && refocus.rect().bottom() <= context.bottom());

    click(&mut harness, "Fit");
    assert_eq!(harness.state().work.canvas.current_zoom(), 1.0);
    harness.key_press(egui::Key::F);
    harness.step();
    assert!(harness.state().work.canvas.current_zoom() > 1.0);

    click(&mut harness, "Fit");
    click_accesskit_button(&mut harness, "Refocus object F");
    assert!(harness.state().work.canvas.current_zoom() > 1.0);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn final_migration_review_approval_preserves_overview_while_next_review_revalidates() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationReview, &egui::Context::default());
    let task_id = app.work.selected_task_id.clone().unwrap();
    let reviewed_at = labello_domain::now();
    let state = app.work.current_state.as_mut().unwrap();
    for (group_id, disposition) in &state.migration_dispositions[&task_id] {
        let target = match &disposition.status {
            labello_domain::MigrationDispositionStatus::Annotated {
                skeleton_annotation_id,
                skeleton_version,
            } => labello_domain::ReviewTarget::AnnotationVersion {
                annotation_id: skeleton_annotation_id.clone(),
                version: *skeleton_version,
            },
            labello_domain::MigrationDispositionStatus::Excluded { .. } => {
                labello_domain::ReviewTarget::MigrationDisposition {
                    task_id: task_id.clone(),
                    object_group_id: group_id.clone(),
                    disposition_version: disposition.disposition_version,
                }
            }
            _ => panic!("expected resolved migration target"),
        };
        let review = labello_domain::ReviewRecord {
            review_id: labello_domain::ReviewId::generate(),
            target,
            reviewer_user_id: app.config.user_id.clone(),
            decision: labello_domain::ReviewDecision::Approved,
            timestamp: reviewed_at,
            comment: None,
        };
        state.review_record_rounds.insert(review.review_id.clone(), state.review_round(&task_id).unwrap().event_id.clone());
        state.reviews.push(review);
    }
    let outgoing = state.clone();
    let mut completed = outgoing.clone();
    let completed_at = reviewed_at + chrono::Duration::seconds(1);
    let previous_round = labello_domain::ReviewRound {
        event_id: labello_domain::EventId::from("migration-submitted-round"),
        event_sequence: 1,
        submitted_by: labello_domain::UserId::from("annotator"),
    };
    for review in &completed.reviews {
        completed
            .review_record_rounds
            .insert(review.review_id.clone(), previous_round.event_id.clone());
    }
    completed
        .review_rounds
        .insert(task_id.clone(), previous_round.clone());
    // Simulate a true new submission round. Wall-clock timestamps do not
    // identify review generations after the review-revision rebase.
    completed.review_rounds.insert(
        task_id.clone(),
        labello_domain::ReviewRound {
            event_id: labello_domain::EventId::from("migration-resubmitted-round"),
            event_sequence: previous_round.event_sequence + 1,
            submitted_by: previous_round.submitted_by.clone(),
        },
    );
    let task = completed
        .task_states
        .entry(task_id.clone())
        .or_insert_with(|| labello_domain::TaskState::new(task_id.clone(), completed_at));
    task.status = labello_domain::TaskStatus::Completed;
    task.outcome = Some(labello_domain::TaskOutcome::Approved);
    task.updated_at = completed_at;
    // A new active round must still disregard the preceding round's approvals.
    app.work.current_state = Some(completed.clone());
    assert_eq!(app.canonical_migration_review_index(), 0);
    // Keep the outgoing captured state visible until the final confirmation
    // command returns. Its previous round is still the one shown in the
    // overview; `completed` is the fresh-round response for the next load.
    app.work.current_state = Some(outgoing);
    let mut assignment = app.work.assignment.clone().unwrap();
    assignment.status = labello_domain::AssignmentStatus::Completed;
    assignment.updated_at = completed_at;
    completed.assignments = vec![assignment.clone()];
    api.respond_to_next_migration_with(labello_client::ManualMigrationCommandResult {
        image_state: completed,
        cursor: None,
        progress: Default::default(),
        active_pass: None,
        confirmation: None,
        assignment: Some(assignment),
        annotation_id: None,
    });
    let mut next_assignment = app.work.assignment.clone().unwrap();
    next_assignment.assignment_id = labello_domain::AssignmentId::from("next-review");
    next_assignment.image_id = labello_domain::ImageId::from("next-image");
    let mut next_image = app.work.current.clone().unwrap();
    next_image.image.image_id = next_assignment.image_id.clone();
    app.work.queue.clear();
    assert!(app.work.queue.push_prepared(crate::app::LoadedImage {
            prepared_until: None,
            review_submitters: Vec::new(),
            reasons: Vec::new(),
        assignment: next_assignment,
        queued: next_image,
        annotations: Vec::new(),
        state: labello_domain::ImageState::new(labello_domain::ImageId::from("next-image")),
        color_image: None,
    }));
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|_| app);
    harness.run_steps(3);
    assert!(matches!(
        harness.state().current_migration_review_target(),
        Some((
            _,
            labello_client::MigrationReviewTarget::Confirmation { .. }
        ))
    ));
    let original_image = harness
        .state()
        .work
        .current
        .as_ref()
        .unwrap()
        .image
        .image_id
        .clone();
    let overview = harness.state().work.canvas.stored_transform();
    assert_eq!(harness.state().work.canvas.current_zoom(), 1.0);
    harness.key_press(egui::Key::Y);
    let mut saw_pending = false;
    for _ in 0..8 {
        harness.step();
        if harness
            .state()
            .work
            .current
            .as_ref()
            .is_none_or(|current| current.image.image_id != original_image)
        {
            break;
        }
        saw_pending |= harness.state().loading.image;
        assert_eq!(
            harness.state().work.canvas.stored_transform(),
            overview,
            "final migration approval must not refocus an object in the outgoing image"
        );
    }
    assert_eq!(api.counts().migration_commands, 1);
    assert!(saw_pending, "exercise the prepared-review revalidation gap");
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_review_confirmation_is_visible_and_uses_space_on_mobile() {
    use crate::inspector_presets::{self, InspectorPreset};

    let api = Rc::new(SpyApi::new());
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationReview, &egui::Context::default());
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(390.0, 667.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();

    let assert_review_layout =
        |harness: &Harness<'static, LabelloApp>| {
            let confirm = harness.get_by_label("Approve").rect();
            let width = harness.ctx.content_rect().width();
            let workflow = harness.get_by_label("Workflow").rect();
            let inspector = harness.get_by_label_contains("Review details: Workflow:").rect();
            let context = harness.get_by_label("Workspace context bar").rect();
            assert!(confirm.left() <= 70.0 && confirm.right() >= width - 16.0);
            assert!(harness.ctx.content_rect().bottom() - confirm.bottom() < 30.0);
            assert!(workflow.top() >= context.top() && workflow.bottom() <= context.bottom());
            assert!(inspector.top() >= context.top() && inspector.bottom() <= context.bottom());
        };

    harness.set_size(egui::vec2(570.0, 667.0));
    harness.step();
    assert_review_layout(&harness);
    assert!(harness.get_by_label("Workflow").rect().width() <= 44.5);
    assert!(harness.get_by_label_contains("Review details: Workflow:").rect().width() > 80.0);

    harness.set_size(egui::vec2(390.0, 667.0));
    harness.step();
    assert_review_layout(&harness);
    assert!(harness.get_by_label("Workflow").rect().width() <= 44.5);
    assert!(harness.get_by_label_contains("Review details: Workflow:").rect().width() >= 44.0);

    harness.set_size(egui::vec2(260.0, 667.0));
    harness.step();
    assert_review_layout(&harness);
    assert!(harness.get_by_label("Workflow").rect().width() <= 44.5);
    assert!(harness.get_by_label_contains("Review details: Workflow:").rect().width() >= 44.0);

    harness.set_size(egui::vec2(150.0, 667.0));
    harness.step();
    assert_control_inside(&harness, "Approve", egui::accesskit::Role::Button, 150.0, 667.0);

    harness.set_size(egui::vec2(390.0, 667.0));
    harness.step();
    harness.key_press(egui::Key::Space);
    harness.step();
    step_until(&mut harness, 8, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 1);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn discovered_review_targets_are_exact_and_coordinate_less_history_uses_full_image() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [
        egui::vec2(320.0, 320.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0),
        egui::vec2(1440.0, 1000.0),
    ] {
        let mut app = inspector_presets::build(
            InspectorPreset::MigrationDiscoveryReview,
            &egui::Context::default(),
        );
        app.sync_manual_migration();
        let first_id = labello_domain::AnnotationId::from("discovered-object-1");
        assert!(
            matches!(app.current_migration_review_target(), Some((_, labello_client::MigrationReviewTarget::Discovered { annotation_id, version: 1 })) if annotation_id == first_id)
        );
        assert!(app.refocus_annotation().is_some());
        let mut harness = Harness::builder().with_size(size).build_eframe(|_| app);
        harness.step();
        assert!(harness.state().work.canvas.current_zoom() > 1.0);
        let user_id = harness.state().config.user_id.clone();
        let state = harness.state_mut().work.current_state.as_mut().unwrap();
        let review = labello_domain::ReviewRecord {
                review_id: labello_domain::ReviewId::from("discovery-review-test"),
                target: labello_domain::ReviewTarget::AnnotationVersion {
                    annotation_id: first_id,
                    version: 1,
                },
                reviewer_user_id: user_id,
                decision: labello_domain::ReviewDecision::Approved,
                timestamp: labello_domain::now(),
                comment: None,
            };
        state.apply_event(&labello_domain::EventLogEntry::new(
            state.current_sequence + 1, state.image_id.clone(),
            review.reviewer_user_id.clone(), labello_domain::DatasetRole::Reviewer,
            review.timestamp, labello_domain::EventPayload::ReviewRecorded { review },
        )).unwrap();
        harness.step();
        assert!(
            matches!(harness.state().current_migration_review_target(), Some((_, labello_client::MigrationReviewTarget::Discovered { annotation_id, version: 1 })) if annotation_id == labello_domain::AnnotationId::from("discovered-object-2"))
        );
        assert!(harness.state().refocus_annotation().is_none());
        assert_eq!(harness.state().work.canvas.current_zoom(), 1.0);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn companion_reconciliation_success_and_failure_retain_unsaved_skeleton_drafts() {
    use crate::inspector_presets::{self, InspectorPreset};
    for succeeds in [false, true] {
        let mut app = inspector_presets::build(
            InspectorPreset::MigrationDiscovery,
            &egui::Context::default(),
        );
        let annotation_id = labello_domain::AnnotationId::from("discovered-object-1");
        let geometry = match &app
            .work
            .current_state
            .as_ref()
            .unwrap()
            .current_annotation(&annotation_id)
            .unwrap()
            .geometry
        {
            labello_domain::AnnotationGeometry::Skeleton(geometry) => geometry.clone(),
            _ => unreachable!(),
        };
        app.work.migration.adding_missing_object = true;
        app.work.migration.editing_missing_annotation_id = Some(annotation_id.clone());
        app.work.migration.draft = Some(geometry.clone());
        app.work.migration.draft_dirty = true;
        app.work.migration.keypoint_index = 1;
        app.work.migration.next_hidden = true;
        app.work.migration.preserving_companion_draft = true;
        app.work.migration.busy = true;
        let operation_id = 77_700;
        let request = test_request(&app, operation_id, Some("demo"));
        app.runtime.active_requests.insert(operation_id);
        let result = if succeeds {
            let image_state = app.work.current_state.clone().unwrap();
            Ok(labello_client::ManualMigrationCommandResult {
                progress: labello_client::ManualMigrationProgress {
                    expected: 0,
                    annotated: 0,
                    excluded: 0,
                    pending: 0,
                },
                image_state,
                cursor: Some(labello_domain::MigrationCursor::FullImage),
                active_pass: None,
                confirmation: None,
                assignment: app.work.assignment.clone(),
                annotation_id: Some(annotation_id.clone()),
            })
        } else {
            Err("The box has another active assignment. Retry after it is released.".to_string().into())
        };
        app.runtime
            .tx
            .send(UiMessage::MigrationFinished {
                request,
                result: Box::new(result),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert_eq!(app.work.migration.draft, Some(geometry));
        assert_eq!(
            app.work.migration.editing_missing_annotation_id,
            Some(annotation_id)
        );
        assert!(
            app.work.migration.draft_dirty
                && app.work.migration.adding_missing_object
                && app.work.migration.next_hidden
        );
        assert_eq!(app.work.migration.keypoint_index, 1);
        assert!(!app.work.migration.busy);
        assert_eq!(app.work.migration.error.is_some(), !succeeds);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn companion_reconciliation_modal_blocks_shortcuts_and_cancels_without_mutation() {
    use crate::inspector_presets::{self, InspectorPreset};
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationDiscovery,
        &egui::Context::default(),
    );
    app.work.migration.pending_companion_reconciliation =
        Some(labello_domain::AnnotationId::from("discovered-object-1"));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|_| app);
    harness.step();
    assert!(harness.query_by_label("Reconcile companion box?").is_some());
    harness.key_press(egui::Key::ArrowRight);
    harness.step();
    assert!(!harness.state().work.migration.busy);
    harness.key_press(egui::Key::Tab);
    for _ in 0..8 {
        harness.step();
    }
    let regenerate = harness.get_by_label("Regenerate companion box");
    assert!(regenerate.rect().top() >= 0.0 && regenerate.rect().bottom() <= 320.0);
    harness.key_press(egui::Key::Tab);
    for _ in 0..8 {
        harness.step();
    }
    let cancel = harness.get_by_label("Cancel");
    assert!(cancel.rect().top() >= 0.0 && cancel.rect().bottom() <= 320.0);
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(
        harness
            .state()
            .work
            .migration
            .pending_companion_reconciliation
            .is_none()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn discovery_conflict_reload_retains_draft_and_refuses_changed_source_version() {
    use crate::inspector_presets::{self, InspectorPreset};
    for source_changed in [false, true] {
        let mut app = inspector_presets::build(
            InspectorPreset::MigrationDiscovery,
            &egui::Context::default(),
        );
        let id = labello_domain::AnnotationId::from("discovered-object-1");
        let draft = match &app
            .work
            .current_state
            .as_ref()
            .unwrap()
            .current_annotation(&id)
            .unwrap()
            .geometry
        {
            labello_domain::AnnotationGeometry::Skeleton(geometry) => geometry.clone(),
            _ => unreachable!(),
        };
        app.work.migration.draft = Some(draft.clone());
        app.work.migration.draft_dirty = true;
        app.work.migration.adding_missing_object = true;
        app.work.migration.editing_missing_annotation_id = Some(id.clone());
        app.work.migration.reloading_discovery_draft = true;
        let mut state = app.work.current_state.clone().unwrap();
        if source_changed {
            state
                .annotations
                .get_mut(&id)
                .unwrap()
                .last_mut()
                .unwrap()
                .version = 2;
        }
        let loaded = crate::live_protocol::LoadedImage {
            prepared_until: None,
            review_submitters: Vec::new(),
            reasons: Vec::new(),
            assignment: app.work.assignment.clone().unwrap(),
            queued: app.work.current.clone().unwrap(),
            annotations: state.active_annotations().cloned().collect(),
            state,
            color_image: None,
        };
        let operation_id = 77_800;
        let request = test_request(&app, operation_id, Some("demo"));
        app.work.active_load_id = Some(operation_id);
        app.runtime.active_requests.insert(operation_id);
        app.runtime
            .tx
            .send(UiMessage::ImageLoaded {
                request,
                operation_id,
                assignment: app.work.assignment.clone(),
                result: Box::new(Ok(Some(loaded))),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert_eq!(app.work.migration.draft, Some(draft));
        assert!(app.work.migration.draft_dirty && app.work.migration.adding_missing_object);
        assert_eq!(app.work.migration.error.is_some(), source_changed);
        assert_eq!(
            app.work
                .current_state
                .as_ref()
                .unwrap()
                .current_annotation(&id)
                .unwrap()
                .version,
            1
        );
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn companion_reconciliation_escape_restores_invoking_button_focus() {
    use crate::inspector_presets::{self, InspectorPreset};
    let app = inspector_presets::build(
        InspectorPreset::MigrationDiscovery,
        &egui::Context::default(),
    );
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|_| app);
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.step();
    click(&mut harness, "Companion boxes: 0 of 2 paired");
    click_accesskit_button(&mut harness, "Reconcile box for added object 1");
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(
        harness
            .get_by_label("Reconcile box for added object 1")
            .is_focused()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_revisit_saves_non_final_target_and_restores_focus() {
    use crate::inspector_presets::{self, InspectorPreset};
    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.state_mut().work.inspector_panel_collapsed = false;
    harness.step();
    click(&mut harness, "Review 2 resolved objects");
    let entry = harness.get_by_role_and_label(
        egui::accesskit::Role::Button,
        "Person: Migration · Person skeleton migration",
    );
    entry.click();
    harness.step();
    step_until(&mut harness, 10, |app| !app.work.migration.busy);
    assert!(
        matches!(harness.state().work.migration.cursor, Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. }) if object_group_id.as_str() == "group-left")
    );
    let mut saved_state = harness.state().work.current_state.clone().unwrap();
    saved_state
        .migration_dependencies
        .get_mut(&TaskId::from("skeleton:person"))
        .unwrap()
        .remove(&labello_domain::ObjectGroupId::from("group-left"));
    api.respond_to_next_migration_with(labello_client::ManualMigrationCommandResult {
        image_state: saved_state,
        cursor: Some(labello_domain::MigrationCursor::FullImage),
        progress: labello_client::ManualMigrationProgress {
            expected: 2,
            annotated: 1,
            excluded: 1,
            pending: 0,
        },
        active_pass: None,
        confirmation: None,
        assignment: harness.state().work.assignment.clone(),
        annotation_id: None,
    });
    click_accesskit_button(&mut harness, "Save object changes");
    step_until(&mut harness, 10, |app| !app.work.migration.busy);
    harness.step();
    assert_eq!(
        harness.state().work.migration.cursor,
        Some(labello_domain::MigrationCursor::FullImage)
    );
    assert_eq!(api.counts().migration_commands, 2);
    assert!(
        harness
            .get_by_label("Guide 1: Skeleton annotated; Guide present")
            .is_focused()
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn direct_revisit_canvas_selects_completed_skeleton_without_drag_activation() {
    use crate::inspector_presets::{self, InspectorPreset};
    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    let skeleton = app
        .work
        .current_state
        .as_ref()
        .unwrap()
        .current_annotation(&labello_domain::AnnotationId::from("reserved-left"))
        .cloned();
    let skeleton = skeleton.unwrap_or_else(|| {
        app.work
            .current_state
            .as_ref()
            .unwrap()
            .active_annotations()
            .find(|a| a.annotation_type == labello_domain::AnnotationType::Skeleton)
            .unwrap()
            .clone()
    });
    let labello_domain::AnnotationGeometry::Skeleton(geometry) = skeleton.geometry else {
        unreachable!()
    };
    let point = geometry.keypoints.iter().find_map(|p| p.point).unwrap();
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .with_max_steps(40)
        .build_eframe(|_| app);
    harness.step();
    let canvas = harness.get_by_label("Annotation canvas").rect();
    let aspect = 1280.0 / 800.0;
    let size = if canvas.width() / canvas.height() > aspect {
        egui::vec2(canvas.height() * aspect, canvas.height())
    } else {
        egui::vec2(canvas.width(), canvas.width() / aspect)
    };
    let image = egui::Rect::from_center_size(canvas.center(), size);
    let position = egui::pos2(
        image.left() + image.width() * point.x,
        image.top() + image.height() * point.y,
    );
    drag_at(&mut harness, position, position + egui::vec2(20.0, 10.0));
    assert_eq!(api.counts().migration_commands, 0);
    harness.state_mut().work.canvas = Default::default();
    harness.step();
    click_at(&mut harness, position);
    step_until(&mut harness, 10, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 1);
    assert!(
        matches!(harness.state().work.migration.cursor,Some(labello_domain::MigrationCursor::Object{ref object_group_id,..}) if object_group_id.as_str()=="group-left")
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn direct_revisit_retry_keeps_exact_request_key_and_unsaved_workspace() {
    use crate::inspector_presets::{self, InspectorPreset};
    let mut app = inspector_presets::build(
        InspectorPreset::MigrationFullImage,
        &egui::Context::default(),
    );
    app.work.migration.draft_dirty = true;
    app.work.migration.exclusion_note = "retained synthetic draft".into();
    app.work.migration.pending_revisit_target =
        Some(labello_domain::ObjectGroupId::from("group-left"));
    app.confirm_pending_migration_revisit();
    let command = app.runtime.commands.pop_back().unwrap();
    let UiCommand::Migration {
        request,
        idempotency_key: first,
        ..
    } = command
    else {
        panic!("expected migration command")
    };
    app.runtime.active_requests.insert(request.request_id);
    app.runtime
        .tx
        .send(UiMessage::MigrationFinished {
            request,
            result: Box::new(Err("synthetic transport failure".to_string().into())),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.work.migration.draft_dirty);
    assert_eq!(
        app.work.migration.exclusion_note,
        "retained synthetic draft"
    );
    assert_eq!(
        app.work.migration.cursor,
        Some(labello_domain::MigrationCursor::FullImage)
    );
    app.request_revisit_migration_target(labello_domain::ObjectGroupId::from("group-left"));
    let UiCommand::Migration {
        idempotency_key: second,
        ..
    } = app.runtime.commands.pop_back().unwrap()
    else {
        panic!("expected retry")
    };
    assert_eq!(first, second);
    app.request_revisit_migration_target(labello_domain::ObjectGroupId::from("group-right"));
    assert!(
        app.runtime.commands.is_empty(),
        "busy activation must not duplicate work"
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn direct_revisit_excluded_overview_is_keyboard_accessible_in_compact_and_medium_layouts() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [
        egui::vec2(320.0, 320.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0),
    ] {
        let api = Rc::new(SpyApi::new());
        let mut app = inspector_presets::build(
            InspectorPreset::MigrationFullImage,
            &egui::Context::default(),
        );
        api.set_image_state(app.work.current_state.clone().unwrap());
        app.runtime.api = Some(api.clone());
        let mut harness = Harness::builder()
            .with_size(size)
            .with_max_steps(40)
            .build_eframe(|_| app);
        harness.step();
        click_accesskit_button(&mut harness, "Inspector");
        harness.get_by_label("Review 2 resolved objects").focus();
        harness.step();
        harness.key_press(egui::Key::Enter);
        harness.step();
        let entry = harness.get_by_role_and_label(
            egui::accesskit::Role::Button,
            "Guide 2: Excluded; Guide present",
        );
        entry.focus();
        for _ in 0..12 {
            harness.step();
        }
        let visible = harness
            .get_by_label("Guide 2: Excluded; Guide present")
            .rect();
        assert!(
            visible.top() >= 0.0 && visible.bottom() <= size.y,
            "focused entry clipped: {visible:?} in {size:?}"
        );
        harness.key_press(egui::Key::Enter);
        harness.step();
        step_until(&mut harness, 10, |app| !app.work.migration.busy);
        assert!(
            matches!(harness.state().work.migration.cursor,Some(labello_domain::MigrationCursor::Object{ref object_group_id,..})if object_group_id.as_str()=="group-right")
        );
        assert_eq!(api.counts().migration_commands, 1);
        assert!(harness.state().work.migration.draft.is_some());
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn direct_revisit_conflict_reload_requires_discard_confirmation() {
    use crate::inspector_presets::{self, InspectorPreset};
    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    app.work.migration.draft_dirty = true;
    app.reload_migration_assignment();
    assert!(app.work.migration.pending_reload_discard);
    assert!(app.work.migration.draft_dirty);
    app.cancel_pending_migration_revisit();
    assert!(!app.work.migration.pending_reload_discard);
    assert!(app.work.migration.draft_dirty);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn direct_revisit_returns_focus_to_primary_action_when_overview_is_closed() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [
        egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0),
        egui::vec2(1440.0, 1000.0),
    ] {
        let mut app = inspector_presets::build(
            InspectorPreset::MigrationFullImage,
            &egui::Context::default(),
        );
        app.work.inspector_panel_collapsed = true;
        app.work.migration.direct_revisit_group =
            Some(labello_domain::ObjectGroupId::from("group-left"));
        app.work.migration.restore_revisit_focus = true;
        let mut harness = Harness::builder().with_size(size).build_eframe(|_| app);
        harness.step();
        assert!(harness.get_by_label("Submit").is_focused());
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_full_image_has_direct_controls_without_a_global_pass_start() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [
        egui::vec2(320.0, 320.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0),
        egui::vec2(1440.0, 1000.0),
    ] {
        let app = inspector_presets::build(
            InspectorPreset::MigrationFullImage,
            &egui::Context::default(),
        );
        let mut harness = Harness::builder().with_size(size).build_eframe(|_| app);
        harness.state_mut().work.inspector_panel_collapsed = false;
        harness.step();
        assert!(harness.query_by_label("Start correction pass").is_none());
        assert!(
            harness
                .query_by_label_contains("Submit")
                .is_some()
        );
        if size.x < 1318.0 {
            harness.state_mut().work.drawer = Some(Drawer::Inspector);
            harness.step();
        }
        assert!(harness.query_by_label("Start correction pass").is_none());
        let overview = harness.get_by_label("Review 2 resolved objects");
        overview.focus();
        harness.key_press(egui::Key::Enter);
        for _ in 0..12 {
            harness.step();
        }
        assert!(
            harness
                .query_by_label("Guide 1: Skeleton annotated; Guide present")
                .is_some()
        );
        assert!(
            harness
                .query_by_label("Guide 2: Excluded; Guide present")
                .is_some()
        );
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn historical_migration_pass_reloads_and_resolves_through_normal_controls() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [egui::vec2(390.0, 844.0), egui::vec2(1440.0, 1000.0)] {
        let api = Rc::new(SpyApi::new());
        let mut app =
            inspector_presets::build(InspectorPreset::MigrationPass, &egui::Context::default());
        let persisted = app.work.current_state.clone().unwrap();
        let task_id = app.work.selected_task_id.clone().unwrap();
        let pass_id = persisted.migration_passes.keys().next().unwrap().clone();
        app.work.migration = Default::default();
        api.set_image_state(persisted);
        app.runtime.api = Some(api.clone());
        let mut harness = Harness::builder().with_size(size).build_eframe(|_| app);
        harness.step();
        assert_eq!(
            harness.state().work.migration.active_pass_id.as_ref(),
            Some(&pass_id)
        );
        for index in 0..2 {
            assert!(harness.query_by_label("Start correction pass").is_none());
            let mut state = harness.state().work.current_state.clone().unwrap();
            let target = state.migration_target_sets[&task_id].targets[index].clone();
            assert!(
                matches!(harness.state().work.migration.cursor.as_ref(), Some(labello_domain::MigrationCursor::Object { object_group_id, .. }) if *object_group_id == target.object_group_id)
            );
            let guide = state
                .current_annotation(&target.guide_annotation_id)
                .unwrap();
            let item = labello_domain::MigrationPassItem {
                object_group_id: target.object_group_id.clone(),
                guide_annotation_version: guide.version,
                guide_deleted: guide.deleted,
                disposition_version: state.migration_dispositions[&task_id]
                    [&target.object_group_id]
                    .disposition_version,
                action: labello_domain::MigrationPassItemAction::Kept,
                event_id: EventId::from(format!("recovery-kept-{index}")),
            };
            state
                .migration_passes
                .get_mut(&pass_id)
                .unwrap()
                .items
                .push(item);
            let cursor = state.migration_cursor(&task_id, Some(&pass_id)).unwrap();
            let active_pass = state.migration_passes[&pass_id].clone();
            api.respond_to_next_migration_with(labello_client::ManualMigrationCommandResult {
                image_state: state,
                cursor: Some(cursor),
                active_pass: Some(active_pass),
                progress: labello_client::ManualMigrationProgress {
                    expected: 2,
                    annotated: 1,
                    excluded: 1,
                    pending: 0,
                },
                confirmation: None,
                assignment: harness.state().work.assignment.clone(),
                annotation_id: None,
            });
            let label = if size.x < 1318.0 {
                "Keep & next"
            } else {
                "Keep current & advance"
            };
            click_accesskit_button(&mut harness, label);
            step_until(&mut harness, 10, |app| !app.work.migration.busy);
        }
        assert_eq!(
            harness.state().work.migration.cursor,
            Some(labello_domain::MigrationCursor::FullImage)
        );
        assert!(
            harness
                .query_by_label_contains("Submit")
                .is_some()
        );
        harness.step();
        assert_eq!(missing_object_scan_frames(&harness).len(), 1,
            "advancing past the final guide activates the scan cue");
        assert_eq!(api.counts().migration_commands, 2);
        harness.state_mut().trigger_migration_primary_action();
        assert!(
            harness
                .state()
                .runtime
                .commands
                .iter()
                .any(|command| matches!(
                    command,
                    UiCommand::Migration {
                        action: crate::app::MigrationAction::Confirm(_),
                        ..
                    }
                ))
        );
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn untouched_migration_navigation_releases_but_keypoint_input_remains_protected() {
    use crate::inspector_presets::{self, InspectorPreset};
    for touched in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut app = inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
        api.state.borrow_mut().active_assignments.push(app.work.assignment.clone().unwrap());
        api.set_image_state(app.work.current_state.clone().unwrap());
        app.runtime.api = Some(api.clone());
        let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_eframe(|_| app);
        harness.step();
        assert!(!harness.state().assignment_has_work());
        if touched {
            let canvas = harness.get_by_label("Annotation canvas").rect();
            click_at(&mut harness, canvas.center());
            assert!(harness.state().assignment_has_work());
            harness.state_mut().remove_last_migration_keypoint();
            assert!(harness.state().assignment_has_work());
        }
        harness.state_mut().open_view(AppView::Setup);
        assert_eq!(harness.state().loading.saving, !touched);
        harness.step();
        if touched {
            assert!(harness.state().work.pending_transition.is_some());
            assert_eq!(api.counts().release_assignment, 0);
        } else {
            assert!(harness.query_by_label("Switch active assignment?").is_none());
            step_until(&mut harness, 12, |app| app.view == AppView::Setup);
            assert_eq!(api.counts().release_assignment, 1);
        }
    }
}

#[test]
fn migration_review_button_and_space_approve_after_retaining_a_correction() {
    use crate::inspector_presets::{self, InspectorPreset};
    // Storage covers admission and round transitions; this fake isolates input dispatch.
    for use_space in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut app =
            inspector_presets::build(InspectorPreset::MigrationReview, &egui::Context::default());
        let task_id = app.work.selected_task_id.clone().unwrap();
        app.work
            .tasks
            .iter_mut()
            .find(|task| task.task_id == task_id)
            .unwrap()
            .skeleton = Some(SkeletonSpec {
            keypoints: vec![KeypointSpec {
                name: "head".into(),
                required: true,
            }],
            edges: vec![],
            allow_hidden: true,
            allow_absent: false,
        });
        let state = app.work.current_state.as_mut().unwrap();
        state.reviews.clear();
        let targets = state.migration_target_sets[&task_id].targets.clone();
        let mut skeleton = state
            .current_annotation(&targets[0].reserved_skeleton_annotation_id)
            .unwrap()
            .clone();
        skeleton.annotation_id = targets[1].reserved_skeleton_annotation_id.clone();
        skeleton.object_group_id = Some(targets[1].object_group_id.clone());
        state
            .annotations
            .insert(skeleton.annotation_id.clone(), vec![skeleton.clone()]);
        state
            .migration_dispositions
            .get_mut(&task_id)
            .unwrap()
            .get_mut(&targets[1].object_group_id)
            .unwrap()
            .status = MigrationDispositionStatus::Annotated {
            skeleton_annotation_id: skeleton.annotation_id,
            skeleton_version: 1,
        };
        app.work.annotations = state.active_annotations().cloned().collect();
        api.set_image_state(state.clone());
        app.work.migration.review_index = 0;
        app.runtime.api = Some(api.clone());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run_steps(3);
        assert_eq!(harness.state().review_position(), 0);
        let id = harness
            .state()
            .work
            .correction_draft
            .as_ref()
            .unwrap()
            .annotation_id
            .clone();
        harness
            .state_mut()
            .edit_correction_keypoint(crate::canvas::KeypointEdit {
                annotation_id: id,
                keypoint_index: 0,
                point: NormalizedPoint { x: 0.4, y: 0.4 },
            });
        harness.run_steps(2);
        click(&mut harness, "Submit correction");
        harness.run_steps(3);
        assert_eq!(harness.state().review_position(), 1);
        assert!(
            harness.state().review_can_approve(),
            "next unchanged skeleton must allow approval"
        );
        if use_space {
            harness.key_press(egui::Key::Space);
        } else {
            click(&mut harness, "Approve");
        }
        harness.run_steps(5);
        assert!(
            harness.state().review_overview(),
            "approval must advance past the second skeleton: position={}, error={:?}, migration_error={:?}",
            harness.state().review_position(),
            harness.state().runtime.error,
            harness.state().work.migration.error
        );
        assert_eq!(missing_object_scan_frames(&harness).len(), 1,
            "migration review uses the same overview cue");
    }
}

#[cfg(feature = "inspector-presets")]
fn missing_object_scan_frames(harness: &Harness<'_, LabelloApp>) -> Vec<egui::Rect> {
    fn collect(shape: &egui::Shape, frames: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Rect(rect)
                if rect.stroke == egui::Stroke::new(4.0, crate::theme::INFO) =>
            {
                frames.push(rect.rect);
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, frames);
                }
            }
            _ => {}
        }
    }
    let mut frames = Vec::new();
    for shape in &harness.output().shapes {
        collect(&shape.shape, &mut frames);
    }
    frames
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_scan_cue_tracks_phase_without_covering_the_canvas() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [
        egui::vec2(320.0, 320.0),
        egui::vec2(320.0, 568.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0),
        egui::vec2(1288.0, 820.0),
        egui::vec2(1440.0, 1000.0),
    ] {
        for preset in [InspectorPreset::MigrationFullImage, InspectorPreset::MigrationDiscovery] {
            let app = inspector_presets::build(preset, &egui::Context::default());
            let mut harness = Harness::builder().with_size(size).build_eframe(|_| app);
            harness.step();
            let frames = missing_object_scan_frames(&harness);
            assert_eq!(frames.len(), 1, "missing overview cue at {size:?}");
            let canvas = harness.get_by_label("Annotation canvas").rect();
            assert!(frames[0].shrink(7.0).contains_rect(canvas));
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(frames[0]));
            harness.state_mut().work.canvas.zoom_in();
            harness.step();
            assert_eq!(missing_object_scan_frames(&harness), frames, "scan phase survives zoom");
            if matches!(preset, InspectorPreset::MigrationFullImage) {
                harness.state_mut().work.migration.inspected_group_id =
                    Some(labello_domain::ObjectGroupId::from("group-left"));
                harness.step();
                assert!(missing_object_scan_frames(&harness).is_empty(), "focused guide clears cue");
                harness.state_mut().work.migration.inspected_group_id = None;
                harness.step();
                assert_eq!(missing_object_scan_frames(&harness), frames);
            }
            harness.state_mut().work.current_texture = None;
            harness.step();
            assert!(missing_object_scan_frames(&harness).is_empty(), "no scan cue without an image");
        }
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_scan_cue_does_not_treat_fit_as_workflow_completion() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [InspectorPreset::MigrationObject, InspectorPreset::MigrationPass, InspectorPreset::MigrationReview, InspectorPreset::MigrationDiscoveryReview] {
        let app = inspector_presets::build(preset, &egui::Context::default());
        let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_eframe(|_| app);
        harness.step();
        harness.state_mut().work.canvas.fit_view();
        harness.step();
        assert!(missing_object_scan_frames(&harness).is_empty(), "fit must not complete {preset:?}");
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_scan_cue_settles_once_and_respects_motion_and_input() {
    use crate::inspector_presets::{self, InspectorPreset};
    fn image_rect(harness: &Harness<'_, LabelloApp>) -> egui::Rect {
        let texture = harness.state().work.current_texture.as_ref().unwrap().id();
        harness.output().shapes.iter().find_map(|shape| {
            if let egui::Shape::Mesh(mesh) = &shape.shape {
                (mesh.texture_id == texture).then(|| mesh.calc_bounds())
            } else { None }
        }).expect("painted image")
    }
    let mut harness = Harness::builder().with_size(egui::vec2(1288.0, 820.0))
        .build_eframe(|ctx| inspector_presets::build(InspectorPreset::MigrationFullImage, &ctx.egui_ctx));
    harness.step();
    let full = image_rect(&harness);
    crate::set_reduced_motion(&harness.ctx, false);
    harness.state_mut().work.migration.inspected_group_id = Some("group-left".into());
    harness.step();
    harness.state_mut().work.migration.inspected_group_id = None;
    harness.input_mut().time = Some(10.0);
    harness.step();
    let transform = harness.state().work.canvas.stored_transform();
    assert!(harness.state().work.canvas.scan_emphasis() > 0.9);
    harness.input_mut().time = Some(10.022);
    harness.step();
    let contracted = image_rect(&harness);
    assert!(contracted.width() < full.width() * 0.93, "phase entry must visibly contract");
    assert!(contracted.center().distance(full.center()) < 0.01);
    assert_eq!(harness.state().work.canvas.stored_transform(), transform, "motion must not enter persisted view preferences");
    harness.input_mut().time = Some(10.05);
    harness.step();
    assert!((image_rect(&harness).width() - full.width()).abs() < 0.1, "rebound");
    harness.input_mut().time = Some(10.075);
    harness.step();
    assert!(image_rect(&harness).width() > contracted.width());
    assert!(image_rect(&harness).width() < full.width());
    harness.input_mut().time = Some(10.101);
    harness.step();
    assert_eq!(image_rect(&harness), full);
    assert_eq!(harness.state().work.canvas.scan_emphasis(), 0.0);
    assert_eq!(missing_object_scan_frames(&harness).len(), 1);
    harness.state_mut().work.canvas.fit_view();
    harness.step();
    assert_eq!(image_rect(&harness), full, "manual fit must not replay motion");

    // A preference change stops active motion and never restarts it mid-phase.
    harness.state_mut().work.migration.inspected_group_id = Some("group-left".into());
    harness.step();
    harness.state_mut().work.migration.inspected_group_id = None;
    harness.step();
    assert!(harness.state().work.canvas.scan_emphasis() > 0.0);
    crate::set_reduced_motion(&harness.ctx, true);
    harness.step();
    assert_eq!(image_rect(&harness), full);
    assert_eq!(missing_object_scan_frames(&harness).len(), 1);
    crate::set_reduced_motion(&harness.ctx, false);
    harness.step();
    assert_eq!(harness.state().work.canvas.scan_emphasis(), 0.0);

    harness.state_mut().work.migration.inspected_group_id = Some("group-left".into());
    harness.step();
    harness.state_mut().work.migration.inspected_group_id = None;
    harness.step();
    assert!(harness.state().work.canvas.scan_emphasis() > 0.0);
    click_at(&mut harness, full.center());
    assert_eq!(harness.state().work.canvas.scan_emphasis(), 0.0);
    assert_eq!(image_rect(&harness), full);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn deleting_unsaved_missing_migration_object_returns_to_overview() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|ctx| inspector_presets::build(InspectorPreset::MigrationFullImage, &ctx.egui_ctx));
    harness.step();
    let center = harness.get_by_label("Annotation canvas").rect().center();
    click_at(&mut harness, center);
    assert!(harness.state().work.migration.adding_missing_object);
    harness.key_press(egui::Key::Delete);
    harness.step();
    assert!(!harness.state().work.migration.adding_missing_object);
    assert!(harness.state().work.migration.draft.is_none());
    assert!(!harness.state().work.migration.draft_dirty);
    assert!(harness.query_by_label("Discard object changes").is_none());
    click_at(&mut harness, center);
    assert!(harness.state().work.migration.adding_missing_object);
    assert_eq!(harness.state().work.migration.keypoint_index, 1);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_with_added_object_and_no_guides_offers_submit() {
    use crate::inspector_presets::{self, InspectorPreset};

    let mut app = inspector_presets::build(InspectorPreset::MigrationDiscovery, &egui::Context::default());
    let task_id = app.work.selected_task_id.clone().unwrap();
    let state = app.work.current_state.as_mut().unwrap();
    state.migration_target_sets.get_mut(&task_id).unwrap().targets.clear();
    state.migration_dispositions.get_mut(&task_id).unwrap().clear();
    app.work.migration.progress = None;
    app.work.migration.cursor = Some(labello_domain::MigrationCursor::FullImage);
    app.work.inspector_panel_collapsed = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|_| app);
    for size in [egui::vec2(1440.0, 900.0), egui::vec2(390.0, 844.0)] {
        harness.set_size(size);
        harness.step();
        assert!(harness.query_by_label("Submit").is_some());
        assert!(harness.query_by_label_contains("Confirm no guides").is_none());
        assert!(harness.query_by_label_contains("needs no skeletons").is_none());
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_multi_keypoint_object_requires_confirmation_before_next_object() {
    use crate::inspector_presets::{self, InspectorPreset};
    let api = Rc::new(SpyApi::new());
    let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
    let task_id = app.work.selected_task_id.clone().unwrap();
    app.work.tasks.iter_mut().find(|task| task.task_id == task_id).unwrap().skeleton = Some(SkeletonSpec {
        keypoints: vec![KeypointSpec { name: "first".into(), required: true }, KeypointSpec { name: "second".into(), required: true }],
        edges: vec![], allow_hidden: true, allow_absent: false,
    });
    api.set_image_state(app.work.current_state.clone().unwrap());
    app.runtime.api = Some(api.clone());
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_eframe(|_| app);
    harness.run_steps(3);
    let first = harness.get_by_label("Annotation canvas").rect().center();
    click_at(&mut harness, first);
    assert_eq!(harness.state().work.migration.keypoint_index, 1);
    harness.step();
    assert!(harness.get_by_label("Save missing object").accesskit_node().is_disabled());
    click_at(&mut harness, first + egui::vec2(70.0, 0.0));
    assert_eq!(harness.state().work.migration.keypoint_index, 2);
    let draft = harness.state().work.migration.draft.clone();
    let next = first + egui::vec2(-90.0, 100.0);
    click_at(&mut harness, next);
    assert_eq!(api.counts().migration_commands, 0);
    assert_eq!(harness.state().work.migration.draft, draft);
    assert!(harness.state().work.migration.pending_overview_intent.is_none());
    click_accesskit_button(&mut harness, "Save missing object");
    step_until(&mut harness, 12, |app| !app.work.migration.busy);
    assert_eq!(api.counts().migration_commands, 1);
    assert!(!harness.state().work.migration.adding_missing_object);
    click_at(&mut harness, next);
    assert_eq!(harness.state().work.migration.keypoint_index, 1);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_added_object_deletion_button_and_key_retry_and_exit_editor() {
    use crate::inspector_presets::{self, InspectorPreset};
    for button in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut app = inspector_presets::build(InspectorPreset::MigrationDiscovery, &egui::Context::default());
        let id = labello_domain::AnnotationId::from("discovered-object-1");
        api.set_image_state(app.work.current_state.clone().unwrap());
        app.runtime.api = Some(api.clone());
        let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_eframe(|_| app);
        harness.run_steps(3);
        harness.state_mut().resume_migration_overview_intent(crate::manual_migration::MigrationOverviewIntent::Select(id.clone(), None));
        harness.step();
        let draft = harness.state().work.migration.draft.clone();
        assert!(draft.is_some());
        api.fail_next_migration();
        for attempt in 0..2 {
            if button { click_accesskit_button(&mut harness, "Remove added object"); }
            else { harness.key_press(egui::Key::Delete); }
            harness.step();
            step_until(&mut harness, 12, |app| !app.work.migration.busy);
            assert_eq!(api.counts().migration_commands, attempt + 1);
            if attempt == 0 {
                assert!(harness.state().work.migration.error.is_some());
                assert_eq!(harness.state().work.migration.draft, draft);
                assert!(harness.state().work.migration.adding_missing_object);
                assert!(!harness.state().work.current_state.as_ref().unwrap().current_annotation(&id).unwrap().deleted);
            }
        }
        assert!(harness.state().work.current_state.as_ref().unwrap().current_annotation(&id).unwrap().deleted);
        assert!(!harness.state().work.migration.adding_missing_object);
        assert!(harness.state().work.migration.draft.is_none());
        assert!(harness.query_by_label("Remove added object").is_none());
        assert!(harness.query_by_label("Discard object changes").is_none());
        assert!(harness.query_by_label("Submit").is_some());
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_single_keypoint_press_drag_repositions_without_advancing() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [egui::vec2(1440.0, 1000.0), egui::vec2(390.0, 844.0)] {
        let mut harness = Harness::builder().with_size(size).build_eframe(|ctx|
            inspector_presets::build(InspectorPreset::MigrationSingleOptional, &ctx.egui_ctx));
        harness.run_steps(3);
        let cursor = harness.state().work.migration.cursor.clone();
        let center = harness.get_by_label("Annotation canvas").rect().center();
        let first = center + egui::vec2(20.0, 20.0);
        drag_at(&mut harness, center, first);
        let before = harness.state().work.migration.draft.clone().unwrap();
        assert!(before.keypoints[0].point.is_some(), "initial held press must place a point");
        assert_eq!(harness.state().work.migration.keypoint_index, 1);
        harness.key_press(egui::Key::H);
        harness.step();
        let start = center + egui::vec2(-30.0, -30.0);
        let end = center + egui::vec2(-10.0, -10.0);
        drag_at(&mut harness, start, end);
        let moved = harness.state().work.migration.draft.clone().unwrap();
        assert_ne!(moved.keypoints[0].point, before.keypoints[0].point);
        assert_eq!(moved.keypoints[0].state, KeypointState::Hidden);
        assert_eq!(moved.keypoints.len(), 1);
        assert_eq!(harness.state().work.migration.keypoint_index, 1);
        assert_eq!(harness.state().work.migration.cursor, cursor);
        assert!(!harness.state().work.migration.adding_missing_object);
        click_at(&mut harness, first);
        assert_ne!(harness.state().work.migration.draft.as_ref().unwrap().keypoints[0].point, moved.keypoints[0].point);

        let retained = harness.state().work.migration.draft.clone();
        harness.event(egui::Event::PointerMoved(start));
        harness.event(egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        harness.step();
        harness.event(egui::Event::PointerMoved(end));
        harness.step();
        assert!(harness.state().work.canvas.is_dragging());
        assert_eq!(harness.state().work.migration.draft, retained, "held placement is a preview until release");
        harness.key_press(egui::Key::Escape);
        harness.step();
        harness.event(egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        harness.step();
        assert!(!harness.state().work.canvas.is_dragging());
        assert_eq!(harness.state().work.migration.draft, retained);
    }
}

/// Supplies repository-owned synthetic state for the manual WASM input check.
#[cfg(feature = "inspector-presets")]
#[test]
#[ignore = "writes synthetic fixtures to LABELLO_MIGRATION_BROWSER_FIXTURE"]
fn export_keypoint_migration_browser_fixture() {
    use crate::inspector_presets::{self, InspectorPreset};
    let fixtures: Vec<_> = [InspectorPreset::MigrationSingleOptional, InspectorPreset::MigrationFullImage]
        .into_iter().map(|preset| {
            let mut app = inspector_presets::build(preset, &egui::Context::default());
            let task_id = app.work.selected_task_id.clone().unwrap();
            let task = app.work.tasks.iter_mut().find(|task| task.task_id == task_id).unwrap();
            task.skeleton = Some(SkeletonSpec {
                keypoints: vec![KeypointSpec { name: "center".into(), required: true }],
                edges: vec![], allow_hidden: true, allow_absent: false,
            });
            let mut metadata = app.datasets.metadata.clone().unwrap();
            metadata.tasks = app.work.tasks.clone();
            serde_json::json!({
                "metadata": metadata, "account": app.auth.account,
                "assignment": app.work.assignment, "state": app.work.current_state,
                "image": app.work.current.as_ref().unwrap().image, "keybindings": app.work.keybindings,
            })
        }).collect();
    let path = std::env::var("LABELLO_MIGRATION_BROWSER_FIXTURE").expect("fixture output path");
    std::fs::write(path, serde_json::to_vec(&fixtures).unwrap()).unwrap();
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_missing_objects_respects_migration_phase_and_preserves_drafts() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::panels::WorkflowActivity;
    for preset in [InspectorPreset::MigrationObject, InspectorPreset::MigrationFullImage] {
        let app = inspector_presets::build(preset, &egui::Context::default());
        let workflow = app.selected_workflow().unwrap();
        let missing = app.workflow_entry_label(&workflow, Some(WorkflowActivity::MissingObjects));
        let migration = app.workflow_entry_label(&workflow, Some(WorkflowActivity::Migration));
        let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_eframe(|_| app);
        harness.run();
        let is_overview = preset == InspectorPreset::MigrationFullImage;
        let node = harness.get_by_role_and_label(egui::accesskit::Role::Button, &missing);
        assert_eq!(node.accesskit_node().is_disabled(), !is_overview);
        assert_eq!(node.accesskit_node().toggled(), Some(if is_overview {
            egui::accesskit::Toggled::True
        } else { egui::accesskit::Toggled::False }));
        if !is_overview {
            assert!(!harness.state().work.migration.adding_missing_object);
            continue;
        }
        click_accesskit_button(&mut harness, &missing);
        assert!(harness.state().work.migration.adding_missing_object);
        let draft = harness.state().work.migration.draft.clone();
        click_accesskit_button(&mut harness, &missing);
        assert_eq!(harness.state().work.migration.draft, draft);
        assert!(harness.state().work.migration.adding_missing_object, "reselecting the activity must not cancel the draft");
        harness.state_mut().work.migration.draft_dirty = true;
        click_accesskit_button(&mut harness, &migration);
        assert!(harness.state().work.migration.pending_revisit_target.is_some());
        assert_eq!(harness.state().work.migration.draft, draft);
        harness.state_mut().cancel_pending_migration_revisit();
        assert!(harness.state().work.migration.draft_dirty);
        assert_eq!(harness.state().work.migration.draft, draft);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_overview_actions_do_not_depend_on_new_assignment_availability() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::panels::WorkflowActivity;
    let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
    let workflow = app.selected_workflow().unwrap();
    let missing = app.workflow_entry_label(&workflow, Some(WorkflowActivity::MissingObjects));
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.resolved = true;
    app.work.availability.tasks.insert(workflow.task_id.clone(), false);
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_eframe(|_| app);
    harness.run();
    assert!(!harness.get_by_role_and_label(egui::accesskit::Role::Button, &missing).accesskit_node().is_disabled());
    harness.state_mut().work.migration.busy = true;
    harness.run();
    assert!(harness.get_by_role_and_label(egui::accesskit::Role::Button, &missing).accesskit_node().is_disabled());
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_migration_configuration_keeps_direct_skeleton_assignments_accessible() {
    use crate::inspector_presets::{self, InspectorPreset};
    let mut app = inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    app.work.current_state.as_mut().unwrap().migration_target_sets.clear();
    assert!(!app.manual_migration_active());
    let label = app.workflow_entry_label(&app.selected_workflow().unwrap(), Some(crate::panels::WorkflowActivity::Skeleton));
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_eframe(|_| app);
    harness.run();
    let chooser = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Person: Skeleton annotation · Choose workflow");
    assert_eq!(chooser.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    chooser.click();
    harness.run();
    let entry = harness.get_by_role_and_label(egui::accesskit::Role::Button, &label);
    assert!(!entry.accesskit_node().is_disabled());
    assert_eq!(entry.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::Button, "Person: Migration").is_none());
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_missing_objects_is_keyboard_reachable_in_scrolling_drawers() {
    use crate::inspector_presets::{self, InspectorPreset};
    for (width, height) in [(320.0, 320.0), (320.0, 568.0), (390.0, 844.0), (600.0, 800.0), (1288.0, 820.0), (1440.0, 1000.0)] {
        let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
        app.work.drawer = (LayoutMode::for_width(width) != LayoutMode::Wide).then_some(Drawer::Workflow);
        let workflow = app.selected_workflow().unwrap();
        let label = app.workflow_entry_label(&workflow, Some(crate::panels::WorkflowActivity::MissingObjects));
        let migration = app.workflow_entry_label(&workflow, Some(crate::panels::WorkflowActivity::Migration));
        let mut harness = Harness::builder().with_size(egui::vec2(width, height)).with_max_steps(40).build_eframe(|_| app);
        harness.run();
        harness.get_by_role_and_label(egui::accesskit::Role::Button, &migration).focus();
        harness.run();
        harness.key_press(egui::Key::Tab);
        harness.run();
        let entry = harness.get_by_role_and_label(egui::accesskit::Role::Button, &label);
        assert!(entry.accesskit_node().is_focused(), "missing-object entry must follow Migration at {width}x{height}");
        let rect = entry.rect();
        assert!(rect.left() >= 0.0 && rect.right() <= width && rect.top() >= 0.0 && rect.bottom() <= height, "focused activity outside viewport at {width}x{height}: {rect:?}");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(harness.state().work.migration.adding_missing_object);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_configured_activities_share_width_evenly() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::panels::WorkflowActivity;
    for (width, height) in [(320.0, 568.0), (390.0, 844.0), (1440.0, 1000.0)] {
        let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
        app.work.drawer = (LayoutMode::for_width(width) != LayoutMode::Wide).then_some(Drawer::Workflow);
        let migration = app.selected_workflow().unwrap();
        let skeleton = app.workflow_choices().into_iter()
            .find(|choice| choice.task_id == TaskId::from("skeleton:person_refinement")).unwrap();
        let labels = [
            "Person: Bounding box annotation · Choose workflow".to_string(),
            app.workflow_entry_label(&migration, Some(WorkflowActivity::Migration)),
            app.workflow_entry_label(&migration, Some(WorkflowActivity::MissingObjects)),
            app.workflow_entry_label(&skeleton, Some(WorkflowActivity::Skeleton)),
        ];
        let mut harness = Harness::builder().with_size(egui::vec2(width, height)).build_eframe(|_| app);
        harness.run();
        let rects = labels.map(|label| harness.get_by_role_and_label(egui::accesskit::Role::Button, &label).rect());
        let heading = harness.get_by_role_and_label(egui::accesskit::Role::Heading, "Person").rect();
        assert!((heading.center().x - (rects[0].left() + rects[3].right()) / 2.0).abs() <= 1.0);
        for label in ["bounding box annotation type", "skeleton annotation type"] {
            for icon in harness.query_all_by_label(label) {
                let icon = icon.rect();
                if let Some(tile) = rects.iter().find(|tile| tile.contains_rect(icon)) {
                    assert!((icon.center().x - tile.center().x).abs() <= 1.0, "icon {icon:?} is off-center in {tile:?}");
                    assert!(icon.top() - tile.top() >= 12.0, "icon {icon:?} needs top padding in {tile:?}");
                }
            }
        }
        for pair in rects.windows(2) {
            assert!((pair[0].width() - pair[1].width()).abs() <= 1.0, "{rects:?}");
            assert_eq!(pair[0].top(), pair[1].top());
            assert_eq!(pair[0].height(), pair[1].height());
            assert!(pair[0].right() < pair[1].left(), "{rects:?}");
        }
        assert!(rects[0].height() >= 44.0 && rects[0].height() <= 94.0, "{rects:?}");
        assert!(rects[0].left() >= 0.0 && rects[3].right() <= width, "{rects:?}");
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_selected_reason_and_chooser_cues_fit_four_columns() {
    use crate::inspector_presets::{self, InspectorPreset};
    use labello_domain::WorkflowUnavailableReason;

    let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
    let boxes = app.workflow_choices().into_iter().find(|choice| choice.annotation_type == AnnotationType::BoundingBox).unwrap();
    app.work.selected_task_id = Some(boxes.task_id.clone());
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.resolved = true;
    app.work.availability.tasks.insert(boxes.task_id.clone(), false);
    app.work.availability.reasons.insert(boxes.task_id, WorkflowUnavailableReason::BalanceLimit);
    app.work.drawer = Some(Drawer::Workflow);
    let mut harness = Harness::builder().with_size(egui::vec2(320.0, 320.0)).build_eframe(|_| app);
    harness.run();

    let tile = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Person: Bounding box annotation · Choose workflow").rect();
    let mut dots = Vec::new();
    let mut segments = Vec::new();
    fn collect(shape: &egui::Shape, tile: egui::Rect, dots: &mut Vec<egui::Pos2>, segments: &mut Vec<[egui::Pos2; 2]>) {
        match shape {
            egui::Shape::Circle(circle) if circle.radius == 4.0 && tile.contains(circle.center) => dots.push(circle.center),
            egui::Shape::LineSegment { points, .. } if points.iter().all(|point| tile.contains(*point)) => segments.push(*points),
            egui::Shape::Vec(shapes) => {
                for shape in shapes { collect(shape, tile, dots, segments); }
            }
            _ => {}
        }
    }
    for shape in &harness.output().shapes {
        collect(&shape.shape, tile, &mut dots, &mut segments);
    }
    assert_eq!(dots.len(), 1, "selected status dot must remain visible in {tile:?}");
    let dot = dots[0];
    let chevron: Vec<_> = segments.into_iter().filter(|points| {
        points.iter().all(|point| point.x > tile.center().x + 10.0 && (point.y - dot.y).abs() <= 3.0)
    }).collect();
    assert_eq!(chevron.len(), 2, "chooser chevron must fit beside selected and reason cues in {tile:?}");
    assert!(dot.x > tile.left() + 4.0 && chevron.iter().flatten().all(|point| point.x < tile.right() - 4.0));
    assert!(tile.height() <= 94.0, "four-activity row should fit its measured content: {tile:?}");
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_chooser_shows_full_names_and_preserves_work_on_cancel() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::panels::WorkflowActivity;
    for (width, height) in [(320.0, 320.0), (390.0, 844.0), (1440.0, 1000.0)] {
        let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
        app.work.drawer = (LayoutMode::for_width(width) != LayoutMode::Wide).then_some(Drawer::Workflow);
        let labels: Vec<_> = app.workflow_choices().iter().filter(|choice| choice.annotation_type == AnnotationType::BoundingBox)
            .map(|choice| app.workflow_entry_label(choice, Some(WorkflowActivity::Boxes))).collect();
        let task = app.work.selected_task_id.clone();
        let cursor = app.work.migration.cursor.clone();
        let mut harness = Harness::builder().with_size(egui::vec2(width, height)).with_max_steps(40).build_eframe(|_| app);
        harness.run();
        let trigger = "Person: Bounding box annotation · Choose workflow";
        harness.get_by_role_and_label(egui::accesskit::Role::Button, trigger).focus();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(egui::Popup::is_any_open(&harness.ctx));
        let heading = harness.get_by_label("Person: Bounding box annotation").rect();
        let first = harness.get_by_role_and_label(egui::accesskit::Role::Button, &labels[0]).rect();
        assert!(heading.top() >= 0.0 && heading.bottom() < first.top(), "{width}x{height}: heading {heading:?}, first option {first:?}");
        for label in &labels {
            let option = harness.get_by_role_and_label(egui::accesskit::Role::Button, label);
            assert!(option.rect().height() >= 44.0);
            assert!(option.rect().left() >= 0.0 && option.rect().right() <= width, "{width}x{height}: {:?}", option.rect());
        }
        harness.get_by_role_and_label(egui::accesskit::Role::Button, labels.last().unwrap()).focus();
        harness.run();
        let rect = harness.get_by_role_and_label(egui::accesskit::Role::Button, labels.last().unwrap()).rect();
        assert!(rect.top() >= 0.0 && rect.bottom() <= height, "{width}x{height}: {rect:?}");
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(!egui::Popup::is_any_open(&harness.ctx));
        assert!(harness.get_by_role_and_label(egui::accesskit::Role::Button, trigger).accesskit_node().is_focused());
        assert_eq!(harness.state().work.selected_task_id, task);
        assert_eq!(harness.state().work.migration.cursor, cursor);
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_chooser_explains_unavailable_options_and_guards_dirty_switches() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::panels::WorkflowActivity;
    let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
    let boxes: Vec<_> = app.workflow_choices().into_iter().filter(|choice| choice.annotation_type == AnnotationType::BoundingBox).collect();
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.resolved = true;
    for choice in &boxes { app.work.availability.tasks.insert(choice.task_id.clone(), false); }
    let labels: Vec<_> = boxes.iter().map(|choice| app.workflow_entry_label(choice, Some(WorkflowActivity::Boxes))).collect();
    let selected = app.work.selected_task_id.clone();
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_eframe(|_| app);
    harness.run();
    let trigger = "Person: Bounding box annotation · Choose workflow";
    let button = harness.get_by_role_and_label(egui::accesskit::Role::Button, trigger);
    assert!(!button.accesskit_node().is_disabled());
    button.click();
    harness.run();
    for label in &labels { assert!(harness.get_by_role_and_label(egui::accesskit::Role::Button, label).accesskit_node().is_disabled()); }
    assert_eq!(harness.query_all_by_label("No assignments available").count(), boxes.len());
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.state_mut().work.availability.tasks.insert(boxes[0].task_id.clone(), true);
    harness.state_mut().trigger_missing_migration_object_action();
    harness.state_mut().work.migration.draft_dirty = true;
    let draft = harness.state().work.migration.draft.clone();
    click_accesskit_button(&mut harness, trigger);
    click_accesskit_button(&mut harness, &labels[0]);
    assert!(harness.state().work.pending_transition.is_some());
    assert_eq!(harness.state().work.selected_task_id, selected);
    assert_eq!(harness.state().work.migration.draft, draft);
}

#[cfg(feature = "inspector-presets")]
#[test]
fn class_workflow_chooser_pointer_hits_options_above_compact_drawer() {
    use crate::inspector_presets::{self, InspectorPreset};
    let mut app = inspector_presets::build(InspectorPreset::MigrationFullImage, &egui::Context::default());
    app.work.drawer = Some(Drawer::Workflow);
    app.trigger_missing_migration_object_action();
    app.work.migration.draft_dirty = true;
    let selected = app.work.selected_task_id.clone();
    let draft = app.work.migration.draft.clone();
    let mut harness = Harness::builder().with_size(egui::vec2(320.0, 320.0)).with_max_steps(40).build_eframe(|_| app);
    harness.run();
    let trigger = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Person: Bounding box annotation · Choose workflow").rect().center();
    click_at(&mut harness, trigger);
    harness.run();
    let option = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Person: Bounding box annotation · Person bounding boxes").rect().center();
    click_at(&mut harness, option);
    harness.run();
    assert!(harness.state().work.pending_transition.is_some());
    assert_eq!(harness.state().work.selected_task_id, selected);
    assert_eq!(harness.state().work.migration.draft, draft);
}
