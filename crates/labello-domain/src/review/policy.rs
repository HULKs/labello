use std::collections::BTreeSet;

use crate::{
    EventLogEntry, EventPayload, ReviewDecision, ReviewRecord, ReviewTarget, TaskId, TaskStatus,
    UserId,
};

pub(crate) fn submitted_review_tasks(event: &EventLogEntry) -> Vec<&TaskId> {
    match &event.payload {
        EventPayload::TaskStateChanged { task_state }
        | EventPayload::ImportedTaskReopened { task_state, .. }
            if task_state.status == TaskStatus::Submitted =>
        {
            vec![&task_state.task_id]
        }
        EventPayload::ImportInitialized {
            task_initializations,
            ..
        } => task_initializations
            .iter()
            .filter(|task| task.initial_state.status == TaskStatus::Submitted)
            .map(|task| &task.task_id)
            .collect(),
        _ => Vec::new(),
    }
}

pub fn current_round_reviews<'a>(
    events: &'a [EventLogEntry],
    task_id: &TaskId,
) -> Vec<&'a ReviewRecord> {
    let Some(start) = events
        .iter()
        .rposition(|event| submitted_review_tasks(event).contains(&task_id))
    else {
        return Vec::new();
    };
    let mut annotations = BTreeSet::new();
    for event in events {
        match &event.payload {
            EventPayload::AnnotationVersionCreated { annotation, .. }
                if annotation.task_id == *task_id =>
            {
                annotations.insert(&annotation.annotation_id);
            }
            EventPayload::ReviewerCorrectionRecorded { annotation, .. }
                if annotation.task_id == *task_id =>
            {
                annotations.insert(&annotation.annotation_id);
            }
            EventPayload::ImportInitialized {
                annotations: imported,
                ..
            } => {
                annotations.extend(
                    imported
                        .iter()
                        .filter(|annotation| annotation.task_id == *task_id)
                        .map(|annotation| &annotation.annotation_id),
                );
            }
            _ => {}
        }
    }
    let mut reviews: Vec<&ReviewRecord> = Vec::new();
    for event in events.iter().skip(start + 1) {
        match &event.payload {
            EventPayload::ReviewRecorded { review }
            | EventPayload::ReviewerCorrectionRecorded { review, .. } => reviews.push(review),
            EventPayload::ReviewRevisionCommitted {
                superseded_review_ids,
                replacement,
                ..
            } => {
                reviews.retain(|review| !superseded_review_ids.contains(&review.review_id));
                reviews.extend(&replacement.reviews);
            }
            _ => {}
        }
    }
    reviews.retain(|review| match &review.target {
        ReviewTarget::AnnotationVersion { annotation_id, .. } => {
            annotations.contains(annotation_id)
        }
        ReviewTarget::Task { task_id: reviewed }
        | ReviewTarget::MigrationDisposition {
            task_id: reviewed, ..
        }
        | ReviewTarget::MigrationConfirmation {
            task_id: reviewed, ..
        } => reviewed == task_id,
        ReviewTarget::Image { .. } => false,
    });
    reviews
}

/// Migration confirmation is invalidated when work returns to correction or
/// annotation. Those prior decisions cannot authorize the next confirmation.
pub fn current_migration_reviews<'a>(
    events: &'a [EventLogEntry],
    task_id: &TaskId,
) -> Vec<&'a ReviewRecord> {
    let Some(start) = events
        .iter()
        .rposition(|event| submitted_review_tasks(event).contains(&task_id))
    else {
        return Vec::new();
    };
    if events
        .iter()
        .skip(start + 1)
        .any(|event| match &event.payload {
            EventPayload::TaskStateChanged { task_state }
            | EventPayload::ReviewRevisionCommitted { task_state, .. }
            | EventPayload::ImportedTaskReopened { task_state, .. } => {
                task_state.task_id == *task_id
                    && matches!(
                        task_state.status,
                        TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::NeedsCorrection
                    )
            }
            _ => false,
        })
    {
        return Vec::new();
    }
    current_round_reviews(events, task_id)
}

pub fn current_task_reviews(events: &[EventLogEntry], task_id: &TaskId) -> Vec<ReviewRecord> {
    current_round_reviews(events, task_id)
        .into_iter()
        .filter(|review| matches!(review.target, ReviewTarget::Task { .. }))
        .cloned()
        .collect()
}

pub fn has_task_review_by_user(
    reviews: &[ReviewRecord],
    task_id: &TaskId,
    user_id: &UserId,
) -> bool {
    reviews.iter().any(|review| {
        review.reviewer_user_id == *user_id
            && matches!(
                &review.target,
                ReviewTarget::Task {
                    task_id: reviewed_task_id
                } if reviewed_task_id == task_id
            )
    })
}

pub fn task_approval_count(reviews: &[ReviewRecord], task_id: &TaskId) -> u32 {
    reviews
        .iter()
        .filter_map(|review| match (&review.target, &review.decision) {
            (ReviewTarget::Task { task_id: reviewed }, ReviewDecision::Approved)
                if reviewed == task_id =>
            {
                Some(&review.reviewer_user_id)
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .len() as u32
}
