use super::*;
use labello_domain::{
    WorkflowAssignmentContext, WorkflowAvailability, WorkflowHistoryEntry, WorkflowItem,
    WorkflowObject, WorkflowSelection, WorkflowUnavailableReason, WorkflowVariant,
};

fn object_app() -> LabelloApp {
    let mut app = crate::inspector_presets::build(
        crate::inspector_presets::InspectorPreset::Annotation,
        &egui::Context::default(),
    );
    let assignment = app.work.assignment.clone().unwrap();
    let annotation = app
        .work
        .annotations
        .iter()
        .find(|a| a.task_id == assignment.task_id)
        .unwrap()
        .clone();
    let mut other = annotation.clone();
    other.annotation_id = "another-object".into();
    app.work.annotations.push(other.clone());
    let state = app.work.current_state.as_mut().unwrap();
    state
        .annotations
        .insert(annotation.annotation_id.clone(), vec![annotation.clone()]);
    state
        .annotations
        .insert(other.annotation_id.clone(), vec![other]);
    state.workflow_assignments.insert(
        assignment.assignment_id.clone(),
        WorkflowAssignmentContext {
            item: WorkflowItem::Object {
                object: WorkflowObject::Annotation {
                    annotation_id: annotation.annotation_id,
                },
            },
            task_fingerprint: String::new(),
            overview_fingerprint: None,
            review_target: None,
            review_exception: false,
            source_assignment_id: None,
        },
    );
    app.install_workflow_item();
    app.work.workflow_panel_collapsed = false;
    app
}

#[test]
fn focused_items_scope_both_canvas_and_save_identity_to_the_leased_object() {
    let app = object_app();
    assert_eq!(app.work.annotations.len(), 1);
    assert_eq!(app.work.persisted_annotations.len(), 1);
    assert!(
        app.work
            .current_state
            .as_ref()
            .unwrap()
            .active_annotations()
            .count()
            > 1
    );
    assert_eq!(app.work.workflow.variant, WorkflowVariant::Objects);
    assert!(app.work.selected_annotation.is_some());
}

#[test]
fn history_goes_back_and_forward_in_visit_order_without_releasing_forward_work() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    let current = app.work.assignment.clone().unwrap();
    let entry = |id: &str| WorkflowHistoryEntry {
        image_id: format!("image-{id}").into(),
        assignment_id: id.into(),
        seen_at: labello_domain::now(),
    };
    let c = WorkflowHistoryEntry {
        image_id: current.image_id.clone(),
        assignment_id: current.assignment_id.clone(),
        seen_at: labello_domain::now(),
    };
    let b = entry("b");
    let a = entry("a");
    app.work.workflow.history = vec![c.clone(), b.clone(), a.clone()];
    assert_eq!(app.previous_work_item(), Some(&b));
    app.work.workflow.current_root = Some(b.assignment_id.clone());
    assert_eq!(app.previous_work_item(), Some(&a));
    assert_eq!(app.forward_work_item(), Some(&c));
    app.work.workflow.current_root = Some(a.assignment_id.clone());
    assert!(app.previous_work_item().is_none());
    assert_eq!(app.forward_work_item(), Some(&b));
    app.request_history_item(b.clone());
    assert!(
        matches!(app.runtime.commands.back(), Some(UiCommand::ReopenWorkItem { item, .. }) if item.assignment_id == b.assignment_id)
    );
    assert_eq!(
        app.work.assignment.as_ref().unwrap().assignment_id,
        current.assignment_id
    );
    assert!(!app.runtime.reservation_cleanup.has_pending_releases());
}

#[test]
fn a_focused_existing_object_submits_directly_without_entering_overview() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    assert!(matches!(
        app.prelabel_primary_action(),
        crate::prelabel_review::PrelabelPrimaryAction::Submit
    ));
    app.submit_and_advance();
    assert!(matches!(
        app.runtime.commands.back(),
        Some(UiCommand::SaveAnnotations { submit: true, .. })
    ));
    assert_eq!(app.work.workflow.variant, WorkflowVariant::Objects);
}

#[test]
fn unavailable_forward_history_continues_with_available_work() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.resolved = true;
    app.work
        .availability
        .tasks
        .insert(app.work.selected_task_id.clone().unwrap(), true);
    app.work.workflow.returning_forward = true;
    let operation_id = 991_603;
    let request = test_request(&app, operation_id, Some(app.config.dataset_id.as_str()));
    app.runtime.active_requests.insert(operation_id);
    app.work.active_load_id = Some(operation_id);
    app.loading.image = true;
    app.runtime
        .tx
        .send(UiMessage::PreviousAssignmentLoaded {
            request,
            operation_id,
            assignment: None,
            result: Box::new(Err("item changed while history was released"
                .to_owned()
                .into())),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(!app.work.workflow.returning_forward);
    assert!(app.work.assignment.is_none());
    assert!(app.work.active_load_id.is_some_and(|id| id != operation_id));
    assert!(app.runtime.error.is_none());
}

#[test]
fn previous_shortcut_uses_item_history_without_a_legacy_image_assignment() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    let current = app.work.assignment.clone().unwrap();
    app.work.previous_assignment = None;
    app.work.workflow.history = vec![
        WorkflowHistoryEntry {
            image_id: current.image_id,
            assignment_id: current.assignment_id,
            seen_at: labello_domain::now(),
        },
        WorkflowHistoryEntry {
            image_id: "earlier-image".into(),
            assignment_id: "earlier-item".into(),
            seen_at: labello_domain::now(),
        },
    ];
    app.trigger_user_action(labello_domain::UserAction::PreviousImage);
    assert!(
        matches!(app.runtime.commands.back(), Some(UiCommand::ReopenWorkItem { item, .. }) if item.assignment_id.as_str() == "earlier-item")
    );
}

#[test]
fn a_new_display_supersedes_history_loaded_before_it() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.request_workflow_history();
    let first = app.work.workflow.history_request.unwrap();
    app.request_workflow_history();
    let second = app.work.workflow.history_request.unwrap();
    assert_ne!(first, second);
    assert!(!app.runtime.active_requests.contains(&first));
    assert_eq!(
        app.runtime
            .commands
            .iter()
            .filter(|command| matches!(command, UiCommand::WorkflowHistory { .. }))
            .count(),
        1
    );
    app.begin_workspace_epoch();
    assert!(app.work.workflow.history_request.is_none());
}

#[test]
fn skip_saves_a_partial_object_before_releasing_it() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.work.assignment_touched = true;
    app.work.save_status = SaveStatus::Dirty;
    let item = app.workflow_item_ref().unwrap();
    app.skip_assignment();
    assert_eq!(app.work.workflow.excluded, Some(item));
    assert!(matches!(
        app.runtime.commands.back(),
        Some(UiCommand::SaveAnnotations {
            draft: Some(_),
            submit: false,
            ..
        })
    ));
    assert!(
        !app.runtime
            .commands
            .iter()
            .any(|command| matches!(command, UiCommand::ReleaseAssignment { .. }))
    );
}

#[test]
fn split_buttons_keep_overview_disabled_with_a_reason_and_have_no_model_picker() {
    for review in [false, true] {
        let mut app = object_app();
        if review {
            app.view = AppView::Review;
        }
        let kind = app.assignment_kind().unwrap();
        let task_id = app.work.selected_task_id.clone().unwrap();
        let workflow = app.selected_workflow().unwrap();
        app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
        app.work.availability.kind = Some(kind.clone());
        app.work.availability.resolved = true;
        app.work.availability.workflows = [WorkflowVariant::Objects, WorkflowVariant::Overview]
            .into_iter()
            .map(|variant| WorkflowAvailability {
                selection: WorkflowSelection {
                    task_id: task_id.clone(),
                    kind: kind.clone(),
                    variant,
                },
                split: true,
                available: variant == WorkflowVariant::Objects,
                reason: (variant == WorkflowVariant::Overview)
                    .then_some(WorkflowUnavailableReason::ObjectsPending),
            })
            .collect();
        let base = app.workflow_entry_label(&workflow, None);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1500.0, 800.0))
            .build_eframe(|_| app);
        harness.run();
        let objects_label = base.clone();
        let overview_label = format!("{} · Overview", base.trim_end_matches(" · Objects"));
        let objects = harness.get_by_role_and_label(egui::accesskit::Role::Button, &objects_label);
        let overview =
            harness.get_by_role_and_label(egui::accesskit::Role::Button, &overview_label);
        assert!(!objects.accesskit_node().is_disabled());
        assert!(overview.accesskit_node().is_disabled());
        assert!(
            overview
                .accesskit_node()
                .description()
                .is_some_and(|description| description.contains("Objects"))
        );
        assert_eq!(objects.rect().width(), overview.rect().width());
        assert_eq!(objects.rect().top(), overview.rect().top());
        assert!(objects.rect().height() >= 44.0);
        assert!((overview.rect().left() - objects.rect().right() - 6.0).abs() < 0.1);
        assert!(harness.query_by_label("Refresh prelabels").is_none());
        assert!(harness.query_by_label("No prelabels").is_none());
    }
}

fn review_object_app() -> LabelloApp {
    let mut app = crate::inspector_presets::build(
        crate::inspector_presets::InspectorPreset::Review,
        &egui::Context::default(),
    );
    let assignment = app.work.assignment.clone().unwrap();
    let annotation = app
        .work
        .annotations
        .iter()
        .find(|a| a.task_id == assignment.task_id)
        .unwrap()
        .clone();
    let state = app.work.current_state.as_mut().unwrap();
    state.workflow_assignments.insert(
        assignment.assignment_id.clone(),
        WorkflowAssignmentContext {
            item: WorkflowItem::Object {
                object: WorkflowObject::Annotation {
                    annotation_id: annotation.annotation_id.clone(),
                },
            },
            task_fingerprint: String::new(),
            overview_fingerprint: None,
            review_target: Some(labello_domain::ReviewTarget::AnnotationVersion {
                annotation_id: annotation.annotation_id.clone(),
                version: annotation.version,
            }),
            review_exception: false,
            source_assignment_id: None,
        },
    );
    app.install_workflow_item();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.begin_review_correction(annotation);
    let AnnotationGeometry::BoundingBox(bbox) =
        &mut app.work.correction_draft.as_mut().unwrap().edited_geometry
    else {
        panic!()
    };
    bbox.x += 0.01;
    app
}

#[test]
fn focused_review_correction_submits_without_visiting_overview() {
    let mut app = review_object_app();
    assert!(!app.review_overview());
    assert!(app.review_can_reject());
    assert!(app.reject_review_item());
    assert!(matches!(
        app.runtime.commands.back(),
        Some(UiCommand::Correction { .. })
    ));
    assert_eq!(app.work.workflow.variant, WorkflowVariant::Objects);
}

#[test]
fn review_skip_saves_correction_draft_before_release() {
    let mut app = review_object_app();
    app.skip_assignment();
    assert!(
        matches!(app.runtime.commands.back(), Some(UiCommand::SaveAnnotations {
        draft: Some(labello_client::SaveWorkflowDraftRequest { draft: labello_client::WorkflowDraftInput::Edits { edits }, .. }), submit: false, ..
    }) if edits.changes.len() == 1)
    );
    assert!(!app.runtime.commands.iter().any(|command| matches!(
        command,
        UiCommand::ReleaseAssignment { .. } | UiCommand::Correction { .. }
    )));
}

#[test]
fn saving_review_draft_keeps_each_reason_once() {
    let mut app = review_object_app();
    app.work.correction_draft.as_mut().unwrap().reason = "Adjusted position".into();
    let expected = app.correction_reason_text();
    let request = app.current_workflow_draft().unwrap();
    let labello_client::WorkflowDraftInput::Edits { edits } = request.draft else {
        panic!()
    };
    let context = app.workflow_context().unwrap().clone();
    let assignment = app.work.assignment.as_ref().unwrap().clone();
    let state = app.work.current_state.as_mut().unwrap();
    state.workflow_edit_drafts.insert(
        assignment.assignment_id,
        labello_domain::WorkflowEditDraft {
            task_id: assignment.task_id,
            item: context.item,
            kind: assignment.kind,
            edits,
            sequence: 1,
            previous_sequence: 0,
        },
    );
    app.restore_workflow_edits();
    app.restore_workflow_edits();
    assert_eq!(app.correction_reason_text(), expected);
}

#[test]
fn switching_workflow_saves_partial_work_without_requiring_completion() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.work.assignment_touched = true;
    let task = app.work.selected_task_id.clone().unwrap();
    app.request_transition(crate::app::PendingTransition::WorkflowVariant(
        task,
        WorkflowVariant::Overview,
    ));
    assert!(matches!(
        app.runtime.commands.back(),
        Some(UiCommand::SaveAnnotations {
            submit: false,
            draft: Some(_),
            ..
        })
    ));
}

#[test]
fn compact_workflow_drawer_restores_keyboard_focus_after_escape() {
    let mut app = object_app();
    app.work.drawer = None;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|_| app);
    harness.run();
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Workflow")
        .focus();
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert_eq!(
        harness.state().work.drawer,
        Some(crate::app::Drawer::Workflow)
    );
    harness.key_press(egui::Key::Tab);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(5);
    assert!(harness.state().work.drawer.is_none());
    assert!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Workflow")
            .is_focused()
    );
}

#[test]
fn new_workflow_chooses_an_available_pass_without_changing_a_committed_pass() {
    let mut app = object_app();
    let task = app.work.selected_task_id.clone().unwrap();
    app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
    app.work.availability.kind = Some(AssignmentKind::Annotation);
    app.work.availability.resolved = true;
    app.work.availability.workflows = [WorkflowVariant::Objects, WorkflowVariant::Overview]
        .into_iter()
        .map(|variant| WorkflowAvailability {
            selection: WorkflowSelection {
                task_id: task.clone(),
                kind: AssignmentKind::Annotation,
                variant,
            },
            available: variant == WorkflowVariant::Objects,
            split: true,
            reason: (variant == WorkflowVariant::Overview)
                .then_some(WorkflowUnavailableReason::ObjectsPending),
        })
        .collect();
    app.work.workflow.variant = WorkflowVariant::Overview;
    app.leave_current_workflow();
    assert!(!app.work.workflow.variant_selected);
    app.select_initial_workflow_variant(&task);
    assert_eq!(app.work.workflow.variant, WorkflowVariant::Objects);
    assert!(app.work.workflow.variant_selected);
    for entry in &mut app.work.availability.workflows {
        entry.available = !entry.available;
    }
    // Only entering a workflow chooses a default; finishing an object leaves Objects selected.
    assert_eq!(app.work.workflow.variant, WorkflowVariant::Objects);
    app.select_initial_workflow_variant(&task);
    assert_eq!(app.work.workflow.variant, WorkflowVariant::Overview);
}

#[test]
fn long_workflow_change_notice_stays_inside_short_canvas_with_larger_text() {
    let mut app = object_app();
    app.work.workflow.change_notice = Some("Other workflows need to catch up. Workflow changed from a previous workflow with a deliberately long class name to another workflow with a deliberately long skeleton name.".into());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|_| app);
    harness.ctx.global_style_mut(|style| {
        for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style
                .text_styles
                .insert(text_style, egui::FontId::proportional(24.0));
        }
    });
    harness.run();
    let canvas = harness.get_by_label("Annotation canvas").rect();
    let notice = harness.get_by_label("Workflow change notice").rect();
    assert!(
        canvas.expand(1.0).contains_rect(notice),
        "canvas={canvas:?}, notice={notice:?}"
    );
    let close =
        harness.get_by_role_and_label(egui::accesskit::Role::Button, "Dismiss workflow change");
    assert!(close.rect().height() >= 44.0 && notice.contains_rect(close.rect()));
    close.focus();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().work.workflow.change_notice.is_none());
}

#[test]
fn related_availability_caches_passes_before_changing_to_review() {
    let mut app = object_app();
    let task = app.work.selected_task_id.clone().unwrap();
    let request = test_request(&app, 981_004, Some(app.config.dataset_id.as_str()));
    app.runtime.active_requests.insert(request.request_id);
    app.work.availability.refresh_after_load = false;
    let workflows = [AssignmentKind::Annotation, AssignmentKind::Review]
        .into_iter()
        .flat_map(|kind| {
            [WorkflowVariant::Objects, WorkflowVariant::Overview]
                .into_iter()
                .map({
                    let task = task.clone();
                    move |variant| WorkflowAvailability {
                        selection: WorkflowSelection {
                            task_id: task.clone(),
                            kind: kind.clone(),
                            variant,
                        },
                        split: true,
                        available: kind == AssignmentKind::Review
                            && variant == WorkflowVariant::Objects,
                        reason: Some(WorkflowUnavailableReason::ObjectsPending),
                    }
                })
        })
        .collect();
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            request,
            checked_assignments: Vec::new(),
            result: Ok(labello_client::AssignmentAvailability {
                kind: AssignmentKind::Annotation,
                workflows,
                queue: None,
                tasks: BTreeMap::from([(task.clone(), false)]),
                reasons: Default::default(),
                related: vec![labello_client::AssignmentAvailabilityEntry {
                    kind: AssignmentKind::Review,
                    tasks: BTreeMap::from([(task.clone(), true)]),
                    reasons: Default::default(),
                }],
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(
        !app.work.availability.tasks[&task],
        "review eligibility must not overwrite annotation eligibility"
    );
    app.reset_assignment_availability_for_workspace();
    app.view = AppView::Review;
    assert!(app.restore_session_assignment_availability());
    app.select_initial_workflow_variant(&task);
    assert_eq!(app.work.workflow.variant, WorkflowVariant::Objects);
}

#[test]
fn review_navigation_does_not_back_up_pristine_or_server_saved_edits() {
    let mut app = review_object_app();
    assert!(app.workflow_review_needs_browser_backup());
    let labello_client::WorkflowDraftInput::Edits { edits } =
        app.current_workflow_draft().unwrap().draft
    else {
        panic!()
    };
    let assignment = app.work.assignment.clone().unwrap();
    let context = app.workflow_context().unwrap().clone();
    app.work
        .current_state
        .as_mut()
        .unwrap()
        .workflow_edit_drafts
        .insert(
            assignment.assignment_id.clone(),
            labello_domain::WorkflowEditDraft {
                task_id: assignment.task_id.clone(),
                item: context.item,
                kind: AssignmentKind::Review,
                edits,
                sequence: 1,
                previous_sequence: 0,
            },
        );
    assert!(
        !app.workflow_review_needs_browser_backup(),
        "saved server proposals need no local recovery prompt"
    );
    app.work
        .current_state
        .as_mut()
        .unwrap()
        .workflow_edit_drafts
        .clear();
    app.work.review_corrections = Default::default();
    app.work.correction_draft = None;
    assert!(
        !app.workflow_review_needs_browser_backup(),
        "viewing or skipping a review item is not an edit"
    );
}

#[test]
fn previous_failure_is_visible_and_keeps_the_current_workspace() {
    for size in [
        egui::vec2(1440.0, 1000.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(320.0, 320.0),
    ] {
        let mut app = object_app();
        let current = app.work.assignment.clone();
        let annotations = app.work.annotations.clone();
        let operation_id = app.begin_load();
        let request = test_request(&app, operation_id, Some(app.config.dataset_id.as_str()));
        app.runtime.active_requests.insert(operation_id);
        app.runtime
            .tx
            .send(UiMessage::PreviousAssignmentLoaded {
                request,
                operation_id,
                assignment: None,
                result: Box::new(Err(
                    "Another worker has changed or reviewed this history item."
                        .to_owned()
                        .into(),
                )),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert_eq!(app.work.assignment, current);
        assert_eq!(app.work.annotations, annotations);
        assert!(!app.loading.image);
        let message = app.work.workflow.navigation_error.clone().unwrap();
        let mut harness = Harness::builder().with_size(size).build_eframe(|_| app);
        harness.run();
        harness.get(
            egui_kittest::kittest::By::new()
                .role(egui::accesskit::Role::Alert)
                .value(&message),
        );
        let canvas = harness.get_by_label("Annotation canvas").rect();
        let notice = harness.get_by_label("Navigation error").rect();
        assert!(
            canvas.expand(1.0).contains_rect(notice),
            "{canvas:?} {notice:?}"
        );
        let dismiss = harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Dismiss navigation error");
        assert!(notice.contains_rect(dismiss.rect()));
        dismiss.focus();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(harness.state().work.workflow.navigation_error.is_none());
        assert_eq!(harness.state().work.assignment, current);
    }
}

#[test]
fn navigation_error_clears_on_retry_or_workflow_departure() {
    let mut app = object_app();
    app.work.workflow.navigation_error = Some("Previous failed".into());
    app.begin_load();
    assert!(app.work.workflow.navigation_error.is_none());
    app.work.workflow.navigation_error = Some("Previous failed".into());
    app.leave_current_workflow();
    assert!(app.work.workflow.navigation_error.is_none());
}

#[test]
fn failed_history_display_revalidation_preserves_current_edits() {
    for unavailable in [false, true] {
        let mut app = object_app();
        let current = app.work.assignment.clone();
        let annotations = app.work.annotations.clone();
        let cached = crate::app::LoadedImage {
            prepared_until: None,
            review_submitters: vec![],
            reasons: vec![],
            assignment: app.work.assignment.clone().unwrap(),
            queued: app.work.current.clone().unwrap(),
            annotations: annotations.clone(),
            state: app.work.current_state.clone().unwrap(),
            color_image: None,
        };
        let operation_id = app.begin_load();
        app.work.workflow.revalidating_history = true;
        app.work.workflow.returning_forward = true;
        app.work.pending_transition = Some(crate::app::PendingTransition::NextAssignment);
        let request = test_request(&app, operation_id, Some(app.config.dataset_id.as_str()));
        app.runtime.active_requests.insert(operation_id);
        app.runtime
            .tx
            .send(UiMessage::PreparedReviewRevalidated {
                request,
                operation_id,
                cached: Box::new(cached),
                result: Box::new(if unavailable {
                    Ok(None)
                } else {
                    Err("Review target changed".to_owned().into())
                }),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert_eq!(app.work.assignment, current);
        assert_eq!(app.work.annotations, annotations);
        assert!(app.work.workflow.navigation_error.is_some());
        assert!(!app.work.workflow.revalidating_history);
        assert!(!app.work.workflow.returning_forward);
        assert!(app.work.pending_transition.is_none());
        assert!(!app.loading.image);
    }
}

#[test]
fn failed_save_before_previous_reports_the_error_without_navigating() {
    let mut app = object_app();
    let current = app.work.assignment.clone().unwrap();
    let annotations = app.work.annotations.clone();
    app.work.pending_transition = Some(crate::app::PendingTransition::WorkItemHistory(
        WorkflowHistoryEntry {
            image_id: "earlier".into(),
            assignment_id: "earlier".into(),
            seen_at: labello_domain::now(),
        },
    ));
    let operation_id = app.begin_operation();
    let request = test_request(&app, operation_id, Some(app.config.dataset_id.as_str()));
    app.runtime.active_requests.insert(operation_id);
    app.runtime
        .tx
        .send(UiMessage::SaveFinished {
            request,
            operation_id,
            assignment_id: current.assignment_id.clone(),
            edit_generation: app.work.edit_generation,
            completed: false,
            result: Box::new(Err("Draft could not be saved".to_owned().into())),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.work.assignment.as_ref(), Some(&current));
    assert_eq!(app.work.annotations, annotations);
    assert_eq!(app.work.save_status, SaveStatus::Retry);
    assert!(app.work.pending_transition.is_none());
    assert!(
        app.work
            .workflow
            .navigation_error
            .as_deref()
            .unwrap()
            .contains("Could not save before returning")
    );
}

#[test]
fn activating_history_removes_its_prefetched_copy_without_releasing_the_active_lease() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    let loaded = crate::app::LoadedImage {
        prepared_until: None,
        review_submitters: vec![],
        reasons: vec![],
        assignment: app.work.assignment.clone().unwrap(),
        queued: app.work.current.clone().unwrap(),
        annotations: app.work.annotations.clone(),
        state: app.work.current_state.clone().unwrap(),
        color_image: None,
    };
    app.work.queue.clear();
    assert!(app.work.queue.push_prepared(loaded.clone()));
    app.apply_loaded_image(&egui::Context::default(), loaded.clone());
    assert!(!app.work.queue.contains_assignment(&loaded.assignment));
    assert_eq!(app.work.assignment.as_ref(), Some(&loaded.assignment));
    assert!(!app.runtime.reservation_cleanup.has_pending_releases());
}

#[test]
fn history_activation_drops_cancelled_prefetch_but_preserves_newer_reservations() {
    let mut app = object_app();
    let mut loaded = crate::app::LoadedImage {
        prepared_until: None,
        review_submitters: vec![],
        reasons: vec![],
        assignment: app.work.assignment.clone().unwrap(),
        queued: app.work.current.clone().unwrap(),
        annotations: app.work.annotations.clone(),
        state: app.work.current_state.clone().unwrap(),
        color_image: None,
    };
    let mut cancelled = loaded.clone();
    cancelled.assignment.assignment_id = "cancelled-prefetch".into();
    let mut recorded = cancelled.assignment.clone();
    recorded.status = labello_domain::AssignmentStatus::Cancelled;
    loaded.state.assignments.push(recorded);
    let mut newer = loaded.clone();
    newer.assignment.assignment_id = "newer-prefetch".into();
    app.work.queue.clear();
    app.work.queue.set_queue_size(2);
    assert!(app.work.queue.push_prepared(cancelled.clone()));
    assert!(app.work.queue.push_prepared(newer.clone()));
    app.apply_loaded_image(&egui::Context::default(), loaded);
    assert!(!app.work.queue.contains_assignment(&cancelled.assignment));
    assert!(app.work.queue.contains_assignment(&newer.assignment));
    assert!(!app.runtime.reservation_cleanup.has_pending_releases());
}

#[test]
fn completed_item_claims_next_before_pending_availability_finishes() {
    for variant in [WorkflowVariant::Objects, WorkflowVariant::Overview] {
        let mut app = object_app();
        app.runtime.api = Some(Rc::new(SpyApi::new()));
        app.work.workflow.variant = variant;
        app.work.workflow.variant_selected = true;
        app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
        app.work.availability.kind = Some(AssignmentKind::Annotation);
        app.work.availability.resolved = false;
        app.work.availability.loading = true;
        app.work.pending_transition = Some(crate::app::PendingTransition::NextAssignment);
        let current = app.work.assignment.clone().unwrap();
        let state = app.work.current_state.clone().unwrap();
        let operation_id = app.begin_operation();
        let request = test_request(&app, operation_id, Some(app.config.dataset_id.as_str()));
        app.runtime.active_requests.insert(operation_id);
        app.runtime
            .tx
            .send(UiMessage::SaveFinished {
                request,
                operation_id,
                assignment_id: current.assignment_id,
                edit_generation: app.work.edit_generation,
                completed: true,
                result: Box::new(Ok(state)),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert!(
            app.runtime.commands.iter().any(|command| matches!(command,
            UiCommand::ClaimAssignment { variant: selected, .. } if *selected == variant)),
            "completed {variant:?} must not wait for advisory availability"
        );
        assert!(app.loading.image);
        assert!(app.work.availability.loading);
    }
}

#[test]
fn empty_continuation_returns_to_availability_routing_without_reclaiming() {
    for kind in [AssignmentKind::Annotation, AssignmentKind::Review] {
        let mut app = object_app();
        app.runtime.api = Some(Rc::new(SpyApi::new()));
        app.view = if kind == AssignmentKind::Review {
            AppView::Review
        } else {
            AppView::Annotate
        };
        app.work.workflow.variant_selected = true;
        app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
        app.work.availability.kind = Some(kind.clone());
        app.work.availability.resolved = false;
        app.retire_current_image();
        app.request_next_image();
        let operation_id = app.work.active_load_id.expect("continuation claim");
        app.runtime
            .tx
            .send(UiMessage::ImageLoaded {
                request: test_request(&app, operation_id, Some(app.config.dataset_id.as_str())),
                operation_id,
                assignment: None,
                result: Box::new(Ok(None)),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert!(!app.work.retired_image);
        assert!(app.work.current.is_none());
        assert!(app.work.availability.load_after_resolution);
        app.runtime.commands.clear();
        app.request_next_image();
        assert!(
            !app.runtime
                .commands
                .iter()
                .any(|command| matches!(command, UiCommand::ClaimAssignment { .. }))
        );
        let request = app.request_identity(Some(app.config.dataset_id.clone()));
        app.runtime.active_requests.insert(request.request_id);
        app.work.availability.refresh_after_load = false;
        app.runtime
            .tx
            .send(UiMessage::AssignmentAvailabilityLoaded {
                request,
                checked_assignments: Vec::new(),
                result: Ok(labello_client::AssignmentAvailability {
                    kind,
                    workflows: vec![],
                    queue: None,
                    tasks: BTreeMap::from([(app.work.selected_task_id.clone().unwrap(), false)]),
                    reasons: Default::default(),
                    related: vec![],
                }),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert!(!app.loading.image);
        assert!(
            !app.runtime
                .commands
                .iter()
                .any(|command| matches!(command, UiCommand::ClaimAssignment { .. }))
        );
    }
}

#[test]
fn prefetch_waits_for_navigation_and_for_return_to_history_frontier() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.work.queue.clear();
    app.loading.image = true;
    app.request_prefetch();
    assert!(!app.work.queue.is_loading());
    app.loading.image = false;
    let assignment = app.work.assignment.clone().unwrap();
    let current = WorkflowHistoryEntry {
        image_id: assignment.image_id,
        assignment_id: assignment.assignment_id,
        seen_at: labello_domain::now(),
    };
    app.work.workflow.current_root = Some(current.assignment_id.clone());
    app.work.workflow.history = vec![
        WorkflowHistoryEntry {
            image_id: "forward-image".into(),
            assignment_id: "forward-item".into(),
            seen_at: labello_domain::now(),
        },
        current,
    ];
    app.request_prefetch();
    assert!(!app.work.queue.is_loading());
    app.work.workflow.current_root = Some("forward-item".into());
    app.request_prefetch();
    assert!(app.work.queue.is_loading());
}

#[test]
fn navigation_dispatch_defers_stats_until_the_image_is_ready() {
    let mut app = object_app();
    app.runtime.api = Some(Rc::new(SpyApi::new()));
    app.runtime.commands.clear();
    let stats = app.request_identity(Some(app.config.dataset_id.clone()));
    let history = app.request_identity(Some(app.config.dataset_id.clone()));
    app.queue_command(UiCommand::Stats {
        request: stats.clone(),
        dataset_id: app.config.dataset_id.clone(),
    });
    app.queue_command(UiCommand::WorkflowHistory {
        request: history.clone(),
        dataset_id: app.config.dataset_id.clone(),
        selection: app.workflow_selection().unwrap(),
    });
    app.loading.image = true;
    app.start_frame_commands();
    assert!(
        matches!(app.runtime.commands.front(), Some(UiCommand::Stats { request, .. }) if request.request_id == stats.request_id)
    );
    assert_eq!(app.runtime.commands.len(), 1);
    assert!(app.runtime.active_requests.contains(&stats.request_id));
    assert!(
        matches!(app.runtime.rx.try_recv().unwrap(), UiMessage::WorkflowHistoryLoaded { request, .. } if request.request_id == history.request_id)
    );
    app.loading.image = false;
    app.start_frame_commands();
    assert!(app.runtime.commands.is_empty());
    assert!(
        matches!(app.runtime.rx.try_recv().unwrap(), UiMessage::StatsLoaded { request, .. } if request.request_id == stats.request_id)
    );
}
