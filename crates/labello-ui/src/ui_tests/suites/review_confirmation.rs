fn keypoint_review_overview(keypoint_count: usize) -> Harness<'static, LabelloApp> {
    let api = Rc::new(SpyApi::new());
    let names = (0..keypoint_count)
        .map(|i| format!("point-{i}"))
        .collect::<Vec<_>>();
    {
        let mut state = api.state.borrow_mut();
        let task = &mut state.metadata.tasks[0];
        task.annotation_type = AnnotationType::Skeleton;
        task.prelabel_config_ids.clear();
        task.skeleton = Some(SkeletonSpec {
            keypoints: names
                .iter()
                .map(|name| KeypointSpec {
                    name: name.clone(),
                    required: true,
                })
                .collect(),
            edges: Vec::new(),
            allow_hidden: true,
            allow_absent: false,
        });
    }
    seed_review_annotation(
        &api,
        AnnotationGeometry::Skeleton(SkeletonGeometry {
            keypoints: names
                .into_iter()
                .map(|name| KeypointAnnotation {
                    name,
                    state: KeypointState::Visible,
                    point: Some(NormalizedPoint { x: 0.5, y: 0.5 }),
                })
                .collect(),
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    harness
        .state_mut()
        .request_review(labello_domain::ReviewDecision::Approved);
    step_until(&mut harness, 12, |app| {
        app.review_overview() && !app.loading.saving
    });
    harness.run_steps(3);
    harness
}

#[test]
fn review_confirmation_keeps_multiple_missing_keypoint_objects() {
    for keypoint_count in [1, 2] {
        let mut harness = keypoint_review_overview(keypoint_count);
        let persisted = harness.state().work.annotations.clone();
        let rect = harness.get_by_label("Annotation canvas").rect();
        for object in 0..3 {
            for point in 0..keypoint_count {
                click_at(
                    &mut harness,
                    rect.center()
                        + egui::vec2(-100.0 + object as f32 * 90.0, 40.0 + point as f32 * 35.0),
                );
                harness.run_steps(2);
                if point + 1 < keypoint_count {
                    assert!(harness.state().work.correction_draft.is_some());
                    assert_eq!(
                        harness.state().work.review_corrections.changes.len(),
                        object
                    );
                }
            }
            assert_eq!(
                harness.state().work.review_corrections.changes.len(),
                object + 1
            );
            assert!(harness.state().work.correction_draft.is_none());
        }
        let mut previews = Vec::new();
        harness.state().apply_staged_review_previews(&mut previews);
        assert_eq!(previews.len(), 3);
        for annotation in previews {
            let AnnotationGeometry::Skeleton(skeleton) = annotation.geometry else {
                panic!("expected skeleton");
            };
            assert!(
                skeleton
                    .keypoints
                    .iter()
                    .all(|keypoint| keypoint.point.is_some())
            );
        }
        assert_eq!(harness.state().work.annotations, persisted);
        assert!(harness.state().work.review_corrections.submission.is_none());
    }
}

#[test]
fn review_confirmation_overview_can_select_decided_items_while_editing_an_addition() {
    let mut harness = keypoint_review_overview(1);
    let original = harness.state().work.annotations[0].annotation_id.clone();
    let rect = harness.get_by_label("Annotation canvas").rect();
    let added_position = rect.center() + egui::vec2(80.0, 50.0);
    click_at(&mut harness, added_position);
    harness.state_mut().retain_review_editor();
    harness.input_mut().time = Some(10.0);
    harness.run_steps(3);
    click_at(&mut harness, added_position);
    harness.run_steps(3);
    assert!(harness.state().work.correction_draft.is_some());
    assert!(harness.state().review_overview());
    click_at(&mut harness, rect.center());
    harness.run_steps(3);
    assert_eq!(
        harness.state().work.selected_annotation.as_ref(),
        Some(&original)
    );
    assert_eq!(harness.state().review_position(), 0);
    assert_eq!(harness.state().work.review_corrections.changes.len(), 1);

    let center = harness.get_by_label("Annotation canvas").rect().center();
    drag_at(&mut harness, center, center + egui::vec2(30.0, 20.0));
    assert!(harness.state().review_editor_changed());
    assert!(harness.state_mut().reject_review_item());
    harness.run_steps(3);
    assert!(harness.state().review_overview());
    let AnnotationGeometry::Skeleton(skeleton) = &harness
        .state()
        .work
        .review_corrections
        .changes
        .iter()
        .find_map(|change| {
            if let labello_domain::ReviewCorrectionChange::Edit { geometry, .. } = change {
                Some(geometry.clone())
            } else {
                None
            }
        })
        .unwrap()
    else {
        panic!("expected correction");
    };
    let corrected_point = skeleton.keypoints[0].point.unwrap();
    let canvas = harness.get_by_label("Annotation canvas").rect();
    let image = harness.state().work.current.as_ref().unwrap().image.clone();
    let scale = (canvas.width() / image.width as f32).min(canvas.height() / image.height as f32);
    let size = egui::vec2(image.width as f32, image.height as f32) * scale;
    let point = canvas.center()
        + egui::vec2(
            (corrected_point.x - 0.5) * size.x,
            (corrected_point.y - 0.5) * size.y,
        );
    harness.input_mut().time = Some(20.0);
    click_at(&mut harness, point);
    harness.run_steps(3);
    assert_eq!(harness.state().review_position(), 0);
    assert!(harness.state().review_editor_changed());
}

#[test]
fn review_confirmation_space_chooses_the_valid_decision_and_only_submits_from_overview() {
    let mut harness = two_object_review_revision_harness();
    harness.run_steps(3);
    edit_test_review_box(harness.state_mut());
    harness.key_press(egui::Key::Space);
    harness.run_steps(3);
    assert_eq!(harness.state().review_position(), 1);
    assert_eq!(harness.state().work.review_corrections.changes.len(), 1);
    assert!(harness.state().work.review_corrections.submission.is_none());
    harness.key_press(egui::Key::Space);
    harness.run_steps(3);
    assert!(harness.state().review_overview());
    assert!(harness.state().work.review_corrections.submission.is_none());
    harness.key_press(egui::Key::Space);
    harness.step();
    assert!(harness.state().work.review_corrections.submission.is_some());
}

#[test]
fn review_confirmation_space_respects_custom_binding_invalid_input_and_loading() {
    let mut harness = two_object_review_revision_harness();
    harness.state_mut().work.keybindings.bindings.insert(
        labello_domain::UserAction::NextImage,
        labello_domain::KeyChord::new("Enter"),
    );
    harness.run_steps(3);
    harness.key_press(egui::Key::Space);
    harness.run_steps(2);
    assert_eq!(harness.state().review_position(), 0);
    harness.state_mut().loading.image = true;
    harness.key_press(egui::Key::Enter);
    harness.step();
    assert_eq!(harness.state().review_position(), 0);
    harness.state_mut().loading.image = false;
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().review_position(), 1);
    harness.state_mut().navigate_review_item(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().work.review_corrections.submission.is_none());
    assert!(harness.state().work.review_revision_commit.is_none());
    harness.state_mut().begin_new_review_object(None);
    harness
        .state_mut()
        .work
        .correction_draft
        .as_mut()
        .unwrap()
        .edited_geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.1,
        y: 0.1,
        width: 0.0,
        height: 0.2,
    });
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().work.correction_draft.is_some());
    assert!(harness.state().work.review_corrections.submission.is_none());
}

#[cfg(feature = "inspector-presets")]
fn migration_keypoint_review_overview() -> Harness<'static, LabelloApp> {
    use crate::inspector_presets::{self, InspectorPreset};
    let app = inspector_presets::build(InspectorPreset::MigrationReview, &egui::Context::default());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|_| app);
    harness.run_steps(3);
    let overview = harness.state().review_object_targets().len();
    harness.state_mut().navigate_review_item(overview);
    harness.run_steps(3);
    assert!(harness.state().review_overview());
    harness
}

#[cfg(feature = "inspector-presets")]
#[test]
fn review_creation_after_reopening_an_addition_preserves_it_and_starts_another() {
    for mut harness in [
        keypoint_review_overview(1),
        keypoint_review_overview(2),
        migration_keypoint_review_overview(),
    ] {
        let count = harness
            .state()
            .selected_task()
            .unwrap()
            .skeleton
            .as_ref()
            .unwrap()
            .keypoints
            .len();
        let center = harness.get_by_label("Annotation canvas").rect().center();
        let first = center + egui::vec2(0.0, -120.0);
        for index in 0..count {
            click_at(&mut harness, first + egui::vec2(0.0, index as f32 * 50.0));
            harness.run_steps(2);
        }
        assert_eq!(harness.state().work.review_corrections.changes.len(), 1);
        harness.input_mut().time = Some(10.0);
        click_at(&mut harness, first);
        harness.run_steps(2);
        assert!(harness.state().work.correction_draft.is_some());
        drag_at(&mut harness, first, first + egui::vec2(20.0, 0.0));
        let previous = harness
            .state()
            .work
            .correction_draft
            .as_ref()
            .unwrap()
            .edited_geometry
            .clone();
        let previous_id = harness
            .state()
            .work
            .correction_draft
            .as_ref()
            .unwrap()
            .annotation_id
            .clone();
        for index in 0..count {
            click_at(&mut harness, first + egui::vec2(-50.0, index as f32 * 50.0));
            harness.run_steps(2);
        }
        let changes = &harness.state().work.review_corrections.changes;
        assert_eq!(
            changes.len(),
            2,
            "a reopened addition must not absorb the next object"
        );
        assert!(changes.iter().any(|change| matches!(change,
            labello_domain::ReviewCorrectionChange::Add { annotation_id, geometry, .. }
                if annotation_id == &previous_id && geometry == &previous)));
        assert!(harness.state().work.correction_draft.is_none());
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn review_creation_continues_unplaced_points_after_selecting_or_dragging_an_earlier_point() {
    for mut harness in [
        keypoint_review_overview(2),
        migration_keypoint_review_overview(),
    ] {
        let count = harness
            .state()
            .selected_task()
            .unwrap()
            .skeleton
            .as_ref()
            .unwrap()
            .keypoints
            .len();
        let first =
            harness.get_by_label("Annotation canvas").rect().center() + egui::vec2(0.0, -120.0);
        click_at(&mut harness, first);
        harness.input_mut().time = Some(10.0);
        click_at(&mut harness, first);
        harness.run_steps(2);
        drag_at(&mut harness, first, first + egui::vec2(20.0, 0.0));
        let AnnotationGeometry::Skeleton(before) = harness
            .state()
            .work
            .correction_draft
            .as_ref()
            .unwrap()
            .edited_geometry
            .clone()
        else {
            panic!("expected skeleton");
        };
        for index in 1..count {
            click_at(&mut harness, first + egui::vec2(0.0, index as f32 * 50.0));
            harness.run_steps(2);
        }
        assert_eq!(harness.state().work.review_corrections.changes.len(), 1);
        let labello_domain::ReviewCorrectionChange::Add {
            geometry: AnnotationGeometry::Skeleton(skeleton),
            ..
        } = &harness.state().work.review_corrections.changes[0]
        else {
            panic!("expected addition");
        };
        assert!(
            skeleton.keypoints[0] == before.keypoints[0],
            "placement must preserve the previously dragged point"
        );
        assert!(skeleton.keypoints.iter().all(|point| point.point.is_some()));
    }
}

#[test]
fn review_creation_primary_label_follows_the_focused_decision() {
    let mut harness = two_object_review_revision_harness();
    harness.run_steps(3);
    assert!(
        !harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Approve")
            .accesskit_node()
            .is_disabled()
    );
    edit_test_review_box(harness.state_mut());
    for (width, height) in [
        (150.0, 568.0),
        (320.0, 320.0),
        (390.0, 844.0),
        (1440.0, 1000.0),
    ] {
        harness.set_size(egui::vec2(width, height));
        harness.run_steps(2);
        assert_control_inside(
            &harness,
            "Submit correction",
            egui::accesskit::Role::Button,
            width,
            height,
        );
    }
    click(&mut harness, "Submit correction");
    harness.run_steps(2);
    assert!(
        !harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Approve")
            .accesskit_node()
            .is_disabled()
    );
    click(&mut harness, "Approve");
    harness.run_steps(2);
    assert!(harness.state().review_overview());
    assert!(
        !harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Submit correction")
            .accesskit_node()
            .is_disabled()
    );
    harness.state_mut().discard_all_review_corrections();
    harness.run_steps(2);
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::Button, "Submit correction")
            .is_none()
    );
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::Button, "Approve")
            .is_some()
    );
}
