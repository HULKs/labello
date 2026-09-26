fn source_guide_api() -> (Rc<SpyApi>, labello_domain::AnnotationId) {
    let api = Rc::new(SpyApi::new());
    let id = seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.425,
            y: 0.425,
            width: 0.15,
            height: 0.15,
        }),
        true,
    );
    {
        let mut spy = api.state.borrow_mut();
        spy.metadata.tasks[0].prelabel_config_ids.clear();
        let state = spy.states.values_mut().next().unwrap();
        let original = state.annotations.get_mut(&id).unwrap().last_mut().unwrap();
        let mut source = original.clone();
        source.annotation_id = "source".into();
        source.task_id = "skeleton".into();
        source.annotation_type = AnnotationType::Skeleton;
        source.geometry = AnnotationGeometry::Skeleton(SkeletonGeometry {
            keypoints: vec![KeypointAnnotation {
                name: "center".into(),
                state: KeypointState::Visible,
                point: Some(NormalizedPoint { x: 0.5, y: 0.5 }),
            }],
        });
        original.revision_source = RevisionSource::MigrationSkeleton {
            annotation_id: source.annotation_id.clone(),
            version: 1,
        };
        state.migration_companions.insert(
            source.annotation_id.clone(),
            labello_domain::MigrationCompanion {
                migration_task_id: source.task_id.clone(),
                guide_task_id: original.task_id.clone(),
                class_id: original.class_id.clone(),
                skeleton_annotation_id: source.annotation_id.clone(),
                skeleton_version: 1,
                box_annotation_id: id.clone(),
                box_version: 1,
            },
        );
        state
            .annotations
            .insert(source.annotation_id.clone(), vec![source]);
    }
    (api, id)
}

#[test]
fn companion_guide_drawing_revises_existing_box_and_preserves_source_on_save_retry() {
    for size in [
        egui::vec2(1440.0, 1000.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(320.0, 568.0),
    ] {
        let (api, id) = source_guide_api();
        let mut harness = loaded_work_harness(api.clone());
        harness.set_size(size);
        harness.run();
        let source_before = harness
            .state()
            .work
            .current_state
            .as_ref()
            .unwrap()
            .current_annotation(&"source".into())
            .unwrap()
            .clone();
        let pending = harness
            .state()
            .work
            .annotations
            .iter()
            .find(|a| a.annotation_id == id)
            .unwrap();
        assert!(harness.state().companion_needs_box(pending));
        let guide = harness.state().companion_guide(pending);
        assert_eq!(guide.annotation_id, id);
        assert_eq!(guide.geometry, source_before.geometry);
        assert!(
            harness
                .query_by_label("Source keypoints are read only. Draw a box for this object.")
                .is_some()
        );
        harness.state_mut().submit_and_advance();
        assert_eq!(api.counts().complete_assignment, 0);
        assert!(
            harness
                .state()
                .runtime
                .error
                .as_ref()
                .unwrap()
                .contains("each keypoint guide")
        );
        harness.state_mut().runtime.error = None;
        harness.run();
        let canvas = harness.get_by_label("Annotation canvas").rect();
        // Start on the source point, proving that it cannot capture a keypoint edit.
        drag_at(
            &mut harness,
            canvas.center(),
            canvas.center() + canvas.size() * 0.4,
        );
        let annotation = harness
            .state()
            .work
            .annotations
            .iter()
            .find(|a| a.annotation_id == id)
            .unwrap();
        assert_eq!(annotation.version, 2);
        assert!(matches!(
            annotation.revision_source,
            RevisionSource::Human {
                action: HumanRevisionKind::Edited
            }
        ));
        assert_eq!(
            harness
                .state()
                .work
                .annotations
                .iter()
                .filter(|a| a.task_id == annotation.task_id)
                .count(),
            1
        );
        assert!(!harness.state().companion_needs_box(annotation));
        harness.state_mut().undo();
        assert!(
            harness.state().companion_needs_box(
                harness
                    .state()
                    .work
                    .annotations
                    .iter()
                    .find(|a| a.annotation_id == id)
                    .unwrap()
            )
        );
        harness.state_mut().redo();
        api.state.borrow_mut().fail_next_batch = true;
        harness.state_mut().request_save(false);
        step_until(&mut harness, 10, |app| {
            app.work.save_status == SaveStatus::Retry
        });
        harness.state_mut().request_save(false);
        step_until(&mut harness, 10, |app| {
            app.work.save_status == SaveStatus::Saved
        });
        let state = harness.state().work.current_state.as_ref().unwrap();
        assert_eq!(state.current_annotation(&id).unwrap().version, 2);
        assert_eq!(
            state.current_annotation(&"source".into()),
            Some(&source_before)
        );
        assert_eq!(
            state.migration_companions[&"source".into()].box_annotation_id,
            id
        );
        harness.state_mut().retry_assignment_load();
        step_until(&mut harness, 15, |app| !app.loading.image);
        assert!(
            !harness.state().companion_needs_box(
                harness
                    .state()
                    .work
                    .annotations
                    .iter()
                    .find(|a| a.annotation_id == id)
                    .unwrap()
            )
        );
    }
}

#[test]
fn companion_guides_preserve_multiple_sources_and_ignore_reviewed_or_missing_sources() {
    let (api, id) = source_guide_api();
    let mut harness = loaded_work_harness(api);
    let original = harness
        .state()
        .work
        .annotations
        .iter()
        .find(|a| a.annotation_id == id)
        .unwrap()
        .clone();
    let state = harness.state_mut().work.current_state.as_mut().unwrap();
    let source = state
        .annotations
        .get_mut(&"source".into())
        .unwrap()
        .last_mut()
        .unwrap();
    if let AnnotationGeometry::Skeleton(skeleton) = &mut source.geometry {
        skeleton.keypoints.push(KeypointAnnotation {
            name: "edge".into(),
            state: KeypointState::Hidden,
            point: Some(NormalizedPoint { x: 1.0, y: 0.0 }),
        });
    }
    let mut later = source.clone();
    later.version = 2;
    later.deleted = true;
    state
        .annotations
        .get_mut(&"source".into())
        .unwrap()
        .push(later);
    let guide = harness.state().companion_guide(&original);
    assert!(
        matches!(&guide.geometry, AnnotationGeometry::Skeleton(skeleton) if skeleton.keypoints.len() == 2)
    );
    // A reviewed companion is no longer pending, even with generated provenance.
    harness
        .state_mut()
        .work
        .current_state
        .as_mut()
        .unwrap()
        .reviews
        .push(ReviewRecord {
            review_id: ReviewId::generate(),
            reviewer_user_id: "reviewer".into(),
            target: ReviewTarget::AnnotationVersion {
                annotation_id: id.clone(),
                version: 1,
            },
            decision: labello_domain::ReviewDecision::Approved,
            timestamp: now(),
            comment: None,
        });
    assert!(!harness.state().companion_needs_box(&original));
    harness
        .state_mut()
        .work
        .current_state
        .as_mut()
        .unwrap()
        .annotations
        .remove(&"source".into());
    assert_eq!(harness.state().companion_guide(&original), original);
}

#[test]
fn deeper_zoom_survives_saved_view_restoration_and_fit() {
    let mut canvas = crate::canvas::CanvasState::default();
    for _ in 0..30 {
        canvas.zoom_in();
    }
    assert_eq!(canvas.current_zoom(), 48.0);
    let stored = canvas.stored_transform();
    let mut restored = crate::canvas::CanvasState::default();
    restored.restore_transform(stored);
    assert_eq!(restored.current_zoom(), 48.0);
    restored.fit_view();
    assert_eq!(restored.current_zoom(), 1.0);
}

#[test]
fn companion_guide_multiple_objects_keep_identity_and_restored_deep_zoom() {
    let (api, first_id) = source_guide_api();
    let second_id = labello_domain::AnnotationId::from("second-box");
    {
        let mut spy = api.state.borrow_mut();
        let state = spy.states.values_mut().next().unwrap();
        let mut source = state.current_annotation(&"source".into()).unwrap().clone();
        source.annotation_id = "second-source".into();
        let mut bbox = state.current_annotation(&first_id).unwrap().clone();
        bbox.annotation_id = second_id.clone();
        bbox.revision_source = RevisionSource::MigrationSkeleton {
            annotation_id: source.annotation_id.clone(),
            version: 1,
        };
        let mut link = state.migration_companions[&"source".into()].clone();
        link.skeleton_annotation_id = source.annotation_id.clone();
        link.box_annotation_id = second_id.clone();
        state
            .migration_companions
            .insert(source.annotation_id.clone(), link);
        state
            .annotations
            .insert(source.annotation_id.clone(), vec![source]);
        state.annotations.insert(second_id.clone(), vec![bbox]);
    }
    let mut harness = loaded_work_harness(api.clone());
    harness.state_mut().work.selected_annotation = Some(second_id.clone());
    harness.run();
    let first = harness
        .state()
        .work
        .annotations
        .iter()
        .find(|a| a.annotation_id == first_id)
        .unwrap()
        .clone();
    // Even drawing the exact generated bounds must record a human decision.
    let AnnotationGeometry::BoundingBox(bounds) = first.geometry else {
        panic!()
    };
    harness.state_mut().create_bbox(bounds);
    assert_eq!(
        harness
            .state()
            .work
            .annotations
            .iter()
            .find(|a| a.annotation_id == first_id),
        Some(&first)
    );
    assert!(harness.state().companion_needs_box(&first));
    assert!(
        !harness.state().companion_needs_box(
            harness
                .state()
                .work
                .annotations
                .iter()
                .find(|a| a.annotation_id == second_id)
                .unwrap()
        )
    );
    for _ in 0..30 {
        harness.state_mut().work.canvas.zoom_in();
    }
    harness.run();
    assert_eq!(harness.state().work.canvas.current_zoom(), 48.0);
    harness.state_mut().persist_workspace_preference();
    assert_eq!(
        harness
            .state()
            .runtime
            .persistence
            .preference
            .as_ref()
            .unwrap()
            .canvas
            .zoom,
        48.0
    );
    harness.state_mut().request_save(false);
    step_until(&mut harness, 10, |app| {
        app.work.save_status == SaveStatus::Saved
    });
    harness.state_mut().retry_assignment_load();
    step_until(&mut harness, 15, |app| !app.loading.image);
    harness.run();
    assert_eq!(harness.state().work.canvas.current_zoom(), 48.0);
    assert_eq!(
        harness.state().work.selected_annotation.as_ref(),
        Some(&second_id)
    );
}

/// Supplies only repository-owned synthetic state to the manual browser check.
#[test]
#[ignore = "writes a synthetic browser fixture to LABELLO_COMPANION_BROWSER_FIXTURE"]
fn export_companion_guide_browser_fixture() {
    let (api, _) = source_guide_api();
    let harness = loaded_work_harness(api);
    let app = harness.state();
    let value = serde_json::json!({
        "metadata": app.datasets.metadata,
        "account": app.auth.account,
        "assignment": app.work.assignment,
        "state": app.work.current_state,
        "image": app.work.current.as_ref().unwrap().image,
        "keybindings": app.work.keybindings,
    });
    let path = std::env::var("LABELLO_COMPANION_BROWSER_FIXTURE").expect("fixture output path");
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn pending_workspace_reload_does_not_replace_saved_deep_view() {
    let (api, _) = source_guide_api();
    let mut harness = loaded_work_harness(api);
    for _ in 0..30 {
        harness.state_mut().work.canvas.zoom_in();
    }
    harness.state_mut().persist_workspace_preference();
    let saved = harness
        .state()
        .runtime
        .persistence
        .preference
        .clone()
        .unwrap();
    let app = harness.state_mut();
    app.runtime.persistence.expected_assignment = saved.assignment_id.clone();
    app.work.assignment = None;
    app.work.selected_annotation = None;
    app.work.canvas.fit_view();
    app.persist_workspace_preference();
    assert_eq!(app.runtime.persistence.preference.as_ref(), Some(&saved));
}
