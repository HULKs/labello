use super::*;
use crate::{
    DatasetRole, EventPayload, ImageId, ReviewDecision, ReviewId, ReviewRecord, TaskId, TaskState,
};

fn timestamp(value: &str) -> Timestamp {
    value.parse().unwrap()
}

fn event(at: Timestamp, image: &str, user: &str, payload: EventPayload) -> EventLogEntry {
    EventLogEntry::new(
        1,
        ImageId::from(image),
        UserId::from(user),
        DatasetRole::Annotator,
        at,
        payload,
    )
}

fn submission(at: Timestamp, image: &str, user: &str, task: &str, reviewed: bool) -> EventLogEntry {
    event(
        at,
        image,
        user,
        EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: TaskId::from(task),
                status: if reviewed {
                    TaskStatus::Submitted
                } else {
                    TaskStatus::Completed
                },
                outcome: (!reviewed).then_some(TaskOutcome::AnnotationCompleted),
                assigned_to: Some(UserId::from(user)),
                completed_by: Some(UserId::from(user)),
                completed_at: Some(at),
                updated_at: at,
            },
        },
    )
}

#[test]
fn daily_activity_deduplicates_committed_history_without_retracting_reopened_work() {
    let at = timestamp("2026-09-05T12:00:00Z");
    let window = UtcActivityWindow::containing(at);
    let mut events = vec![
        submission(at, "one", "alice", "boxes", true),
        submission(at, "one", "alice", "boxes", true),
        event(
            at,
            "one",
            "alice",
            EventPayload::TaskStateChanged {
                task_state: TaskState::new(TaskId::from("boxes"), at),
            },
        ),
        submission(at, "one", "alice", "migration", true),
        submission(at, "two", "alice", "boxes", false),
        submission(at, "one", "bob", "boxes", true),
        submission(
            window.start - chrono::Duration::nanoseconds(1),
            "prior",
            "alice",
            "boxes",
            true,
        ),
        submission(window.end, "tomorrow", "alice", "boxes", true),
    ];
    let mut wrong_actor = submission(at, "wrong", "alice", "boxes", true);
    wrong_actor.actor_user_id = UserId::from("bob");
    events.push(wrong_actor);
    let counts = daily_activity_from_events(&events, window);
    assert_eq!(counts[&UserId::from("alice")].annotation_tasks_submitted, 3);
    assert_eq!(counts[&UserId::from("bob")].annotation_tasks_submitted, 1);
    assert_eq!(counts[&UserId::from("alice")].final_task_reviews, 0);
    events.push(submission(window.end, "one", "alice", "boxes", true));
    let next = daily_activity_from_events(&events, UtcActivityWindow::containing(window.end));
    assert_eq!(next[&UserId::from("alice")].annotation_tasks_submitted, 2);
    assert!(window.contains(window.start));
    assert!(!window.contains(window.end));
}

#[test]
fn daily_activity_counts_final_approved_or_rejected_reviews_and_ignores_object_decisions() {
    let at = timestamp("2026-09-05T12:00:00Z");
    let window = UtcActivityWindow::containing(at);
    let targets = [
        ReviewTarget::Task {
            task_id: TaskId::from("boxes"),
        },
        ReviewTarget::Task {
            task_id: TaskId::from("boxes"),
        },
        ReviewTarget::MigrationConfirmation {
            task_id: TaskId::from("migration"),
            confirmation_hash: serde_json::from_value(serde_json::Value::String("a".repeat(64)))
                .unwrap(),
        },
        ReviewTarget::AnnotationVersion {
            annotation_id: crate::AnnotationId::from("ann"),
            version: 1,
        },
        ReviewTarget::Image {
            image_id: ImageId::from("one"),
        },
        ReviewTarget::MigrationDisposition {
            task_id: TaskId::from("migration"),
            object_group_id: crate::ObjectGroupId::from("group"),
            disposition_version: 1,
        },
    ];
    let mut events = targets
        .into_iter()
        .enumerate()
        .map(|(index, target)| {
            event(
                at,
                "one",
                "reviewer",
                EventPayload::ReviewRecorded {
                    review: ReviewRecord {
                        review_id: ReviewId::generate(),
                        target,
                        reviewer_user_id: UserId::from("reviewer"),
                        decision: if index == 0 {
                            ReviewDecision::Approved
                        } else {
                            ReviewDecision::Rejected
                        },
                        timestamp: at,
                        comment: None,
                    },
                },
            )
        })
        .collect::<Vec<_>>();
    // A later rejection/reopening cannot remove committed activity from this day.
    events.push(event(
        at,
        "one",
        "reviewer",
        EventPayload::TaskStateChanged {
            task_state: TaskState::new(TaskId::from("boxes"), at),
        },
    ));
    let counts = daily_activity_from_events(&events, window);
    assert_eq!(
        counts[&UserId::from("reviewer")],
        DailyActivityCounts {
            annotation_tasks_submitted: 0,
            final_task_reviews: 2
        }
    );
    let mut other = events[0].clone();
    other.actor_user_id = UserId::from("other");
    assert!(daily_activity_from_events(&[other], window).is_empty());
}

#[test]
fn daily_activity_ignores_imported_reviewed_and_correction_only_outcomes() {
    let at = timestamp("2026-09-05T12:00:00Z");
    let window = UtcActivityWindow::containing(at);
    let user = UserId::from("reviewer");
    let task = TaskId::from("boxes");
    let mut completed = TaskState {
        task_id: task.clone(),
        status: TaskStatus::Completed,
        outcome: Some(TaskOutcome::ImportedGroundTruth),
        assigned_to: Some(user.clone()),
        completed_by: Some(user.clone()),
        completed_at: Some(at),
        updated_at: at,
    };
    let mut events = Vec::new();
    for outcome in [
        TaskOutcome::ImportedGroundTruth,
        TaskOutcome::Approved,
        TaskOutcome::ReviewerCorrected,
        TaskOutcome::Adjudicated,
    ] {
        completed.outcome = Some(outcome);
        events.push(event(
            at,
            "one",
            user.as_str(),
            EventPayload::TaskStateChanged {
                task_state: completed.clone(),
            },
        ));
    }
    let mut annotation = crate::AnnotationVersion::native(
        crate::AnnotationId::from("ann"),
        task.clone(),
        crate::ClassId::from("person"),
        crate::AnnotationType::BoundingBox,
        crate::AnnotationGeometry::BoundingBox(crate::BoundingBox {
            x: 0.1,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        }),
        user.clone(),
        at,
    );
    events.push(event(
        at,
        "one",
        user.as_str(),
        EventPayload::AnnotationVersionCreated {
            annotation: annotation.clone(),
            previous_version: None,
            reason: None,
        },
    ));
    events.push(event(
        at,
        "one",
        user.as_str(),
        EventPayload::ImportInitialized {
            import_id: crate::ImportId::from("import"),
            annotations: vec![],
            task_initializations: vec![],
            migration_target_sets: vec![],
        },
    ));
    annotation.version = 2;
    completed.outcome = Some(TaskOutcome::ReviewerCorrected);
    events.push(event(
        at,
        "one",
        user.as_str(),
        EventPayload::ReviewerCorrectionRecorded {
            correction: crate::ReviewerCorrectionRecord {
                correction_id: crate::CorrectionId::from("correction"),
                assignment_id: crate::AssignmentId::from("assignment"),
                annotation_id: annotation.annotation_id.clone(),
                previous_version: 1,
                corrected_version: 2,
                task_id: task,
                reviewer_user_id: user.clone(),
                timestamp: at,
                reason: None,
            },
            review: ReviewRecord {
                review_id: ReviewId::from("correction-review"),
                target: ReviewTarget::AnnotationVersion {
                    annotation_id: annotation.annotation_id.clone(),
                    version: 1,
                },
                reviewer_user_id: user.clone(),
                decision: ReviewDecision::Rejected,
                timestamp: at,
                comment: None,
            },
            annotation: Box::new(annotation),
            task_state: completed,
            assignments: vec![],
        },
    ));
    assert!(daily_activity_from_events(&events, window).is_empty());
}

#[test]
fn daily_activity_review_revisions_use_commit_day_and_deduplicate_without_retracting_history() {
    let at = timestamp("2026-09-05T23:59:59Z");
    let window = UtcActivityWindow::containing(at);
    let target = ReviewTarget::Task {
        task_id: TaskId::from("boxes"),
    };
    let review = ReviewRecord {
        review_id: ReviewId::generate(),
        target: target.clone(),
        reviewer_user_id: UserId::from("reviewer"),
        decision: ReviewDecision::Approved,
        timestamp: at,
        comment: None,
    };
    let original = event(
        at,
        "one",
        "reviewer",
        EventPayload::ReviewRecorded {
            review: review.clone(),
        },
    );
    let revision = |commit_at, target| {
        let mut final_review = review.clone();
        final_review.review_id = ReviewId::generate();
        final_review.target = target;
        final_review.decision = ReviewDecision::Rejected;
        event(
            commit_at,
            "one",
            "reviewer",
            EventPayload::ReviewRevisionCommitted {
                assignment: crate::Assignment {
                    assignment_id: crate::AssignmentId::generate(),
                    image_id: ImageId::from("one"),
                    task_id: TaskId::from("boxes"),
                    assigned_to: UserId::from("reviewer"),
                    kind: crate::AssignmentKind::Review,
                    status: crate::AssignmentStatus::Completed,
                    expires_at: None,
                    created_at: at,
                    updated_at: commit_at,
                },
                superseded_review_ids: vec![review.review_id.clone()],
                replacement: crate::ReviewRevisionCommit {
                    reviews: vec![final_review],
                },
                task_state: TaskState::new(TaskId::from("boxes"), commit_at),
            },
        )
    };
    let same_day = revision(at, target.clone());
    let next_day = revision(window.end, target);
    let migration = revision(
        window.end,
        ReviewTarget::MigrationConfirmation {
            task_id: TaskId::from("migration"),
            confirmation_hash: serde_json::from_value(serde_json::Value::String("a".repeat(64)))
                .unwrap(),
        },
    );
    let object_only = revision(
        window.end,
        ReviewTarget::AnnotationVersion {
            annotation_id: crate::AnnotationId::from("ann"),
            version: 1,
        },
    );
    let events = vec![
        original,
        same_day.clone(),
        same_day,
        next_day.clone(),
        next_day,
        migration,
        object_only,
    ];
    assert_eq!(
        daily_activity_from_events(&events, window)[&UserId::from("reviewer")].final_task_reviews,
        1
    );
    assert_eq!(
        daily_activity_from_events(&events, UtcActivityWindow::containing(window.end))
            [&UserId::from("reviewer")]
            .final_task_reviews,
        2
    );
}
