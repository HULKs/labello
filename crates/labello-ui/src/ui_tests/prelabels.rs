use super::*;
use crate::prelabel_flow::{HintStatus, PrelabelAction, PrelabelReply};
use labello_domain::{PrelabelGeneration, PrelabelResponse};

#[test]
fn prelabel_choice_defaults_to_available_model_and_explicit_none_survives_preferences() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    let task = app.selected_task().unwrap().task_id.clone();
    assert_eq!(app.prelabel_choice(&task), Some("demo-prelabel".into()));
    let key = format!("{}/{task}", app.config.dataset_id);
    app.work.prelabels.choices.insert(key.clone(), None);
    assert_eq!(app.prelabel_choice(&task), None);
    app.persist_workspace_preference();
    let preference = app.runtime.persistence.preference.as_ref().unwrap();
    let decoded: WorkspacePreference =
        serde_json::from_slice(&serde_json::to_vec(preference).unwrap()).unwrap();
    assert_eq!(decoded.prelabel_choices.get(&key), Some(&None));
    app.work
        .prelabels
        .choices
        .insert(key, Some("removed-model".into()));
    assert_eq!(app.prelabel_choice(&task), None);
}

#[test]
fn prelabel_filter_reacts_to_draft_creation_edit_deletion_and_confidence() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
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
    step_until(&mut harness, 20, |app| !app.visible_prelabels().is_empty());
    let app = harness.state_mut();
    app.cancel_prelabel_load();
    let hint = app.visible_prelabels().remove(0);
    app.accept_prelabel(&hint);
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
        "Accept",
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
