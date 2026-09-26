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
fn prelabel_cards_keep_long_classes_and_actions_inside_the_inspector() {
    let mut loaded = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
    let mut app = std::mem::replace(loaded.state_mut(), base_live_app(Rc::new(SpyApi::new())));
    app.cancel_prelabel_load();
    // Isolate rendering from background work while exercising the production panel.
    app.runtime.api = None;
    let class_id =
        "a_very_long_dataset_class_id_that_must_not_push_confidence_or_actions_outside_the_card";
    let task_id = app.selected_task().unwrap().task_id.clone();
    app.work
        .tasks
        .iter_mut()
        .find(|task| task.task_id == task_id)
        .unwrap()
        .class_ids = vec![class_id.into()];
    let hint = &mut app.work.current.as_mut().unwrap().prelabels[0];
    hint.class_id = class_id.into();
    hint.confidence = 0.87;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(300.0, 1000.0))
        .build_ui_state(|ui, app: &mut LabelloApp| app.right_panel(ui, false), app);
    for width in [260.0, 280.0, 320.0, 390.0] {
        harness.set_size(egui::vec2(width, 1000.0));
        harness.run_steps(3);
        let class = harness.get_by_label(class_id).rect();
        let confidence = harness.get_by_label("87%").rect();
        let approve = harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Approve")
            .rect();
        let discard = harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Discard")
            .rect();
        assert!(class.right() < confidence.left());
        assert!((class.center().y - confidence.center().y).abs() < 1.0);
        assert!((confidence.right() - discard.right()).abs() < 1.0);
        assert!((approve.width() - discard.width()).abs() < 1.0);
        assert!((approve.top() - discard.top()).abs() < 1.0);
        assert!(approve.top() >= class.bottom());
        assert!(approve.top() - class.bottom() <= 12.0);
        assert!(discard.bottom() - class.top() <= 100.0);
        assert!(approve.height() >= 44.0 && discard.height() >= 44.0);
        assert!(discard.right() <= width);
        fn truncated(shape: &egui::Shape, text: &str) -> bool {
            match shape {
                egui::Shape::Text(shape) => shape.galley.job.text == text && shape.galley.elided,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| truncated(shape, text)),
                _ => false,
            }
        }
        assert!(
            harness
                .output()
                .shapes
                .iter()
                .any(|shape| truncated(&shape.shape, class_id))
        );
    }
    harness.state_mut().loading.saving = true;
    harness.step();
    for label in ["Approve", "Discard"] {
        assert!(harness.get_by_label(label).accesskit_node().is_disabled());
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
fn prelabel_actions_support_keyboard_approval_and_discard() {
    for approve in [true, false] {
        let mut harness = loaded_prelabel_work_harness(Rc::new(SpyApi::new()));
        harness.get_by_label("Approve").focus();
        harness.step();
        if !approve {
            harness.key_press(egui::Key::Tab);
            harness.step();
            assert!(harness.get_by_label("Discard").is_focused());
        }
        harness.key_press(egui::Key::Enter);
        harness.step();
        assert!(harness.state().visible_prelabels().is_empty());
        assert_eq!(harness.state().work.annotations.len(), usize::from(approve));
    }
}
