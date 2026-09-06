use crate::export_flow::{ExportAction, ExportReply};
use labello_client::ExportPhase;
use labello_domain::{ExportClassSelection, ExportProfile, ExportSplit};

fn export_harness(api: Rc<SpyApi>) -> Harness<'static, LabelloApp> {
    let mut harness = loaded_admin_harness(api);
    harness.set_size(egui::vec2(1440.0, 3000.0));
    select_admin_section(&mut harness, "Export");
    step_until(&mut harness, 12, |app| app.admin.export.loaded);
    harness.run_steps(3);
    harness
}

fn export_select_and_preflight(harness: &mut Harness<'static, LabelloApp>) {
    click(
        harness,
        "Person boxes / Person [bounding_box:person · person]",
    );
    click(harness, "Run export preflight");
    step_until(harness, 12, |app| app.admin.export.selected_job().is_some());
    harness.run_steps(3);
}

#[test]
fn export_requires_explicit_selection_review_and_same_options_before_start() {
    let api = Rc::new(SpyApi::new());
    let mut harness = export_harness(api.clone());
    assert_eq!(
        harness.state().admin.export.options.fallback_split,
        ExportSplit::Train
    );
    assert!(
        harness
            .get_by_label("Run export preflight")
            .accesskit_node()
            .is_disabled()
    );
    export_select_and_preflight(&mut harness);
    assert!(
        harness
            .get_by_label("Start export")
            .accesskit_node()
            .is_disabled()
    );
    click(&mut harness, "I reviewed the captured export summary");
    assert!(
        !harness
            .get_by_label("Start export")
            .accesskit_node()
            .is_disabled()
    );
    click(
        &mut harness,
        "Person boxes / Person [bounding_box:person · person]",
    );
    assert!(
        harness
            .get_by_label("Start export")
            .accesskit_node()
            .is_disabled()
    );
    assert!(!harness.state().admin.export.reviewed);
    assert!(
        harness
            .get_by_label("Run export preflight")
            .accesskit_node()
            .is_disabled()
    );
    assert_eq!(
        api.state
            .borrow()
            .export_calls
            .iter()
            .filter(|c| *c == "preflight")
            .count(),
        1
    );
    click(&mut harness, "Cancel export");
    step_until(&mut harness, 12, |app| {
        app.admin.export.selected_job().unwrap().phase == ExportPhase::Cancelled
    });
    click(
        &mut harness,
        "Person boxes / Person [bounding_box:person · person]",
    );
    assert!(
        !harness
            .get_by_label("Run export preflight")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn export_start_poll_download_failure_retry_and_history_use_live_commands() {
    let api = Rc::new(SpyApi::new());
    let mut harness = export_harness(api.clone());
    export_select_and_preflight(&mut harness);
    click(&mut harness, "I reviewed the captured export summary");
    click(&mut harness, "Start export");
    step_until(&mut harness, 12, |app| {
        app.admin.export.selected_job().unwrap().phase == ExportPhase::Building
    });
    harness.state_mut().admin.export.last_poll = Some(Instant::now() - Duration::from_secs(2));
    step_until(&mut harness, 12, |app| {
        app.admin.export.selected_job().unwrap().phase == ExportPhase::Succeeded
    });
    api.state.borrow_mut().fail_next_export = true;
    click(&mut harness, "Download export archive");
    step_until(&mut harness, 12, |app| app.admin.export.error.is_some());
    assert!(harness.state().runtime.error.is_none());
    click(&mut harness, "Retry export status");
    step_until(&mut harness, 12, |app| app.admin.export.notice.is_some());
    assert!(
        harness
            .state()
            .admin
            .export
            .notice
            .as_deref()
            .unwrap()
            .contains("Download requested")
    );
    let restored = export_harness(api.clone());
    assert_eq!(
        restored.state().admin.export.selected_job().unwrap().phase,
        ExportPhase::Succeeded
    );
    assert_eq!(restored.state().admin.export.options.classes.len(), 1);
    assert_eq!(
        api.state
            .borrow()
            .export_calls
            .iter()
            .filter(|c| *c == "start")
            .count(),
        1
    );
    assert_eq!(
        api.state
            .borrow()
            .export_calls
            .iter()
            .filter(|c| *c == "download")
            .count(),
        2
    );
}

#[test]
fn export_pending_load_coalesces_and_queue_failure_stays_in_export_region() {
    let api = Rc::new(SpyApi::new());
    let mut harness = export_harness(api);
    let app = harness.state_mut();
    app.request_export(ExportAction::Load);
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.request_export(ExportAction::Load);
    assert_eq!(
        app.runtime
            .commands
            .iter()
            .filter(|c| matches!(c, UiCommand::Export { .. }))
            .count(),
        1
    );
    app.runtime
        .commands
        .retain(|c| !matches!(c, UiCommand::Export { .. }));
    app.runtime
        .tx
        .send(UiMessage::RequestFailed {
            request,
            error: "synthetic dispatcher failure".into(),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.admin.export.pending.is_none());
    assert!(
        app.admin
            .export
            .error
            .as_deref()
            .unwrap()
            .contains("dispatcher failure")
    );
    assert!(app.runtime.error.is_none());
    app.runtime.commands.clear();
    for n in 0..64 {
        app.runtime.commands.push_back(UiCommand::Stats {
            request: app.operation_identity(1000 + n, app.config.dataset_id.clone()),
            dataset_id: app.config.dataset_id.clone(),
        });
    }
    app.request_export(ExportAction::Load);
    assert!(app.admin.export.pending.is_none());
    assert!(
        app.admin
            .export
            .error
            .as_deref()
            .unwrap()
            .contains("queue is full")
    );
    assert!(app.runtime.error.is_none());
}

#[test]
fn export_rejects_same_dataset_and_job_after_auth_endpoint_or_workspace_change() {
    for kind in 0..3 {
        let mut harness = export_harness(Rc::new(SpyApi::new()));
        let app = harness.state_mut();
        app.request_export(ExportAction::Load);
        let request = app.runtime.commands.back().unwrap().request().clone();
        if kind == 0 {
            app.begin_auth_epoch();
        } else if kind == 1 {
            app.config.api_base_url = "http://127.0.0.1:19116".into();
            app.begin_auth_epoch();
        } else {
            app.begin_workspace_epoch();
        }
        app.runtime
            .tx
            .send(UiMessage::ExportFinished {
                request,
                result: Box::new(Ok(ExportReply::Loaded {
                    capabilities: labello_client::ExportCapabilities {
                        available: true,
                        limits: Default::default(),
                    },
                    jobs: vec![],
                })),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert!(!app.admin.export.loaded);
        assert!(app.admin.export.pending.is_none());
    }
}

#[test]
fn export_failure_retains_loaded_data_and_mutation_retry_reconciles_history() {
    let api = Rc::new(SpyApi::new());
    let mut harness = export_harness(api.clone());
    export_select_and_preflight(&mut harness);
    let job = harness.state().admin.export.selected_job().unwrap().clone();
    click(&mut harness, "I reviewed the captured export summary");
    api.state.borrow_mut().fail_next_export = true;
    click(&mut harness, "Start export");
    step_until(&mut harness, 12, |app| app.admin.export.error.is_some());
    assert_eq!(harness.state().admin.export.selected_job(), Some(&job));
    assert_eq!(harness.state().admin.export.retry, Some(ExportAction::Load));
    click(&mut harness, "Retry export status");
    step_until(&mut harness, 12, |app| app.admin.export.pending.is_none());
    assert_eq!(
        api.state
            .borrow()
            .export_calls
            .iter()
            .filter(|c| *c == "start")
            .count(),
        1
    );
    assert!(harness.state().admin.export.error.is_none());
}

#[test]
fn export_polling_is_bounded_and_stops_outside_export_admin() {
    let api = Rc::new(SpyApi::new());
    let mut harness = export_harness(api);
    export_select_and_preflight(&mut harness);
    let app = harness.state_mut();
    app.admin.export.jobs[0].phase = ExportPhase::Building;
    app.admin.export.last_poll = Some(Instant::now());
    app.refresh_export_if_due(&egui::Context::default());
    assert!(app.admin.export.pending.is_none());
    app.admin.export.last_poll = Some(Instant::now() - Duration::from_secs(2));
    app.admin.section = AdminSection::Overview;
    app.refresh_export_if_due(&egui::Context::default());
    assert!(app.admin.export.pending.is_none());
    app.admin.section = AdminSection::Export;
    app.refresh_export_if_due(&egui::Context::default());
    app.refresh_export_if_due(&egui::Context::default());
    assert_eq!(
        app.runtime
            .commands
            .iter()
            .filter(|c| matches!(c, UiCommand::Export { .. }))
            .count(),
        1
    );
}

#[test]
fn export_profile_and_shape_validation_use_canonical_mapping_policy() {
    let mut harness = export_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    let selection = ExportClassSelection {
        task_id: TaskId::from("bounding_box:person"),
        class_id: ClassId::from("person"),
    };
    app.admin.export.options.profile = ExportProfile::UltralyticsYoloPoseV1;
    app.admin.export.options.classes.insert(selection);
    let options = app.admin.export.options.clone();
    app.request_export(ExportAction::Preflight(options));
    assert!(app.admin.export.pending.is_none());
    harness.run_steps(3);
    assert!(
        harness
            .get_by_label("Run export preflight")
            .accesskit_node()
            .is_disabled()
    );
    assert!(harness.query_by_label("No tasks match this export profile. Choose another profile or configure compatible tasks.").is_some());
}

#[test]
fn export_blockers_empty_and_failure_states_preserve_explicit_choices() {
    let mut harness = export_harness(Rc::new(SpyApi::new()));
    export_select_and_preflight(&mut harness);
    let app = harness.state_mut();
    let job = &mut app.admin.export.jobs[0];
    job.phase = ExportPhase::Blocked;
    job.summary = Some(labello_client::ExportSummary {
        blocking_images: 1,
        blockers: vec![labello_client::ExportBlocker {
            image_id: ImageId::from("img_1"),
            reason: labello_client::ExportFailure::Policy(
                labello_domain::ExportPolicyError::SplitConflict,
            ),
        }],
        ..Default::default()
    });
    harness.run_steps(3);
    assert!(harness.query_by_label("Split for image img_1").is_some());
    assert!(
        harness
            .state()
            .admin
            .export
            .options
            .split_choices
            .is_empty()
    );
    assert!(harness.query_by_label("Start export").is_none());
    assert!(
        !harness
            .get_by_label("Cancel export")
            .accesskit_node()
            .is_disabled()
    );
    harness.state_mut().admin.export.jobs[0].phase = ExportPhase::Failed;
    harness.state_mut().admin.export.jobs[0].failure =
        Some(labello_client::ExportFailure::Interrupted);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("export was interrupted by a server restart")
            .is_some()
    );
    assert!(
        !harness
            .get_by_label("Run export preflight")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn export_initial_failure_retry_and_loading_do_not_render_empty_history() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_admin_harness(api.clone());
    api.state.borrow_mut().fail_next_export = true;
    select_admin_section(&mut harness, "Export");
    step_until(&mut harness, 12, |app| app.admin.export.error.is_some());
    assert!(!harness.state().admin.export.loaded);
    assert!(
        harness
            .query_by_label(
                "No exports yet. Choose a profile and task/class mappings, then run preflight."
            )
            .is_none()
    );
    click(&mut harness, "Retry export status");
    step_until(&mut harness, 12, |app| app.admin.export.loaded);
    assert!(harness.state().admin.export.error.is_none());
}

#[test]
fn export_controls_and_long_task_labels_reflow_at_supported_sizes() {
    for size in [
        (320.0, 568.0),
        (390.0, 844.0),
        (600.0, 800.0),
        (1288.0, 820.0),
        (1440.0, 1000.0),
        (320.0, 320.0),
    ] {
        let api = Rc::new(SpyApi::new());
        let long_name = "A long task name with several words that still identifies the selected annotation task";
        api.state.borrow_mut().metadata.tasks[0].name = long_name.into();
        let mut harness = export_harness(api);
        harness.set_size(egui::vec2(size.0, size.1));
        harness.run_steps(3);
        for name in [
            "Export profile".to_string(),
            format!("{long_name} / Person [bounding_box:person · person]"),
            "Run export preflight".into(),
        ] {
            harness.get_by_label(&name).scroll_to_me();
            harness.run_steps(3);
            let rect = harness.get_by_label(&name).rect();
            assert!(
                rect.left() >= 0.0 && rect.right() <= size.0 + 1.0,
                "{size:?} {name} {rect:?}"
            );
            assert!(
                rect.top() >= 55.0 && rect.bottom() <= size.1 + 1.0,
                "{size:?} {name} {rect:?}"
            );
        }
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn export_pose_reimport_policy_is_explicit_dirty_acknowledged_and_recoverable() {
    use crate::inspector_presets::{self, InspectorPreset};
    use labello_client::{ImportProfile, YoloZeroKeypointPolicy};
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1180.0, 5000.0))
        .build_eframe(|ctx| {
            let mut app = inspector_presets::build(InspectorPreset::ImportReady, &ctx.egui_ctx);
            app.import.profile = ImportProfile::UltralyticsYoloPoseV1;
            app.import.job.as_mut().unwrap().profile = ImportProfile::UltralyticsYoloPoseV1;
            app
        });
    harness.run_steps(3);
    assert_eq!(
        harness.state().import.yolo_zero_keypoints,
        YoloZeroKeypointPolicy::Incomplete
    );
    let original = harness.state().import_plan_request();
    click(&mut harness, "YOLO poses with no placed keypoints");
    click(&mut harness, "Preserve object; all points absent");
    harness.run_steps(3);
    let request = harness.state().import_plan_request();
    assert_eq!(
        request.compatibility.yolo_zero_keypoints,
        YoloZeroKeypointPolicy::PreserveAbsent
    );
    assert_ne!(request, original);
    assert!(harness.query_by_label("All-zero YOLO keypoints will preserve the object with every point absent. Choose this only when the source explicitly uses zeros for absent points; diagnostic acknowledgement is required when encountered.").is_some());
    assert!(
        harness
            .get_by_label("Commit import")
            .accesskit_node()
            .is_disabled()
    );
    let mut job = harness.state().import.job.as_ref().unwrap().clone();
    let mut plan = harness.state().import.plan.as_ref().unwrap().clone();
    plan.accepted_request = Some(request);
    job.recovery = Some(labello_client::ImportRecoveryState {
        accepted_plan: Some(plan),
        ..Default::default()
    });
    let mut recovered = LabelloApp::default();
    recovered.import.hydrate_job_contract(&job);
    assert_eq!(
        recovered.import.yolo_zero_keypoints,
        YoloZeroKeypointPolicy::PreserveAbsent
    );
    assert_eq!(
        recovered
            .import_plan_request()
            .compatibility
            .yolo_zero_keypoints,
        YoloZeroKeypointPolicy::PreserveAbsent
    );
}

#[test]
fn export_keeps_identical_display_names_distinguishable_by_task_and_class() {
    let api = Rc::new(SpyApi::new());
    {
        let mut state = api.state.borrow_mut();
        for class in &mut state.metadata.label_classes {
            class.name = "Same display name".into();
        }
        state.metadata.tasks[0].class_ids = vec![ClassId::from("person"), ClassId::from("vehicle")];
    }
    let mut harness = export_harness(api);
    let first = "Person boxes / Same display name [bounding_box:person · person]";
    let second = "Person boxes / Same display name [bounding_box:person · vehicle]";
    click(&mut harness, first);
    click(&mut harness, second);
    assert_eq!(harness.state().admin.export.options.classes.len(), 2);
    assert_eq!(
        harness.get_by_label(first).accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    assert_eq!(
        harness.get_by_label(second).accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn export_pose_import_policy_stays_within_the_visible_compact_page() {
    use crate::inspector_presets::{self, InspectorPreset};
    for size in [egui::vec2(390.0, 844.0), egui::vec2(320.0, 320.0)] {
        let mut harness = Harness::builder().with_size(size).build_eframe(|ctx| {
            let mut app = inspector_presets::build(InspectorPreset::ImportReady, &ctx.egui_ctx);
            app.import.profile = labello_client::ImportProfile::UltralyticsYoloPoseV1;
            app.import.job.as_mut().unwrap().profile =
                labello_client::ImportProfile::UltralyticsYoloPoseV1;
            app.import.yolo_zero_keypoints = labello_client::YoloZeroKeypointPolicy::PreserveAbsent;
            app
        });
        harness.run_steps(3);
        for label in [
            "YOLO poses with no placed keypoints",
            "Preserve only when all-zero keypoint entries explicitly mean that the object exists and every point is absent. This does not infer labels for an unlabelled source.",
            "All-zero YOLO keypoints will preserve the object with every point absent. Choose this only when the source explicitly uses zeros for absent points; diagnostic acknowledgement is required when encountered.",
        ] {
            let node = harness.get_by_label(label);
            node.scroll_to_me();
            harness.run_steps(3);
            let rect = harness.get_by_label(label).rect();
            assert!(
                rect.left() >= 0.0 && rect.right() <= size.x,
                "{size:?}: {label}: {rect:?}"
            );
        }
    }
}
