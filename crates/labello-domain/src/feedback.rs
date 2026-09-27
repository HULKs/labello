//! Immutable reviewer-correction comparisons and per-recipient inbox policy.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEFAULT_FEEDBACK_THRESHOLD: u32 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackSummary {
    pub event_id: EventId,
    pub dataset_id: DatasetId,
    pub dataset_name: String,
    pub image_id: ImageId,
    pub task_id: TaskId,
    pub workflow_name: String,
    pub reviewer: UserId,
    pub timestamp: Timestamp,
    pub mandatory: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionFeedback {
    pub summary: FeedbackSummary,
    pub recipients: BTreeSet<UserId>,
    pub task: TaskDefinition,
    pub before: Vec<AnnotationVersion>,
    pub after: Vec<AnnotationVersion>,
    pub changes: Vec<ReviewCorrectionChange>,
    pub before_dispositions: std::collections::BTreeMap<ObjectGroupId, MigrationDisposition>,
    pub after_dispositions: std::collections::BTreeMap<ObjectGroupId, MigrationDisposition>,
    pub reason: Option<String>,
}

/// Resolve a correction round back to its human submission, including repeated
/// reviewer corrections. A reviewer receipt is never treated as original authorship.
fn submitter(events: &[EventLogEntry], round: &ReviewRound) -> UserId {
    let mut round = round;
    while let Some(EventLogEntry {
        payload: EventPayload::ReviewCorrectionSubmitted { submission, .. },
        ..
    }) = events.iter().find(|e| e.event_id == round.event_id)
    {
        if submission.round.event_sequence >= round.event_sequence {
            break;
        }
        round = &submission.round;
    }
    round.submitted_by.clone()
}

pub fn correction_feedback(
    metadata: &DatasetMetadata,
    events: &[EventLogEntry],
    receipt: usize,
) -> DomainResult<Option<CorrectionFeedback>> {
    let event = &events[receipt];
    let EventPayload::ReviewCorrectionSubmitted {
        assignment,
        submission,
        ..
    } = &event.payload
    else {
        return Ok(None);
    };
    let before_end =
        events.partition_point(|e| e.event_sequence <= submission.round.event_sequence);
    let before = rebuild_state(event.image_id.clone(), &events[..before_end])?;
    let after = rebuild_state(event.image_id.clone(), &events[..=receipt])?;
    let task = after
        .review_assignment_contexts
        .get(&assignment.assignment_id)
        .map(|context| context.task.clone())
        .or_else(|| metadata.task(&assignment.task_id).cloned())
        .ok_or_else(|| {
            DomainError::InvalidReviewerCorrection("feedback workflow is missing".into())
        })?;
    let original_submitter = submitter(events, &submission.round);
    let mut recipients = BTreeSet::new();
    for change in &submission.changes {
        let annotation_id = match change {
            ReviewCorrectionChange::Edit { annotation_id, .. }
            | ReviewCorrectionChange::Remove { annotation_id, .. } => Some(annotation_id),
            _ => None,
        };
        // The latest human version owns the corrected work; reviewer versions
        // retain that attribution. Additions/exclusions belong to the submission.
        let author = annotation_id
            .and_then(|id| before.annotations.get(id))
            .and_then(|versions| {
                versions.iter().rev().find(|a| {
                    matches!(
                        a.revision_source,
                        RevisionSource::Human { .. } | RevisionSource::PrelabelSuggestion { .. }
                    )
                })
            })
            .map(|a| a.author_user_id.clone())
            .unwrap_or_else(|| original_submitter.clone());
        if author != event.actor_user_id {
            recipients.insert(author);
        }
    }
    let annotations = |state: &ImageState| {
        state
            .active_annotations()
            .filter(|a| {
                a.task_id == assignment.task_id
                    || task
                        .manual_box_guide_migration
                        .as_ref()
                        .is_some_and(|migration| a.task_id == migration.guide_task_id)
            })
            .cloned()
            .collect()
    };
    Ok(Some(CorrectionFeedback {
        summary: FeedbackSummary {
            event_id: event.event_id.clone(),
            dataset_id: metadata.dataset_id.clone(),
            dataset_name: metadata.name.clone(),
            image_id: event.image_id.clone(),
            task_id: assignment.task_id.clone(),
            workflow_name: task.name.clone(),
            reviewer: event.actor_user_id.clone(),
            timestamp: event.timestamp,
            mandatory: false,
        },
        recipients,
        task: task.clone(),
        before: annotations(&before),
        after: annotations(&after),
        changes: submission.changes.clone(),
        before_dispositions: before
            .migration_dispositions
            .get(&assignment.task_id)
            .cloned()
            .unwrap_or_default(),
        after_dispositions: after
            .migration_dispositions
            .get(&assignment.task_id)
            .cloned()
            .unwrap_or_default(),
        reason: submission.reason.clone(),
    }))
}
