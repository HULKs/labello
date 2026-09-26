use super::*;
use crate::prelabel_flow::{HintStatus, PrelabelAction, PrelabelReply};
use labello_domain::{PrelabelGeneration, PrelabelResponse};

#[test]
fn model_check_replies_cannot_replace_a_changed_filename_or_removed_configuration() {
    let api = Rc::new(SpyApi::new());
    let metadata = api.metadata();
    let mut app = base_live_app(api);
    app.datasets.admin_config = Some(metadata);
    app.auth.prelabel_available = true;
    let config = app.datasets.admin_config.as_ref().unwrap().prelabel_configs[0].clone();
    app.request_prelabels(PrelabelAction::InspectModel {
        config_id: config.config_id.clone(),
        location: config.model.location.clone(),
    });
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.datasets.admin_config.as_mut().unwrap().prelabel_configs[0]
        .model
        .location = "different.onnx".into();
    app.runtime
        .tx
        .send(UiMessage::PrelabelFinished {
            request,
            result: Box::new(Err("old file failed".into())),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    let check = &app.admin.prelabels.model_checks[&config.config_id];
    assert!(check.pending.is_none());
    assert!(check.result.is_none());
    assert_eq!(
        app.datasets.admin_config.as_ref().unwrap().prelabel_configs[0]
            .model
            .location,
        "different.onnx"
    );
}

#[test]
fn prelabels_wait_for_explicit_selection_and_stop_when_disabled() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    let task = harness.state().selected_task().unwrap().task_id.clone();
    assert_eq!(harness.state().prelabel_choice(&task), None);
    assert!(harness.state().visible_prelabels().is_empty());
    assert!(
        harness
            .query_all_by_role(egui::accesskit::Role::ComboBox)
            .any(|node| node.accesskit_node().value().as_deref() == Some("No prelabels"))
    );
    assert!(harness.query_by_label("Prelabels turned off").is_some());
    assert_eq!(api.counts().prelabel_suggestions, 0);
    assert_eq!(api.counts().prelabel_generation, 0);

    choose_prelabels(&mut harness, "No prelabels", "Demo prelabels");
    step_until(&mut harness, 20, |app| !app.visible_prelabels().is_empty());
    assert_eq!(
        harness.state().prelabel_choice(&task),
        Some("demo-prelabel".into())
    );
    assert!(api.counts().prelabel_suggestions > 0);

    choose_prelabels(&mut harness, "Demo prelabels", "No prelabels");
    assert_eq!(harness.state().prelabel_choice(&task), None);
    assert!(harness.state().visible_prelabels().is_empty());
    assert!(harness.state().work.prelabels.pending.is_none());
    let before = api.counts();
    for _ in 0..8 {
        harness.step();
    }
    assert_eq!(
        api.counts().prelabel_suggestions,
        before.prelabel_suggestions
    );
    assert_eq!(api.counts().prelabel_generation, before.prelabel_generation);
}

#[test]
fn explicit_prelabel_choices_survive_preferences_and_unavailable_models_become_none() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    let task = app.selected_task().unwrap().task_id.clone();
    let key = format!("{}/{task}", app.config.dataset_id);
    for choice in [Some("demo-prelabel".into()), None] {
        app.work
            .prelabels
            .choices
            .insert(key.clone(), choice.clone());
        assert_eq!(app.prelabel_choice(&task), choice);
        app.persist_workspace_preference();
        let preference = app.runtime.persistence.preference.as_ref().unwrap();
        let decoded: WorkspacePreference =
            serde_json::from_slice(&serde_json::to_vec(preference).unwrap()).unwrap();
        assert_eq!(decoded.prelabel_choices.get(&key), Some(&choice));
    }
    app.work
        .prelabels
        .choices
        .insert(key, Some("removed-model".into()));
    assert_eq!(app.prelabel_choice(&task), None);
}

#[test]
fn prelabel_filter_reacts_to_draft_creation_edit_deletion_and_confidence() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    choose_prelabels(&mut harness, "No prelabels", "Demo prelabels");
    step_until(&mut harness, 20, |app| !app.visible_prelabels().is_empty());
    let app = harness.state_mut();
    let hint = app.visible_prelabels().remove(0);
    let mut box_annotation = labello_domain::AnnotationVersion::native(
        "draft".into(),
        hint.task_id.clone(),
        hint.class_id.clone(),
        AnnotationType::BoundingBox,
        hint.geometry.clone(),
        "admin".into(),
        labello_domain::now(),
    );
    app.work.annotations.push(box_annotation.clone());
    assert!(app.visible_prelabels().is_empty());
    box_annotation.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.7,
        y: 0.7,
        width: 0.1,
        height: 0.1,
    });
    *app.work.annotations.last_mut().unwrap() = box_annotation.clone();
    assert_eq!(app.visible_prelabels().len(), 1);
    box_annotation.geometry = hint.geometry;
    box_annotation.deleted = true;
    *app.work.annotations.last_mut().unwrap() = box_annotation;
    assert_eq!(app.visible_prelabels().len(), 1);
    app.datasets.metadata.as_mut().unwrap().prelabel_configs[0]
        .output_processing
        .confidence_threshold = 0.99;
    assert!(app.visible_prelabels().is_empty());
}

#[test]
fn prelabel_generation_reset_clears_hints_and_preserves_edits_and_rejects_stale_response() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    choose_prelabels(&mut harness, "No prelabels", "Demo prelabels");
    step_until(&mut harness, 20, |app| !app.visible_prelabels().is_empty());
    let app = harness.state_mut();
    app.cancel_prelabel_load();
    let hint = app.visible_prelabels().remove(0);
    app.sync_prelabel_review();
    assert!(app.confirm_prelabel_object());
    let annotations = app.work.annotations.clone();
    let query = PrelabelSuggestionRequest {
        image_id: app.work.current.as_ref().unwrap().image.image_id.clone(),
        task_id: hint.task_id.clone(),
        config_id: hint.config_id.clone(),
    };
    let key = (
        query.image_id.clone(),
        query.task_id.clone(),
        query.config_id.clone(),
    );
    let generation = PrelabelGeneration {
        generation: 0,
        scope_generation: 0,
        paused: false,
    };
    app.work.prelabels.hints.insert(
        key.clone(),
        HintStatus {
            execution: None,
            generation: Some(generation.clone()),
            error: None,
            from_batch: false,
            checked_at: Instant::now(),
        },
    );
    app.request_prelabels(PrelabelAction::Check(query.clone()));
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.runtime
        .tx
        .send(UiMessage::PrelabelFinished {
            request: request.clone(),
            result: Box::new(Ok(PrelabelReply::Generation(PrelabelGeneration {
                generation: 1,
                scope_generation: 1,
                paused: true,
            }))),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.work.current.as_ref().unwrap().prelabels.is_empty());
    assert!(
        app.work.prelabels.hints[&key]
            .generation
            .as_ref()
            .unwrap()
            .paused
    );
    assert_eq!(app.work.annotations, annotations);
    app.runtime
        .tx
        .send(UiMessage::PrelabelFinished {
            request,
            result: Box::new(Ok(PrelabelReply::Hints(Box::new(PrelabelResponse {
                execution: None,
                generation,
                suggestions: vec![hint],
                from_batch: true,
                browser_grant: None,
            })))),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.work.current.as_ref().unwrap().prelabels.is_empty());
    assert_eq!(app.work.annotations, annotations);
}

#[test]
fn prelabel_admin_removal_requires_confirmation_and_controls_fit_narrow_and_short_views() {
    let api = Rc::new(SpyApi::new());
    let metadata = api.metadata();
    let mut app = base_live_app(api);
    app.auth.prelabel_available = true;
    app.datasets.admin_baseline = Some(metadata);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 900.0))
        .build_ui_state(
            |ui, app: &mut LabelloApp| {
                egui::Frame::new().inner_margin(24).show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| app.prelabel_admin_panel(ui));
                });
            },
            app,
        );
    harness.step();
    assert!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Remove hints and pause")
            .accesskit_node()
            .is_disabled()
    );
    click(&mut harness, "Confirm hint removal");
    assert!(
        harness.state().admin.prelabels.confirm_reset,
        "confirmation did not toggle; pending: {:?}",
        harness.state().admin.prelabels.pending
    );
    step_until(&mut harness, 8, |app| {
        app.admin.prelabels.confirm_reset && app.admin.prelabels.pending.is_none()
    });
    harness.step();
    assert!(
        !harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Remove hints and pause")
            .accesskit_node()
            .is_disabled()
    );
    for (width, height) in [
        (320.0, 568.0),
        (390.0, 667.0),
        (1024.0, 600.0),
        (1440.0, 900.0),
    ] {
        harness.set_size(egui::vec2(width, height));
        harness.step();
        assert_visible_controls_clamped(&harness, width, height);
    }
}

#[test]
fn prelabel_loading_and_failure_do_not_claim_successful_empty_predictions() {
    let mut loaded = loaded_work_harness(Rc::new(SpyApi::new()));
    choose_prelabels(&mut loaded, "No prelabels", "Demo prelabels");
    let mut app = std::mem::replace(loaded.state_mut(), base_live_app(Rc::new(SpyApi::new())));
    app.cancel_prelabel_load();
    app.work.prelabels.hints.clear();
    app.work.current.as_mut().unwrap().prelabels.clear();
    let task = app.selected_task().unwrap().task_id.clone();
    let key = (
        app.work.current.as_ref().unwrap().image.image_id.clone(),
        task.clone(),
        app.prelabel_choice(&task).unwrap(),
    );
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 1000.0))
        .build_ui_state(|ui, app: &mut LabelloApp| app.right_panel(ui, false), app);
    harness.step();
    assert!(
        harness
            .query_by_label("Preparing hints… You can annotate while they load.")
            .is_some()
    );
    assert!(harness.query_by_label("No remaining suggestions").is_none());
    harness.state_mut().work.prelabels.hints.insert(
        key.clone(),
        HintStatus {
            execution: None,
            generation: None,
            error: Some("Inference failed; manual annotation is available".into()),
            from_batch: false,
            checked_at: Instant::now(),
        },
    );
    harness.step();
    assert!(
        harness
            .query_by_label("Inference failed; manual annotation is available")
            .is_some()
    );
    assert!(harness.query_by_label("No remaining suggestions").is_none());
    let status = harness
        .state_mut()
        .work
        .prelabels
        .hints
        .get_mut(&key)
        .unwrap();
    status.error = None;
    status.generation = Some(PrelabelGeneration {
        generation: 0,
        scope_generation: 0,
        paused: false,
    });
    harness.step();
    assert!(harness.query_by_label("No remaining suggestions").is_some());
}

#[test]
fn prelabel_admin_start_queues_the_selected_preflight_run() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api.clone());
    app.auth.prelabel_available = true;
    app.datasets.admin_baseline = Some(api.metadata());
    app.admin.prelabels.state = Some(labello_domain::PrelabelAdminState {
        runs: vec![labello_domain::PrelabelRunSummary {
            run_id: "prepared-run".into(),
            phase: labello_domain::PrelabelRunPhase::Ready,
            created_at: labello_domain::now(),
            updated_at: labello_domain::now(),
            total: 2,
            pending: 2,
            reusable: 0,
            ineligible: 0,
            generated: 0,
            empty: 0,
            skipped: 0,
            failed: 0,
            blockers: vec![],
        }],
        ..Default::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 1100.0))
        .build_ui_state(|ui, app: &mut LabelloApp| app.prelabel_admin_panel(ui), app);
    harness.step();
    click(&mut harness, "Start generation");
    assert!(
        matches!(&harness.state().admin.prelabels.pending, Some((_, PrelabelAction::Admin(Some(labello_domain::PrelabelAdminCommand::Start { run_id })))) if run_id == "prepared-run")
    );
}

#[test]
fn disabled_server_hides_annotation_hints_and_never_requests_them() {
    let api = Rc::new(SpyApi::new());
    api.state.borrow_mut().prelabel_available = false;
    let mut harness = loaded_work_harness(api.clone());
    assert!(!harness.state().auth.prelabel_available);
    assert!(harness.state().visible_prelabels().is_empty());
    assert!(
        harness
            .query_by_label("Prelabeling is disabled by server configuration.")
            .is_some()
    );
    for label in [
        "No prelabels",
        "Refresh hints",
        "Preparing hints… You can annotate while they load.",
        "Approve",
        "Discard",
    ] {
        assert!(harness.query_by_label(label).is_none(), "{label}");
    }
    let app = harness.state_mut();
    // Retained candidates must remain hidden even if a previous session loaded them.
    app.work
        .current
        .as_mut()
        .unwrap()
        .prelabels
        .push(labello_domain::PrelabelSuggestion {
            suggestion_id: "retained".into(),
            config_id: "demo-prelabel".into(),
            task_id: app.work.selected_task_id.clone().unwrap(),
            class_id: "person".into(),
            confidence: 0.9,
            geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.1,
                y: 0.1,
                width: 0.2,
                height: 0.2,
            }),
            evidence: None,
        });
    assert!(app.visible_prelabels().is_empty());
    app.request_prelabels(PrelabelAction::Load(PrelabelSuggestionRequest {
        image_id: app.work.current.as_ref().unwrap().image.image_id.clone(),
        task_id: app.work.selected_task_id.clone().unwrap(),
        config_id: "demo-prelabel".into(),
    }));
    app.request_prelabels(PrelabelAction::Admin(None));
    assert!(app.work.prelabels.pending.is_none());
    assert!(app.admin.prelabels.pending.is_none());
    for _ in 0..8 {
        harness.step();
    }
    let counts = &api.state.borrow().counts;
    assert_eq!(counts.prelabel_suggestions, 0);
    assert_eq!(counts.prelabel_generation, 0);
    assert_eq!(counts.prelabel_admin, 0);
    assert!(
        counts.assign_next_image > 0,
        "manual annotation still loads work"
    );
}

#[test]
fn disabled_server_hides_admin_hint_controls_and_preserves_model_configuration() {
    let api = Rc::new(SpyApi::new());
    api.state.borrow_mut().prelabel_available = false;
    let mut harness = loaded_admin_harness(api.clone());
    let before = harness.state().datasets.admin_config.clone();
    harness.state_mut().admin.section = AdminSection::Automation;
    for _ in 0..8 {
        harness.step();
    }
    assert!(
        harness
            .query_by_label("Prelabeling is disabled by server configuration.")
            .is_some()
    );
    for label in [
        "Add browser prelabel config",
        "Dataset hints",
        "Check remaining workflows",
        "Remove hints and pause",
        "Confirm hint removal",
    ] {
        assert!(harness.query_by_label(label).is_none(), "{label}");
    }
    assert!(harness.query_by_label("Assignment Balance").is_some());
    assert_eq!(harness.state().datasets.admin_config, before);
    assert!(!harness.state().admin_changes_dirty());
    assert!(harness.state().admin.prelabels.error.is_none());
    assert_eq!(api.state.borrow().counts.prelabel_admin, 0);
    harness.set_size(egui::vec2(1300.0, 8000.0));
    harness.state_mut().admin.section = AdminSection::Schema;
    harness.step();
    click_accesskit_button(
        &mut harness,
        "Person boxes | bounding_box | Person | Enabled",
    );
    assert!(harness.query_by_label("Prelabel sources").is_none());
    assert!(
        harness
            .query_by_label("Prelabeling is disabled by server configuration.")
            .is_some()
    );
    assert_eq!(harness.state().datasets.admin_config, before);
    // A fresh session obtains the updated capability after the operator enables it.
    api.state.borrow_mut().prelabel_available = true;
    harness.state_mut().request_session();
    step_until(&mut harness, 8, |app| app.auth.prelabel_available);
}

#[test]
fn pending_object_summaries_truncate_long_names_and_keep_confidence_visible() {
    let mut loaded = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    let mut app = std::mem::replace(loaded.state_mut(), base_live_app(Rc::new(SpyApi::new())));
    app.cancel_prelabel_load();
    app.runtime.api = None;
    let class = app
        .work
        .classes
        .iter_mut()
        .find(|class| class.class_id == app.work.current.as_ref().unwrap().prelabels[0].class_id)
        .unwrap();
    class.name =
        "a very long dataset class name that must not push confidence outside the object card"
            .into();
    let label = format!("Object 1 | {} | Selected", class.name);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(300.0, 1000.0))
        .build_ui_state(|ui, app: &mut LabelloApp| app.right_panel(ui, false), app);
    for width in [260.0, 280.0, 320.0, 390.0] {
        harness.set_size(egui::vec2(width, 1000.0));
        harness.run_steps(3);
        let object = harness.get_by_label(&label).rect();
        let confidence = harness.get_by_label("88%").rect();
        assert!(object.right() <= width && confidence.right() <= width);
        assert!(object.height() >= 44.0);
        assert!(harness.query_by_label("Needs confirmation").is_some());
        assert!(harness.query_by_label("Approve").is_none());
        assert!(harness.query_by_label("Discard").is_none());
    }
}

#[test]
fn prelabel_refresh_is_inline_named_and_disabled_until_hints_are_ready() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    let refresh = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Refresh hints");
    assert!(refresh.accesskit_node().is_disabled());
    let selector = harness
        .query_all_by_role(egui::accesskit::Role::ComboBox)
        .find(|node| node.accesskit_node().value().as_deref() == Some("No prelabels"))
        .unwrap();
    assert!((refresh.rect().center().y - selector.rect().center().y).abs() < 1.0);
    assert!(refresh.rect().left() > selector.rect().right());
    assert!(refresh.rect().width() >= 44.0 && refresh.rect().height() >= 44.0);
    choose_prelabels(&mut harness, "No prelabels", "Demo prelabels");
    step_until(&mut harness, 20, |app| {
        !app.visible_prelabels().is_empty() && app.work.prelabels.pending.is_none()
    });
    let mut app = std::mem::replace(harness.state_mut(), base_live_app(Rc::new(SpyApi::new())));
    let key = app.work.prelabels.hints.keys().next().unwrap().clone();
    app.work.prelabels.hints.get_mut(&key).unwrap().error = Some("Inference failed".into());
    app.datasets.metadata.as_mut().unwrap().prelabel_configs[0].name =
        "A very long model name that must leave room for the refresh icon".into();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 1000.0))
        .build_ui_state(|ui, app: &mut LabelloApp| app.right_panel(ui, false), app);
    harness.run_steps(3);
    for width in [260.0, 280.0, 320.0] {
        harness.set_size(egui::vec2(width, 1000.0));
        harness.run_steps(3);
        let refresh = harness.get_by_label("Refresh hints").rect();
        let selector = harness
            .query_all_by_role(egui::accesskit::Role::ComboBox)
            .find(|node| {
                node.accesskit_node()
                    .value()
                    .is_some_and(|value| value.starts_with("A very long model"))
            })
            .unwrap()
            .rect();
        assert!(refresh.right() <= width);
        assert!(selector.right() < refresh.left());
        assert!((selector.center().y - refresh.center().y).abs() < 1.0);
    }
    assert!(
        !harness
            .get_by_label("Refresh hints")
            .accesskit_node()
            .is_disabled()
    );
    harness
        .state_mut()
        .request_prelabels(PrelabelAction::Check(PrelabelSuggestionRequest {
            image_id: key.0.clone(),
            task_id: key.1.clone(),
            config_id: key.2.clone(),
        }));
    harness.step();
    assert!(
        harness
            .get_by_label("Refresh hints")
            .accesskit_node()
            .is_disabled()
    );
    harness.state_mut().cancel_prelabel_load();
    harness.step();
    harness.get_by_label("Refresh hints").focus();
    harness.step();
    harness.key_press(egui::Key::Enter);
    harness.step();
    assert!(!harness.state().work.prelabels.hints.contains_key(&key));
    harness.step();
    assert!(
        harness
            .get_by_label("Refresh hints")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn prelabel_actions_support_keyboard_confirmation_and_delete() {
    for confirm in [true, false] {
        let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
        harness
            .get_by_label(if confirm { "Confirm & next" } else { "Delete" })
            .focus();
        harness.step();
        harness.key_press(egui::Key::Enter);
        harness.step();
        assert!(harness.state().visible_prelabels().is_empty());
        assert_eq!(harness.state().work.annotations.len(), usize::from(confirm));
        assert!(harness.query_by_label("Submit & next").is_some());
    }
}

#[test]
fn prelabels_open_as_selected_objects_without_accepting_them() {
    let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    harness.run_steps(3);
    assert!(harness.state().work.selected_annotation.is_some());
    assert!(harness.state().work.annotations.is_empty());
    assert!(harness.query_by_label("Confirm & next").is_some());
    assert!(harness.query_by_label("Delete").is_some());
    assert!(harness.query_by_label("Approve").is_none());
    assert!(harness.query_by_label("Discard").is_none());
}

fn add_second_prelabel(app: &mut LabelloApp) {
    let mut hint = app.work.current.as_ref().unwrap().prelabels[0].clone();
    hint.suggestion_id = "second-object".into();
    hint.confidence = 0.8;
    hint.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.7,
        y: 0.6,
        width: 0.15,
        height: 0.2,
    });
    app.work.current.as_mut().unwrap().prelabels.push(hint);
    app.sync_prelabel_review();
}

#[test]
fn model_objects_are_editable_before_confirmation_and_advance_without_submitting() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_prelabel_work_harness(api.clone());
    add_second_prelabel(harness.state_mut());
    harness.state_mut().work.inspector_panel_collapsed = true;
    harness.run_steps(3);
    let first = harness.state().selected_prelabel_object().unwrap().clone();
    let zoom = harness.state().work.canvas.current_zoom();
    assert!(zoom > 1.0);
    let edited = BoundingBox {
        x: 0.15,
        y: 0.2,
        width: 0.2,
        height: 0.25,
    };
    harness.state_mut().edit_bbox(BoundingBoxEdit {
        annotation_id: first.annotation.annotation_id.clone(),
        bounding_box: edited,
    });
    harness.step();
    assert_eq!(
        harness.state().work.canvas.current_zoom(),
        zoom,
        "editing must not refocus each frame"
    );
    harness.state_mut().request_save(false);
    step_until(&mut harness, 12, |app| !app.loading.saving);
    assert!(
        api.events().is_empty(),
        "autosave must not accept either pending object"
    );
    assert_eq!(
        harness
            .state()
            .selected_prelabel_object()
            .unwrap()
            .annotation
            .geometry,
        AnnotationGeometry::BoundingBox(edited)
    );
    harness.key_press(egui::Key::Space);
    harness.run_steps(3);
    assert_eq!(harness.state().work.annotations.len(), 1);
    assert_eq!(
        harness.state().work.annotations[0].geometry,
        AnnotationGeometry::BoundingBox(edited)
    );
    assert_eq!(
        harness
            .state()
            .selected_prelabel_object()
            .unwrap()
            .suggestion
            .suggestion_id,
        "second-object"
    );
    assert_eq!(api.counts().complete_assignment, 0);
    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    assert!(harness.state().pending_prelabel_objects().is_empty());
    assert_eq!(harness.state().work.annotations.len(), 1);
    assert_eq!(harness.state().work.canvas.current_zoom(), 1.0);
    assert!(harness.query_by_label("Image overview").is_some());
    assert!(harness.query_by_label("Submit & next").is_some());
    assert_eq!(api.counts().complete_assignment, 0);
    harness.key_press(egui::Key::Space);
    step_until(&mut harness, 12, |_| api.counts().complete_assignment == 1);
    assert_eq!(
        api.events()
            .iter()
            .filter(|event| matches!(event, EventPayload::AnnotationVersionCreated { .. }))
            .count(),
        1
    );
}

#[test]
fn pending_object_deletion_confirmation_and_edits_share_undo_history() {
    let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    add_second_prelabel(harness.state_mut());
    let first = harness.state().selected_prelabel_object().unwrap().clone();
    click(&mut harness, "Delete");
    assert_eq!(harness.state().pending_prelabel_objects().len(), 1);
    harness.state_mut().undo();
    harness.step();
    assert_eq!(harness.state().pending_prelabel_objects().len(), 2);
    assert_eq!(
        harness.state().work.selected_annotation.as_ref(),
        Some(&first.annotation.annotation_id)
    );
    harness.state_mut().redo();
    harness.step();
    assert_eq!(harness.state().pending_prelabel_objects().len(), 1);
    click(&mut harness, "Confirm & next");
    assert_eq!(harness.state().work.annotations.len(), 1);
    harness.state_mut().undo();
    harness.step();
    assert!(harness.state().work.annotations.is_empty());
    assert_eq!(harness.state().pending_prelabel_objects().len(), 1);
    assert!(harness.state().selected_prelabel_object().is_some());
}

#[test]
fn pending_edits_recover_separately_from_annotations_after_autosave() {
    let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    let id = harness.state().work.selected_annotation.clone().unwrap();
    let edited = BoundingBox {
        x: 0.2,
        y: 0.15,
        width: 0.2,
        height: 0.25,
    };
    harness.state_mut().edit_bbox(BoundingBoxEdit {
        annotation_id: id.clone(),
        bounding_box: edited,
    });
    harness.state_mut().request_save(false);
    step_until(&mut harness, 12, |app| !app.loading.saving);
    harness.state_mut().queue_current_drafts();
    harness.run_steps(10);
    let before = harness.state().work.prelabel_review.clone();
    let decoded: crate::prelabel_review::PrelabelReview =
        serde_json::from_slice(&serde_json::to_vec(&before).unwrap()).unwrap();
    assert_eq!(decoded, before);
    harness.state_mut().work.prelabel_review = Default::default();
    harness.state_mut().work.selected_annotation = None;
    harness.state_mut().runtime.notice = None;
    harness.state_mut().request_work_draft_load();
    step_until(&mut harness, 12, |app| {
        app.runtime.notice.as_deref() == Some("Recovered the validated browser draft.")
    });
    assert!(harness.state().work.annotations.is_empty());
    assert_eq!(
        harness
            .state()
            .selected_prelabel_object()
            .unwrap()
            .annotation
            .geometry,
        AnnotationGeometry::BoundingBox(edited)
    );
    assert_eq!(harness.state().work.selected_annotation, Some(id));
}

#[test]
fn pending_objects_block_completion_and_retain_edits_when_model_is_disabled() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_prelabel_work_harness(api.clone());
    let id = harness.state().work.selected_annotation.clone().unwrap();
    harness.state_mut().edit_bbox(BoundingBoxEdit {
        annotation_id: id.clone(),
        bounding_box: BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.2,
            height: 0.3,
        },
    });
    harness.state_mut().request_save(true);
    assert!(!harness.state().loading.saving);
    assert_eq!(api.counts().complete_assignment, 0);
    choose_prelabels(&mut harness, "Demo prelabels", "No prelabels");
    assert!(harness.state().pending_prelabel_objects().is_empty());
    assert!(harness.state().work.annotations.is_empty());
    assert!(harness.state().work.selected_annotation.is_none());
    choose_prelabels(&mut harness, "No prelabels", "Demo prelabels");
    step_until(&mut harness, 20, |app| {
        app.selected_prelabel_object().is_some()
    });
    assert_eq!(harness.state().work.selected_annotation, Some(id));
    assert!(
        matches!(harness.state().selected_prelabel_object().unwrap().annotation.geometry,
        AnnotationGeometry::BoundingBox(ref bbox) if bbox.x == 0.2)
    );
}

#[test]
fn confirm_and_delete_are_visible_and_guarded_at_every_workspace_size() {
    let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().work.inspector_panel_collapsed = true;
    for (width, height) in [
        (320.0, 320.0),
        (320.0, 568.0),
        (390.0, 844.0),
        (600.0, 800.0),
        (1288.0, 820.0),
        (1440.0, 1000.0),
    ] {
        harness.set_size(egui::vec2(width, height));
        harness.run_steps(4);
        for label in ["Confirm & next", "Delete"] {
            assert_control_inside(
                &harness,
                label,
                egui::accesskit::Role::Button,
                width,
                height,
            );
        }
        assert!(harness.get_by_label("Annotation canvas").rect().height() >= 44.0);
    }
    harness.state_mut().loading.saving = true;
    harness.step();
    for label in ["Confirm & next", "Delete"] {
        assert!(harness.get_by_label(label).accesskit_node().is_disabled());
    }
    let pending = harness.state().pending_prelabel_objects().len();
    harness.key_press(egui::Key::Delete);
    harness.key_press(egui::Key::Space);
    harness.step();
    assert_eq!(harness.state().pending_prelabel_objects().len(), pending);
}

#[test]
fn edited_model_object_keeps_original_signed_prediction_for_confirmation() {
    let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    let hint = app.work.current.as_ref().unwrap().prelabels[0].clone();
    let proof = Box::new(labello_domain::PrelabelEvidence {
        provenance: labello_domain::PrelabelProvenance {
            dataset_id: app.config.dataset_id.clone(),
            image_id: app.work.current.as_ref().unwrap().image.image_id.clone(),
            image_hash: "image-digest".into(),
            task_id: hint.task_id.clone(),
            class_id: hint.class_id.clone(),
            config_id: hint.config_id.clone(),
            config_digest: "configuration-digest".into(),
            model_id: "detector".into(),
            model_version: Some("1".into()),
            model_digest: "model-digest".into(),
            execution: labello_domain::PrelabelExecutionKind::ServerCpu,
            trust: labello_domain::PredictionTrust::ServerGenerated,
            processing: OutputProcessing {
                confidence_threshold: 0.25,
                suppress_overlaps_iou: Some(0.5),
            },
            generation: 0,
            scope_generation: 0,
            suggestion_id: hint.suggestion_id,
            confidence: hint.confidence,
        },
        predicted_geometry: hint.geometry,
        signature: "fixture-signature".into(),
    });
    app.work.current.as_mut().unwrap().prelabels[0].evidence = Some(proof.clone());
    app.sync_prelabel_review();
    let id = app.work.selected_annotation.clone().unwrap();
    let edited = BoundingBox {
        x: 0.2,
        y: 0.2,
        width: 0.3,
        height: 0.4,
    };
    app.edit_bbox(BoundingBoxEdit {
        annotation_id: id.clone(),
        bounding_box: edited,
    });
    assert!(app.confirm_prelabel_object());
    app.request_save(false);
    let UiCommand::SaveAnnotations {
        annotations,
        prelabel_evidence,
        submit,
        ..
    } = app.runtime.commands.back().unwrap()
    else {
        panic!("annotation save");
    };
    assert!(!submit);
    assert_eq!(
        annotations[0].geometry,
        AnnotationGeometry::BoundingBox(edited)
    );
    assert_eq!(prelabel_evidence.get(&id), Some(&proof));
    assert_ne!(annotations[0].geometry, proof.predicted_geometry);
}

#[test]
fn confirming_again_after_saved_undo_reuses_the_annotation_identity_and_version() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_prelabel_work_harness(api.clone());
    let id = harness.state().work.selected_annotation.clone().unwrap();
    click(&mut harness, "Confirm & next");
    harness.state_mut().request_save(false);
    step_until(&mut harness, 12, |app| !app.loading.saving);
    harness.state_mut().undo();
    harness.state_mut().request_save(false);
    step_until(&mut harness, 12, |app| !app.loading.saving);
    assert!(harness.state().selected_prelabel_object().is_some());
    click(&mut harness, "Confirm & next");
    harness.state_mut().request_save(false);
    step_until(&mut harness, 12, |app| !app.loading.saving);
    let annotations = &harness.state().work.annotations;
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].annotation_id, id);
    assert_eq!(annotations[0].version, 2);
    assert!(api.events().iter().any(|event| matches!(event,
        EventPayload::AnnotationVersionCreated { annotation, previous_version: Some(1), .. }
        if annotation.annotation_id == id)));
}

#[test]
fn pending_box_edits_refilter_other_model_objects_without_destroying_their_drafts() {
    let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    add_second_prelabel(harness.state_mut());
    let app = harness.state_mut();
    app.datasets.metadata.as_mut().unwrap().prelabel_configs[0]
        .output_processing
        .suppress_overlaps_iou = Some(0.5);
    let first = app
        .selected_prelabel_object()
        .unwrap()
        .annotation
        .annotation_id
        .clone();
    let second = app.pending_prelabel_objects()[1].clone();
    let AnnotationGeometry::BoundingBox(bbox) = second.annotation.geometry else {
        panic!("box");
    };
    app.edit_bbox(BoundingBoxEdit {
        annotation_id: first,
        bounding_box: bbox,
    });
    assert_eq!(app.pending_prelabel_objects().len(), 1);
    assert_eq!(app.work.prelabel_review.objects.len(), 2);
    app.undo();
    assert_eq!(app.pending_prelabel_objects().len(), 2);
}

#[test]
fn pose_model_objects_keep_editable_keypoints_before_confirmation() {
    let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    app.cancel_prelabel_load();
    app.runtime.api = None;
    let task_id = app.work.selected_task_id.clone().unwrap();
    let task = app
        .work
        .tasks
        .iter_mut()
        .find(|task| task.task_id == task_id)
        .unwrap();
    task.annotation_type = AnnotationType::Skeleton;
    task.skeleton = Some(SkeletonSpec {
        keypoints: vec![KeypointSpec {
            name: "head".into(),
            required: true,
        }],
        edges: vec![],
        allow_hidden: true,
        allow_absent: false,
    });
    let hint = &mut app.work.current.as_mut().unwrap().prelabels[0];
    hint.suggestion_id = "pose-object".into();
    hint.geometry = AnnotationGeometry::Skeleton(SkeletonGeometry {
        keypoints: vec![KeypointAnnotation {
            name: "head".into(),
            state: KeypointState::Visible,
            point: Some(NormalizedPoint { x: 0.4, y: 0.4 }),
        }],
    });
    app.work.prelabel_review = Default::default();
    app.work.selected_annotation = None;
    app.sync_prelabel_review();
    let id = app.work.selected_annotation.clone().unwrap();
    let edited = NormalizedPoint { x: 0.6, y: 0.4 };
    app.edit_keypoint(crate::canvas::KeypointEdit {
        annotation_id: id,
        keypoint_index: 0,
        point: edited,
    });
    assert!(app.work.annotations.is_empty());
    assert!(app.confirm_prelabel_object());
    let AnnotationGeometry::Skeleton(skeleton) = &app.work.annotations[0].geometry else {
        panic!("pose object");
    };
    assert_eq!(skeleton.keypoints[0].point, Some(edited));
    assert_eq!(skeleton.keypoints[0].state, KeypointState::Visible);
}
