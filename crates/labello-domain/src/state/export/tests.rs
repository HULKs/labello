use super::*;
use crate::{
    AnnotationGeometry, BoundingBox, ClassId, DatasetRole, ImportTaskInitialization, ReviewConfig,
    ReviewId, TutorialContent, UserId, now,
};

fn task() -> TaskDefinition {
    TaskDefinition {
        task_id: TaskId::from("boxes"),
        name: "Boxes".into(),
        annotation_type: AnnotationType::BoundingBox,
        class_ids: vec![ClassId::from("class")],
        instructions: TutorialContent {
            title: "Boxes".into(),
            example_text: String::new(),
            example_images: vec![],
        },
        skeleton: None,
        review: ReviewConfig {
            workflow: ReviewWorkflow::None,
            ..ReviewConfig::default()
        },
        prelabel_config_ids: vec![],
        manual_box_guide_migration: None,
        enabled: true,
    }
}

fn state(task: &TaskDefinition, outcome: TaskOutcome) -> TaskState {
    TaskState {
        task_id: task.task_id.clone(),
        status: TaskStatus::Completed,
        outcome: Some(outcome),
        assigned_to: None,
        completed_by: Some(UserId::from("author")),
        completed_at: Some(now()),
        updated_at: now(),
    }
}

fn event(sequence: u64, payload: EventPayload) -> EventLogEntry {
    EventLogEntry::new(
        sequence,
        ImageId::from("image"),
        UserId::from("author"),
        DatasetRole::Annotator,
        now(),
        payload,
    )
}

#[test]
fn only_completed_coverage_can_become_an_empty_label() {
    let task = task();
    for status in [
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Submitted,
        TaskStatus::NeedsCorrection,
    ] {
        let mut terminal = state(&task, TaskOutcome::AnnotationCompleted);
        terminal.status = status;
        let events = [event(
            1,
            EventPayload::TaskStateChanged {
                task_state: terminal,
            },
        )];
        let image = rebuild_state(ImageId::from("image"), &events).unwrap();
        assert_eq!(
            image.export_task_omission(&task, &events),
            Some(ExportOmissionReason::Unfinished)
        );
    }
    let events = [event(
        1,
        EventPayload::TaskStateChanged {
            task_state: state(&task, TaskOutcome::AnnotationCompleted),
        },
    )];
    let image = rebuild_state(ImageId::from("image"), &events).unwrap();
    assert_eq!(image.export_task_omission(&task, &events), None);
    let mut changed = task.clone();
    changed.review.workflow = ReviewWorkflow::Approval;
    assert_eq!(
        image.export_task_omission(&changed, &events),
        Some(ExportOmissionReason::ChangedReviewPolicy)
    );
}

#[test]
fn completed_import_still_requires_complete_or_verified_empty_coverage() {
    let task = task();
    for (coverage, expected) in [
        (ImportCoverage::VerifiedEmpty, None),
        (ImportCoverage::Complete, None),
        (
            ImportCoverage::Incomplete,
            Some(ExportOmissionReason::IncompleteCoverage),
        ),
        (
            ImportCoverage::Excluded,
            Some(ExportOmissionReason::ExcludedCoverage),
        ),
    ] {
        let terminal = state(&task, TaskOutcome::ImportedGroundTruth);
        let events = [event(
            1,
            EventPayload::ImportInitialized {
                import_id: ImportId::from("import"),
                annotations: vec![],
                task_initializations: vec![ImportTaskInitialization {
                    task_id: task.task_id.clone(),
                    coverage,
                    initial_state: terminal.clone(),
                }],
                migration_target_sets: vec![],
            },
        )];
        // Exercise conservative policy even for an inconsistent legacy state; replay
        // separately rejects impossible import initialization combinations.
        let mut image = ImageState::new(ImageId::from("image"));
        image.task_states.insert(task.task_id.clone(), terminal);
        image.import_coverage.insert(task.task_id.clone(), coverage);
        assert_eq!(image.export_task_omission(&task, &events), expected);
    }
}

#[test]
fn edits_after_completion_and_prelabels_are_not_ground_truth() {
    let task = task();
    let annotation = AnnotationVersion::native(
        AnnotationId::from("annotation"),
        task.task_id.clone(),
        ClassId::from("class"),
        AnnotationType::BoundingBox,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.1,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        }),
        UserId::from("author"),
        now(),
    );
    let complete = event(
        1,
        EventPayload::TaskStateChanged {
            task_state: state(&task, TaskOutcome::AnnotationCompleted),
        },
    );
    let added = event(
        2,
        EventPayload::AnnotationVersionCreated {
            annotation: annotation.clone(),
            previous_version: None,
            reason: None,
        },
    );
    let events = vec![complete, added];
    let image = rebuild_state(ImageId::from("image"), &events).unwrap();
    assert_eq!(
        image.export_task_omission(&task, &events),
        Some(ExportOmissionReason::Unfinished)
    );
    let mut suggestion = annotation;
    suggestion.revision_source = RevisionSource::PrelabelSuggestion {
        config_id: crate::PrelabelConfigId::from("model"),
        model_id: "model".into(),
        confidence: 0.9,
    };
    let events = vec![
        event(
            1,
            EventPayload::AnnotationVersionCreated {
                annotation: suggestion,
                previous_version: None,
                reason: None,
            },
        ),
        event(
            2,
            EventPayload::TaskStateChanged {
                task_state: state(&task, TaskOutcome::AnnotationCompleted),
            },
        ),
    ];
    let image = rebuild_state(ImageId::from("image"), &events).unwrap();
    assert_eq!(
        image.export_task_omission(&task, &events),
        Some(ExportOmissionReason::UnverifiedAnnotations)
    );
}

#[test]
fn approval_uses_current_effective_reviews_and_current_required_count() {
    let mut task = task();
    task.review.workflow = ReviewWorkflow::Approval;
    task.review.required_reviews = 1;
    let mut submitted = state(&task, TaskOutcome::Approved);
    submitted.status = TaskStatus::Submitted;
    submitted.outcome = None;
    let review = ReviewRecord {
        review_id: ReviewId::from("review"),
        target: ReviewTarget::Task {
            task_id: task.task_id.clone(),
        },
        reviewer_user_id: UserId::from("reviewer"),
        decision: ReviewDecision::Approved,
        timestamp: now(),
        comment: None,
    };
    let events = vec![
        event(
            1,
            EventPayload::TaskStateChanged {
                task_state: submitted,
            },
        ),
        event(
            2,
            EventPayload::ReviewRecorded {
                review: review.clone(),
            },
        ),
        event(
            3,
            EventPayload::TaskStateChanged {
                task_state: state(&task, TaskOutcome::Approved),
            },
        ),
    ];
    let mut image = rebuild_state(ImageId::from("image"), &events).unwrap();
    assert_eq!(image.export_task_omission(&task, &events), None);
    task.review.required_reviews = 2;
    assert_eq!(
        image.export_task_omission(&task, &events),
        Some(ExportOmissionReason::ChangedReviewPolicy)
    );
    task.review.required_reviews = 1;
    image.superseded_review_ids.insert(review.review_id);
    assert_eq!(
        image.export_task_omission(&task, &events),
        Some(ExportOmissionReason::ChangedReviewPolicy)
    );
}
