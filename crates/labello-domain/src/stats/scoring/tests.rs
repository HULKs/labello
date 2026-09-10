use super::*;
use crate::{
    AnnotationType, BoundingBox, ClassId, DatasetRole, EventPayload, ReviewId, TaskState,
    rebuild_state,
};

fn time() -> Timestamp {
    "2026-01-02T12:00:00Z".parse().unwrap()
}

fn annotation(id: &str) -> AnnotationVersion {
    AnnotationVersion::native(
        AnnotationId::from(id),
        TaskId::from("boxes"),
        ClassId::from("person"),
        AnnotationType::BoundingBox,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.1,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        }),
        UserId::from("author"),
        time(),
    )
}

fn push(events: &mut Vec<EventLogEntry>, timestamp: Timestamp, payload: EventPayload) {
    events.push(EventLogEntry::new(
        events.len() as u64 + 1,
        ImageId::from("image"),
        UserId::from("author"),
        DatasetRole::Annotator,
        timestamp,
        payload,
    ));
}

fn save(events: &mut Vec<EventLogEntry>, annotation: AnnotationVersion, timestamp: Timestamp) {
    let previous_version = (annotation.version > 1).then(|| annotation.version - 1);
    push(
        events,
        timestamp,
        EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version,
            reason: None,
        },
    );
}

fn submit(events: &mut Vec<EventLogEntry>, timestamp: Timestamp) {
    let mut task_state = TaskState::new(TaskId::from("boxes"), timestamp);
    task_state.status = TaskStatus::Submitted;
    task_state.completed_by = Some(UserId::from("author"));
    task_state.completed_at = Some(timestamp);
    push(
        events,
        timestamp,
        EventPayload::TaskStateChanged { task_state },
    );
}

fn review(
    events: &mut Vec<EventLogEntry>,
    id: &str,
    version: u32,
    decision: ReviewDecision,
    timestamp: Timestamp,
) {
    push(
        events,
        timestamp,
        EventPayload::ReviewRecorded {
            review: ReviewRecord {
                review_id: ReviewId::from(id),
                target: ReviewTarget::AnnotationVersion {
                    annotation_id: AnnotationId::from("label"),
                    version,
                },
                reviewer_user_id: UserId::from("reviewer"),
                decision,
                timestamp,
                comment: None,
            },
        },
    );
}

fn scores(events: &[EventLogEntry], focus: &[FocusWindow]) -> BTreeMap<UserId, ContributorStats> {
    let state = rebuild_state(ImageId::from("image"), events).unwrap();
    let mut projection = ScoringProjection::default();
    projection.record_image(&state, events);
    let mut contributors = BTreeMap::new();
    projection.finish(&mut contributors, focus);
    contributors
}

#[test]
fn geometry_weights_and_daily_tiers_have_exact_boundaries() {
    assert_eq!(base_value(&annotation("a").geometry), 2000);
    for (count, expected) in [(1, 1000), (2, 1500), (3, 2000), (5, 3000)] {
        let geometry = AnnotationGeometry::Skeleton(crate::SkeletonGeometry {
            keypoints: (0..count)
                .map(|i| crate::KeypointAnnotation {
                    name: format!("p{i}"),
                    point: None,
                    state: crate::KeypointState::Hidden,
                })
                .collect(),
        });
        assert_eq!(base_value(&geometry), expected);
    }
    for (count, expected) in [
        (0, 100),
        (99, 100),
        (100, 110),
        (199, 110),
        (200, 120),
        (499, 140),
        (500, 150),
        (900, 150),
    ] {
        assert_eq!(daily_multiplier(count), expected);
    }
    assert_eq!(displayed_score(10_000), 100);
    assert_eq!(displayed_score(100_000), 316);
    assert_eq!(displayed_score(-10_000), -100);
}

#[test]
fn historical_daily_rewards_are_marginal_deterministic_and_reset() {
    let mut events = Vec::new();
    for index in 0..502 {
        save(
            &mut events,
            annotation(&format!("label-{index:04}")),
            time(),
        );
    }
    submit(&mut events, time());
    submit(&mut events, time());
    let tomorrow = time() + chrono::Duration::days(1);
    save(&mut events, annotation("tomorrow"), tomorrow);
    submit(&mut events, tomorrow);
    let result = scores(&events, &[]);
    let days = &result[&UserId::from("author")].history;
    assert_eq!(days[0].score.labels, 502);
    assert_eq!(
        days[0].score.labeling,
        100 * (2200 + 2420 + 2640 + 2860 + 3080) + 2 * 3300
    );
    assert_eq!(days[1].score.labels, 1);
    assert_eq!(days[1].score.labeling, 2200);
}

#[test]
fn prelabel_provenance_survives_edits_and_focus_expires_at_exact_boundary() {
    let mut events = Vec::new();
    let mut label = annotation("label");
    label.revision_source = RevisionSource::PrelabelSuggestion {
        config_id: crate::PrelabelConfigId::from("model"),
        model_id: "model".into(),
        confidence: 0.9,
    };
    save(&mut events, label.clone(), time());
    label.version = 2;
    label.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    save(&mut events, label, time());
    submit(&mut events, time());
    let focus = FocusWindow {
        starts_at: time(),
        ends_at: time() + chrono::Duration::minutes(20),
        task_id: Some(TaskId::from("boxes")),
    };
    let end = focus.ends_at;
    save(&mut events, annotation("later"), end);
    submit(&mut events, end);
    let result = scores(&events, &[focus]);
    let day = &result[&UserId::from("author")].history[0].score;
    assert_eq!(day.labels, 2);
    assert_eq!(day.labeling, 2500 + 2200);
}

#[test]
fn rejected_then_corrected_keeps_penalty_and_rewards_corrector_once() {
    let mut events = Vec::new();
    let mut label = annotation("label");
    save(&mut events, label.clone(), time());
    submit(&mut events, time());
    review(&mut events, "reject", 1, ReviewDecision::Rejected, time());
    review(
        &mut events,
        "reject-again",
        1,
        ReviewDecision::Rejected,
        time(),
    );
    label.version = 2;
    label.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    label.author_user_id = UserId::from("corrector");
    if let AnnotationGeometry::BoundingBox(bbox) = &mut label.geometry {
        bbox.width = 0.3;
    }
    save(&mut events, label, time());
    submit(&mut events, time());
    review(&mut events, "approve", 2, ReviewDecision::Approved, time());
    review(
        &mut events,
        "approve-again",
        2,
        ReviewDecision::Approved,
        time(),
    );
    let result = scores(&events, &[]);
    let author = &result[&UserId::from("author")].history[0].score;
    assert_eq!(
        (author.labels, author.labeling, author.deductions),
        (1, 2200, 1000)
    );
    assert_eq!(
        result[&UserId::from("corrector")].history[0]
            .score
            .corrections,
        400
    );
    assert_eq!(
        result[&UserId::from("corrector")].history[0].score.labels,
        0
    );
    assert_eq!(
        result[&UserId::from("reviewer")].history[0].score.reviewing,
        600
    );
}

#[test]
fn historical_reviewer_correction_keeps_immediate_reward() {
    let mut events = Vec::new();
    let mut label = annotation("label");
    save(&mut events, label.clone(), time());
    submit(&mut events, time());
    let reviewer = UserId::from("reviewer");
    let correction_id = crate::CorrectionId::from("correction");
    let assignment_id = crate::AssignmentId::from("review");
    label.version = 2;
    label.author_user_id = reviewer.clone();
    label.revision_source = RevisionSource::ReviewerCorrection {
        correction_id: correction_id.clone(),
    };
    if let AnnotationGeometry::BoundingBox(bbox) = &mut label.geometry {
        bbox.width = 0.3;
    }
    let mut task_state = TaskState::new(label.task_id.clone(), time());
    task_state.status = TaskStatus::Completed;
    task_state.outcome = Some(TaskOutcome::ReviewerCorrected);
    task_state.completed_by = Some(reviewer.clone());
    task_state.completed_at = Some(time());
    push(
        &mut events,
        time(),
        EventPayload::ReviewerCorrectionRecorded {
            correction: crate::ReviewerCorrectionRecord {
                correction_id,
                assignment_id: assignment_id.clone(),
                annotation_id: label.annotation_id.clone(),
                previous_version: 1,
                corrected_version: 2,
                task_id: label.task_id.clone(),
                reviewer_user_id: reviewer.clone(),
                timestamp: time(),
                reason: None,
            },
            review: ReviewRecord {
                review_id: ReviewId::from("rejected"),
                target: ReviewTarget::AnnotationVersion {
                    annotation_id: label.annotation_id.clone(),
                    version: 1,
                },
                reviewer_user_id: reviewer.clone(),
                decision: ReviewDecision::Rejected,
                timestamp: time(),
                comment: None,
            },
            assignments: vec![crate::Assignment {
                assignment_id,
                image_id: ImageId::from("image"),
                task_id: label.task_id.clone(),
                assigned_to: reviewer.clone(),
                kind: crate::AssignmentKind::Review,
                status: crate::AssignmentStatus::Completed,
                expires_at: None,
                created_at: time(),
                updated_at: time(),
            }],
            annotation: Box::new(label),
            task_state,
        },
    );
    let event = events.last_mut().unwrap();
    event.actor_user_id = reviewer.clone();
    event.actor_role = DatasetRole::Reviewer;
    let result = scores(&events, &[]);
    let author = &result[&UserId::from("author")].history[0].score;
    assert_eq!(
        (author.labels, author.labeling, author.deductions),
        (1, 2200, 1000)
    );
    let reviewer = &result[&reviewer].history[0].score;
    assert_eq!((reviewer.reviewing, reviewer.corrections), (600, 400));
}

#[test]
fn withdrawn_rejection_and_task_rejection_do_not_penalize_valid_labels() {
    let mut events = Vec::new();
    save(&mut events, annotation("label"), time());
    submit(&mut events, time());
    review(&mut events, "wrong", 1, ReviewDecision::Rejected, time());
    let mut state = rebuild_state(ImageId::from("image"), &events).unwrap();
    state.superseded_review_ids.insert(ReviewId::from("wrong"));
    let mut projection = ScoringProjection::default();
    projection.record_image(&state, &events);
    let mut result = BTreeMap::new();
    projection.finish(&mut result, &[]);
    assert_eq!(
        result[&UserId::from("author")].history[0].score.deductions,
        0
    );
    if let EventPayload::ReviewRecorded { review } = &mut events.last_mut().unwrap().payload {
        review.target = ReviewTarget::Task {
            task_id: TaskId::from("boxes"),
        };
    }
    let result = scores(&events, &[]);
    assert_eq!(
        result[&UserId::from("author")].history[0].score.deductions,
        0
    );
}

#[test]
fn unsent_deleted_and_imported_geometry_do_not_earn_labels() {
    let mut events = Vec::new();
    save(&mut events, annotation("draft"), time());
    assert!(scores(&events, &[]).is_empty());
    push(
        &mut events,
        time(),
        EventPayload::AnnotationDeleted {
            annotation_id: AnnotationId::from("draft"),
            version: 1,
            reason: None,
        },
    );
    let mut imported = annotation("import");
    imported.revision_source = RevisionSource::Import {
        import_id: crate::ImportId::from("import"),
    };
    save(&mut events, imported, time());
    submit(&mut events, time());
    assert!(scores(&events, &[]).is_empty());
}
