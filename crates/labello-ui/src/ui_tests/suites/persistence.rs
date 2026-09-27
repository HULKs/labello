#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_task_spawner_delivers_live_messages() {
    let mut app = LabelloApp::default();
    let scheduled = Rc::new(RefCell::new(None));
    let scheduled_for_spawner = scheduled.clone();
    app.set_native_task_spawner(move |future| {
        *scheduled_for_spawner.borrow_mut() = Some(future);
    });
    let request = RequestIdentity {
        auth_epoch: 0,
        workspace_epoch: 0,
        request_id: 1,
        dataset_id: None,
    };

    app.spawn_message(request.clone(), async move {
        UiMessage::RequestFailed {
            request,
            error: "scheduled".to_string(),
        }
    });

    let task = scheduled
        .borrow_mut()
        .take()
        .expect("native task was not scheduled");
    poll_ready_task(task);
    let message = app.runtime.rx.try_recv().unwrap();
    assert!(matches!(
        message,
        UiMessage::RequestFailed { error, .. } if error == "scheduled"
    ));
}

#[cfg(feature = "inspector-presets")]
#[test]
fn assignment_reload_discards_stale_manual_cursor_pass_and_local_draft() {
    use crate::app::LoadedImage;
    use crate::inspector_presets::{self, InspectorPreset};

    let mut app =
        inspector_presets::build(InspectorPreset::MigrationObject, &egui::Context::default());
    let loaded = LoadedImage {
            prepared_until: None,
            review_submitters: Vec::new(),
            reasons: Vec::new(),
        assignment: app.work.assignment.clone().unwrap(),
        queued: app.work.current.clone().unwrap(),
        annotations: app.work.annotations.clone(),
        state: app.work.current_state.clone().unwrap(),
        color_image: None,
    };
    app.work.migration.cursor = Some(labello_domain::MigrationCursor::FullImage);
    app.work.migration.active_pass_id = Some(labello_domain::MigrationPassId::from("stale-pass"));
    app.work.migration.draft =
        Some(crate::manual_migration::ManualMigrationState::empty_skeleton(["stale".to_string()]));
    app.work.migration.draft_group = Some(labello_domain::ObjectGroupId::from("stale-group"));
    app.work.migration.error = Some("stale failure".to_string());
    let operation_id = 77_001;
    let request = test_request(&app, operation_id, Some("demo"));
    app.work.active_load_id = Some(operation_id);
    app.runtime.active_requests.insert(operation_id);
    app.runtime
        .tx
        .send(UiMessage::ImageLoaded {
            request,
            operation_id,
            assignment: Some(loaded.assignment.clone()),
            result: Box::new(Ok(Some(loaded))),
        })
        .unwrap();

    app.process_messages(&egui::Context::default());

    assert!(app.work.migration.cursor.is_none());
    assert!(app.work.migration.active_pass_id.is_none());
    assert!(app.work.migration.draft.is_none());
    assert!(app.work.migration.draft_group.is_none());
    assert!(app.work.migration.error.is_none());
    app.sync_manual_migration();
    assert!(matches!(
        app.work.migration.cursor,
        Some(labello_domain::MigrationCursor::Object { ref object_group_id, .. })
            if object_group_id == &labello_domain::ObjectGroupId::from("group-left")
    ));
}

#[test]
fn replacement_session_request_ignores_the_stale_result() {
    let api = Rc::new(SpyApi::new());
    let account = api.state.borrow().users[0].account.clone();
    let mut app = base_live_app(api);
    app.auth.account = None;

    app.request_session();
    let stale_request = app.runtime.commands.back().unwrap().request().clone();
    app.request_session();
    let active_request = app.runtime.commands.back().unwrap().request().clone();
    assert_ne!(stale_request, active_request);

    app.runtime
        .tx
        .send(UiMessage::SessionLoaded {
            request: stale_request,
            result: Ok(SessionInfo {
                account: account.clone(),
                can_create_datasets: true,
                prelabel_available: true,
                csrf_token: "stale-csrf-token".to_string(),
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.loading.session);
    assert!(app.auth.account.is_none());
    assert_eq!(
        app.auth.active_session_request_id,
        Some(active_request.request_id)
    );

    app.runtime
        .tx
        .send(UiMessage::SessionLoaded {
            request: active_request,
            result: Ok(SessionInfo {
                account: account.clone(),
                can_create_datasets: true,
                prelabel_available: true,
                csrf_token: "active-csrf-token".to_string(),
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(!app.loading.session);
    assert_eq!(app.auth.account, Some(account));
}

#[test]
fn snapshot_load_history_advances_only_after_a_successful_catalog_request() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    for (request_id, result) in [
        (1, Err("initial failure".to_string())),
        (2, Ok(Vec::new())),
        (3, Err("refresh failure".to_string())),
    ] {
        let request = test_request(&app, request_id, Some("demo"));
        app.runtime.active_requests.insert(request_id);
        app.loading.snapshots = true;
        app.runtime
            .tx
            .send(UiMessage::SnapshotsLoaded { request, result: result.map_err(Into::into) })
            .unwrap();
        app.process_messages(&egui::Context::default());
        if request_id == 1 {
            assert!(!app.admin.snapshots_loaded);
        } else {
            assert!(app.admin.snapshots_loaded);
        }
    }
    assert_eq!(
        app.admin.snapshots_error.as_deref(),
        Some("refresh failure")
    );
}

#[test]
fn assignment_availability_poll_waits_for_the_in_flight_request() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api);
    app.view = AppView::Annotate;
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.loading = true;
    app.work.availability.last_attempt = Some(Instant::now() - Duration::from_secs(11));
    let queued_before = app.runtime.commands.len();

    app.refresh_assignment_availability_if_due();

    assert_eq!(app.runtime.commands.len(), queued_before);
    assert!(!app.work.availability.refresh_after_load);
    assert!(app.work.availability.loading);
}

#[test]
fn assignment_availability_poll_is_scheduled_from_completion() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api);
    app.view = AppView::Annotate;
    app.request_assignment_availability();
    let UiCommand::AssignmentAvailability { request, .. } =
        app.runtime.commands.pop_back().unwrap()
    else {
        panic!("expected availability request");
    };
    app.work.availability.last_attempt = Some(Instant::now() - Duration::from_secs(30));
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            checked_assignments: Vec::new(),
            request,
            result: Ok(labello_client::AssignmentAvailability {
                queue: None,
                reasons: Default::default(),
                kind: AssignmentKind::Annotation,
                tasks: BTreeMap::from([(TaskId::from("bounding_box:person"), true)]),
                related: Vec::new(),
            }),
        })
        .unwrap();

    app.process_messages(&egui::Context::default());
    let queued_before = app.runtime.commands.len();
    app.refresh_assignment_availability_if_due();

    assert!(!app.work.availability.loading);
    assert_eq!(app.runtime.commands.len(), queued_before);
    assert!(
        app.work.availability
            .last_attempt
            .is_some_and(|completed| completed.elapsed() < Duration::from_secs(1))
    );
}

#[test]
fn assignment_availability_mutations_invalidate_current_and_persisted_state() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api.clone());
    app.sync_work_config(api.metadata());
    app.view = AppView::Annotate;
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.tasks = app
        .work.tasks
        .iter()
        .map(|task| (task.task_id.clone(), true))
        .collect();
    app.work.availability.resolved = true;
    app.work.availability.checked_at = Some(labello_domain::now());
    app.work.availability.load_after_resolution = true;
    app.runtime.persistence.preference = Some(WorkspacePreference {
                    prelabel_choices: Default::default(),
        version: 2,
        dataset_id: app.config.dataset_id.clone(),
        view: StoredView::Annotate,
        task_id: app.work.selected_task_id.clone(),
        assignment_id: None,
        assignment_image_id: None,
        assignment_kind: None,
        drawer: None,
        workflow_panel_collapsed: false,
        inspector_panel_collapsed: false,
        show_settings: false,
        show_tutorial: false,
        selected_annotation: None,
        canvas: StoredCanvasTransform {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        },
        availability: Some(StoredAssignmentAvailability {
                reasons: Default::default(),
            kind: AssignmentKind::Annotation,
            tasks: app.work.availability.tasks.clone(),
            checked_at: labello_domain::now(),
        }),
    });
    let request = test_request(&app, 42, Some("demo"));

    app.queue_command(UiCommand::Ingest {
        request,
        dataset_id: app.config.dataset_id.clone(),
    });

    assert!(app.work.availability.checked_at.is_none());
    assert!(!app.work.availability.resolved);
    assert!(!app.work.availability.load_after_resolution);
    assert!(
        app.runtime
            .persistence
            .preference
            .as_ref()
            .is_some_and(|preference| preference.availability.is_none())
    );
}

#[test]
fn assignment_availability_waits_for_post_mutation_result_before_claiming() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api.clone());
    app.sync_work_config(api.metadata());
    app.view = AppView::Annotate;

    app.request_next_image();
    let request_a = take_assignment_availability_request(&mut app);
    assert!(app.work.availability.load_after_resolution);

    let mutation_request = test_request(&app, 42_000, Some("demo"));
    app.queue_command(UiCommand::Ingest {
        request: mutation_request,
        dataset_id: app.config.dataset_id.clone(),
    });
    assert!(!app.work.availability.load_after_resolution);
    assert!(app.work.availability.refresh_after_load);

    deliver_assignment_availability(&mut app, request_a, true);
    let request_b = take_assignment_availability_request(&mut app);
    deliver_assignment_availability(&mut app, request_b, true);
    assert!(app.work.availability.resolved);
    assert!(!app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::ClaimAssignment { .. }
    )));

    app.assignment_availability_mutation_completed(&app.config.dataset_id.clone(), true);
    let request_c = take_assignment_availability_request(&mut app);
    assert!(!app.work.availability.resolved);
    assert!(!app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::ClaimAssignment { .. }
    )));

    deliver_assignment_availability(&mut app, request_c, true);
    assert!(app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::ClaimAssignment { .. }
    )));
}

#[test]
fn assignment_availability_completion_supersedes_an_in_flight_interim_result() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api.clone());
    app.sync_work_config(api.metadata());
    app.view = AppView::Annotate;

    app.request_next_image();
    let request_a = take_assignment_availability_request(&mut app);
    app.queue_command(UiCommand::Ingest {
        request: test_request(&app, 42_001, Some("demo")),
        dataset_id: app.config.dataset_id.clone(),
    });
    deliver_assignment_availability(&mut app, request_a, true);
    let request_b = take_assignment_availability_request(&mut app);

    app.assignment_availability_mutation_completed(&app.config.dataset_id.clone(), true);
    assert!(app.work.availability.refresh_after_load);
    deliver_assignment_availability(&mut app, request_b, true);
    let request_c = take_assignment_availability_request(&mut app);
    assert!(!app.work.availability.resolved);
    assert!(!app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::ClaimAssignment { .. }
    )));

    deliver_assignment_availability(&mut app, request_c, true);
    assert!(app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::ClaimAssignment { .. }
    )));
}

#[test]
fn assignment_availability_ingest_completion_waits_and_failure_does_not_loop() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api.clone());
    app.sync_work_config(api.metadata());
    app.view = AppView::Annotate;
    let dataset_id = app.config.dataset_id.clone();

    let completed_request = test_request(&app, 42_100, Some("demo"));
    app.queue_command(UiCommand::Ingest {
        request: completed_request.clone(),
        dataset_id: dataset_id.clone(),
    });
    app.runtime.commands.pop_back();
    app.runtime
        .tx
        .send(UiMessage::IngestJobLoaded {
            request: completed_request,
            result: Ok(IngestJob {
                job_id: "completed".to_string(),
                dataset_id: dataset_id.clone(),
                status: IngestJobStatus::Completed,
                report: Some(IngestReport {
                    discovered_files: 1,
                    new_images: 1,
                    ..Default::default()
                }),
                error: None,
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.work.availability.load_after_resolution);
    assert!(app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::AssignmentAvailability { .. }
    )));
    assert!(!app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::ClaimAssignment { .. }
    )));

    app.runtime.commands.clear();
    app.runtime.active_requests.clear();
    app.work.availability.loading = false;
    let failed_request = test_request(&app, 42_101, Some("demo"));
    app.queue_command(UiCommand::Ingest {
        request: failed_request.clone(),
        dataset_id,
    });
    app.runtime.commands.pop_back();
    app.runtime
        .tx
        .send(UiMessage::IngestJobLoaded {
            request: failed_request,
            result: Err("ingest failed".to_string().into()),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(!app.work.availability.load_after_resolution);
    let refresh = take_assignment_availability_request(&mut app);
    deliver_assignment_availability_error(&mut app, refresh);
    assert!(!app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::AssignmentAvailability { .. }
    )));
}

#[test]
fn assignment_availability_invalidated_persisted_state_cannot_be_restored() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    step_until(&mut harness, 8, |app| {
        app.runtime
            .persistence
            .preference
            .as_ref()
            .is_some_and(|preference| preference.availability.is_some())
    });
    let app = harness.state_mut();
    let persisted = app.runtime.persistence.preference.clone().unwrap();
    app.work.availability = Default::default();
    app.runtime.persistence.preference = Some(persisted);
    assert!(app.restore_cached_assignment_availability());

    app.queue_command(UiCommand::Ingest {
        request: test_request(app, 42_200, Some("demo")),
        dataset_id: app.config.dataset_id.clone(),
    });
    assert!(!app.work.availability.resolved);
    assert!(
        app.runtime
            .persistence
            .preference
            .as_ref()
            .is_some_and(|preference| preference.availability.is_none())
    );

    app.begin_workspace_epoch();
    app.clear_current_image();
    assert!(!app.restore_cached_assignment_availability());
    app.request_next_image();
    assert!(app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::AssignmentAvailability { .. }
    )));
    app.persist_workspace_preference();
    assert!(
        app.runtime
            .persistence
            .preference
            .as_ref()
            .is_some_and(|preference| preference.availability.is_none())
    );
}

fn take_assignment_availability_request(app: &mut LabelloApp) -> RequestIdentity {
    let index = app
        .runtime
        .commands
        .iter()
        .position(|command| matches!(command, UiCommand::AssignmentAvailability { .. }))
        .expect("expected an assignment availability request");
    let UiCommand::AssignmentAvailability { request, .. } =
        app.runtime.commands.remove(index).unwrap()
    else {
        unreachable!()
    };
    request
}

fn deliver_assignment_availability(
    app: &mut LabelloApp,
    request: RequestIdentity,
    available: bool,
) {
    let tasks = app
        .work
        .tasks
        .iter()
        .map(|task| (task.task_id.clone(), available))
        .collect();
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            checked_assignments: Vec::new(),
            request,
            result: Ok(labello_client::AssignmentAvailability {
                queue: None,
                reasons: Default::default(),
                kind: AssignmentKind::Annotation,
                tasks,
                related: Vec::new(),
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
}

fn deliver_assignment_availability_error(app: &mut LabelloApp, request: RequestIdentity) {
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            checked_assignments: Vec::new(),
            request,
            result: Err("availability failed".to_string().into()),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
}

#[test]
fn stale_availability_is_discarded_after_refresh_and_dataset_switch() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api);
    app.view = AppView::Annotate;
    app.request_assignment_availability();
    let UiCommand::AssignmentAvailability { request, .. } =
        app.runtime.commands.pop_back().unwrap()
    else {
        panic!("expected availability request");
    };

    app.request_assignment_availability();
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            checked_assignments: Vec::new(),
            request,
            result: Ok(labello_client::AssignmentAvailability {
                queue: None,
                reasons: Default::default(),
                kind: AssignmentKind::Annotation,
                tasks: BTreeMap::from([(TaskId::from("bounding_box:person"), false)]),
                related: Vec::new(),
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(
        app.workflow_availability(&TaskId::from("bounding_box:person")),
        None,
        "a result superseded by a transition refresh must remain advisory"
    );
    let UiCommand::AssignmentAvailability { request, .. } =
        app.runtime.commands.pop_back().unwrap()
    else {
        panic!("expected replacement availability request");
    };

    app.begin_workspace_epoch();
    app.config.dataset_id = DatasetId::from("other");
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            checked_assignments: Vec::new(),
            request,
            result: Ok(labello_client::AssignmentAvailability {
                queue: None,
                reasons: Default::default(),
                kind: AssignmentKind::Annotation,
                tasks: BTreeMap::from([(TaskId::from("bounding_box:person"), false)]),
                related: Vec::new(),
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.work.availability.tasks.is_empty());
    assert_eq!(app.work.availability.dataset_id, None);
}

#[test]
fn stale_save_responses_cannot_replace_the_current_image_state() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let current_id = harness
        .state()
        .work.current
        .as_ref()
        .unwrap()
        .image
        .image_id
        .clone();
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::SaveFinished {
            request: test_request(harness.state(), u64::MAX, Some("demo")),
            operation_id: u64::MAX,
            assignment_id: AssignmentId::generate(),
            edit_generation: 0,
            completed: false,
            result: Box::new(Ok(ImageState::new(ImageId::from("img_stale")))),
        })
        .unwrap();
    harness.step();

    assert_eq!(
        harness.state().work.current.as_ref().unwrap().image.image_id,
        current_id
    );
    assert_eq!(
        harness.state().work.current_state.as_ref().unwrap().image_id,
        current_id
    );
}

#[test]
fn keybindings_are_editable_and_persisted() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    assert!(harness.query_by_label("Keyboard shortcuts").is_none());
    click_application_menu_item(&mut harness, "Settings");
    assert!(harness.query_by_label("Keyboard shortcuts").is_some());
    assert!(harness.query_by_label("Refocus").is_some());
    assert_eq!(
        harness
            .state()
            .work
            .shortcut_settings
            .draft
            .as_ref()
            .unwrap()
            .bindings[&labello_domain::UserAction::RefocusObject],
        labello_domain::KeyChord::new("R")
    );
    click_accesskit_button(&mut harness, "Record shortcut for Confirm / submit");
    assert_eq!(
        harness.state().work.shortcut_settings.recording,
        Some(labello_domain::UserAction::NextImage),
    );
    harness.key_press(egui::Key::Enter);
    harness.step();
    assert_eq!(harness.state().work.shortcut_settings.recording, None);
    assert_eq!(
        harness
            .state()
            .work.shortcut_settings
            .draft
            .as_ref()
            .unwrap()
            .bindings[&labello_domain::UserAction::NextImage]
            .key,
        "Enter"
    );
    let save = harness
        .query_all_by_role_and_label(egui::accesskit::Role::Button, "Save changes")
        .next()
        .unwrap();
    assert!(!save.accesskit_node().is_disabled());
    click_accesskit_button(&mut harness, "Save changes");
    step_until(&mut harness, 8, |app| !app.loading.keybindings);

    assert_eq!(api.counts().save_keybindings, 1);
    assert_eq!(
        harness.state().work.keybindings.bindings[&labello_domain::UserAction::NextImage].key,
        "Enter"
    );
    assert_eq!(
        harness.state().runtime.notice.as_deref(),
        Some("Keyboard shortcuts saved")
    );
    assert!(!harness.state().work.show_settings);
    assert!(harness.state().work.shortcut_settings.draft.is_none());
    assert!(harness.query_by_label("Keyboard shortcuts").is_none());
    harness.key_press(egui::Key::Enter);
    step_until(&mut harness, 16, |_| api.counts().complete_assignment == 1);
    assert_eq!(api.counts().complete_assignment, 1);
}

#[test]
fn pan_drag_shortcut_is_listed_and_persists() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    click_application_menu_item(&mut harness, "Settings");
    harness.state_mut().work.shortcut_settings.search = "pan".to_string();
    harness.set_size(egui::vec2(720.0, 700.0));
    harness.step();
    harness.step();

    let pan_drag_name = harness
        .query_all_by_value("Pan drag")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Label)
        .expect("Pan drag name")
        .rect();
    let pan_drag_control = harness
        .query_all_by_label_contains("Record shortcut for Pan drag: Ctrl")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("Pan drag recorder")
        .rect();
    let hint = harness
        .query_all_by_value("Hold the modifier and left-drag. Middle-drag also pans.")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Label)
        .expect("pan gesture hint").rect();
    let reset = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Reset Pan drag shortcut").rect();
    assert!(pan_drag_name.bottom() <= hint.top());
    assert!(hint.bottom() <= pan_drag_control.top());
    assert!(!pan_drag_control.intersects(reset));

    click_accesskit_button(&mut harness, "Record shortcut for Pan drag: Ctrl");
    assert!(harness.state().work.shortcut_settings.recording_pan_drag);
    harness.input_mut().modifiers = egui::Modifiers::ALT;
    harness.step();
    harness.input_mut().modifiers = egui::Modifiers::NONE;
    assert!(!harness.state().work.shortcut_settings.recording_pan_drag);
    assert_eq!(
        harness
            .state()
            .work
            .shortcut_settings
            .draft
            .as_ref()
            .expect("settings draft")
            .pan_drag_modifier,
        labello_domain::PanDragModifier::Alt
    );
    assert!(
        harness
            .query_by_role_and_label(
                egui::accesskit::Role::Button,
                "Record shortcut for Pan drag: Alt"
            )
            .is_some()
    );

    click_accesskit_button(&mut harness, "Save changes");
    step_until(&mut harness, 8, |app| !app.loading.keybindings);
    assert_eq!(api.counts().save_keybindings, 1);
    assert_eq!(
        harness.state().work.keybindings.pan_drag_modifier,
        labello_domain::PanDragModifier::Alt
    );
}

#[test]
fn shortcut_search_includes_assigned_keys_modifiers_and_status() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    click_application_menu_item(&mut harness, "Settings");

    harness.state_mut().work.shortcut_settings.search = "control left-drag".to_string();
    harness.step();
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Pan:")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Confirm / submit")
            .is_none()
    );

    harness.state_mut().work.shortcut_settings.search = "middle-drag".to_string();
    harness.step();
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Pan:")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Confirm / submit")
            .is_none()
    );

    harness.state_mut().work.shortcut_settings.search = "space".to_string();
    harness.step();
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Confirm / submit")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Undo")
            .is_none()
    );

    harness.state_mut().work.shortcut_settings.search = "assigned control z".to_string();
    harness.step();
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Undo")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Confirm / submit")
            .is_none()
    );

    let draft = harness
        .state_mut()
        .work
        .shortcut_settings
        .draft
        .as_mut()
        .expect("settings draft");
    let conflicting_chord =
        draft.bindings[&labello_domain::UserAction::MarkKeypointAbsent].clone();
    draft
        .bindings
        .insert(labello_domain::UserAction::NextImage, conflicting_chord);
    harness.state_mut().work.shortcut_settings.search = "conflict".to_string();
    harness.step();
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Confirm / submit")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Not present")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Undo")
            .is_none()
    );

    harness
        .state_mut()
        .work
        .shortcut_settings
        .draft
        .as_mut()
        .expect("settings draft")
        .bindings
        .remove(&labello_domain::UserAction::FitImage);
    harness.state_mut().work.shortcut_settings.search = "unassigned".to_string();
    harness.step();
    assert!(
        harness
            .query_by_label("Record shortcut for Fit: Unassigned")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Record shortcut for Confirm / submit")
            .is_none()
    );

    harness.state_mut().work.shortcut_settings.search = "no-such-shortcut".to_string();
    harness.step();
    assert!(
        harness
            .query_by_label("No shortcuts match your search.")
            .is_some()
    );
}

#[test]
fn failed_shortcut_save_keeps_the_draft_and_shows_the_error_in_settings() {
    let api = Rc::new(SpyApi::new());
    let mut app = LabelloApp::default();
    app.runtime.api = Some(api);
    app.open_shortcut_settings();
    app.work.shortcut_settings
        .draft
        .as_mut()
        .unwrap()
        .bindings
        .get_mut(&labello_domain::UserAction::NextImage)
        .unwrap()
        .key = "Enter".to_string();
    let draft = app.work.shortcut_settings.draft.clone();

    app.request_keybindings_save();
    let UiCommand::SaveKeybindings { request, .. } =
        app.runtime.commands.pop_back().expect("save command")
    else {
        panic!("expected keybinding save command");
    };
    app.runtime
        .tx
        .send(UiMessage::KeybindingsSaved {
            request,
            result: Err("settings unavailable".to_string().into()),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());

    assert!(app.work.show_settings);
    assert_eq!(app.work.shortcut_settings.draft, draft);
    assert_eq!(
        app.work.shortcut_settings.error.as_deref(),
        Some("settings unavailable")
    );
    let harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 780.0))
        .build_eframe(move |_| app);
    assert!(
        harness
            .query_by_label("Could not save shortcuts: settings unavailable")
            .is_some()
    );
}

#[test]
fn shortcut_settings_cancel_discards_the_draft() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let baseline =
        harness.state().work.keybindings.bindings[&labello_domain::UserAction::NextImage].clone();
    click_application_menu_item(&mut harness, "Settings");
    click_accesskit_button(&mut harness, "Record shortcut for Confirm / submit");
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(harness.state().work.show_settings);
    assert_eq!(harness.state().work.shortcut_settings.recording, None);
    assert!(!harness.state().work.shortcut_settings.confirm_discard);
    click_accesskit_button(&mut harness, "Record shortcut for Confirm / submit");
    harness.key_press(egui::Key::Enter);
    harness.step();
    click(&mut harness, "Cancel");
    harness.step();
    assert!(
        harness
            .query_by_label("Discard shortcut changes?")
            .is_some()
    );
    click_accesskit_button(&mut harness, "Discard changes");

    assert!(!harness.state().work.show_settings);
    assert_eq!(
        harness.state().work.keybindings.bindings[&labello_domain::UserAction::NextImage],
        baseline
    );
}

#[test]
fn shortcut_settings_lock_editing_while_saving() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    click_application_menu_item(&mut harness, "Settings");
    harness.state_mut().loading.keybindings = true;
    harness.step();

    assert!(harness.query_by_label("Close window").is_none());

    for label in [
        "Record shortcut for Confirm / submit",
        "Reset Confirm / submit",
        "Restore all defaults",
        "Cancel",
    ] {
        let control = harness
            .query_all_by_label_contains(label)
            .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap_or_else(|| panic!("missing {label}"));
        assert!(control.accesskit_node().is_disabled(), "{label} is enabled");
    }
}

#[test]
fn draft_recovery_modal_blocks_background_controls() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 780.0))
        .build_eframe(|_| LabelloApp::default());
    let menu = harness
        .query_all_by_label_contains("Open settings")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("settings button")
        .rect()
        .center();
    let metadata = SpyApi::new().metadata();
    let identity = crate::persistence::StorageIdentity::new(
        &harness.state().config.api_base_url,
        harness.state().config.user_id.clone(),
    )
    .unwrap();
    let draft = crate::persistence::AdminDraft::new(
        &identity,
        metadata.dataset_id.clone(),
        &metadata,
        &metadata,
    );
    harness.state_mut().runtime.persistence.recovery =
        Some(crate::persistence::DraftRecovery::Admin(
            Box::new(draft),
            crate::persistence::DraftValidation::Valid,
        ));
    harness.step();
    assert!(harness.query_by_label("Unsaved admin draft").is_some());

    click_at(&mut harness, menu);

    assert!(!harness.state().work.show_settings);
    assert!(harness.state().runtime.persistence.recovery.is_some());
}

#[test]
fn overlays_and_menus_block_background_shortcuts() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    let image_id = harness
        .state()
        .work.assignment
        .as_ref()
        .unwrap()
        .image_id
        .clone();
    harness.state_mut().work.drawer = Some(Drawer::Inspector);
    harness.step();

    harness.key_press(egui::Key::ArrowRight);
    harness.step();

    assert_eq!(api.counts().complete_assignment, 0);
    assert_eq!(
        harness.state().work.assignment.as_ref().unwrap().image_id,
        image_id
    );

    harness.state_mut().work.canvas.zoom_in();
    harness.state_mut().work.canvas.toggle_pan_mode();
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(harness.state().work.canvas.pan_mode());
    harness.state_mut().work.drawer = None;
    harness.step();

    harness.set_size(egui::vec2(320.0, 568.0));
    harness.step();
    click(&mut harness, "Open navigation");
    harness.key_press(egui::Key::ArrowRight);
    harness.step();
    assert_eq!(api.counts().complete_assignment, 0);
    assert_eq!(
        harness.state().work.assignment.as_ref().unwrap().image_id,
        image_id
    );
}

#[test]
fn pan_mode_shortcut_requires_zoom_and_escape_returns_to_annotation_mode() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let zoom = harness.state().work.keybindings.bindings[&labello_domain::UserAction::ZoomIn].clone();
    harness
        .state_mut()
        .work.keybindings
        .bindings
        .insert(labello_domain::UserAction::RetryImageLoad, zoom);
    assert!(harness.state().work.keybindings.validate().is_ok());

    harness.key_press(egui::Key::P);
    harness.step();
    assert!(!harness.state().work.canvas.pan_mode());
    harness.key_press(egui::Key::Plus);
    harness.step();
    assert!(harness.state().work.canvas.current_zoom() > 1.0);
    harness.key_press(egui::Key::P);
    harness.step();
    assert!(harness.state().work.canvas.pan_mode());
    assert!(harness.query_by_label("Pan").is_some());

    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(!harness.state().work.canvas.pan_mode());
}

#[test]
fn logical_primary_and_shifted_punctuation_shortcuts_dispatch() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    harness.state_mut().create_bbox(BoundingBox {
        x: 0.1,
        y: 0.1,
        width: 0.2,
        height: 0.2,
    });
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::S);
    harness.step();
    step_until(&mut harness, 8, |app| !app.loading.saving);
    assert_eq!(api.counts().append_event, 1);

    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Questionmark);
    harness.step();
    assert!(harness.state().work.show_tutorial);
}

#[test]
fn stale_prefetch_response_cannot_enter_the_queue() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let loaded = harness.state_mut().work.queue.pop_prepared().unwrap();
    harness.state_mut().work.queue.clear();
    let operation_id = 90_001;
    let request = test_request(harness.state(), operation_id, Some("demo"));
    harness.state_mut().work.active_prefetch_id = Some(operation_id);
    harness
        .state_mut()
        .runtime
        .active_requests
        .insert(operation_id);
    harness.state_mut().begin_workspace_epoch();
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::PrefetchLoaded {
            imbalance_limited: false,
            request,
            operation_id,
            assignment: Some(loaded.assignment.clone()),
            result: Box::new(Ok(Some(loaded))),
        })
        .unwrap();

    harness
        .state_mut()
        .process_messages(&egui::Context::default());
    assert!(harness.state().work.queue.is_empty());
    step_until(&mut harness, 8, |_| api.counts().release_assignment > 0);
}

#[test]
fn fresh_prefetch_response_does_not_trust_the_local_wall_clock() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let mut loaded = harness.state_mut().work.queue.pop_prepared().unwrap();
    harness.state_mut().work.queue.clear();
    loaded.assignment.expires_at = Some(now() - chrono::Duration::seconds(1));
    let loaded_image_id = loaded.assignment.image_id.clone();
    let operation_id = 90_002;
    let request = test_request(harness.state(), operation_id, Some("demo"));
    harness.state_mut().work.active_prefetch_id = Some(operation_id);
    harness.state_mut().work.queue.set_loading(true);
    harness
        .state_mut()
        .runtime
        .active_requests
        .insert(operation_id);
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::PrefetchLoaded {
            imbalance_limited: false,
            request,
            operation_id,
            assignment: Some(loaded.assignment.clone()),
            result: Box::new(Ok(Some(loaded))),
        })
        .unwrap();

    harness
        .state_mut()
        .process_messages(&egui::Context::default());

    assert!(
        harness
            .state()
            .work
            .queue
            .prepared_image_ids()
            .contains(&loaded_image_id)
    );
}

#[test]
fn delayed_obsolete_load_does_not_release_the_current_assignment() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let assignment = harness.state().work.assignment.clone().unwrap();
    let mut request = test_request(harness.state(), 90_010, Some("demo"));
    request.workspace_epoch = request.workspace_epoch.wrapping_sub(1);
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::ImageLoaded {
            request,
            operation_id: 90_010,
            assignment: Some(assignment.clone()),
            result: Box::new(Err("superseded image load".to_string().into())),
        })
        .unwrap();
    harness
        .state_mut()
        .process_messages(&egui::Context::default());
    for _ in 0..8 {
        harness.step();
    }
    assert!(
        api.has_active_assignment(&assignment.assignment_id),
        "delayed cleanup cancelled the assignment currently shown for labeling"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn overlapping_claims_remain_saveable_in_both_response_orders() {
    for obsolete_first in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut harness = loaded_work_harness(api.clone());
        step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
        let original = harness.state().work.assignment.clone().unwrap();
        let scheduled = Rc::new(RefCell::new(std::collections::VecDeque::new()));
        let scheduled_for_spawner = scheduled.clone();
        let app = harness.state_mut();
        // Existing reservations stay on the server, as during an interrupted load.
        app.work.queue.clear();
        app.begin_workspace_epoch();
        app.clear_current_image();
        app.set_native_task_spawner(move |future| {
            scheduled_for_spawner.borrow_mut().push_back(future);
        });
        let start_claim = |app: &mut LabelloApp| {
            let operation_id = app.next_operation();
            let request = test_request(app, operation_id, Some("demo"));
            app.work.active_load_id = Some(operation_id);
            app.loading.image = true;
            app.runtime.active_requests.insert(operation_id);
            app.start_workflow_command(
                api.clone(),
                UiCommand::ClaimAssignment {
                    request,
                    operation_id,
                    dataset_id: DatasetId::from("demo"),
                    task_id: original.task_id.clone(),
                    prelabel_config_ids: Vec::new(),
                    kind: AssignmentKind::Annotation,
                    reclaim_assignment_id: Some(original.assignment_id.clone()),
                    excluded_image_ids: Vec::new(),
                },
            );
        };
        start_claim(app);
        poll_ready_task(scheduled.borrow_mut().pop_front().unwrap());
        let obsolete = app.runtime.rx.try_recv().unwrap();
        app.begin_workspace_epoch();
        start_claim(app);
        if obsolete_first {
            app.runtime.tx.send(obsolete).unwrap();
            app.process_messages(&egui::Context::default());
            assert_eq!(api.counts().release_assignment, 0);
            poll_ready_task(scheduled.borrow_mut().pop_front().unwrap());
        } else {
            poll_ready_task(scheduled.borrow_mut().pop_front().unwrap());
            app.runtime.tx.send(obsolete).unwrap();
        }
        app.process_messages(&egui::Context::default());
        // Run any cleanup that was scheduled, so an erroneous release cannot hide.
        while let Some(task) = scheduled.borrow_mut().pop_front() {
            poll_ready_task(task);
        }
        app.runtime.native_task_spawner = None;
        assert!(api.has_active_assignment(&original.assignment_id));
        assert_eq!(
            app.work.assignment.as_ref().unwrap().assignment_id,
            original.assignment_id
        );
        app.create_bbox(BoundingBox {
            x: 0.1,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        });
        app.request_save(false);
        step_until(&mut harness, 12, |app| !app.loading.saving);
        assert_eq!(
            harness.state().work.save_status,
            SaveStatus::Saved,
            "save failed after overlapping claims: {:?}",
            harness.state().runtime.error
        );
        assert_eq!(api.counts().annotation_batch, 1);
    }
}

#[test]
fn failed_prefetch_keeps_current_and_prepared_reservations() {
    for prepared in [false, true] {
        for stale in [false, true] {
            let api = Rc::new(SpyApi::new());
            let mut harness = loaded_work_harness(api.clone());
            step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
            let assignment = if prepared {
                let loaded = harness.state_mut().work.queue.pop_prepared().unwrap();
                let assignment = loaded.assignment.clone();
                harness.state_mut().work.queue.push_prepared(loaded);
                assignment
            } else {
                harness.state().work.assignment.clone().unwrap()
            };
            let operation_id = harness.state_mut().next_operation();
            let mut request = test_request(harness.state(), operation_id, Some("demo"));
            harness.state_mut().work.active_prefetch_id = Some(operation_id);
            harness
                .state_mut()
                .runtime
                .active_requests
                .insert(operation_id);
            if stale {
                request.workspace_epoch = request.workspace_epoch.wrapping_sub(1);
            }
            harness
                .state()
                .runtime
                .tx
                .send(UiMessage::PrefetchLoaded {
                    imbalance_limited: false,
                    request,
                    operation_id,
                    assignment: Some(assignment.clone()),
                    result: Box::new(Err("preview failed".to_string().into())),
                })
                .unwrap();
            harness
                .state_mut()
                .process_messages(&egui::Context::default());
            assert!(api.has_active_assignment(&assignment.assignment_id));
            assert_eq!(api.counts().release_assignment, 0);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn new_claim_waits_for_unused_reservation_release() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let scheduled = Rc::new(RefCell::new(std::collections::VecDeque::new()));
    let scheduled_for_spawner = scheduled.clone();
    let app = harness.state_mut();
    app.runtime.commands.clear();
    app.set_native_task_spawner(move |future| {
        scheduled_for_spawner.borrow_mut().push_back(future);
    });
    let unused = app.work.queue.pop_prepared().unwrap().assignment;
    app.release_reservation(DatasetId::from("demo"), unused.clone());
    app.process_messages(&egui::Context::default());
    assert_eq!(scheduled.borrow().len(), 1);
    let operation_id = app.next_operation();
    let request = test_request(app, operation_id, Some("demo"));
    app.runtime.active_requests.insert(operation_id);
    app.start_workflow_command(
        api.clone(),
        UiCommand::ClaimAssignment {
            request,
            operation_id,
            dataset_id: DatasetId::from("demo"),
            task_id: unused.task_id.clone(),
            prelabel_config_ids: Vec::new(),
            kind: AssignmentKind::Annotation,
            reclaim_assignment_id: None,
            excluded_image_ids: Vec::new(),
        },
    );
    assert_eq!(
        scheduled.borrow().len(),
        1,
        "claim ran before release finished"
    );
    assert_eq!(app.runtime.commands.len(), 1);
    poll_ready_task(scheduled.borrow_mut().pop_front().unwrap());
    app.process_messages(&egui::Context::default());
    assert!(!api.has_active_assignment(&unused.assignment_id));
    app.start_next_command();
    assert_eq!(
        scheduled.borrow().len(),
        1,
        "claim did not resume after release"
    );
    poll_ready_task(scheduled.borrow_mut().pop_front().unwrap());
    assert_eq!(api.counts().release_assignment, 1);
}

#[test]
fn stale_blocking_claim_releases_its_assignment() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let assignment = harness.state_mut().work.queue.pop_prepared().unwrap().assignment;
    harness.state_mut().work.queue.clear();
    let operation_id = 90_002;
    let request = test_request(harness.state(), operation_id, Some("demo"));
    harness.state_mut().work.active_load_id = Some(operation_id);
    harness
        .state_mut()
        .runtime
        .active_requests
        .insert(operation_id);
    harness.state_mut().begin_workspace_epoch();
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::ImageLoaded {
            request,
            operation_id,
            assignment: Some(assignment),
            result: Box::new(Err("stale load".to_string().into())),
        })
        .unwrap();

    harness
        .state_mut()
        .process_messages(&egui::Context::default());
    step_until(&mut harness, 8, |_| api.counts().release_assignment > 0);
}

#[test]
fn queue_saturation_rolls_back_dataset_admin_and_session_owners() {
    let api = Rc::new(SpyApi::new());
    let metadata = api.metadata();
    let users = api.dataset_users();
    let mut app = base_live_app(api);
    app.auth.checked = true;
    app.datasets.metadata = Some(metadata.clone());
    app.datasets.admin_config = Some(metadata.clone());
    app.datasets.admin_baseline = Some(metadata);
    app.datasets.users = users.clone();
    app.datasets.users_baseline = users;

    saturate_command_queue(&mut app);
    app.request_dataset_list();
    assert!(!app.loading.datasets);
    assert!(app.datasets.summaries_error.is_some());

    saturate_command_queue(&mut app);
    app.setup.create_dataset_id = "queued-dataset".to_string();
    app.setup.create_dataset_name = "Queued dataset".to_string();
    app.request_create_dataset();
    assert!(!app.loading.dataset);

    saturate_command_queue(&mut app);
    app.request_admin_dataset();
    assert!(!app.loading.admin);
    assert!(app.admin.load_error.is_some());

    saturate_command_queue(&mut app);
    app.request_admin_save();
    assert!(!app.loading.admin);

    app.datasets
        .users
        .iter_mut()
        .find(|user| user.account.user_id == UserId::from("reviewer"))
        .unwrap()
        .roles
        .push(DatasetRole::Reviewer);
    saturate_command_queue(&mut app);
    app.request_admin_changes_save();
    assert!(app.loading.roles_user.is_none());
    assert!(app.admin.pending_role_saves.is_empty());

    saturate_command_queue(&mut app);
    app.request_images();
    assert!(!app.loading.images);
    assert!(app.admin.images_error.is_some());

    saturate_command_queue(&mut app);
    app.request_snapshots();
    assert!(!app.loading.snapshots);
    assert!(app.admin.snapshots_error.is_some());

    saturate_command_queue(&mut app);
    app.request_snapshot_create();
    assert!(!app.loading.creating_snapshot);
    assert!(app.admin.snapshot_action_error.is_some());

    saturate_command_queue(&mut app);
    app.request_snapshot_download("snapshot".to_string(), "manifest.json".to_string());
    assert!(app.loading.snapshot_file.is_none());
    assert!(app.admin.snapshot_action_error.is_some());

    saturate_command_queue(&mut app);
    app.request_ingest();
    assert!(!app.loading.ingesting);
    assert!(!app.loading.ingest_polling);

    saturate_command_queue(&mut app);
    app.loading.ingesting = true;
    app.loading.ingest_job_id = Some("job".to_string());
    app.loading.last_ingest_poll = Some(Instant::now() - Duration::from_secs(1));
    app.refresh_ingest_if_due();
    assert!(app.loading.ingesting);
    assert!(!app.loading.ingest_polling);

    saturate_command_queue(&mut app);
    app.request_keybindings_save();
    assert!(!app.loading.keybindings);
    assert!(app.work.shortcut_settings.error.is_some());

    app.view = AppView::Stats;
    saturate_command_queue(&mut app);
    app.request_stats();
    assert!(!app.loading.stats);
    assert!(app.datasets.active_stats_request.is_none());
    assert!(app.datasets.stats_error.is_some());

    saturate_command_queue(&mut app);
    let session_request = test_request(&app, 90_001, None);
    app.loading.session = true;
    app.auth.checked = false;
    app.auth.active_session_request_id = Some(session_request.request_id);
    assert!(!app.queue_command(UiCommand::Session {
        request: session_request
    }));
    assert!(!app.loading.session);
    assert!(app.auth.checked);
    assert!(app.auth.active_session_request_id.is_none());

    saturate_command_queue(&mut app);
    let logout_request = test_request(&app, 90_002, None);
    app.loading.logout = true;
    assert!(!app.queue_command(UiCommand::Logout {
        request: logout_request
    }));
    assert!(!app.loading.logout);
}

#[test]
fn queue_saturation_rolls_back_claim_release_review_and_correction() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);

    saturate_command_queue(harness.state_mut());
    harness.state_mut().skip_assignment();
    assert!(!harness.state().loading.saving);
    assert!(harness.state().work.active_operation_id.is_none());
    assert!(harness.state().work.pending_transition.is_none());

    harness.state_mut().clear_current_image();
    saturate_command_queue(harness.state_mut());
    harness.state_mut().request_next_image();
    assert!(!harness.state().loading.image);
    assert!(harness.state().work.active_load_id.is_none());
    assert!(!harness.state().work.queue.is_loading());

    let api = Rc::new(SpyApi::new());
    seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut review = loaded_review_harness(api);
    saturate_command_queue(review.state_mut());
    review
        .state_mut()
        .request_review(labello_domain::ReviewDecision::Approved);
    assert!(!review.state().loading.saving);
    assert!(review.state().work.active_operation_id.is_none());

    let annotation_id = review.state().work.selected_annotation.clone().unwrap();
    review.state_mut().start_correction();
    review.state_mut().edit_correction_bbox(BoundingBoxEdit {
        annotation_id,
        bounding_box: BoundingBox {
            x: 0.3,
            y: 0.3,
            width: 0.2,
            height: 0.2,
        },
    });
    saturate_command_queue(review.state_mut());
    review.state_mut().request_correction();
    review.state_mut().submit_staged_review_corrections();
    assert!(!review.state().loading.saving);
    assert!(review.state().work.active_operation_id.is_none());
    assert!(review.state().has_review_corrections());


}

#[test]
fn stale_auth_and_workspace_messages_cannot_mutate_current_owners() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    let stale_auth = test_request(&app, 100, None);
    app.begin_auth_epoch();
    app.loading.datasets = true;
    app.runtime.active_requests.insert(101);
    app.runtime
        .tx
        .send(UiMessage::DatasetList {
            request: stale_auth,
            result: Ok(vec![DatasetSummary {
                dataset_id: DatasetId::from("stale"),
                name: "Stale".to_string(),
                roles: vec![DatasetRole::DataAdmin],
                total_images: 999,
            }]),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.loading.datasets);
    assert!(app.datasets.summaries.is_empty());
    assert!(app.runtime.active_requests.contains(&101));

    let stale_workspace = test_request(&app, 102, Some("demo"));
    app.begin_workspace_epoch();
    app.config.dataset_id = DatasetId::from("other");
    app.loading.admin = true;
    app.runtime.active_requests.insert(103);
    app.runtime
        .tx
        .send(UiMessage::AdminSaved {
            request: stale_workspace,
            result: Box::new(Ok(SpyApi::new().metadata())),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.loading.admin);
    assert!(app.datasets.admin_config.is_none());
    assert!(app.runtime.active_requests.contains(&103));
}

#[test]
fn api_login_logout_dataset_and_view_boundaries_rotate_epochs() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    let initial_auth = app.auth_epoch;
    let initial_workspace = app.workspace_epoch;
    app.datasets.requested_view = Some(AppView::Admin);
    app.runtime.persistence.restoration_attempted = true;

    app.rebuild_http_api();
    assert!(app.auth_epoch > initial_auth);
    assert!(app.workspace_epoch > initial_workspace);
    assert!(app.datasets.requested_view.is_none());
    assert!(!app.runtime.persistence.restoration_attempted);

    let rebuilt_auth = app.auth_epoch;
    app.request_session();
    assert!(app.auth_epoch > rebuilt_auth);
    let login_request = app.runtime.commands.back().unwrap().request();
    assert_eq!(login_request.auth_epoch, app.auth_epoch);
    assert_eq!(login_request.workspace_epoch, app.workspace_epoch);

    app.loading.session = false;
    let login_auth = app.auth_epoch;
    app.request_logout();
    assert!(app.auth_epoch > login_auth);
    let logout_request = app.runtime.commands.back().unwrap().request();
    assert_eq!(logout_request.auth_epoch, app.auth_epoch);

    app.loading.logout = false;
    app.runtime.commands.clear();
    let before_dataset = app.workspace_epoch;
    app.request_load_dataset();
    assert!(app.workspace_epoch > before_dataset);

    app.loading.dataset = false;
    app.runtime.commands.clear();
    app.datasets.metadata = Some(SpyApi::new().metadata());
    app.view = AppView::Annotate;
    let before_view = app.workspace_epoch;
    app.execute_transition(crate::app::PendingTransition::View(AppView::Stats));
    assert!(app.workspace_epoch > before_view);
}

#[test]
fn dataset_states_distinguish_loading_and_stale_refresh_failure() {
    let api = Rc::new(SpyApi::new());
    let mut harness = live_harness(api);
    step_until(&mut harness, 8, |app| !app.datasets.summaries.is_empty());
    let summaries = harness.state().datasets.summaries.clone();

    harness.state_mut().auth.checked = false;
    harness.state_mut().loading.session = true;
    harness.step();
    assert!(
        harness
            .query_by_label("Checking your session...")
            .is_some()
    );
    assert!(
        harness
            .query_by_label("Continue with Demo Dataset")
            .is_none()
    );
    harness.state_mut().auth.checked = true;
    harness.state_mut().loading.session = false;
    harness.step();

    harness.state_mut().datasets.summaries.clear();
    harness.state_mut().loading.datasets = true;
    harness.step();
    assert!(harness.query_by_label("Loading datasets...").is_some());
    assert!(
        harness
            .query_by_label("No accessible datasets yet.")
            .is_none()
    );

    harness.state_mut().loading.datasets = false;
    harness.state_mut().datasets.summaries_error = Some("initial failure".to_string());
    harness.step();
    assert!(
        harness
            .query_by_label("Could not load datasets: initial failure")
            .is_some()
    );
    assert!(
        harness
            .query_by_label("No accessible datasets yet.")
            .is_none()
    );

    harness.state_mut().datasets.summaries = summaries.clone();
    harness.state_mut().request_dataset_list();
    let UiCommand::DatasetList { request } = harness
        .state_mut()
        .runtime
        .commands
        .pop_back()
        .expect("dataset list command")
    else {
        panic!("expected dataset list command");
    };
    harness
        .state_mut()
        .runtime
        .tx
        .send(UiMessage::DatasetList {
            request,
            result: Err("dataset service unavailable".to_string().into()),
        })
        .unwrap();
    harness
        .state_mut()
        .process_messages(&egui::Context::default());
    harness.step();

    assert_eq!(harness.state().datasets.summaries, summaries);
    assert!(
        harness
            .query_by_label("Showing saved results. Refresh failed: dataset service unavailable")
            .is_some()
    );
    assert!(
        harness
            .query_by_label("Continue with Demo Dataset")
            .is_some()
    );
    harness.set_size(egui::vec2(320.0, 568.0));
    harness.state_mut().loading.dataset = true;
    harness.step();
    let refresh = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Refresh");
    let retry = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Retry");
    let opening = harness.get_by_label("Opening dataset...").rect();
    assert!(opening.top() >= refresh.rect().bottom());
    assert!(refresh.accesskit_node().is_disabled());
    assert!(retry.accesskit_node().is_disabled());
}

#[test]
fn stale_assignment_operations_do_not_clear_the_active_loading_owner() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let assignment = harness.state().work.assignment.clone().unwrap();
    let state = harness.state().work.current_state.clone().unwrap();
    harness.state_mut().work.active_operation_id = Some(77);
    harness.state_mut().work.background_save_operation_id = Some(77);
    harness.state_mut().loading.saving = true;
    harness.state_mut().runtime.active_requests.insert(77);
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::SaveFinished {
            request: test_request(harness.state(), 76, Some("demo")),
            operation_id: 76,
            assignment_id: assignment.assignment_id.clone(),
            edit_generation: 0,
            completed: false,
            result: Box::new(Ok(state.clone())),
        })
        .unwrap();
    harness.step();
    assert!(harness.state().loading.saving);
    assert_eq!(harness.state().work.active_operation_id, Some(77));
    assert!(!harness.state().saving_blocks_interaction());

    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::SaveFinished {
            request: test_request(harness.state(), 77, Some("demo")),
            operation_id: 77,
            assignment_id: assignment.assignment_id,
            edit_generation: 0,
            completed: false,
            result: Box::new(Ok(state)),
        })
        .unwrap();
    harness.step();
    assert!(!harness.state().loading.saving);
    assert_eq!(harness.state().work.active_operation_id, None);
    assert_eq!(harness.state().work.background_save_operation_id, None);
}

#[test]
fn editing_a_persisted_box_saves_a_new_annotation_version() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_prelabel_work_harness(api.clone());
    click(&mut harness, "Confirm & next");
    click(&mut harness, "Save");
    step_until(&mut harness, 10, |app| app.work.save_status == SaveStatus::Saved);

    let annotation_id = harness.state().work.annotations[0].annotation_id.clone();
    let origin = harness.state().work.annotations[0].origin.clone();
    let object_group_id = harness.state().work.annotations[0].object_group_id.clone();
    harness.state_mut().edit_bbox(BoundingBoxEdit {
        annotation_id: annotation_id.clone(),
        bounding_box: BoundingBox {
            x: 0.2,
            y: 0.25,
            width: 0.3,
            height: 0.35,
        },
    });
    assert_eq!(harness.state().work.annotations[0].version, 2);
    assert_eq!(harness.state().work.annotations[0].origin, origin);
    assert_eq!(
        harness.state().work.annotations[0].object_group_id,
        object_group_id
    );
    assert!(matches!(
        harness.state().work.annotations[0].revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::Edited
        }
    ));
    assert_eq!(
        harness.state().work.annotations[0].author_user_id,
        UserId::from("admin")
    );
    assert!(matches!(
        origin,
        AnnotationOrigin::Native { legacy_v2: false }
    ));
    harness.state_mut().autosave();
    step_until(&mut harness, 10, |app| app.work.save_status == SaveStatus::Saved);

    assert!(api.events().iter().any(|payload| matches!(
        payload,
        EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version: Some(1),
            ..
        } if annotation.annotation_id == annotation_id && annotation.version == 2
    )));
}

#[test]
fn dragging_a_persisted_skeleton_keypoint_saves_a_new_annotation_version() {
    let api = Rc::new(SpyApi::new());
    {
        let mut state = api.state.borrow_mut();
        let task = &mut state.metadata.tasks[0];
        task.annotation_type = AnnotationType::Skeleton;
        task.prelabel_config_ids.clear();
        task.skeleton = Some(SkeletonSpec {
            keypoints: vec![KeypointSpec {
                name: "head".to_string(),
                required: true,
            }],
            edges: Vec::new(),
            allow_hidden: true,
            allow_absent: false,
        });
    }
    let mut harness = loaded_work_harness(api.clone());
    let canvas = harness.get_by_label("Annotation canvas").rect();
    click_at(&mut harness, canvas.center());
    click(&mut harness, "Save");
    step_until(&mut harness, 10, |app| app.work.save_status == SaveStatus::Saved);

    let annotation_id = harness.state().work.annotations[0].annotation_id.clone();
    drag_at(
        &mut harness,
        canvas.center(),
        canvas.center() + egui::vec2(40.0, -20.0),
    );
    let annotation = &harness.state().work.annotations[0];
    assert_eq!(annotation.version, 2);
    assert!(matches!(
        annotation.revision_source,
        RevisionSource::Human {
            action: HumanRevisionKind::Edited
        }
    ));
    assert!(harness.state().work.modified_annotations.contains(&annotation_id));
    assert!(matches!(
        annotation.geometry,
        AnnotationGeometry::Skeleton(ref skeleton)
            if skeleton.keypoints[0]
                .point
                .is_some_and(|point| point.x > 0.5 && point.y < 0.5)
    ));

    harness.state_mut().autosave();
    step_until(&mut harness, 10, |app| app.work.save_status == SaveStatus::Saved);
    assert!(api.events().iter().any(|payload| matches!(
        payload,
        EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version: Some(1),
            ..
        } if annotation.annotation_id == annotation_id && annotation.version == 2
    )));
}

#[test]
fn corrected_item_shortcuts_stage_locally_and_saturated_submission_retains_changes() {
    let api = Rc::new(SpyApi::new());
    seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api.clone());
    edit_test_review_box(harness.state_mut());
    assert!(harness.state().work.correction_draft.is_some());

    harness.key_press(egui::Key::Y);
    harness.step();
    harness.key_press(egui::Key::N);
    harness.step();
    assert_eq!(api.counts().record_review, 0);
    assert!(harness.state().review_overview());
    assert!(harness.state().has_review_corrections());
    assert_eq!(api.counts().record_correction, 0);
    saturate_command_queue(harness.state_mut());
    harness.state_mut().request_review(labello_domain::ReviewDecision::Rejected);
    assert!(harness.state().has_review_corrections());
    assert!(!harness.state().loading.saving);
    let submission = harness.state().work.review_corrections.submission.clone();
    harness.state_mut().runtime.commands.clear();
    harness.state_mut().runtime.active_requests.clear();
    harness.state_mut().request_review(labello_domain::ReviewDecision::Rejected);
    assert!(harness.state().loading.saving);
    assert_eq!(harness.state().work.review_corrections.submission, submission);
}

#[test]
fn stats_ignore_stale_request_and_dataset_responses() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    app.view = AppView::Stats;
    app.loading.stats = true;
    app.datasets.active_stats_request = Some((2, DatasetId::from("demo")));
    app.datasets.stats_error = Some("stale refresh failure".to_string());
    app.runtime.active_requests.insert(2);

    for (request_id, dataset_id) in [(1, "demo"), (2, "other")] {
        app.runtime
            .tx
            .send(UiMessage::StatsLoaded {
                request: test_request(&app, request_id, Some(dataset_id)),
                result: Ok(stats(request_id as usize)),
            })
            .unwrap();
    }
    app.process_messages(&egui::Context::default());
    assert!(app.loading.stats);
    assert_eq!(app.datasets.stats.total_images, 0);

    app.runtime
        .tx
        .send(UiMessage::StatsLoaded {
            request: test_request(&app, 2, Some("demo")),
            result: Ok(stats(42)),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(!app.loading.stats);
    assert_eq!(app.datasets.stats.total_images, 42);
    assert!(app.datasets.last_stats_completion.is_some());
    assert!(app.datasets.stats_error.is_none());
}

#[test]
fn stats_polling_is_scheduled_from_completion_and_queue_failure_recovers() {
    let api = Rc::new(SpyApi::new());
    let metadata = api.state.borrow().metadata.clone();
    let mut app = base_live_app(api);
    app.setup.started = true;
    app.view = AppView::Stats;
    app.datasets.metadata = Some(metadata);
    app.datasets.last_stats_attempt = Some(Instant::now());

    app.refresh_stats_if_due();
    assert!(app.runtime.commands.is_empty());

    app.datasets.last_stats_attempt = Some(Instant::now() - Duration::from_secs(4));
    app.refresh_stats_if_due();
    assert!(app.loading.stats);
    assert_eq!(app.runtime.commands.len(), 1);

    app.loading.stats = false;
    app.datasets.active_stats_request = None;
    app.runtime.commands.clear();
    for request_id in 10_000..10_064 {
        app.runtime.commands.push_back(UiCommand::DatasetList {
            request: test_request(&app, request_id, None),
        });
    }
    app.request_stats();
    assert!(!app.loading.stats);
    assert!(app.datasets.active_stats_request.is_none());
    assert!(app.datasets.last_stats_attempt.is_some());
    assert!(app.datasets.last_stats_completion.is_none());
}

#[test]
fn changing_datasets_cancels_an_inflight_stats_request() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    app.loading.stats = true;
    app.datasets.active_stats_request = Some((7, DatasetId::from("demo")));
    app.datasets.stats = stats(99);

    app.open_dataset(DatasetId::from("other"), AppView::Stats);

    assert!(!app.loading.stats);
    assert!(app.datasets.active_stats_request.is_none());
    assert_eq!(app.datasets.stats, DatasetStats::default());
}

#[test]
fn zoom_help_shows_configured_keys_and_gestures_without_workspace_widgets() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    for (action, key) in [
        (labello_domain::UserAction::ZoomIn, "F9"),
        (labello_domain::UserAction::ZoomOut, "F10"),
    ] {
        harness.state_mut().work.keybindings.bindings.insert(action, labello_domain::KeyChord::new(key));
    }
    harness.key_press(egui::Key::F9);
    harness.step();
    assert!(harness.state().work.canvas.current_zoom() > 1.0);
    harness.key_press(egui::Key::F10);
    harness.step();
    assert_eq!(harness.state().work.canvas.current_zoom(), 1.0);
    click_application_menu_item(&mut harness, "Settings");
    harness.state_mut().work.shortcut_settings.search = "zoom".to_string();
    harness.set_size(egui::vec2(720.0, 700.0));
    harness.step();
    harness.step();
    for label in ["Record shortcut for Zoom in: F9", "Record shortcut for Zoom out: F10"] {
        let button = harness.get_by_role_and_label(egui::accesskit::Role::Button, label);
        assert!(!button.accesskit_node().is_disabled());
        assert!(button.rect().bottom() < 700.0);
    }
    assert!(harness.query_by_label("Increase canvas zoom with this shortcut. Wheel, touchpad scrolling and pinch also zoom.").is_some());
    assert!(harness.query_by_label("Decrease canvas zoom with this shortcut. Wheel, touchpad scrolling and pinch also zoom.").is_some());
}

#[test]
fn browser_review_recovery_preserves_local_decisions_without_marking_an_untouched_editor_dirty() {
    use crate::persistence::{DraftRecovery, DraftValidation, ReviewDraft, StorageIdentity, StoredCorrectionDraft, WorkDraft, WorkDraftPayload};
    for changed in [false, true] {
        let api = Rc::new(SpyApi::new());
        seed_review_annotation(&api, AnnotationGeometry::BoundingBox(BoundingBox { x: 0.2, y: 0.2, width: 0.3, height: 0.3 }), true);
        let mut harness = loaded_review_harness(api);
        if changed { edit_test_review_box(harness.state_mut()); harness.state_mut().reject_review_item(); }
        let app = harness.state_mut();
        let identity = StorageIdentity::new(&app.config.api_base_url, app.config.user_id.clone()).unwrap();
        let expected = app.work.review_corrections.clone();
        let draft = WorkDraft::new(&identity, app.config.dataset_id.clone(), app.work.assignment.as_ref().unwrap(), app.work.current_state.as_ref().unwrap().current_sequence, 0, WorkDraftPayload::Review(ReviewDraft {
            target_annotation: app.work.selected_annotation.clone(),
            correction: app.work.correction_draft.as_ref().map(StoredCorrectionDraft::from),
            staged_corrections: Box::new(expected.clone()),
        }));
        let round_trip: WorkDraft = serde_json::from_str(&serde_json::to_string(&draft).unwrap()).unwrap();
        app.work.review_corrections = Default::default();
        app.work.correction_draft = None;
        app.work.review_index = 0;
        app.work.assignment_touched = false;
        app.runtime.persistence.recovery = Some(DraftRecovery::Work(Box::new(round_trip), DraftValidation::Valid));
        app.recover_browser_draft();
        assert_eq!(app.work.review_corrections, expected);
        assert_eq!(app.has_review_corrections(), changed);
        assert_eq!(app.assignment_has_work(), changed);
        assert_eq!(app.review_overview(), changed);
    }
}

#[test]
fn mouse_binding_records_saves_and_dispatches_only_on_canvas() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    click_application_menu_item(&mut harness, "Settings");
    click_accesskit_button(&mut harness, "Record shortcut for Confirm / submit");
    let position = egui::pos2(750.0, 150.0);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::SHIFT,
        });
        harness.step();
    }
    assert_eq!(harness.state().work.shortcut_settings.recording, None);
    let draft = harness.state().work.shortcut_settings.draft.as_ref().unwrap();
    assert_eq!(draft.bindings[&labello_domain::UserAction::NextImage].to_string(), "Shift+Right click");
    assert_eq!(api.counts().complete_assignment, 0);
    click_accesskit_button(&mut harness, "Save changes");
    step_until(&mut harness, 8, |app| !app.loading.keybindings);
    assert_eq!(api.counts().save_keybindings, 1);
    assert_eq!(harness.state().shortcut_text(&harness.ctx, labello_domain::UserAction::NextImage), "Shift+Right click");
    assert!(!harness.state().work.show_settings);
    let center = harness.get_by_label("Annotation canvas").rect().center();
    for (pos, modifiers) in [
        (egui::pos2(5.0, 5.0), egui::Modifiers::SHIFT),
        (center, egui::Modifiers::NONE),
    ] {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers });
            harness.step();
        }
        assert_eq!(api.counts().complete_assignment, 0);
    }
    harness.event(egui::Event::PointerButton { pos: center, button: egui::PointerButton::Secondary, pressed: true, modifiers: egui::Modifiers::SHIFT });
    step_until(&mut harness, 16, |_| api.counts().complete_assignment == 1);
    harness.run_steps(4);
    assert_eq!(api.counts().complete_assignment, 1);
    harness.event(egui::Event::PointerButton { pos: center, button: egui::PointerButton::Secondary, pressed: false, modifiers: egui::Modifiers::SHIFT });
    harness.run_steps(4);
    assert_eq!(api.counts().complete_assignment, 1);
}

#[test]
fn mouse_delete_obeys_annotation_loading_and_settings_guards() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    let center = harness.get_by_label("Annotation canvas").rect().center();
    drag_at(&mut harness, center - egui::vec2(55.0, 55.0), center + egui::vec2(55.0, 55.0));
    let selected = harness.state().work.selected_annotation.clone().expect("created box");
    harness.state_mut().work.keybindings.bindings.insert(
        labello_domain::UserAction::DeleteAnnotation,
        labello_domain::KeyChord::new("MouseRight"),
    );
    let right_click = |harness: &mut Harness<'static, LabelloApp>| {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton { pos: center, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        }
        harness.step();
    };
    for blocked in ["loading", "settings", "pen"] {
        match blocked {
            "loading" => harness.state_mut().loading.image = true,
            "settings" => harness.state_mut().work.show_settings = true,
            "pen" => crate::pointer_input::set_pen_pointer(&harness.ctx, true),
            _ => unreachable!(),
        }
        right_click(&mut harness);
        assert!(harness.state().work.annotations.iter().any(|a| a.annotation_id == selected && !a.deleted), "{blocked}");
        harness.state_mut().loading.image = false;
        harness.state_mut().work.show_settings = false;
        crate::pointer_input::set_pen_pointer(&harness.ctx, false);
        harness.run_steps(2);
    }
    right_click(&mut harness);
    assert!(harness.state().work.annotations.iter().any(|a| a.annotation_id == selected && a.deleted));
    assert!(harness.state().work.selected_annotation.is_none());
}

#[test]
fn compact_mouse_binding_rows_keep_labels_above_controls() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().open_shortcut_settings();
    harness.state_mut().work.shortcut_settings.search = "delete".to_string();
    let mut chord = labello_domain::KeyChord::primary("MouseExtra2");
    chord.shift = true;
    chord.alt = true;
    harness.state_mut().work.shortcut_settings.draft.as_mut().unwrap().bindings.insert(
        labello_domain::UserAction::DeleteAnnotation, chord,
    );
    for size in [egui::vec2(320.0, 568.0), egui::vec2(390.0, 844.0)] {
        harness.set_size(size);
        harness.run_steps(3);
        let label = harness.query_all_by_value("Delete")
            .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Label)
            .unwrap().rect();
        let control = harness.query_all_by_label_contains("Record shortcut for Delete")
            .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap().rect();
        assert!(label.bottom() <= control.top(), "action label overlaps mouse binding");
        let footer = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Restore all defaults").rect();
        assert!(control.bottom() <= footer.top(), "binding is clipped behind the footer");
        assert_visible_controls_clamped(&harness, size.x, size.y);
        assert_label_inside(&harness, "Keyboard shortcuts", size.x, size.y);
    }
}

#[test]
fn shortcut_button_names_find_primary_action_and_previous_image() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().open_shortcut_settings();
    for name in ["Next guide", "Confirm & next", "Submit correction", "Save missing object", "Save & next", "Confirm all guides & finish"] {
        harness.state_mut().work.shortcut_settings.search = name.into();
        harness.step();
        assert!(harness.query_by_label_contains("Record shortcut for Confirm / submit:").is_some(), "{name}");
    }
    harness.state_mut().work.shortcut_settings.search = "Previous image".into();
    harness.step();
    assert!(harness.query_by_label_contains("Record shortcut for Previous image:").is_some());
    assert!(harness.query_by_label_contains("Record shortcut for Previous object:").is_none());
}

#[test]
fn shortcut_rows_keep_text_outside_controls_at_every_viewport() {
    use egui::accesskit::Role;
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().open_shortcut_settings();
    for (width, height) in viewport_sizes() {
        harness.set_size(egui::vec2(width, height));
        for query in ["Next guide", "pan drag", "Undo", "Previous image"] {
            harness.state_mut().work.shortcut_settings.search = query.into();
            if let Some(draft) = harness.state_mut().work.shortcut_settings.draft.as_mut() {
                draft.bindings.insert(labello_domain::UserAction::NextImage, labello_domain::KeyChord {
                    key: "ArrowRight".into(), ctrl: true, shift: true, alt: true, command: true,
                });
            }
            harness.step();
            harness.step();
            for text in harness.query_all_by_role(Role::Label).filter(|node| {
                let value = node.accesskit_node().value().unwrap_or_default();
                ["Confirm / submit", "The primary work button:", "Pan", "Hold the modifier", "Use primary drag", "Undo", "Return to the last", "Previous image"]
                    .iter().any(|prefix| value.starts_with(prefix))
            }) {
                let rect = text.rect();
                // Compare sibling text against shortcut controls, including scrollable rows
                // outside the viewport. Viewport clamping alone cannot detect this overlap.
                for control in harness.query_all_by_role(Role::Button).filter(|node| {
                    let label = node.accesskit_node().label().unwrap_or_default();
                    label.starts_with("Record shortcut for ") || label.starts_with("Reset ")
                }) {
                    assert!(!rect.intersects(control.rect()), "{width}x{height} {query}: {text:?} overlaps {control:?}");
                }
            }
            assert_visible_controls_clamped(&harness, width, height);
        }
    }
}

#[test]
fn shortcut_conflicts_name_the_other_action_and_offer_a_filter() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().open_shortcut_settings();
    let draft = harness.state_mut().work.shortcut_settings.draft.as_mut().unwrap();
    draft.bindings.insert(labello_domain::UserAction::RedoEdit, draft.bindings[&labello_domain::UserAction::UndoEdit].clone());
    harness.state_mut().work.shortcut_settings.search = "Redo".into();
    harness.step();
    assert!(harness.query_by_label("Conflicts with Undo in annotation / migration.").is_some());
    click_accesskit_button(&mut harness, "Show conflicting shortcuts");
    harness.step();
    assert_eq!(harness.state().work.shortcut_settings.search, "conflict");
    assert!(harness.query_by_label_contains("Record shortcut for Undo:").is_some());
    assert!(harness.query_by_label_contains("Record shortcut for Redo:").is_some());
    assert!(harness.query_by_label_contains("Record shortcut for Previous image:").is_none());
}

#[test]
fn shortcut_last_action_and_footer_remain_reachable_by_scrolling() {
    use egui::accesskit::Role;
    for (width, height) in [(320.0, 320.0), (320.0, 568.0), (390.0, 844.0), (600.0, 800.0), (1440.0, 1000.0)] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        harness.set_size(egui::vec2(width, height));
        harness.state_mut().open_shortcut_settings();
        harness.step();
        harness.step();
        let last = "Record shortcut for Reject directly: N";
        harness.get_by_role_and_label(Role::Button, last).scroll_to_me();
        harness.run();
        assert_control_inside(&harness, last, Role::Button, width, height);
        for label in ["Restore all defaults", "Cancel", "Save changes"] {
            harness.get_by_role_and_label(Role::Button, label).scroll_to_me();
            harness.run();
            assert_control_inside(&harness, label, Role::Button, width, height);
        }
    }
}

#[cfg(feature = "inspector-presets")]
#[test]
fn shortcut_search_accepts_the_actual_keypoint_button_name() {
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 1000.0))
        .build_eframe(|cc| crate::inspector_presets::build(
            crate::inspector_presets::InspectorPreset::MigrationObject, &cc.egui_ctx));
    let name = harness.state().selected_task().unwrap().skeleton.as_ref().unwrap().keypoints[0].name.clone();
    harness.state_mut().open_shortcut_settings();
    harness.state_mut().work.shortcut_settings.search = format!("Mark {name} as not present");
    harness.step();
    assert!(harness.query_by_label_contains("Record shortcut for Not present:").is_some());
}

#[test]
fn shortcut_close_button_stays_visible_and_protects_unsaved_changes() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    for size in [egui::vec2(1440.0, 1000.0), egui::vec2(390.0, 844.0), egui::vec2(320.0, 568.0), egui::vec2(320.0, 320.0)] {
        harness.state_mut().open_shortcut_settings();
        harness.set_size(size);
        harness.run_steps(3);
        let close = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Close keyboard shortcuts").rect();
        let title = harness.query_all_by_value("Keyboard shortcuts")
            .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Label).unwrap().rect();
        assert!(close.width() >= 44.0 && close.height() >= 44.0);
        assert!(close.right() <= size.x && close.bottom() <= size.y);
        assert!(title.right() <= close.left());
        click_accesskit_button(&mut harness, "Close keyboard shortcuts");
        assert!(!harness.state().work.show_settings);
    }
    harness.state_mut().open_shortcut_settings();
    harness.state_mut().work.shortcut_settings.draft.as_mut().unwrap().bindings.insert(
        labello_domain::UserAction::NextImage, labello_domain::KeyChord::new("F9"));
    harness.step();
    click_accesskit_button(&mut harness, "Close keyboard shortcuts");
    assert!(harness.state().work.shortcut_settings.confirm_discard);
    harness.step();
    click_accesskit_button(&mut harness, "Keep editing");
    assert!(harness.state().work.show_settings);
    assert_eq!(harness.state().work.shortcut_settings.draft.as_ref().unwrap().bindings[&labello_domain::UserAction::NextImage].key, "F9");
    harness.state_mut().loading.keybindings = true;
    harness.step();
    assert!(harness.get_by_role_and_label(egui::accesskit::Role::Button, "Close keyboard shortcuts").accesskit_node().is_disabled());
}

#[test]
fn glossary_prelabel_names_are_searchable_and_accessible_at_every_viewport() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().open_shortcut_settings();
    for (width, height) in viewport_sizes() {
        harness.set_size(egui::vec2(width, height));
        for name in ["Confirm selected prelabel", "Delete selected prelabel"] {
            harness.state_mut().work.shortcut_settings.search = name.into();
            harness.run_steps(3);
            let accessible_name = format!("Record shortcut for {name}:");
            let control = harness.get_by_label_contains(&accessible_name);
            assert!(!control.accesskit_node().is_disabled());
            assert!(control.rect().height() >= 44.0);
            assert!(harness.query_by_label_contains("selected model object").is_none());
            assert_visible_controls_clamped(&harness, width, height);
        }
    }
}
