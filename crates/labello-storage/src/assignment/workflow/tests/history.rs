use super::*;

async fn revisit_completed_overview(
    other_work_pending: bool,
    skeleton: bool,
) -> StorageResult<Assignment> {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(2, &["author", "reviewer"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    if skeleton {
        metadata.tasks[0].annotation_type = AnnotationType::Skeleton;
        metadata.tasks[0].skeleton = Some(labello_domain::SkeletonSpec {
            keypoints: vec![labello_domain::KeypointSpec {
                name: "center".into(),
                required: true,
            }],
            edges: vec![],
            allow_hidden: false,
            allow_absent: false,
        });
    }
    for role in &mut metadata.role_assignments {
        role.roles.insert(DatasetRole::Reviewer);
    }
    repo.save_dataset(&metadata).await.unwrap();
    let annotation = WorkflowSelection {
        task_id: task.clone(),
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    for _ in 0..2 {
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
    let displayed = repo
        .display_workflow_item(&users[1], context(&first))
        .await
        .unwrap();
    assert!(!displayed.workflow_assignments[&first.assignment_id].review_exception);
    assert!(!displayed.workflow_contributors.contains(&users[1]));
    let approved = approve_item(&repo, &users[1], &first).await;
    assert!(approved.workflow_contributors.contains(&users[1]));
    let mut history_context = approved.workflow_assignments[&first.assignment_id].clone();
    assert!(!approved.workflow_assignment_independent_reviewer(&history_context, &users[1]));
    history_context.source_assignment_id = Some(first.assignment_id.clone());
    assert!(approved.workflow_assignment_independent_reviewer(&history_context, &users[1]));
    for invalid in [0, 1, 2, 3, 4] {
        let mut changed = approved.clone();
        match invalid {
            0 => {
                changed
                    .workflow_assignments
                    .get_mut(&first.assignment_id)
                    .unwrap()
                    .review_exception = true
            }
            1 => {
                changed.workflow_confirmations.clear();
            }
            2 => {
                changed.workflow_seen.clear();
            }
            3 => {
                changed
                    .workflow_assignments
                    .get_mut(&first.assignment_id)
                    .unwrap()
                    .overview_fingerprint = Some("changed".into())
            }
            _ => {
                changed
                    .assignments
                    .iter_mut()
                    .find(|a| a.assignment_id == first.assignment_id)
                    .unwrap()
                    .assigned_to = users[0].clone()
            }
        }
        assert!(!changed.workflow_assignment_independent_reviewer(&history_context, &users[1]));
    }
    assert!(matches!(
        repo.reopen_workflow_item(&users[0], context(&first)).await,
        Err(StorageError::Unauthorized(_))
    ));
    let second = repo
        .claim_workflow_item(&users[1], &review, &[])
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(&users[1], context(&second))
        .await
        .unwrap();
    assert_ne!(first.image_id, second.image_id);
    assert_eq!(
        repo.workflow_history(&users[1], &review)
            .await
            .unwrap()
            .len(),
        2
    );
    if !other_work_pending {
        approve_item(&repo, &users[1], &second).await;
    }
    metadata.workflow_queue.history_depth = 0;
    repo.save_dataset(&metadata).await?;
    assert!(matches!(
        repo.reopen_workflow_item(&users[1], context(&first)).await,
        Err(StorageError::AssignmentConflict(_))
    ));
    metadata.workflow_queue.history_depth = 5;
    metadata.role_assignments[1]
        .roles
        .remove(&DatasetRole::Reviewer);
    repo.save_dataset(&metadata).await?;
    assert!(
        repo.reopen_workflow_item(&users[1], context(&first))
            .await
            .is_err()
    );
    metadata.role_assignments[1]
        .roles
        .insert(DatasetRole::Reviewer);
    repo.save_dataset(&metadata).await?;
    let reopened = repo
        .reopen_workflow_item(&users[1], context(&first))
        .await?;
    let displayed = repo
        .display_workflow_item(&users[1], context(&reopened))
        .await?;
    assert!(!displayed.workflow_assignments[&reopened.assignment_id].review_exception);
    let approved = approve_item(&repo, &users[1], &reopened).await;
    assert_eq!(approved.task_states[&task].status, TaskStatus::Completed);
    let again = repo
        .reopen_workflow_item(&users[1], context(&reopened))
        .await?;
    repo.display_workflow_item(&users[1], context(&again))
        .await?;
    let approved = approve_item(&repo, &users[1], &again).await;
    let receipt = approved.workflow_confirmations[&again.assignment_id]
        .review
        .clone()
        .unwrap();
    assert_eq!(
        repo.confirm_workflow_review(&users[1], context(&again), receipt)
            .await?,
        approved
    );
    let stats = repo.dataset_stats().await?.contributors.unwrap();
    assert_eq!(
        stats[&users[1]]
            .history
            .iter()
            .map(|day| day.reviewed)
            .sum::<usize>(),
        if other_work_pending { 1 } else { 2 }
    );
    let correcting = repo
        .reopen_workflow_item(&users[1], context(&again))
        .await?;
    let state = repo
        .display_workflow_item(&users[1], context(&correcting))
        .await?;
    let geometry = if skeleton {
        AnnotationGeometry::Skeleton(labello_domain::SkeletonGeometry {
            keypoints: vec![labello_domain::KeypointAnnotation {
                name: "center".into(),
                state: labello_domain::KeypointState::Visible,
                point: Some(labello_domain::NormalizedPoint { x: 0.3, y: 0.4 }),
            }],
        })
    } else {
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.2,
            y: 0.2,
            width: 0.2,
            height: 0.3,
        })
    };
    let changes = vec![labello_domain::ReviewCorrectionChange::Add {
        annotation_id: "missing".into(),
        class_id: metadata.tasks[0].class_ids[0].clone(),
        geometry,
    }];
    let submission = labello_domain::ReviewCorrectionSubmission {
        correction_id: labello_domain::CorrectionId::generate(),
        round: state.review_round(&task).unwrap().clone(),
        target_fingerprint: state.review_target_fingerprint(&metadata.tasks[0]),
        changes: changes.clone(),
        reason: None,
    };
    repo.save_workflow_edits(
        &users[1],
        context(&correcting),
        labello_domain::WorkflowEdits {
            changes,
            reason: None,
        },
        0,
    )
    .await?;
    repo.display_workflow_item(&users[1], context(&correcting))
        .await?;
    let corrected = repo
        .submit_review_corrections(&users[1], context(&correcting), submission.clone())
        .await?;
    assert_eq!(corrected.task_states[&task].status, TaskStatus::Submitted);
    assert_eq!(
        repo.submit_review_corrections(&users[1], context(&correcting), submission)
            .await?,
        corrected
    );
    assert!(
        repo.reopen_workflow_item(&users[1], context(&first))
            .await
            .is_err()
    );
    let events = repo.load_events(&first.image_id).await?;
    for end in 1..=events.len() {
        labello_domain::rebuild_state(first.image_id.clone(), &events[..end]).unwrap();
    }
    assert_eq!(
        labello_domain::rebuild_state(first.image_id.clone(), &events).unwrap(),
        corrected
    );
    let forward = repo
        .reopen_workflow_item(&users[1], context(&second))
        .await?;
    repo.display_workflow_item(&users[1], context(&forward))
        .await?;
    Ok(reopened)
}

#[tokio::test]
async fn previous_overview_can_revisit_own_approval_with_more_work() {
    let result = revisit_completed_overview(true, false).await;
    assert!(result.is_ok(), "Previous failed: {:?}", result.err());
}

#[tokio::test]
async fn previous_overview_can_revisit_own_approval_without_more_work() {
    let result = revisit_completed_overview(false, false).await;
    assert!(result.is_ok(), "Previous failed: {:?}", result.err());
}

#[tokio::test]
async fn previous_overview_can_revisit_skeleton_review_with_more_work() {
    revisit_completed_overview(true, true).await.unwrap();
}

#[tokio::test]
#[ignore = "exports disposable two-user Overview data for browser verification"]
async fn export_overview_history_browser_fixture() {
    let destination =
        std::path::PathBuf::from(std::env::var("LABELLO_HISTORY_BROWSER_FIXTURE").unwrap());
    assert!(!destination.exists());
    let source = std::path::PathBuf::from(std::env::var("LABELLO_HISTORY_BROWSER_IMAGES").unwrap());
    let (temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(3, &["author", "admin"]).await;
    let mut metadata = repo.load_dataset().await.unwrap();
    metadata.name = "Overview history fixture".into();
    metadata.role_assignments[1]
        .roles
        .extend([DatasetRole::Reviewer, DatasetRole::DataAdmin]);
    if std::env::var("LABELLO_HISTORY_BROWSER_KIND").as_deref() == Ok("skeleton") {
        metadata.tasks[0].annotation_type = AnnotationType::Skeleton;
        metadata.tasks[0].skeleton = Some(labello_domain::SkeletonSpec {
            keypoints: vec![labello_domain::KeypointSpec {
                name: "center".into(),
                required: true,
            }],
            edges: vec![],
            allow_hidden: false,
            allow_absent: false,
        });
    }
    repo.save_dataset(&metadata).await.unwrap();
    let mut index = repo.load_images_index().await.unwrap();
    std::fs::create_dir_all(temp.path().join("images")).unwrap();
    let mut images = std::collections::BTreeMap::new();
    for mut record in index.images_by_hash.into_values() {
        let bytes = std::fs::read(source.join(&record.file_name)).unwrap();
        record.blake3 = blake3::hash(&bytes).to_hex().to_string();
        record.byte_size = bytes.len() as u64;
        record.width = 640;
        record.height = 480;
        std::fs::write(temp.path().join(&record.canonical_path), bytes).unwrap();
        images.insert(record.blake3.clone(), record);
    }
    index.images_by_hash = images;
    repo.save_images_index(&index).await.unwrap();
    let selection = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    for _ in 0..3 {
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
    std::fs::rename(temp.path(), destination).unwrap();
}

#[tokio::test]
async fn reopening_a_prefetched_item_preserves_the_requested_history_position() {
    let (_temp, repo, task, users) =
        crate::assignment::tests::annotation_repo(2, &["author", "other"]).await;
    let user = &users[0];
    let selection = WorkflowSelection {
        task_id: task,
        kind: AssignmentKind::Annotation,
        variant: WorkflowVariant::Overview,
    };
    let first = repo
        .claim_workflow_item(user, &selection, &[])
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(user, context(&first))
        .await
        .unwrap();
    repo.release_assignment(
        user,
        &first.assignment_id,
        &first.image_id,
        &first.task_id,
        AssignmentKind::Annotation,
    )
    .await
    .unwrap();
    let second = repo
        .claim_workflow_item(
            user,
            &selection,
            &[WorkflowItemRef {
                image_id: first.image_id.clone(),
                item: WorkflowItem::Overview,
            }],
        )
        .await
        .unwrap()
        .unwrap();
    repo.display_workflow_item(user, context(&second))
        .await
        .unwrap();
    let prefetched = repo
        .claim_workflow_item_with_prefetch(
            user,
            &selection,
            &[WorkflowItemRef {
                image_id: second.image_id.clone(),
                item: WorkflowItem::Overview,
            }],
            true,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(prefetched.image_id, first.image_id);
    let before = repo.load_image_state(&first.image_id).await.unwrap();
    assert!(matches!(
        repo.reopen_workflow_item(&users[1], context(&first)).await,
        Err(StorageError::Unauthorized(_))
    ));
    assert_eq!(
        repo.load_image_state(&first.image_id).await.unwrap(),
        before
    );
    let reopened = repo
        .reopen_workflow_item(user, context(&first))
        .await
        .unwrap();
    let state = repo
        .display_workflow_item(user, context(&reopened))
        .await
        .unwrap();
    assert_eq!(
        state.workflow_assignments[&reopened.assignment_id]
            .source_assignment_id
            .as_ref(),
        Some(&first.assignment_id)
    );
    let history = repo.workflow_history(user, &selection).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|entry| &entry.assignment_id)
            .collect::<Vec<_>>(),
        vec![&second.assignment_id, &first.assignment_id]
    );
    assert_eq!(
        state
            .assignments
            .iter()
            .find(|a| a.assignment_id == prefetched.assignment_id)
            .unwrap()
            .status,
        AssignmentStatus::Cancelled
    );
    assert_eq!(
        repo.reopen_workflow_item(user, context(&first))
            .await
            .unwrap()
            .assignment_id,
        reopened.assignment_id
    );
    let events = repo.load_events(&first.image_id).await.unwrap();
    for boundary in 0..=events.len() {
        labello_domain::rebuild_state(first.image_id.clone(), &events[..boundary]).unwrap();
    }
    assert_eq!(
        repo.rebuild_image_state(&first.image_id).await.unwrap(),
        state
    );
}
