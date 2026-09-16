use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationId, EventId, EventLogEntry, EventPayload, ImageId, ImageState,
    MigrationDispositionStatus, MigrationExclusionReason, ObjectGroupId, ReviewTarget, TaskId,
    Timestamp, UserId,
};

/// Read-only explanation projected from authoritative history. Never persisted separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowReason {
    pub image_id: ImageId,
    pub event_id: EventId,
    pub event_sequence: u64,
    pub actor_user_id: UserId,
    pub timestamp: Timestamp,
    pub action: WorkflowReasonAction,
    pub task_id: Option<TaskId>,
    pub annotation_id: Option<AnnotationId>,
    pub object_group_id: Option<ObjectGroupId>,
    pub text: Option<String>,
    pub category: Option<MigrationExclusionReason>,
    pub current_round: bool,
    pub superseded: bool,
    pub current_exclusion: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowReasonAction {
    AnnotationEdit,
    AnnotationDeletion,
    ReviewComment,
    ReviewRevisionComment,
    ReviewerCorrection,
    MigrationExclusion,
    ImportedTaskReopened,
    ImportCoverageIncluded,
}

/// Keep event order and distinct explanations, including superseded decisions and exclusions.
/// `state` and `events` must describe the same replay boundary.
pub fn workflow_reasons(state: &ImageState, events: &[EventLogEntry]) -> Vec<WorkflowReason> {
    let mut reasons = Vec::new();
    for event in events {
        let mut entry = WorkflowReason {
            image_id: event.image_id.clone(),
            event_id: event.event_id.clone(),
            event_sequence: event.event_sequence,
            actor_user_id: event.actor_user_id.clone(),
            timestamp: event.timestamp,
            action: WorkflowReasonAction::ReviewComment,
            task_id: event.task_id().cloned(),
            annotation_id: None,
            object_group_id: None,
            text: None,
            category: None,
            current_round: false,
            current_exclusion: false,
            superseded: false,
        };
        match &event.payload {
            EventPayload::AnnotationVersionCreated {
                annotation, reason, ..
            } => {
                entry.action = WorkflowReasonAction::AnnotationEdit;
                entry.annotation_id = Some(annotation.annotation_id.clone());
                entry.object_group_id = annotation.object_group_id.clone();
                entry.text = human_text(reason.as_deref());
            }
            EventPayload::AnnotationDeleted {
                annotation_id,
                reason,
                ..
            } => {
                entry.action = WorkflowReasonAction::AnnotationDeletion;
                set_target(
                    &mut entry,
                    state,
                    &ReviewTarget::AnnotationVersion {
                        annotation_id: annotation_id.clone(),
                        version: 0,
                    },
                );
                entry.text = human_text(reason.as_deref());
            }
            EventPayload::ReviewRecorded { review } => {
                set_target(&mut entry, state, &review.target);
                entry.text = human_text(review.comment.as_deref());
                entry.superseded = state.superseded_review_ids.contains(&review.review_id);
            }
            EventPayload::ReviewRevisionCommitted { replacement, .. } => {
                for review in &replacement.reviews {
                    let mut comment = entry.clone();
                    comment.action = WorkflowReasonAction::ReviewRevisionComment;
                    set_target(&mut comment, state, &review.target);
                    comment.text = human_text(review.comment.as_deref());
                    comment.superseded = state.superseded_review_ids.contains(&review.review_id);
                    push_reason(&mut reasons, state, comment);
                }
                continue;
            }
            EventPayload::ReviewerCorrectionRecorded {
                correction, review, ..
            } => {
                entry.action = WorkflowReasonAction::ReviewerCorrection;
                entry.annotation_id = Some(correction.annotation_id.clone());
                entry.text = human_text(correction.reason.as_deref());
                // The legacy transaction normally copies the reason into the review comment.
                if human_text(review.comment.as_deref()) != entry.text {
                    let mut comment = entry.clone();
                    comment.action = WorkflowReasonAction::ReviewComment;
                    comment.text = human_text(review.comment.as_deref());
                    comment.superseded = state.superseded_review_ids.contains(&review.review_id);
                    push_reason(&mut reasons, state, comment);
                }
            }
            EventPayload::ReviewCorrectionSubmitted { submission, .. } => {
                entry.action = WorkflowReasonAction::ReviewerCorrection;
                entry.text = human_text(submission.reason.as_deref());
            }
            EventPayload::MigrationDispositionChanged {
                task_id,
                object_group_id,
                disposition,
            }
            | EventPayload::MigrationDispositionReopened {
                task_id,
                object_group_id,
                disposition,
            } => {
                let MigrationDispositionStatus::Excluded { exclusion } = &disposition.status else {
                    continue;
                };
                // Reopening may carry the same exclusion; its source event identifies it once.
                if reasons.iter().any(|reason| {
                    reason.action == WorkflowReasonAction::MigrationExclusion
                        && reason.event_id == exclusion.event_id
                }) {
                    continue;
                }
                entry.action = WorkflowReasonAction::MigrationExclusion;
                entry.event_id = exclusion.event_id.clone();
                entry.actor_user_id = exclusion.actor_user_id.clone();
                entry.timestamp = exclusion.timestamp;
                entry.object_group_id = Some(object_group_id.clone());
                entry.category = Some(exclusion.reason);
                entry.text = human_text(exclusion.note.as_deref());
                entry.current_exclusion = state.migration_dispositions.get(task_id)
                    .and_then(|objects| objects.get(object_group_id))
                    .is_some_and(|current| matches!(&current.status,
                        MigrationDispositionStatus::Excluded { exclusion: active } if active.event_id == exclusion.event_id));
                entry.superseded = !entry.current_exclusion;
            }
            EventPayload::ImportedTaskReopened { reason, .. } => {
                entry.action = WorkflowReasonAction::ImportedTaskReopened;
                entry.text = human_text(Some(reason));
            }
            EventPayload::ImportCoverageIncluded { reason, .. } => {
                entry.action = WorkflowReasonAction::ImportCoverageIncluded;
                entry.text = human_text(Some(reason));
            }
            _ => continue,
        }
        push_reason(&mut reasons, state, entry);
    }
    reasons
}

fn set_target(entry: &mut WorkflowReason, state: &ImageState, target: &ReviewTarget) {
    match target {
        ReviewTarget::AnnotationVersion { annotation_id, .. } => {
            entry.annotation_id = Some(annotation_id.clone());
            if let Some(annotation) = state
                .annotations
                .get(annotation_id)
                .and_then(|versions| versions.first())
            {
                entry.task_id = Some(annotation.task_id.clone());
                entry.object_group_id = annotation.object_group_id.clone();
            }
        }
        ReviewTarget::MigrationDisposition {
            task_id,
            object_group_id,
            ..
        } => {
            entry.task_id = Some(task_id.clone());
            entry.object_group_id = Some(object_group_id.clone());
        }
        ReviewTarget::Task { task_id } | ReviewTarget::MigrationConfirmation { task_id, .. } => {
            entry.task_id = Some(task_id.clone());
        }
        ReviewTarget::Image { .. } => {}
    }
}

fn push_reason(reasons: &mut Vec<WorkflowReason>, state: &ImageState, mut reason: WorkflowReason) {
    if reason.text.is_none() && reason.category.is_none() {
        return;
    }
    reason.current_round = !reason.superseded
        && reason
            .task_id
            .as_ref()
            .and_then(|task| state.review_rounds.get(task))
            .is_some_and(|round| reason.event_sequence >= round.event_sequence);
    reasons.push(reason);
}

fn human_text(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    if text.is_empty()
        || matches!(
            text,
            "companion box derived from the exact manually authored migration skeleton version"
                | "withdrawn still-derived companion of a removed migration skeleton"
                | "annotator_edit"
                | "manual migration exclusion"
                | "manual migration dependency correction"
                | "object discovered during full-image migration review"
                | "edited object discovered during full-image review"
                | "removed object discovered during full-image migration review"
        )
    {
        return None;
    }
    Some(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DatasetRole, MigrationDisposition, MigrationExclusion, ReviewDecision, ReviewId,
        ReviewRecord, ReviewRound, now,
    };

    fn event(sequence: u64, payload: EventPayload) -> EventLogEntry {
        EventLogEntry::new(
            sequence,
            ImageId::from("image"),
            UserId::from("reviewer"),
            DatasetRole::Reviewer,
            now(),
            payload,
        )
    }

    #[test]
    fn workflow_reasons_keep_distinct_comments_and_mark_current_round_without_machine_markers() {
        let task_id = TaskId::from("task");
        let events: Vec<_> = [
            "earlier explanation",
            "annotator_edit",
            "  ",
            "é".repeat(1200).as_str(),
            "latest explanation",
        ]
        .iter()
        .enumerate()
        .map(|(index, text)| {
            event(
                index as u64 + 1,
                EventPayload::ReviewRecorded {
                    review: ReviewRecord {
                        review_id: ReviewId::from(format!("review-{index}")),
                        target: ReviewTarget::Task {
                            task_id: task_id.clone(),
                        },
                        reviewer_user_id: UserId::from("reviewer"),
                        decision: ReviewDecision::Rejected,
                        timestamp: now(),
                        comment: Some((*text).into()),
                    },
                },
            )
        })
        .collect();
        let mut state = ImageState::new(ImageId::from("image"));
        state.review_rounds.insert(
            task_id.clone(),
            ReviewRound {
                event_id: events[3].event_id.clone(),
                event_sequence: 4,
                submitted_by: UserId::from("author"),
            },
        );
        let reasons = workflow_reasons(&state, &events);
        assert_eq!(reasons.len(), 3);
        assert_eq!(reasons[0].text.as_deref(), Some("earlier explanation"));
        assert!(!reasons[0].current_round);
        assert_eq!(reasons[1].text.as_ref().unwrap().len(), 2400);
        assert!(reasons[1].current_round);
        assert!(reasons[2].current_round);
        assert_eq!(reasons[2].task_id, Some(task_id));
        assert_eq!(reasons[2].event_id, events[4].event_id);
        state
            .superseded_review_ids
            .insert(ReviewId::from("review-4"));
        let superseded = workflow_reasons(&state, &events);
        assert!(superseded[2].superseded);
        assert!(!superseded[2].current_round);
    }

    #[test]
    fn workflow_reasons_retain_replaced_exclusions_and_notes_without_repeating_reopen_copies() {
        let task_id = TaskId::from("migration");
        let object_group_id = ObjectGroupId::from("object");
        let disposition = |id: &str, note: Option<&str>| MigrationDisposition {
            disposition_version: 2,
            status: MigrationDispositionStatus::Excluded {
                exclusion: MigrationExclusion {
                    event_id: EventId::from(id),
                    actor_user_id: UserId::from("annotator"),
                    timestamp: now(),
                    reason: MigrationExclusionReason::Other,
                    note: note.map(str::to_owned),
                },
            },
        };
        let first = disposition("exclusion-1", Some("earlier object note"));
        let second = disposition("exclusion-2", Some("replacement note"));
        let mut events = Vec::new();
        for (index, disposition) in [first.clone(), first, second.clone()]
            .into_iter()
            .enumerate()
        {
            events.push(event(
                index as u64 + 1,
                EventPayload::MigrationDispositionChanged {
                    task_id: task_id.clone(),
                    object_group_id: object_group_id.clone(),
                    disposition,
                },
            ));
        }
        let mut state = ImageState::new(ImageId::from("image"));
        state
            .migration_dispositions
            .entry(task_id)
            .or_default()
            .insert(object_group_id.clone(), second);
        let reasons = workflow_reasons(&state, &events);
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0].text.as_deref(), Some("earlier object note"));
        assert!(!reasons[0].current_exclusion);
        assert!(reasons[1].current_exclusion);
        assert_eq!(reasons[1].object_group_id, Some(object_group_id));
    }
}
