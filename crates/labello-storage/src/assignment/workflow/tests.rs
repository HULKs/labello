use super::*;
use labello_domain::{AnnotationType, AnnotationVersion, BoundingBox, ClassId, ReviewId};

fn context(assignment: &Assignment) -> AssignmentContext<'_> {
    AssignmentContext {
        assignment_id: &assignment.assignment_id,
        image_id: &assignment.image_id,
        task_id: &assignment.task_id,
        kind: assignment.kind.clone(),
    }
}

#[tokio::test]
async fn object_prefetch_keeps_distinct_items_on_one_image_and_respects_exclusions() {
    let (_temp, repo, task, users) = crate::assignment::tests::annotation_repo(1, &["a"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.bounding_box_visibility.iou_threshold = 1.0;
    metadata.preload_queue_size = 2;
    repo.save_dataset(&metadata).await.unwrap();
    for id in ["first", "second", "third"] {
        seed(&repo, &task, &users[0], "img_0", id).await;
    }
    let selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let mut excluded = Vec::new();
    let first = repo
        .claim_workflow_item(&users[0], &selection, &excluded)
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[0], context(&first))
        .await
        .unwrap();
    excluded.push(WorkflowItemRef {
        image_id: first.image_id.clone(),
        item: state.workflow_assignments[&first.assignment_id]
            .item
            .clone(),
    });
    for _ in 0..2 {
        let next = repo
            .claim_workflow_item_with_prefetch(&users[0], &selection, &excluded, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.image_id, first.image_id);
        let state = repo.load_image_state(&next.image_id).await.unwrap();
        assert!(!state.workflow_seen.contains_key(&next.assignment_id));
        let item = WorkflowItemRef {
            image_id: next.image_id,
            item: state.workflow_assignments[&next.assignment_id].item.clone(),
        };
        assert!(!excluded.contains(&item));
        excluded.push(item);
    }
    assert!(
        repo.claim_workflow_item_with_prefetch(&users[0], &selection, &excluded, true)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn availability_reports_each_pass_and_matches_its_prerequisites() {
    use labello_domain::WorkflowUnavailableReason as Reason;
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["a", "b"]).await;
    seed(&repo, &task, &users[0], "img_0", "box").await;
    let selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let available = repo
        .workflow_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(
        available
            .iter()
            .any(|a| a.selection == selection && a.split && a.available)
    );
    assert!(
        available
            .iter()
            .any(|a| a.selection.variant == WorkflowVariant::Overview
                && a.reason == Some(Reason::ObjectsPending))
    );
    let object = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let unavailable = repo
        .workflow_availability(&users[1], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(
        unavailable
            .iter()
            .any(|a| a.selection == selection && a.reason == Some(Reason::ClaimedByOthers))
    );
    repo.display_workflow_item(&users[0], context(&object))
        .await
        .unwrap();
    repo.apply_annotation_batch(&users[0], context(&object), vec![], true)
        .await
        .unwrap();
    let available = repo
        .workflow_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(
        available
            .iter()
            .any(|a| a.selection == selection && a.reason == Some(Reason::NoObjects))
    );
    assert!(
        available
            .iter()
            .any(|a| a.selection.variant == WorkflowVariant::Overview && a.available)
    );
}

#[tokio::test]
async fn history_preserves_forward_reservations_and_releases_them_on_departure() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(32, &["a", "b"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.workflow_queue.history_depth = 2;
    repo.save_dataset(&metadata).await.unwrap();
    let selection = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    let mut visits = Vec::new();
    let mut excluded = Vec::new();
    for _ in 0..3 {
        let assignment = repo
            .claim_workflow_item(&users[0], &selection, &excluded)
            .await
            .unwrap()
            .unwrap();
        repo.display_workflow_item(&users[0], context(&assignment))
            .await
            .unwrap();
        excluded.push(WorkflowItemRef {
            image_id: assignment.image_id.clone(),
            item: WorkflowItem::Overview,
        });
        visits.push(assignment);
    }
    let history = repo.workflow_history(&users[0], &selection).await.unwrap();
    assert_eq!(
        history.iter().map(|e| &e.assignment_id).collect::<Vec<_>>(),
        visits
            .iter()
            .rev()
            .map(|a| &a.assignment_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        repo.reopen_workflow_item(&users[0], context(&visits[1]))
            .await
            .unwrap()
            .assignment_id,
        visits[1].assignment_id
    );
    assert_eq!(
        repo.reopen_workflow_item(&users[0], context(&visits[0]))
            .await
            .unwrap()
            .assignment_id,
        visits[0].assignment_id
    );
    assert_eq!(
        repo.reopen_workflow_item(&users[0], context(&visits[2]))
            .await
            .unwrap()
            .assignment_id,
        visits[2].assignment_id
    );
    assert!(
        repo.reopen_workflow_item(&users[1], context(&visits[1]))
            .await
            .is_err()
    );
    assert_eq!(
        repo.workflow_history(&users[0], &selection).await.unwrap(),
        history
    );
    let fourth = repo
        .claim_workflow_item(&users[0], &selection, &excluded)
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[0], context(&fourth))
        .await
        .unwrap();
    assert!(
        repo.reopen_workflow_item(&users[0], context(&visits[0]))
            .await
            .is_err()
    );
    assert_eq!(
        repo.load_image_state(&visits[0].image_id)
            .await
            .unwrap()
            .assignments
            .last()
            .unwrap()
            .status,
        AssignmentStatus::Cancelled
    );
    let other = repo
        .claim_workflow_item(&users[1], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    excluded.push(WorkflowItemRef {
        image_id: fourth.image_id.clone(),
        item: WorkflowItem::Overview,
    });
    let unseen = repo
        .claim_workflow_item_with_prefetch(&users[0], &selection, &excluded, true)
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[0], context(&fourth))
        .await
        .unwrap();
    let prefetched = repo.load_image_state(&unseen.image_id).await.unwrap();
    assert!(!prefetched.workflow_seen.contains_key(&unseen.assignment_id));
    assert!(prefetched.assignments.iter().any(|a| {
        a.assignment_id == unseen.assignment_id && a.status == AssignmentStatus::Active
    }));
    repo.workflow_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    repo.reset_image_state_load_count();
    repo.leave_workflow(&users[0], &selection).await.unwrap();
    assert!(
        repo.image_state_load_count() <= 8,
        "departure must reload only retained and unseen reservation images"
    );
    for visit in visits.iter().chain([&fourth, &unseen]) {
        assert!(
            repo.load_image_state(&visit.image_id)
                .await
                .unwrap()
                .assignments
                .iter()
                .all(|a| a.assigned_to != users[0] || a.status != AssignmentStatus::Active)
        );
    }
    let other_state = repo.load_image_state(&other.image_id).await.unwrap();
    assert!(other_state.assignments.iter().any(|a| {
        a.assignment_id == other.assignment_id && a.status == AssignmentStatus::Active
    }));
    let reacquired = repo
        .reopen_workflow_item(&users[0], context(&visits[1]))
        .await
        .unwrap();
    repo.display_workflow_item(&users[0], context(&reacquired))
        .await
        .unwrap();
    assert_eq!(
        repo.workflow_history(&users[0], &selection)
            .await
            .unwrap()
            .len(),
        3
    );
    let state = repo.load_image_state(&reacquired.image_id).await.unwrap();
    assert_eq!(
        state.workflow_assignments[&reacquired.assignment_id].source_assignment_id,
        Some(visits[1].assignment_id.clone())
    );
}

#[tokio::test]
async fn workflow_advancing_reloads_only_changed_images() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(32, &["a", "b"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.role_assignments[1]
        .roles
        .insert(DatasetRole::Reviewer);
    repo.save_dataset(&metadata).await.unwrap();
    for image in ["img_8", "img_9"] {
        seed(&repo, &task, &users[0], image, "box").await;
    }
    // Put finished images before the two remaining candidates in queue order.
    for image in metadata
        .images
        .keys()
        .filter(|id| id.as_str() != "img_8" && id.as_str() != "img_9")
    {
        repo.append_payload(
            image,
            &Actor {
                user_id: users[0].clone(),
                role: DatasetRole::Annotator,
            },
            EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: task.clone(),
                    status: TaskStatus::Completed,
                    outcome: None,
                    assigned_to: None,
                    completed_by: Some(users[0].clone()),
                    completed_at: Some(labello_domain::now()),
                    updated_at: labello_domain::now(),
                },
            },
        )
        .await
        .unwrap();
    }
    repo.prepare_review_history().await.unwrap();
    for (kind, user) in [
        (AssignmentKind::Annotation, &users[0]),
        (AssignmentKind::Review, &users[1]),
    ] {
        for variant in [WorkflowVariant::Objects, WorkflowVariant::Overview] {
            let selection = WorkflowSelection {
                task_id: task.clone(),
                kind: kind.clone(),
                variant,
            };
            repo.workflow_availability(user, kind.clone())
                .await
                .unwrap();
            for _ in 0..2 {
                repo.reset_image_state_load_count();
                repo.reset_event_load_count();
                let item = repo
                    .claim_workflow_item(user, &selection, &[])
                    .await
                    .unwrap()
                    .unwrap();
                assert!(
                    repo.image_state_load_count() <= 4,
                    "{kind:?} {variant:?} claim must skip finished images: {} loads",
                    repo.image_state_load_count()
                );
                assert!(
                    repo.event_load_count() <= 6,
                    "{kind:?} {variant:?} claim must skip finished histories: {} loads",
                    repo.event_load_count()
                );
                // Isolate warm navigation from the separate score-window initialization.
                repo.scoring_focus(labello_domain::now()).await.unwrap();
                repo.reset_image_state_load_count();
                repo.reset_event_load_count();
                if kind == AssignmentKind::Review {
                    approve_item(&repo, user, &item).await;
                } else {
                    repo.display_workflow_item(user, context(&item))
                        .await
                        .unwrap();
                    repo.apply_annotation_batch(user, context(&item), vec![], true)
                        .await
                        .unwrap();
                }
                assert!(
                    repo.image_state_load_count() <= 6,
                    "{kind:?} {variant:?} advance must not reload unrelated images: {} loads",
                    repo.image_state_load_count()
                );
                assert!(
                    repo.event_load_count() <= 10,
                    "{kind:?} {variant:?} advance must not reread unrelated histories: {} loads",
                    repo.event_load_count()
                );
            }
        }
    }
}

#[tokio::test]
async fn completed_object_history_does_not_duplicate_visit_or_completion() {
    let (_temp, repo, task, users) = crate::assignment::tests::annotation_repo(1, &["a"]).await;
    seed(&repo, &task, &users[0], "img_0", "box").await;
    let selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let first = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[0], context(&first))
        .await
        .unwrap();
    repo.apply_annotation_batch(&users[0], context(&first), vec![], true)
        .await
        .unwrap();
    let previous = repo
        .reopen_workflow_item(&users[0], context(&first))
        .await
        .unwrap();
    repo.display_workflow_item(&users[0], context(&previous))
        .await
        .unwrap();
    repo.apply_annotation_batch(&users[0], context(&previous), vec![], true)
        .await
        .unwrap();
    assert_eq!(
        repo.workflow_history(&users[0], &selection)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        repo.load_image_state(&first.image_id)
            .await
            .unwrap()
            .workflow_pending_objects(&task)
            .is_empty()
    );
    let stats = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        stats[&users[0]]
            .history
            .iter()
            .map(|d| d.labeled)
            .sum::<usize>(),
        1
    );
}

#[tokio::test]
async fn overview_cap_counts_images_and_reserves_capacity_before_objects_finish() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(2, &["a", "b"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.workflow_queue.max_pending_overviews = Some(1);
    repo.save_dataset(&metadata).await.unwrap();
    seed(&repo, &task, &users[0], "img_0", "a").await;
    seed(&repo, &task, &users[0], "img_1", "b").await;
    let selection = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let object = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    assert!(
        repo.claim_workflow_item(&users[1], &selection, &[])
            .await
            .unwrap()
            .is_none()
    );
    repo.display_workflow_item(&users[0], context(&object))
        .await
        .unwrap();
    repo.apply_annotation_batch(&users[0], context(&object), vec![], true)
        .await
        .unwrap();
    assert!(
        repo.claim_workflow_item(&users[1], &selection, &[])
            .await
            .unwrap()
            .is_none()
    );
    let overview = repo
        .claim_workflow_item(
            &users[1],
            &WorkflowSelection {
                variant: WorkflowVariant::Overview,
                ..selection.clone()
            },
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[1], context(&overview))
        .await
        .unwrap();
    repo.apply_annotation_batch(&users[1], context(&overview), vec![], true)
        .await
        .unwrap();
    let next = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    assert_ne!(next.image_id, object.image_id);
}

#[tokio::test]
async fn unfinished_skeleton_keypoints_are_durable_without_confirming_an_annotation() {
    use labello_domain::{
        KeypointAnnotation, KeypointSpec, KeypointState, NormalizedPoint, SkeletonGeometry,
        SkeletonSpec,
    };
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["a", "b"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.tasks[0].annotation_type = AnnotationType::Skeleton;
    metadata.tasks[0].skeleton = Some(SkeletonSpec {
        keypoints: ["head", "foot"]
            .map(|name| KeypointSpec {
                name: name.into(),
                required: true,
            })
            .into(),
        edges: vec![],
        allow_hidden: false,
        allow_absent: false,
    });
    repo.save_dataset(&metadata).await.unwrap();
    let point = |name: &str| KeypointAnnotation {
        name: name.into(),
        state: KeypointState::Visible,
        point: Some(NormalizedPoint { x: 0.2, y: 0.3 }),
    };
    repo.append_payload(
        &ImageId::from("img_0"),
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::AnnotationVersionCreated {
            annotation: AnnotationVersion::native(
                AnnotationId::from("pose"),
                task.clone(),
                ClassId::from("person"),
                AnnotationType::Skeleton,
                AnnotationGeometry::Skeleton(SkeletonGeometry {
                    keypoints: vec![point("head"), point("foot")],
                }),
                users[0].clone(),
                labello_domain::now(),
            ),
            previous_version: None,
            reason: None,
        },
    )
    .await
    .unwrap();
    let selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let first = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[0], context(&first))
        .await
        .unwrap();
    let partial = AnnotationGeometry::Skeleton(SkeletonGeometry {
        keypoints: vec![point("head")],
    });
    let saved = repo
        .save_workflow_draft(&users[0], context(&first), partial.clone(), 0)
        .await
        .unwrap();
    assert_eq!(
        repo.save_workflow_draft(&users[0], context(&first), partial.clone(), 0)
            .await
            .unwrap(),
        saved
    );
    repo.release_assignment(
        &users[0],
        &first.assignment_id,
        &first.image_id,
        &task,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    let second = repo
        .claim_workflow_item(&users[1], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let resumed = repo
        .display_workflow_item(&users[1], context(&second))
        .await
        .unwrap();
    let item = &resumed.workflow_assignments[&second.assignment_id].item;
    assert_eq!(
        resumed.workflow_object_draft(&task, item).unwrap().geometry,
        partial
    );
    assert!(resumed.workflow_confirmations.is_empty());
    assert_eq!(
        resumed
            .current_annotation(&AnnotationId::from("pose"))
            .unwrap()
            .version,
        1
    );
    assert!(
        repo.save_workflow_draft(&users[1], context(&second), partial, 0)
            .await
            .is_err()
    );
    let stats = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        stats
            .values()
            .flat_map(|s| &s.history)
            .map(|d| d.labeled)
            .sum::<usize>(),
        0
    );
    let mut annotation = resumed
        .current_annotation(&AnnotationId::from("pose"))
        .unwrap()
        .clone();
    annotation.version += 1;
    annotation.author_user_id = users[1].clone();
    repo.apply_annotation_batch(
        &users[1],
        context(&second),
        vec![EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version: Some(1),
            reason: None,
        }],
        true,
    )
    .await
    .unwrap();
    let stats = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        stats[&users[1]]
            .history
            .iter()
            .map(|d| d.labeled)
            .sum::<usize>(),
        1,
        "a whole skeleton counts once for its finisher"
    );
}

async fn seed(repo: &DatasetRepository, task: &TaskId, user: &UserId, image: &str, id: &str) {
    repo.append_payload(
        &ImageId::from(image),
        &Actor {
            user_id: user.clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::AnnotationVersionCreated {
            annotation: AnnotationVersion::native(
                AnnotationId::from(id),
                task.clone(),
                ClassId::from("person"),
                AnnotationType::BoundingBox,
                AnnotationGeometry::BoundingBox(BoundingBox {
                    x: if id == "b" { 0.6 } else { 0.1 },
                    y: 0.1,
                    width: 0.2,
                    height: 0.2,
                }),
                user.clone(),
                labello_domain::now(),
            ),
            previous_version: None,
            reason: None,
        },
    )
    .await
    .unwrap();
}

async fn approve_item(
    repo: &DatasetRepository,
    user: &UserId,
    assignment: &Assignment,
) -> ImageState {
    let state = repo
        .display_workflow_item(user, context(assignment))
        .await
        .unwrap();
    repo.confirm_workflow_review(
        user,
        context(assignment),
        ReviewRecord {
            review_id: ReviewId::generate(),
            target: state.workflow_assignments[&assignment.assignment_id]
                .review_target
                .clone()
                .unwrap(),
            reviewer_user_id: user.clone(),
            decision: ReviewDecision::Approved,
            timestamp: labello_domain::now(),
            comment: None,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn workflow_corrections_preserve_approvals_and_only_additions_need_objects() {
    use labello_domain::{CorrectionId, ReviewCorrectionChange, ReviewCorrectionSubmission};
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["author", "reviewer", "overview"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    for role in &mut metadata.role_assignments {
        role.roles.insert(DatasetRole::Reviewer);
    }
    repo.save_dataset(&metadata).await.unwrap();
    seed(&repo, &task, &users[0], "img_0", "a").await;
    seed(&repo, &task, &users[0], "img_0", "b").await;
    let annotation = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    for variant in [
        WorkflowVariant::Objects,
        WorkflowVariant::Objects,
        WorkflowVariant::Overview,
    ] {
        let item = repo
            .claim_workflow_item(
                &users[0],
                &WorkflowSelection {
                    variant,
                    ..annotation.clone()
                },
                &[],
            )
            .await
            .unwrap()
            .unwrap();
        repo.display_workflow_item(&users[0], context(&item))
            .await
            .unwrap();
        repo.apply_annotation_batch(&users[0], context(&item), vec![], true)
            .await
            .unwrap();
    }
    let review = WorkflowSelection {
        kind: AssignmentKind::Review,
        ..annotation
    };
    let first = repo
        .claim_workflow_item(&users[1], &review, &[])
        .await
        .unwrap()
        .unwrap();
    approve_item(&repo, &users[1], &first).await;
    let second = repo
        .claim_workflow_item(&users[1], &review, &[])
        .await
        .unwrap()
        .unwrap();
    let before = repo
        .display_workflow_item(&users[1], context(&second))
        .await
        .unwrap();
    let ReviewTarget::AnnotationVersion {
        annotation_id,
        version,
    } = before.workflow_assignments[&second.assignment_id]
        .review_target
        .clone()
        .unwrap()
    else {
        panic!("object review")
    };
    let moved = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.4,
        y: 0.4,
        width: 0.2,
        height: 0.2,
    });
    let submission = ReviewCorrectionSubmission {
        correction_id: CorrectionId::generate(),
        round: before.review_round(&task).unwrap().clone(),
        target_fingerprint: before.review_target_fingerprint(&metadata.tasks[0]),
        changes: vec![ReviewCorrectionChange::Edit {
            annotation_id,
            expected_version: version,
            geometry: moved.clone(),
        }],
        reason: None,
    };
    let corrected = repo
        .submit_review_corrections(&users[1], context(&second), submission.clone())
        .await
        .unwrap();
    assert_eq!(
        repo.submit_review_corrections(&users[1], context(&second), submission)
            .await
            .unwrap(),
        corrected
    );
    assert!(
        corrected
            .workflow_pending_reviews(&metadata.tasks[0])
            .unwrap()
            .is_empty()
    );
    assert_eq!(corrected.task_states[&task].status, TaskStatus::Submitted);
    let overview_selection = WorkflowSelection {
        variant: WorkflowVariant::Overview,
        ..review.clone()
    };
    let overview = repo
        .claim_workflow_item(&users[2], &overview_selection, &[])
        .await
        .unwrap()
        .unwrap();
    let before = repo
        .display_workflow_item(&users[2], context(&overview))
        .await
        .unwrap();
    let submission = ReviewCorrectionSubmission {
        correction_id: CorrectionId::generate(),
        round: before.review_round(&task).unwrap().clone(),
        target_fingerprint: before.review_target_fingerprint(&metadata.tasks[0]),
        changes: vec![ReviewCorrectionChange::Add {
            annotation_id: AnnotationId::from("missing"),
            class_id: ClassId::from("person"),
            geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.1,
                y: 0.7,
                width: 0.2,
                height: 0.2,
            }),
        }],
        reason: None,
    };
    let corrected = repo
        .submit_review_corrections(&users[2], context(&overview), submission)
        .await
        .unwrap();
    assert_eq!(
        corrected
            .workflow_pending_reviews(&metadata.tasks[0])
            .unwrap(),
        vec![ReviewTarget::AnnotationVersion {
            annotation_id: AnnotationId::from("missing"),
            version: 1
        }]
    );
    assert!(
        repo.claim_workflow_item(&users[1], &overview_selection, &[])
            .await
            .unwrap()
            .is_none()
    );
    let added = repo
        .claim_workflow_item(&users[1], &review, &[])
        .await
        .unwrap()
        .unwrap();
    approve_item(&repo, &users[1], &added).await;
    let final_overview = repo
        .claim_workflow_item(&users[1], &overview_selection, &[])
        .await
        .unwrap()
        .unwrap();
    let final_state = approve_item(&repo, &users[1], &final_overview).await;
    assert_eq!(final_state.task_states[&task].status, TaskStatus::Completed);
    let events = repo.load_events(&first.image_id).await.unwrap();
    for boundary in 0..=events.len() {
        labello_domain::rebuild_state(first.image_id.clone(), &events[..boundary]).unwrap();
    }
    assert_eq!(
        repo.rebuild_image_state(&first.image_id).await.unwrap(),
        final_state
    );
    let mut projection = labello_domain::stats::scoring::ScoringProjection::default();
    projection.record_image(&final_state, &events);
    let mut scores = std::collections::BTreeMap::new();
    projection.finish(&mut scores, &[]);
    assert_eq!(
        scores[&users[1]]
            .history
            .iter()
            .map(|d| d.score.corrections)
            .sum::<i64>(),
        400
    );
    let stats = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        stats[&users[0]]
            .history
            .iter()
            .map(|d| d.labeled)
            .sum::<usize>(),
        3
    );
    assert_eq!(
        stats[&users[1]]
            .history
            .iter()
            .map(|d| d.reviewed)
            .sum::<usize>(),
        4
    );
    assert_eq!(
        stats[&users[2]]
            .history
            .iter()
            .map(|d| d.reviewed)
            .sum::<usize>(),
        1
    );
}

#[tokio::test]
async fn self_review_fallback_checks_other_workflows_and_is_rechecked_at_submission() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(2, &["a", "b"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    for role in &mut metadata.role_assignments {
        role.roles.insert(DatasetRole::Reviewer);
    }
    let mut other = metadata.tasks[0].clone();
    other.task_id = TaskId::from("other_boxes");
    metadata.tasks.push(other.clone());
    repo.save_dataset(&metadata).await.unwrap();
    seed(&repo, &task, &users[0], "img_0", "own").await;
    let submitted = |task: TaskId, user: UserId| EventPayload::TaskStateChanged {
        task_state: TaskState {
            task_id: task,
            status: TaskStatus::Submitted,
            outcome: None,
            assigned_to: None,
            completed_by: Some(user),
            completed_at: Some(labello_domain::now()),
            updated_at: labello_domain::now(),
        },
    };
    repo.append_payload(
        &ImageId::from("img_0"),
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        submitted(task.clone(), users[0].clone()),
    )
    .await
    .unwrap();
    let selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Review,
        variant: WorkflowVariant::Objects,
    };
    let fallback = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let displayed = repo
        .display_workflow_item(&users[0], context(&fallback))
        .await
        .unwrap();
    assert!(displayed.workflow_assignments[&fallback.assignment_id].review_exception);
    seed(&repo, &other.task_id, &users[1], "img_1", "independent").await;
    repo.append_payload(
        &ImageId::from("img_1"),
        &Actor {
            user_id: users[1].clone(),
            role: DatasetRole::Annotator,
        },
        submitted(other.task_id.clone(), users[1].clone()),
    )
    .await
    .unwrap();
    let denied = repo
        .confirm_workflow_review(
            &users[0],
            context(&fallback),
            ReviewRecord {
                review_id: ReviewId::generate(),
                target: displayed.workflow_assignments[&fallback.assignment_id]
                    .review_target
                    .clone()
                    .unwrap(),
                reviewer_user_id: users[0].clone(),
                decision: ReviewDecision::Approved,
                timestamp: labello_domain::now(),
                comment: None,
            },
        )
        .await;
    assert!(denied.is_err());
    assert_eq!(
        repo.load_image_state(&fallback.image_id).await.unwrap(),
        displayed
    );
    repo.release_assignment(
        &users[0],
        &fallback.assignment_id,
        &fallback.image_id,
        &task,
        AssignmentKind::Review,
    )
    .await
    .unwrap();
    assert!(
        repo.claim_workflow_item(&users[0], &selection, &[])
            .await
            .unwrap()
            .is_none()
    );
    let independent = repo
        .claim_workflow_item(
            &users[0],
            &WorkflowSelection {
                task_id: other.task_id,
                ..selection
            },
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[0], context(&independent))
        .await
        .unwrap();
    assert!(!state.workflow_assignments[&independent.assignment_id].review_exception);
}

#[tokio::test]
async fn objects_finish_individually_before_overview_and_retry_does_not_publish_twice() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(2, &["a", "b"]).await;
    seed(&repo, &task, &users[0], "img_0", "a").await;
    seed(&repo, &task, &users[0], "img_1", "b").await;
    let objects = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let overview = WorkflowSelection {
        variant: WorkflowVariant::Overview,
        ..objects.clone()
    };
    let first = repo
        .claim_workflow_item(&users[0], &objects, &[])
        .await
        .unwrap()
        .unwrap();
    assert!(
        repo.claim_workflow_item(&users[1], &overview, &[])
            .await
            .unwrap()
            .is_none()
    );
    repo.display_workflow_item(&users[0], context(&first))
        .await
        .unwrap();
    let finished = repo
        .apply_annotation_batch(&users[0], context(&first), vec![], true)
        .await
        .unwrap();
    assert_eq!(finished.task_states[&task].status, TaskStatus::InProgress);
    assert!(finished.workflow_pending_objects(&task).is_empty());
    let retry = repo
        .apply_annotation_batch(&users[0], context(&first), vec![], true)
        .await
        .unwrap();
    assert_eq!(finished, retry);
    let second = repo
        .claim_workflow_item(&users[0], &objects, &[])
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.image_id, second.image_id);
    let overview_assignment = repo
        .claim_workflow_item(&users[1], &overview, &[])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(overview_assignment.image_id, first.image_id);
    repo.display_workflow_item(&users[1], context(&overview_assignment))
        .await
        .unwrap();
    let submitted = repo
        .apply_annotation_batch(&users[1], context(&overview_assignment), vec![], true)
        .await
        .unwrap();
    assert_eq!(submitted.task_states[&task].status, TaskStatus::Submitted);
    let contributors = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        contributors[&users[0]]
            .history
            .iter()
            .map(|day| day.labeled)
            .sum::<usize>(),
        1,
        "Objects confirmation earns streak credit before Overview"
    );
    assert_eq!(
        contributors[&users[1]]
            .history
            .iter()
            .map(|day| day.labeled)
            .sum::<usize>(),
        1,
        "Overview receipt and task submission count only once"
    );
    let restarted = DatasetRepository::new(_temp.path());
    assert_eq!(
        restarted
            .dataset_stats()
            .await
            .unwrap()
            .contributors
            .unwrap(),
        contributors
    );
    assert_eq!(
        submitted,
        labello_domain::rebuild_state(
            first.image_id.clone(),
            &repo.load_events(&first.image_id).await.unwrap()
        )
        .unwrap()
    );
}

#[tokio::test]
async fn empty_images_have_only_overview_and_competing_claims_cannot_share_it() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["a", "b"]).await;
    let objects = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    assert!(
        repo.claim_workflow_item(&users[0], &objects, &[])
            .await
            .unwrap()
            .is_none()
    );
    let overview = WorkflowSelection {
        variant: WorkflowVariant::Overview,
        ..objects
    };
    let (a, b) = tokio::join!(
        repo.claim_workflow_item(&users[0], &overview, &[]),
        repo.claim_workflow_item(&users[1], &overview, &[])
    );
    assert_ne!(a.unwrap().is_some(), b.unwrap().is_some());
}

#[tokio::test]
async fn skip_preserves_saved_geometry_and_releases_object_to_another_worker() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["a", "b"]).await;
    seed(&repo, &task, &users[0], "img_0", "a").await;
    let selection = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let first = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[0], context(&first))
        .await
        .unwrap();
    let mut edited = state
        .current_annotation(&AnnotationId::from("a"))
        .unwrap()
        .clone();
    edited.version += 1;
    edited.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.2,
        width: 0.3,
        height: 0.3,
    });
    repo.apply_annotation_batch(
        &users[0],
        context(&first),
        vec![EventPayload::AnnotationVersionCreated {
            annotation: edited.clone(),
            previous_version: Some(1),
            reason: None,
        }],
        false,
    )
    .await
    .unwrap();
    repo.release_assignment(
        &users[0],
        &first.assignment_id,
        &first.image_id,
        &first.task_id,
        first.kind.clone(),
    )
    .await
    .unwrap();
    let next = repo
        .claim_workflow_item(&users[1], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[1], context(&next))
        .await
        .unwrap();
    assert_eq!(
        state.current_annotation(&AnnotationId::from("a")),
        Some(&edited)
    );
    assert!(state.workflow_confirmations.is_empty());
    repo.apply_annotation_batch(&users[1], context(&next), vec![], true)
        .await
        .unwrap();
    let stats = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        stats
            .get(&users[0])
            .map_or(0, |s| s.history.iter().map(|d| d.labeled).sum::<usize>()),
        0
    );
    assert_eq!(
        stats[&users[1]]
            .history
            .iter()
            .map(|d| d.labeled)
            .sum::<usize>(),
        1
    );
}

#[tokio::test]
async fn review_waits_for_annotation_overview_then_objects_and_records_final_fallback() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["author", "reviewer"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    for role in &mut metadata.role_assignments {
        role.roles.insert(DatasetRole::Reviewer);
    }
    repo.save_dataset(&metadata).await.unwrap();
    seed(&repo, &task, &users[0], "img_0", "a").await;
    let annotation = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Objects,
    };
    let review = WorkflowSelection {
        kind: AssignmentKind::Review,
        ..annotation.clone()
    };
    let item = repo
        .claim_workflow_item(&users[0], &annotation, &[])
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[0], context(&item))
        .await
        .unwrap();
    repo.apply_annotation_batch(&users[0], context(&item), vec![], true)
        .await
        .unwrap();
    assert!(
        repo.claim_workflow_item(&users[1], &review, &[])
            .await
            .unwrap()
            .is_none()
    );
    let overview = repo
        .claim_workflow_item(
            &users[0],
            &WorkflowSelection {
                variant: WorkflowVariant::Overview,
                ..annotation
            },
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[0], context(&overview))
        .await
        .unwrap();
    repo.apply_annotation_batch(&users[0], context(&overview), vec![], true)
        .await
        .unwrap();
    let item = repo
        .claim_workflow_item(&users[1], &review, &[])
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[1], context(&item))
        .await
        .unwrap();
    assert!(!state.workflow_assignments[&item.assignment_id].review_exception);
    let decision = ReviewRecord {
        review_id: ReviewId::generate(),
        target: state.workflow_assignments[&item.assignment_id]
            .review_target
            .clone()
            .unwrap(),
        reviewer_user_id: users[1].clone(),
        decision: ReviewDecision::Approved,
        timestamp: labello_domain::now(),
        comment: None,
    };
    let confirmed = repo
        .confirm_workflow_review(&users[1], context(&item), decision.clone())
        .await
        .unwrap();
    assert_eq!(
        repo.confirm_workflow_review(&users[1], context(&item), decision)
            .await
            .unwrap(),
        confirmed
    );
    let overview = repo
        .claim_workflow_item(
            &users[1],
            &WorkflowSelection {
                variant: WorkflowVariant::Overview,
                ..review
            },
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[1], context(&overview))
        .await
        .unwrap();
    assert!(state.workflow_assignments[&overview.assignment_id].review_exception);
    let state = repo
        .confirm_workflow_review(
            &users[1],
            context(&overview),
            ReviewRecord {
                review_id: ReviewId::generate(),
                target: state.workflow_assignments[&overview.assignment_id]
                    .review_target
                    .clone()
                    .unwrap(),
                reviewer_user_id: users[1].clone(),
                decision: ReviewDecision::Approved,
                timestamp: labello_domain::now(),
                comment: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.task_states[&task].status, TaskStatus::Completed);
    let stats = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        stats[&users[0]]
            .history
            .iter()
            .map(|d| d.labeled)
            .sum::<usize>(),
        2
    );
    assert_eq!(
        stats[&users[1]]
            .history
            .iter()
            .map(|d| d.reviewed)
            .sum::<usize>(),
        2
    );
    let reopened = repo
        .reopen_workflow_item(&users[1], context(&overview))
        .await
        .unwrap();
    approve_item(&repo, &users[1], &reopened).await;
    let repeated = repo.dataset_stats().await.unwrap().contributors.unwrap();
    assert_eq!(
        repeated[&users[1]]
            .history
            .iter()
            .map(|d| d.reviewed)
            .sum::<usize>(),
        2
    );
}

#[tokio::test]
async fn partial_overview_edits_survive_skip_reassignment_and_replay_without_completion() {
    use labello_domain::{
        KeypointAnnotation, KeypointSpec, KeypointState, NormalizedPoint, ReviewCorrectionChange,
        SkeletonGeometry, SkeletonSpec, WorkflowEdits,
    };
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["first", "finisher"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.tasks[0].annotation_type = AnnotationType::Skeleton;
    metadata.tasks[0].skeleton = Some(SkeletonSpec {
        keypoints: ["a", "b"]
            .into_iter()
            .map(|name| KeypointSpec {
                name: name.into(),
                required: true,
            })
            .collect(),
        edges: vec![],
        allow_hidden: true,
        allow_absent: false,
    });
    repo.save_dataset(&metadata).await.unwrap();
    let selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    let first = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let edits = WorkflowEdits {
        changes: vec![ReviewCorrectionChange::Add {
            annotation_id: "partial-skeleton".into(),
            class_id: metadata.tasks[0].class_ids[0].clone(),
            geometry: AnnotationGeometry::Skeleton(SkeletonGeometry {
                keypoints: vec![KeypointAnnotation {
                    name: "a".into(),
                    state: KeypointState::Visible,
                    point: Some(NormalizedPoint { x: 0.3, y: 0.4 }),
                }],
            }),
        }],
        reason: None,
    };
    assert!(
        repo.save_workflow_edits(&users[0], context(&first), edits.clone(), 0)
            .await
            .is_err()
    );
    repo.display_workflow_item(&users[0], context(&first))
        .await
        .unwrap();
    assert!(
        repo.save_workflow_edits(&users[1], context(&first), edits.clone(), 0)
            .await
            .is_err()
    );
    let saved = repo
        .save_workflow_edits(&users[0], context(&first), edits.clone(), 0)
        .await
        .unwrap();
    assert_eq!(
        repo.save_workflow_edits(&users[0], context(&first), edits.clone(), 0)
            .await
            .unwrap(),
        saved
    );
    assert!(saved.annotations.is_empty());
    assert!(saved.workflow_confirmations.is_empty());
    let snapshot = repo.create_snapshot().await.unwrap();
    let bytes = repo
        .snapshot_file(
            &snapshot.snapshot_id,
            &format!("annotations/{}/state.json", first.image_id),
        )
        .await
        .unwrap();
    assert_eq!(serde_json::from_slice::<ImageState>(&bytes).unwrap(), saved);
    let offline = repo
        .create_offline_bundle(&users[0], 10, false)
        .await
        .unwrap();
    let decoded: labello_domain::OfflineBundle =
        serde_json::from_slice(&serde_json::to_vec(&offline).unwrap()).unwrap();
    assert!(decoded.images.iter().any(|image| image.state == saved));

    repo.leave_workflow(&users[0], &selection).await.unwrap();
    let next = repo
        .claim_workflow_item(&users[1], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[1], context(&next))
        .await
        .unwrap();
    let draft = state
        .pending_workflow_edits(&task, &WorkflowItem::Overview, &AssignmentKind::Annotation)
        .unwrap();
    assert_eq!(draft.edits, edits);
    assert!(
        repo.save_workflow_edits(&users[1], context(&next), edits.clone(), 0)
            .await
            .is_err()
    );
    let mut finished = edits.clone();
    let ReviewCorrectionChange::Add {
        annotation_id,
        class_id,
        geometry: AnnotationGeometry::Skeleton(skeleton),
    } = &mut finished.changes[0]
    else {
        panic!()
    };
    skeleton.keypoints.push(KeypointAnnotation {
        name: "b".into(),
        state: KeypointState::Visible,
        point: Some(NormalizedPoint { x: 0.6, y: 0.4 }),
    });
    let annotation = AnnotationVersion::native(
        annotation_id.clone(),
        task.clone(),
        class_id.clone(),
        AnnotationType::Skeleton,
        AnnotationGeometry::Skeleton(skeleton.clone()),
        users[1].clone(),
        labello_domain::now(),
    );
    let completed = repo
        .apply_annotation_batch(
            &users[1],
            context(&next),
            vec![EventPayload::AnnotationVersionCreated {
                annotation,
                previous_version: None,
                reason: None,
            }],
            true,
        )
        .await
        .unwrap();
    assert!(
        completed
            .pending_workflow_edits(&task, &WorkflowItem::Overview, &AssignmentKind::Annotation)
            .is_none()
    );
    assert_eq!(
        completed
            .current_annotation(&"partial-skeleton".into())
            .unwrap()
            .author_user_id,
        users[1]
    );
    let events = repo.load_events(&next.image_id).await.unwrap();
    assert_eq!(
        labello_domain::rebuild_state(next.image_id.clone(), &events).unwrap(),
        completed
    );
    let encoded = serde_json::to_string(&completed).unwrap();
    assert_eq!(
        serde_json::from_str::<ImageState>(&encoded).unwrap(),
        completed
    );
}

#[tokio::test]
async fn partial_review_edits_are_reassigned_without_publishing_a_correction_or_reward() {
    use labello_domain::{ReviewCorrectionChange, WorkflowEdits};
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["author", "first", "finisher"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    for role in &mut metadata.role_assignments {
        role.roles.insert(DatasetRole::Reviewer);
    }
    repo.save_dataset(&metadata).await.unwrap();
    seed(&repo, &task, &users[0], "img_0", "box").await;
    for variant in [WorkflowVariant::Objects, WorkflowVariant::Overview] {
        let selection = WorkflowSelection {
            task_id: task.clone(),
            kind: AssignmentKind::Annotation,
            variant,
        };
        let item = repo
            .claim_workflow_item(&users[0], &selection, &[])
            .await
            .unwrap()
            .unwrap();
        repo.display_workflow_item(&users[0], context(&item))
            .await
            .unwrap();
        repo.apply_annotation_batch(&users[0], context(&item), vec![], true)
            .await
            .unwrap();
    }
    let selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Review,
        variant: WorkflowVariant::Objects,
    };
    let first = repo
        .claim_workflow_item(&users[1], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[1], context(&first))
        .await
        .unwrap();
    let annotation = state.current_annotation(&"box".into()).unwrap();
    let edits = WorkflowEdits {
        changes: vec![ReviewCorrectionChange::Edit {
            annotation_id: annotation.annotation_id.clone(),
            expected_version: annotation.version,
            geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.3,
                y: 0.3,
                width: 0.2,
                height: 0.2,
            }),
        }],
        reason: Some("partial correction".into()),
    };
    let saved = repo
        .save_workflow_edits(&users[1], context(&first), edits.clone(), 0)
        .await
        .unwrap();
    assert_eq!(
        saved.current_annotation(&annotation.annotation_id),
        Some(annotation)
    );
    assert!(saved.reviews.is_empty());
    repo.leave_workflow(&users[1], &selection).await.unwrap();
    let next = repo
        .claim_workflow_item(&users[2], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[2], context(&next))
        .await
        .unwrap();
    let item = &state.workflow_assignments[&next.assignment_id].item;
    assert_eq!(
        state
            .pending_workflow_edits(&task, item, &AssignmentKind::Review)
            .unwrap()
            .edits,
        edits
    );
    let corrected = repo
        .submit_review_corrections(
            &users[2],
            context(&next),
            labello_domain::ReviewCorrectionSubmission {
                correction_id: labello_domain::CorrectionId::generate(),
                round: state.review_round(&task).unwrap().clone(),
                target_fingerprint: state.review_target_fingerprint(&metadata.tasks[0]),
                changes: edits.changes,
                reason: edits.reason,
            },
        )
        .await
        .unwrap();
    assert!(
        corrected
            .pending_workflow_edits(&task, item, &AssignmentKind::Review)
            .is_none()
    );
    assert_eq!(
        corrected
            .current_annotation(&annotation.annotation_id)
            .unwrap()
            .author_user_id,
        users[2]
    );
    assert!(
        corrected
            .workflow_pending_reviews(&metadata.tasks[0])
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn overview_can_revise_confirmed_objects_and_complete_after_autosave() {
    use labello_domain::{
        KeypointAnnotation, KeypointSpec, KeypointState, NormalizedPoint, SkeletonGeometry,
        SkeletonSpec,
    };
    for skeleton in [false, true] {
        for autosave in [false, true] {
            let (_temp, repo, task, users) =
                crate::assignment::tests::annotation_repo(1, &["author", "overview"]).await;
            let geometry = if skeleton {
                let mut metadata = repo.load_dataset().await.unwrap();
                metadata.tasks[0].annotation_type = AnnotationType::Skeleton;
                metadata.tasks[0].skeleton = Some(SkeletonSpec {
                    keypoints: vec![KeypointSpec {
                        name: "head".into(),
                        required: true,
                    }],
                    edges: vec![],
                    allow_hidden: false,
                    allow_absent: false,
                });
                repo.save_dataset(&metadata).await.unwrap();
                AnnotationGeometry::Skeleton(SkeletonGeometry {
                    keypoints: vec![KeypointAnnotation {
                        name: "head".into(),
                        state: KeypointState::Visible,
                        point: Some(NormalizedPoint { x: 0.2, y: 0.3 }),
                    }],
                })
            } else {
                AnnotationGeometry::BoundingBox(BoundingBox {
                    x: 0.1,
                    y: 0.1,
                    width: 0.2,
                    height: 0.2,
                })
            };
            let mut annotation = AnnotationVersion::native(
                "object".into(),
                task.clone(),
                "person".into(),
                if skeleton {
                    AnnotationType::Skeleton
                } else {
                    AnnotationType::BoundingBox
                },
                geometry,
                users[0].clone(),
                labello_domain::now(),
            );
            repo.append_payload(
                &"img_0".into(),
                &Actor {
                    user_id: users[0].clone(),
                    role: DatasetRole::Annotator,
                },
                EventPayload::AnnotationVersionCreated {
                    annotation: annotation.clone(),
                    previous_version: None,
                    reason: None,
                },
            )
            .await
            .unwrap();
            let mut selection = WorkflowSelection {
                task_id: task.clone(),
                kind: AssignmentKind::Annotation,
                variant: WorkflowVariant::Objects,
            };
            let object = repo
                .claim_workflow_item(&users[0], &selection, &[])
                .await
                .unwrap()
                .unwrap();
            repo.display_workflow_item(&users[0], context(&object))
                .await
                .unwrap();
            let confirmed = repo
                .apply_annotation_batch(&users[0], context(&object), vec![], true)
                .await
                .unwrap();
            selection.variant = WorkflowVariant::Overview;
            let overview = repo
                .claim_workflow_item(&users[1], &selection, &[])
                .await
                .unwrap()
                .unwrap();
            repo.display_workflow_item(&users[1], context(&overview))
                .await
                .unwrap();
            annotation.version += 1;
            annotation.author_user_id = users[1].clone();
            match &mut annotation.geometry {
                AnnotationGeometry::BoundingBox(bbox) => bbox.x += 0.1,
                AnnotationGeometry::Skeleton(pose) => {
                    pose.keypoints[0].point.as_mut().unwrap().x += 0.1
                }
            }
            let result = repo
                .apply_annotation_batch(
                    &users[1],
                    context(&overview),
                    vec![EventPayload::AnnotationVersionCreated {
                        annotation,
                        previous_version: Some(1),
                        reason: None,
                    }],
                    !autosave,
                )
                .await
                .unwrap();
            assert!(result.workflow_pending_objects(&task).is_empty());
            let result = if autosave {
                repo.apply_annotation_batch(&users[1], context(&overview), vec![], true)
                    .await
                    .unwrap()
            } else {
                result
            };
            assert_eq!(result.task_states[&task].status, TaskStatus::Submitted);
            assert_eq!(
                serde_json::from_slice::<ImageState>(&serde_json::to_vec(&result).unwrap())
                    .unwrap(),
                result
            );
            assert_eq!(
                result.workflow_confirmations[&object.assignment_id],
                confirmed.workflow_confirmations[&object.assignment_id],
                "the original receipt and score owner stay unchanged"
            );
            assert_eq!(
                result,
                repo.apply_annotation_batch(&users[1], context(&overview), vec![], true)
                    .await
                    .unwrap()
            );
            assert_eq!(
                result,
                labello_domain::rebuild_state(
                    overview.image_id.clone(),
                    &repo.load_events(&overview.image_id).await.unwrap()
                )
                .unwrap()
            );
        }
    }
}

#[tokio::test]
async fn empty_review_save_does_not_exclude_independent_overview_reviewer() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["author", "visitor", "reviewer"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    for role in &mut metadata.role_assignments {
        role.roles.insert(DatasetRole::Reviewer);
    }
    repo.save_dataset(&metadata).await.unwrap();
    seed(&repo, &task, &users[0], "img_0", "box").await;
    for variant in [WorkflowVariant::Objects, WorkflowVariant::Overview] {
        let selection = WorkflowSelection {
            task_id: task.clone(),
            kind: AssignmentKind::Annotation,
            variant,
        };
        let item = repo
            .claim_workflow_item(&users[0], &selection, &[])
            .await
            .unwrap()
            .unwrap();
        repo.display_workflow_item(&users[0], context(&item))
            .await
            .unwrap();
        repo.apply_annotation_batch(&users[0], context(&item), vec![], true)
            .await
            .unwrap();
    }
    let mut selection = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Review,
        variant: WorkflowVariant::Objects,
    };
    let item = repo
        .claim_workflow_item(&users[1], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[1], context(&item))
        .await
        .unwrap();
    repo.save_workflow_edits(
        &users[1],
        context(&item),
        labello_domain::WorkflowEdits::default(),
        0,
    )
    .await
    .unwrap();
    repo.leave_workflow(&users[1], &selection).await.unwrap();
    let item = repo
        .claim_workflow_item(&users[2], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    approve_item(&repo, &users[2], &item).await;
    let mut old_cache = repo.load_image_state(&item.image_id).await.unwrap();
    old_cache.review_projection_version = 2;
    old_cache.workflow_contributors.insert(users[1].clone());
    crate::fsjson::write_json_atomic(&repo.state_path(&item.image_id), &old_cache)
        .await
        .unwrap();
    let replay = repo.load_image_state(&item.image_id).await.unwrap();
    assert_eq!(replay.review_projection_version, 3);
    assert!(!replay.workflow_contributors.contains(&users[1]));
    selection.variant = WorkflowVariant::Overview;
    let overview = repo
        .claim_workflow_item(&users[1], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    let state = repo
        .display_workflow_item(&users[1], context(&overview))
        .await
        .unwrap();
    assert!(!state.workflow_assignments[&overview.assignment_id].review_exception);
}

#[tokio::test]
async fn workflow_polling_reuses_images_without_holding_admission() {
    let (_temp, repo, _task, users) = crate::assignment::tests::annotation_repo(3, &["a"]).await;
    repo.workflow_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    repo.reset_image_state_load_count();
    let _admission = repo.assignment_claim_lock.lock().await;
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        repo.workflow_availability(&users[0], AssignmentKind::Annotation),
    )
    .await
    .expect("polling must not wait for mutation admission")
    .unwrap();
    assert_eq!(
        repo.image_state_load_count(),
        0,
        "warm polls must not reread histories"
    );
}

#[tokio::test]
async fn workflow_polling_cold_scan_and_invalidation_do_not_serialize_mutations() {
    let (_temp, repo, task, users) = crate::assignment::tests::annotation_repo(2, &["a"]).await;
    let lock = repo.image_lock(&"img_1".into());
    let image = lock.lock().await;
    repo.reset_image_state_load_count();
    let polling = repo.clone();
    let user = users[0].clone();
    let scan = tokio::spawn(async move {
        polling
            .workflow_availability(&user, AssignmentKind::Annotation)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while repo.image_state_load_count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let admission = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        repo.assignment_claim_lock.lock(),
    )
    .await
    .expect("a cold reader must leave admission free while an image is locked");
    seed(&repo, &task, &users[0], "img_0", "box").await;
    drop(admission);
    drop(image);
    scan.await.unwrap().unwrap();
    repo.reset_image_state_load_count();
    let available = repo
        .workflow_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(
        available
            .iter()
            .any(|a| a.selection.variant == WorkflowVariant::Objects && a.available)
    );
    assert!(
        repo.image_state_load_count() <= 1,
        "only the invalidated image may be reread"
    );
    repo.reset_image_state_load_count();
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.tasks[0].enabled = false;
    repo.save_dataset(&metadata).await.unwrap();
    let available = repo
        .workflow_availability(&users[0], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(
        available
            .iter()
            .all(|a| a.reason == Some(labello_domain::WorkflowUnavailableReason::WorkflowDisabled))
    );
    assert_eq!(repo.image_state_load_count(), 0);
}

#[tokio::test]
async fn workflow_polling_rechecks_lease_expiry_without_reloading_images() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(1, &["a", "b"]).await;
    let selection = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    let mut item = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    item.expires_at = Some(labello_domain::now() + std::time::Duration::from_secs(2));
    repo.append_payload(
        &item.image_id,
        &Actor {
            user_id: users[0].clone(),
            role: DatasetRole::Annotator,
        },
        EventPayload::AssignmentUpdated {
            assignment: item.clone(),
        },
    )
    .await
    .unwrap();
    let occupied = repo
        .workflow_availability(&users[1], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(occupied.iter().any(|a| a.selection == selection
        && a.reason == Some(labello_domain::WorkflowUnavailableReason::ClaimedByOthers)));
    repo.reset_image_state_load_count();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    let expired = repo
        .workflow_availability(&users[1], AssignmentKind::Annotation)
        .await
        .unwrap();
    assert!(
        expired
            .iter()
            .any(|a| a.selection == selection && a.available)
    );
    assert_eq!(repo.image_state_load_count(), 0);
}

mod history;

#[tokio::test]
async fn displaying_history_does_not_load_unvisited_images_for_reservation_cleanup() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(16, &["author"]).await;
    let selection = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    let item = repo
        .claim_workflow_item(&users[0], &selection, &[])
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[0], context(&item))
        .await
        .unwrap();
    repo.polling_images.lock().clear();
    repo.reset_image_state_load_count();
    repo.display_workflow_item(&users[0], context(&item))
        .await
        .unwrap();
    assert!(
        repo.image_state_load_count() <= 4,
        "display cleanup loaded {} images despite only one history visit",
        repo.image_state_load_count()
    );
}
