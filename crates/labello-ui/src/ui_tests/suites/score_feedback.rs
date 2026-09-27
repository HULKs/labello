#[test]
fn focus_feedback_uses_the_current_stage_and_announces_each_window_once() {
    for review in [false, true] {
        let mut harness = if review {
            loaded_review_harness(Rc::new(SpyApi::new()))
        } else {
            loaded_work_harness(Rc::new(SpyApi::new()))
        };
        harness.set_size(egui::vec2(390.0, 844.0));
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        let task = app.selected_task().unwrap().clone();
        let now = labello_domain::now();
        app.datasets.stats.scoring_focus = Some(labello_domain::FocusWindow {
            starts_at: now,
            ends_at: now + chrono::Duration::minutes(2),
            task_id: Some(task.task_id.clone()),
        });
        app.datasets.stats.review_scoring_focus = Some(labello_domain::FocusWindow {
            starts_at: now,
            ends_at: now + chrono::Duration::minutes(10),
            task_id: Some(task.task_id),
        });
        app.observe_scoring_focus(&ctx);
        app.runtime.commands.clear();
        app.runtime.api = None;
        let stage = if review { "Review" } else { "Annotation" };
        let minutes = if review { 10 } else { 2 };
        let label = format!(
            "{stage} focus changed: {}, ×1.5 points, {minutes} min left",
            task.name
        );
        harness.run_steps(2);
        let rect = harness.get_by_label(&label).rect();
        assert!(rect.left() >= 194.5 && rect.right() <= 390.0, "{rect:?}");
        harness.run_steps(8);
        harness.state_mut().observe_scoring_focus(&ctx);
        harness.step();
        assert!(
            harness.query_by_label(&label).is_none(),
            "refresh repeated unchanged focus notification"
        );
        let app = harness.state_mut();
        let focus = if review {
            app.datasets.stats.review_scoring_focus.as_mut()
        } else {
            app.datasets.stats.scoring_focus.as_mut()
        }
        .unwrap();
        focus.starts_at += chrono::Duration::nanoseconds(1);
        app.observe_scoring_focus(&ctx);
        harness.run_steps(2);
        assert!(
            harness.query_by_label(&label).is_some(),
            "new focus window should notify"
        );
        if review {
            harness.state_mut().datasets.stats_error = Some("unavailable".into());
        } else {
            harness
                .state_mut()
                .datasets
                .stats
                .scoring_focus
                .as_mut()
                .unwrap()
                .ends_at = labello_domain::now();
        }
        harness.step();
        assert!(
            harness.query_by_label(&label).is_none(),
            "failed or expired focus must disappear"
        );
    }
}

#[test]
fn focus_feedback_refreshes_only_the_current_stage_balance_block() {
    for review in [false, true] {
        for reason in [
            labello_domain::WorkflowUnavailableReason::BalanceLimit,
            labello_domain::WorkflowUnavailableReason::ClaimedByOthers,
        ] {
            let mut harness = if review {
                loaded_review_harness(Rc::new(SpyApi::new()))
            } else {
                loaded_work_harness(Rc::new(SpyApi::new()))
            };
            step_until(&mut harness, 20, |app| !app.loading.stats);
            let app = harness.state_mut();
            app.runtime.commands.clear();
            let task_id = app.work.selected_task_id.clone().unwrap();
            let now = labello_domain::now();
            let focus = Some(labello_domain::FocusWindow {
                starts_at: now,
                ends_at: now + chrono::Duration::minutes(10),
                task_id: Some(task_id.clone()),
            });
            if review {
                app.datasets.stats.review_scoring_focus = focus;
            } else {
                app.datasets.stats.scoring_focus = focus;
            }
            app.work.availability.dataset_id = Some(app.config.dataset_id.clone());
            app.work.availability.kind = app.assignment_kind();
            app.work.availability.resolved = true;
            app.work.availability.error = None;
            app.work.availability.tasks.insert(task_id.clone(), false);
            app.work.availability.reasons.insert(task_id, reason);
            app.refresh_blocked_focus();
            assert_eq!(
                app.runtime
                    .commands
                    .iter()
                    .any(|command| matches!(command, UiCommand::Stats { .. })),
                reason == labello_domain::WorkflowUnavailableReason::BalanceLimit
            );
            app.runtime.commands.clear();
            app.work.availability.kind = Some(if review {
                AssignmentKind::Annotation
            } else {
                AssignmentKind::Review
            });
            assert!(!app.focused_workflow_blocked());
        }
    }
}

fn pending_image_score(app: &LabelloApp) -> Option<(RequestIdentity, ImageId, u64, u64)> {
    app.runtime
        .commands
        .iter()
        .find_map(|command| match command {
            UiCommand::ImageScore {
                request,
                image_id,
                after_sequence,
                through_sequence,
                ..
            } => Some((
                request.clone(),
                image_id.clone(),
                *after_sequence,
                *through_sequence,
            )),
            _ => None,
        })
}

#[test]
fn score_feedback_requests_only_confirmed_completion_and_keeps_the_assignment_window() {
    for kind in ["annotation", "review", "correction", "skip"] {
        for succeeds in [false, true] {
            for completed in [false, true] {
                if matches!(kind, "correction" | "skip") && !completed {
                    continue;
                }
                let mut harness = if kind == "annotation" {
                    loaded_work_harness(Rc::new(SpyApi::new()))
                } else {
                    loaded_review_harness(Rc::new(SpyApi::new()))
                };
                let app = harness.state_mut();
                app.runtime.commands.clear();
                let assignment = app.work.assignment.clone().unwrap();
                let start = app.work.current_state.as_ref().unwrap().current_sequence;
                // Reloading the same assignment after an object decision must not lose its award.
                app.work.score_feedback.loaded(&assignment, start + 2, 0.0);
                let mut state = app.work.current_state.clone().unwrap();
                state.current_sequence = start + 4;
                let result = if succeeds {
                    Ok(state)
                } else {
                    Err("Synthetic failure".to_string().into())
                };
                let request = test_request(app, 77, Some("demo"));
                app.work.active_operation_id = Some(77);
                app.runtime.active_requests.insert(77);
                let assignment_id = assignment.assignment_id;
                let message = match kind {
                    "annotation" => {
                        app.work.pending_transition =
                            Some(crate::app::PendingTransition::NextAssignment);
                        UiMessage::SaveFinished {
                            request,
                            operation_id: 77,
                            assignment_id,
                            edit_generation: app.work.edit_generation,
                            completed,
                            result: Box::new(result),
                        }
                    }
                    "review" => UiMessage::ReviewFinished {
                        request,
                        operation_id: 77,
                        assignment_id,
                        phase: if completed {
                            crate::app::ReviewPhase::FullImage
                        } else {
                            crate::app::ReviewPhase::Object
                        },
                        decision: labello_domain::ReviewDecision::Approved,
                        result: Box::new(result),
                    },
                    "skip" => UiMessage::ReleaseFinished {
                        request,
                        operation_id: 77,
                        assignment_id,
                        result: result.map(|_| ()),
                    },
                    _ => UiMessage::CorrectionFinished {
                        request,
                        operation_id: 77,
                        assignment_id,
                        result: result.map(|state| state.current_sequence),
                    },
                };
                app.runtime.tx.send(message).unwrap();
                app.process_messages(&egui::Context::default());
                let receipt = pending_image_score(app);
                assert_eq!(
                    receipt.is_some(),
                    succeeds && completed && kind != "skip",
                    "{kind}, success={succeeds}, completed={completed}"
                );
                if let Some((_, image, after, through)) = receipt {
                    assert_eq!(image, assignment.image_id);
                    assert_eq!((after, through), (start, start + 4));
                }
            }
        }
    }
}

#[test]
fn score_feedback_waits_for_next_image_and_receipt_in_either_order() {
    for receipt_first in [false, true] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        step_until(&mut harness, 20, |app| !app.work.queue.is_empty());
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        app.runtime.commands.clear();
        let sequence = app.work.current_state.as_ref().unwrap().current_sequence;
        app.request_image_score(&ctx, sequence + 1);
        let (request, _, _, _) = pending_image_score(app).unwrap();
        let message = || UiMessage::ImageScoreLoaded {
            request: request.clone(),
            result: Ok(labello_client::ImageScore { hundredths: 2725 }),
        };
        if receipt_first {
            app.runtime.tx.send(message()).unwrap();
            app.process_messages(&ctx);
        }
        assert!(app.promote_prepared_assignment(&ctx, None));
        if !receipt_first {
            app.runtime.tx.send(message()).unwrap();
            app.process_messages(&ctx);
        }
        app.runtime.commands.clear();
        app.runtime.api = None;
        harness.run_steps(2);
        assert!(
            harness
                .query_by_label("Score gained: +27.25 points")
                .is_some()
        );
        harness.run_steps(80);
        assert!(
            harness
                .query_by_label("Score gained: +27.25 points")
                .is_none()
        );
        let app = harness.state_mut();
        app.runtime.tx.send(message()).unwrap();
        app.process_messages(&ctx);
        harness.step();
        assert!(
            harness
                .query_by_label("Score gained: +27.25 points")
                .is_none(),
            "duplicate response restarted feedback"
        );
    }
}

#[test]
fn score_feedback_suppresses_nonpositive_failed_and_obsolete_receipts() {
    for outcome in ["zero", "negative", "failure", "workspace", "auth"] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        step_until(&mut harness, 20, |app| !app.work.queue.is_empty());
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        app.runtime.commands.clear();
        app.request_image_score(
            &ctx,
            app.work.current_state.as_ref().unwrap().current_sequence + 1,
        );
        let (request, _, _, _) = pending_image_score(app).unwrap();
        assert!(app.promote_prepared_assignment(&ctx, None));
        match outcome {
            "workspace" => app.begin_workspace_epoch(),
            "auth" => app.begin_auth_epoch(),
            _ => {}
        }
        let result = match outcome {
            "failure" => Err("Receipt unavailable".to_string().into()),
            "zero" => Ok(labello_client::ImageScore { hundredths: 0 }),
            "negative" => Ok(labello_client::ImageScore { hundredths: -100 }),
            _ => Ok(labello_client::ImageScore { hundredths: 2725 }),
        };
        app.runtime
            .tx
            .send(UiMessage::ImageScoreLoaded { request, result })
            .unwrap();
        app.process_messages(&ctx);
        app.runtime.commands.clear();
        app.runtime.api = None;
        harness.run_steps(2);
        assert!(
            harness
                .query_by_label("Score gained: +27.25 points")
                .is_none(),
            "{outcome}"
        );
    }
}

#[test]
fn score_feedback_stays_at_right_edge_with_reduced_motion() {
    for size in [
        egui::vec2(320.0, 320.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(600.0, 800.0),
        egui::vec2(1440.0, 1000.0),
    ] {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        step_until(&mut harness, 20, |app| !app.work.queue.is_empty());
        harness.set_size(size);
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        app.runtime.commands.clear();
        app.request_image_score(
            &ctx,
            app.work.current_state.as_ref().unwrap().current_sequence + 1,
        );
        let (request, _, _, _) = pending_image_score(app).unwrap();
        assert!(app.promote_prepared_assignment(&ctx, None));
        app.runtime
            .tx
            .send(UiMessage::ImageScoreLoaded {
                request,
                result: Ok(labello_client::ImageScore { hundredths: 2700 }),
            })
            .unwrap();
        app.process_messages(&ctx);
        app.runtime.commands.clear();
        app.runtime.api = None;
        harness.run_steps(2);
        let first = harness.get_by_label("Score gained: +27 points").rect();
        assert!(
            first.left() > size.x / 2.0 && first.right() <= size.x,
            "{size:?}: {first:?}"
        );
        assert!(first.top() >= 0.0 && first.bottom() <= size.y);
        harness.step();
        assert_eq!(
            harness.get_by_label("Score gained: +27 points").rect(),
            first
        );
    }
}

#[test]
fn score_feedback_animation_rises_and_is_click_through() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    step_until(&mut harness, 20, |app| !app.work.queue.is_empty());
    crate::set_reduced_motion(&harness.ctx, false);
    let ctx = harness.ctx.clone();
    let app = harness.state_mut();
    app.runtime.commands.clear();
    app.request_image_score(
        &ctx,
        app.work.current_state.as_ref().unwrap().current_sequence + 1,
    );
    let (request, _, _, _) = pending_image_score(app).unwrap();
    assert!(app.promote_prepared_assignment(&ctx, None));
    app.runtime
        .tx
        .send(UiMessage::ImageScoreLoaded {
            request,
            result: Ok(labello_client::ImageScore { hundredths: 2700 }),
        })
        .unwrap();
    app.process_messages(&ctx);
    app.runtime.commands.clear();
    app.runtime.api = None;
    harness.run_steps(2);
    let first = harness.get_by_label("Score gained: +27 points").rect();
    let popup_layer = egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("score-gain-feedback"),
    );
    assert_ne!(ctx.layer_id_at(first.center()), Some(popup_layer));
    harness.step();
    let moved = harness.get_by_label("Score gained: +27 points").rect();
    assert!(moved.center().y < first.center().y);
    harness.run_steps(3);
    assert!(harness.query_by_label("Score gained: +27 points").is_none());
}

#[test]
fn score_feedback_does_not_attach_a_late_receipt_to_a_later_completion() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    step_until(&mut harness, 20, |app| app.work.queue.len() == 2);
    let ctx = harness.ctx.clone();
    let app = harness.state_mut();
    app.runtime.commands.clear();
    app.request_image_score(
        &ctx,
        app.work.current_state.as_ref().unwrap().current_sequence + 1,
    );
    let (old_request, _, _, _) = pending_image_score(app).unwrap();
    assert!(app.promote_prepared_assignment(&ctx, None));
    app.runtime.commands.clear();
    app.request_image_score(
        &ctx,
        app.work.current_state.as_ref().unwrap().current_sequence + 1,
    );
    let (new_request, _, _, _) = pending_image_score(app).unwrap();
    assert!(app.promote_prepared_assignment(&ctx, None));
    for (request, hundredths) in [(old_request, 2700), (new_request, 5000)] {
        app.runtime
            .tx
            .send(UiMessage::ImageScoreLoaded {
                request,
                result: Ok(labello_client::ImageScore { hundredths }),
            })
            .unwrap();
        app.process_messages(&ctx);
    }
    app.runtime.commands.clear();
    app.runtime.api = None;
    harness.run_steps(2);
    assert!(harness.query_by_label("Score gained: +27 points").is_none());
    assert!(harness.query_by_label("Score gained: +50 points").is_some());
}

#[cfg(feature = "inspector-presets")]
#[test]
fn score_feedback_migration_waits_for_assignment_completion() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [
        InspectorPreset::MigrationFullImage,
        InspectorPreset::MigrationReview,
    ] {
        for completed in [false, true] {
            let ctx = egui::Context::default();
            let mut app = inspector_presets::build(preset, &ctx);
            app.runtime.api = Some(Rc::new(SpyApi::new()));
            let mut assignment = app.work.assignment.clone().unwrap();
            let mut state = app.work.current_state.clone().unwrap();
            let start = state.current_sequence;
            app.work.score_feedback.loaded(&assignment, start, 0.0);
            state.current_sequence += 2;
            if completed {
                assignment.status = AssignmentStatus::Completed;
            }
            let result = labello_client::ManualMigrationCommandResult {
                image_state: state,
                progress: labello_client::ManualMigrationProgress {
                    expected: 0,
                    annotated: 0,
                    excluded: 0,
                    pending: 0,
                },
                cursor: Some(labello_domain::MigrationCursor::FullImage),
                active_pass: None,
                confirmation: None,
                assignment: Some(assignment),
                annotation_id: None,
            };
            let request = test_request(&app, 77, Some("demo"));
            app.runtime.active_requests.insert(77);
            app.runtime
                .tx
                .send(UiMessage::MigrationFinished {
                    request,
                    result: Box::new(Ok(result)),
                })
                .unwrap();
            app.process_messages(&ctx);
            let receipt = pending_image_score(&app);
            assert_eq!(receipt.is_some(), completed);
            if let Some((_, _, after, through)) = receipt {
                assert_eq!((after, through), (start, start + 2));
            }
        }
    }
}
