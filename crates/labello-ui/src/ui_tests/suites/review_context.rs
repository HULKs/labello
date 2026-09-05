#[test]
fn review_inspector_identifies_the_exact_persisted_target() {
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
    let harness = loaded_review_harness(api);
    assert!(harness.query_by_label("Active review target").is_some());
    assert!(harness.query_by_label("Persisted version 1").is_some());
    assert!(
        harness
            .query_by_label("Review position: Object 1 of 1")
            .is_some()
    );
    assert!(
        harness
            .query_by_label("Class: Person · Type: Bounding boxes")
            .is_some()
    );
}

#[test]
fn review_context_rejects_missing_stale_and_lost_assignments_but_retains_preview_failure() {
    let api = Rc::new(SpyApi::new());
    let id = seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    let valid = harness.state().review_context().unwrap();
    harness.state_mut().work.current_texture = None;
    let missing_preview = harness.state().review_context().unwrap();
    assert!(missing_preview.preview_unavailable);
    assert_eq!(valid.phase, missing_preview.phase);
    harness.state_mut().loading.image = true;
    assert!(harness.state().review_context().is_none());
    harness.state_mut().loading.image = false;
    let assignment = harness.state().work.assignment.clone();
    harness.state_mut().work.assignment = None;
    assert!(harness.state().review_context().is_none());
    harness.state_mut().work.assignment = assignment;
    harness
        .state_mut()
        .work
        .current_state
        .as_mut()
        .unwrap()
        .annotations
        .get_mut(&id)
        .unwrap()
        .last_mut()
        .unwrap()
        .version += 1;
    assert!(harness.state().review_context().is_none());
    harness.state_mut().work.annotations[0].version += 1;
    assert!(matches!(
        harness.state().review_context().unwrap().phase,
        crate::review_context::ReviewContextPhase::Object {
            annotation_version: Some(2),
            ..
        }
    ));
    harness
        .state_mut()
        .work
        .assignment
        .as_mut()
        .unwrap()
        .task_id = TaskId::from("another-task");
    assert!(harness.state().review_context().is_none());
}

#[test]
fn review_context_distinguishes_correction_input_and_final_check_from_persisted_objects() {
    let api = Rc::new(SpyApi::new());
    let id = seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    harness.state_mut().start_correction();
    let correction = harness
        .state()
        .review_context()
        .unwrap()
        .correction
        .unwrap();
    assert_eq!(correction.base_version, 1);
    assert!(!correction.unsaved_input);
    harness.state_mut().edit_correction_bbox(BoundingBoxEdit {
        annotation_id: id,
        bounding_box: BoundingBox {
            x: 0.3,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        },
    });
    let context = harness.state().review_context().unwrap();
    assert!(context.correction.as_ref().unwrap().unsaved_input);
    assert!(matches!(
        context.phase,
        crate::review_context::ReviewContextPhase::Object {
            annotation_version: Some(1),
            ..
        }
    ));
    harness.state_mut().discard_correction();
    assert!(
        harness
            .state()
            .review_context()
            .unwrap()
            .correction
            .is_none()
    );
    harness.state_mut().work.review_index = 1;
    harness.state_mut().sync_review_selection();
    let final_check = harness.state().review_context().unwrap();
    assert!(matches!(
        final_check.phase,
        crate::review_context::ReviewContextPhase::FullImage { migration: false }
    ));
    assert_eq!(final_check.phase_label(), "Final check / Full image");
    assert!(!final_check.accessible_summary().contains("version"));
    assert!(!final_check.accessible_summary().contains("Object 2"));
}

#[test]
fn review_context_uses_task_identity_for_duplicate_names_and_wraps_full_accessible_text() {
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
    let mut harness = loaded_review_harness(api);
    let selected = harness.state().work.selected_task_id.clone().unwrap();
    let name = "A long repeated workflow and class name that must remain complete in the Inspector";
    let task = harness
        .state_mut()
        .work
        .tasks
        .iter_mut()
        .find(|task| task.task_id == selected)
        .unwrap();
    task.name = name.to_string();
    let mut duplicate = task.clone();
    duplicate.task_id = TaskId::from("other-workflow");
    duplicate.annotation_type = AnnotationType::Skeleton;
    harness.state_mut().work.tasks.insert(0, duplicate);
    for class in &mut harness.state_mut().work.classes {
        class.name = name.to_string();
    }
    let context = harness.state().review_context().unwrap();
    assert_eq!(context.annotation_type, AnnotationType::BoundingBox);
    assert_eq!(context.workflow_name, name);
    assert_eq!(context.class_name, name);
    for (width, height) in [(390.0, 844.0), (320.0, 320.0)] {
        harness.set_size(egui::vec2(width, height));
        harness.state_mut().work.drawer = Some(Drawer::Inspector);
        harness.run_steps(3);
        let full = format!(
            "Active review context: {}",
            harness
                .state()
                .review_context()
                .unwrap()
                .accessible_summary()
        );
        assert!(harness.query_by_label(&full).is_some());
        assert_control_inside(
            &harness,
            "Close Inspector",
            egui::accesskit::Role::Button,
            width,
            height,
        );
        harness
            .get_by_label("Current decision: Not reviewed")
            .scroll_to_me();
        harness.run_steps(8);
        let last = harness
            .get_by_label("Current decision: Not reviewed")
            .rect();
        let modal = harness
            .get_by_role_and_label(egui::accesskit::Role::Window, "Inspector")
            .rect();
        assert!(
            modal.contains_rect(last),
            "last context line must be reachable: {last:?}, {modal:?}"
        );
        assert!(last.bottom() <= height && last.right() <= width);
    }
}

#[test]
fn review_context_tracks_keyboard_decisions_refocus_and_rejects_stale_replies() {
    let api = Rc::new(SpyApi::new());
    let first = seed_review_annotation(
        &api,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        }),
        true,
    );
    {
        let mut spy = api.state.borrow_mut();
        let state = spy.states.values_mut().next().unwrap();
        let mut second = state.current_annotation(&first).unwrap().clone();
        second.annotation_id = labello_domain::AnnotationId::from("second_review_object");
        state
            .apply_event(&EventLogEntry::new(
                state.current_sequence + 1,
                state.image_id.clone(),
                second.author_user_id.clone(),
                DatasetRole::Annotator,
                now(),
                EventPayload::AnnotationVersionCreated {
                    annotation: second,
                    previous_version: None,
                    reason: None,
                },
            ))
            .unwrap();
    }
    let mut harness = loaded_review_harness(api.clone());
    assert_eq!(
        harness.state().review_context().unwrap().phase_label(),
        "Object 1 of 2"
    );
    harness.key_press(egui::Key::Y);
    step_until(&mut harness, 12, |app| {
        !app.loading.saving && app.work.review_index == 1
    });
    let second = harness.state().review_context().unwrap();
    assert_eq!(second.phase_label(), "Object 2 of 2");
    assert_eq!(
        second.decision, None,
        "the first object's approval must not follow selection"
    );
    harness.key_press(egui::Key::R);
    harness.step();
    assert_eq!(harness.state().review_context().unwrap(), second);
    harness
        .state()
        .runtime
        .tx
        .send(UiMessage::ReviewFinished {
            request: test_request(harness.state(), u64::MAX, Some("demo")),
            operation_id: u64::MAX,
            assignment_id: AssignmentId::from("obsolete_assignment"),
            phase: crate::app::ReviewPhase::FullImage,
            decision: labello_domain::ReviewDecision::Rejected,
            result: Box::new(Ok(ImageState::new(ImageId::from("stale_image")))),
        })
        .unwrap();
    harness.step();
    assert_eq!(harness.state().review_context().unwrap(), second);
    harness.key_press(egui::Key::Y);
    step_until(&mut harness, 12, |app| {
        !app.loading.saving && app.work.review_index == 2
    });
    let final_check = harness.state().review_context().unwrap();
    assert_eq!(final_check.phase_label(), "Final check / Full image");
    assert_eq!(final_check.decision, None);
    assert!(
        !final_check
            .accessible_summary()
            .contains("Persisted version")
    );
    assert!(!final_check.accessible_summary().contains("Object 3"));
    let old_image = harness
        .state()
        .work
        .assignment
        .as_ref()
        .unwrap()
        .image_id
        .clone();
    harness.key_press(egui::Key::Y);
    step_until(&mut harness, 16, |app| {
        app.work
            .assignment
            .as_ref()
            .is_some_and(|a| a.image_id != old_image)
    });
    assert!(
        harness
            .state()
            .review_context()
            .is_none_or(|context| context.decision.is_none())
    );
}

#[test]
fn review_context_uses_effective_policy_and_keeps_staged_revision_decisions_separate() {
    let api = Rc::new(SpyApi::new());
    let id = seed_review_annotation(
        &api,
        AnnotationGeometry::Skeleton(SkeletonGeometry {
            keypoints: vec![KeypointAnnotation {
                name: "legacy_point".into(),
                state: KeypointState::Absent,
                point: None,
            }],
        }),
        true,
    );
    let mut harness = loaded_review_harness(api);
    enter_test_review_revision(harness.state_mut());
    let target = ReviewTarget::AnnotationVersion {
        annotation_id: id,
        version: 1,
    };
    let user = harness.state().config.user_id.clone();
    let task = harness.state().selected_task().unwrap().task_id.clone();
    let state = harness.state_mut().work.current_state.as_mut().unwrap();
    let round = state.review_rounds[&task].event_id.clone();
    let review = ReviewRecord {
        review_id: ReviewId::from("effective_object_review"),
        target: target.clone(),
        reviewer_user_id: user.clone(),
        decision: labello_domain::ReviewDecision::Approved,
        timestamp: now(),
        comment: None,
    };
    state
        .review_record_rounds
        .insert(review.review_id.clone(), round);
    state.reviews.push(review);
    // A later audit entry from another round must not override effective state.
    let stale = ReviewRecord {
        review_id: ReviewId::from("old_round_rejection"),
        target: target.clone(),
        reviewer_user_id: user.clone(),
        decision: labello_domain::ReviewDecision::Rejected,
        timestamp: now(),
        comment: None,
    };
    state
        .review_record_rounds
        .insert(stale.review_id.clone(), EventId::from("other_round"));
    state.reviews.push(stale);
    let context = harness.state().review_context().unwrap();
    assert_eq!(context.annotation_type, AnnotationType::Skeleton);
    assert!(context.revision_mode);
    assert_eq!(
        context.decision,
        Some(labello_domain::ReviewDecision::Approved)
    );
    harness
        .state_mut()
        .work
        .staged_review_decisions
        .push(ReviewRecord {
            review_id: ReviewId::from("staged_rejection"),
            target,
            reviewer_user_id: user,
            decision: labello_domain::ReviewDecision::Rejected,
            timestamp: now(),
            comment: None,
        });
    let context = harness.state().review_context().unwrap();
    assert_eq!(
        context.decision,
        Some(labello_domain::ReviewDecision::Approved)
    );
    assert_eq!(
        context.staged_decision,
        Some(labello_domain::ReviewDecision::Rejected)
    );
    assert!(
        context
            .accessible_summary()
            .contains("Staged decision: Rejected (not committed)")
    );
    harness.state_mut().work.review_index = 1;
    harness.state_mut().sync_review_selection();
    let final_check = harness.state().review_context().unwrap();
    assert_eq!(
        final_check.decision,
        Some(labello_domain::ReviewDecision::Approved)
    );
    assert_eq!(final_check.staged_decision, None);
}

#[test]
fn review_context_retains_unsaved_correction_on_failure_and_clears_after_commit() {
    let api = Rc::new(SpyApi::new());
    let id = seed_review_annotation(
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
    harness.state_mut().start_correction();
    harness.state_mut().edit_correction_bbox(BoundingBoxEdit {
        annotation_id: id,
        bounding_box: BoundingBox {
            x: 0.3,
            y: 0.2,
            width: 0.3,
            height: 0.3,
        },
    });
    let before = harness.state().review_context().unwrap();
    let old_image = harness
        .state()
        .work
        .assignment
        .as_ref()
        .unwrap()
        .image_id
        .clone();
    api.fail_next_correction();
    harness.state_mut().request_correction();
    step_until(&mut harness, 12, |app| !app.loading.saving);
    assert_eq!(harness.state().review_context().unwrap(), before);
    harness.state_mut().request_correction();
    step_until(&mut harness, 16, |app| {
        app.work
            .assignment
            .as_ref()
            .is_some_and(|a| a.image_id != old_image)
    });
    assert!(
        harness
            .state()
            .review_context()
            .is_none_or(|context| context.correction.is_none())
    );
    assert!(harness.state().work.correction_draft.is_none());
}

#[cfg(feature = "inspector-presets")]
#[test]
fn migration_review_context_follows_canonical_discovered_and_final_targets() {
    use crate::inspector_presets::{self, InspectorPreset};
    use crate::review_context::ReviewContextPhase;
    use labello_domain::{ReviewDecision, ReviewId, ReviewRecord, ReviewTarget};

    for preset in [
        InspectorPreset::MigrationReview,
        InspectorPreset::MigrationDiscoveryReview,
    ] {
        let mut app = inspector_presets::build(preset, &egui::Context::default());
        // These inspector states intentionally omit final confirmation. Add the
        // authoritative historical confirmation needed for this sequence test.
        let task_id = app.work.selected_task_id.clone().unwrap();
        let state = app.work.current_state.as_mut().unwrap();
        let target_set_hash = state.migration_target_sets[&task_id]
            .target_set_hash
            .clone();
        let state_hash = state.current_migration_state_hash(&task_id).unwrap();
        state.migration_confirmations.insert(
            task_id.clone(),
            labello_domain::MigrationConfirmation {
                task_id,
                confirmation_hash: labello_domain::migration_confirmation_hash(
                    &target_set_hash,
                    &state_hash,
                )
                .unwrap(),
                target_set_hash,
                state_hash,
                actor_user_id: UserId::from("annotator"),
                timestamp: now(),
            },
        );
        app.sync_manual_migration();
        let mut observed = Vec::new();
        for index in 0..8 {
            let context = app
                .review_context()
                .expect("active canonical review target");
            assert_eq!(context.annotation_type, AnnotationType::Skeleton);
            assert_eq!(context.decision, None);
            observed.push(context.phase.clone());
            let (task_id, target) = app.current_migration_review_target().unwrap();
            let target = match target {
                labello_client::MigrationReviewTarget::Disposition {
                    object_group_id,
                    disposition_version,
                } => {
                    let state = app.work.current_state.as_ref().unwrap();
                    match &state.migration_dispositions[&task_id][&object_group_id].status {
                        labello_domain::MigrationDispositionStatus::Annotated {
                            skeleton_annotation_id,
                            skeleton_version,
                        } => {
                            assert!(
                                matches!(context.phase, ReviewContextPhase::Object { annotation_version: Some(version), disposition_version: Some(disposition), .. } if version == *skeleton_version && disposition == disposition_version)
                            );
                            ReviewTarget::AnnotationVersion {
                                annotation_id: skeleton_annotation_id.clone(),
                                version: *skeleton_version,
                            }
                        }
                        labello_domain::MigrationDispositionStatus::Excluded { .. } => {
                            assert!(
                                matches!(context.phase, ReviewContextPhase::Object { annotation_version: None, disposition_version: Some(version), .. } if version == disposition_version)
                            );
                            assert!(context.accessible_summary().contains("Excluded object"));
                            ReviewTarget::MigrationDisposition {
                                task_id,
                                object_group_id,
                                disposition_version,
                            }
                        }
                        labello_domain::MigrationDispositionStatus::Pending => {
                            panic!("pending object is not reviewable")
                        }
                    }
                }
                labello_client::MigrationReviewTarget::Discovered {
                    annotation_id,
                    version,
                } => {
                    assert!(
                        matches!(context.phase, ReviewContextPhase::Object { annotation_version: Some(persisted), disposition_version: None, .. } if persisted == version)
                    );
                    assert!(context.accessible_summary().contains("Discovered skeleton"));
                    // A historical object without positioned points remains the exact
                    // review target even though refocus uses the full image.
                    if annotation_id == labello_domain::AnnotationId::from("discovered-object-2") {
                        assert!(app.refocus_annotation().is_none());
                    }
                    ReviewTarget::AnnotationVersion {
                        annotation_id,
                        version,
                    }
                }
                labello_client::MigrationReviewTarget::Confirmation { .. } => {
                    assert!(matches!(
                        context.phase,
                        ReviewContextPhase::FullImage { migration: true }
                    ));
                    assert!(!context.accessible_summary().contains("version"));
                    assert!(!context.phase_label().starts_with("Object"));
                    break;
                }
            };
            let state = app.work.current_state.as_mut().unwrap();
            let timestamp = now();
            state
                .apply_event(&EventLogEntry::new(
                    state.current_sequence + 1,
                    state.image_id.clone(),
                    app.config.user_id.clone(),
                    DatasetRole::Reviewer,
                    timestamp,
                    EventPayload::ReviewRecorded {
                        review: ReviewRecord {
                            review_id: ReviewId::from(format!("context-review-{index}")),
                            target,
                            reviewer_user_id: app.config.user_id.clone(),
                            decision: ReviewDecision::Approved,
                            timestamp,
                            comment: None,
                        },
                    },
                ))
                .unwrap();
            app.sync_manual_migration();
        }
        assert!(matches!(
            observed.last(),
            Some(ReviewContextPhase::FullImage { migration: true })
        ));
        let numbers = observed
            .iter()
            .filter_map(|phase| match phase {
                ReviewContextPhase::Object { number, total, .. } => Some((*number, *total)),
                ReviewContextPhase::FullImage { .. } => None,
            })
            .collect::<Vec<_>>();
        assert!(!numbers.is_empty());
        assert!(
            numbers
                .windows(2)
                .all(|pair| pair[1].0 == pair[0].0 + 1 && pair[1].1 == pair[0].1)
        );
        assert_eq!(
            numbers.last().map(|(number, total)| number == total),
            Some(true)
        );
    }
}
