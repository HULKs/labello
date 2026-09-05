use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AssignmentId, EventId, ImageState, ReviewId, ReviewRecord, ReviewTarget, TaskDefinition,
    TaskId, UserId,
};

/// The authoritative submission event, independent of wall-clock timestamps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRound {
    pub event_id: EventId,
    pub event_sequence: u64,
    pub submitted_by: UserId,
}

/// Captured when a review lease is created. Renewal never changes its targets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewAssignmentContext {
    pub assignment_id: AssignmentId,
    pub source_assignment_id: Option<AssignmentId>,
    pub round: ReviewRound,
    pub task: TaskDefinition,
    pub target_fingerprint: String,
    pub targets: Vec<ReviewTarget>,
    pub superseded_review_ids: Vec<ReviewId>,
    pub decision_revision: bool,
}

/// One immutable replacement transaction. Its ID is the fresh assignment ID;
/// exact retries submit the identical records, including their review IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRevisionCommit {
    pub reviews: Vec<ReviewRecord>,
}

impl ImageState {
    pub fn review_object_targets(
        &self,
        task: &TaskDefinition,
    ) -> crate::DomainResult<Vec<ReviewTarget>> {
        let mut targets = Vec::new();
        if task.manual_box_guide_migration.is_some() {
            let set = self
                .migration_target_sets
                .get(&task.task_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidReviewRevision(
                        "migration target set is missing".into(),
                    )
                })?;
            let mut canonical = set.targets.iter().collect::<Vec<_>>();
            canonical.sort_by_key(|target| target.sequence_index);
            for target in canonical {
                let disposition = self
                    .migration_dispositions
                    .get(&task.task_id)
                    .and_then(|values| values.get(&target.object_group_id))
                    .ok_or_else(|| {
                        crate::DomainError::InvalidReviewRevision(
                            "migration disposition is missing".into(),
                        )
                    })?;
                targets.push(match &disposition.status {
                    crate::MigrationDispositionStatus::Annotated {
                        skeleton_annotation_id,
                        skeleton_version,
                    } => ReviewTarget::AnnotationVersion {
                        annotation_id: skeleton_annotation_id.clone(),
                        version: *skeleton_version,
                    },
                    crate::MigrationDispositionStatus::Excluded { .. } => {
                        ReviewTarget::MigrationDisposition {
                            task_id: task.task_id.clone(),
                            object_group_id: target.object_group_id.clone(),
                            disposition_version: disposition.disposition_version,
                        }
                    }
                    crate::MigrationDispositionStatus::Pending => {
                        return Err(crate::DomainError::InvalidReviewRevision(
                            "migration disposition is pending".into(),
                        ));
                    }
                });
            }
            targets.extend(
                self.migration_discovered_skeletons(&task.task_id)
                    .into_iter()
                    .map(|annotation| ReviewTarget::AnnotationVersion {
                        annotation_id: annotation.annotation_id.clone(),
                        version: annotation.version,
                    }),
            );
        } else {
            targets.extend(
                self.active_annotations()
                    .filter(|annotation| annotation.task_id == task.task_id)
                    .map(|annotation| ReviewTarget::AnnotationVersion {
                        annotation_id: annotation.annotation_id.clone(),
                        version: annotation.version,
                    }),
            );
        }
        Ok(targets)
    }

    pub fn review_targets(&self, task: &TaskDefinition) -> crate::DomainResult<Vec<ReviewTarget>> {
        let mut targets = self.review_object_targets(task)?;
        if task.manual_box_guide_migration.is_some() {
            let confirmation =
                self.migration_confirmations
                    .get(&task.task_id)
                    .ok_or_else(|| {
                        crate::DomainError::InvalidReviewRevision(
                            "migration confirmation is missing or invalidated".into(),
                        )
                    })?;
            targets.push(ReviewTarget::MigrationConfirmation {
                task_id: task.task_id.clone(),
                confirmation_hash: confirmation.confirmation_hash.clone(),
            });
        } else {
            targets.push(ReviewTarget::Task {
                task_id: task.task_id.clone(),
            });
        }
        Ok(targets)
    }

    /// Canonical migration object rejection requires a new disposition. Discovery
    /// rejection is bound directly to its exact skeleton version by review policy.
    pub fn migration_review_correction_marker(
        &self,
        task_id: &TaskId,
        target: &ReviewTarget,
        event_id: &crate::EventId,
        timestamp: crate::Timestamp,
    ) -> crate::DomainResult<Option<(crate::ObjectGroupId, crate::MigrationDependencyMarker)>> {
        let Some(set) = self.migration_target_sets.get(task_id) else {
            return Ok(None);
        };
        let group = match target {
            ReviewTarget::AnnotationVersion { annotation_id, .. } => set
                .targets
                .iter()
                .find(|target| target.reserved_skeleton_annotation_id == *annotation_id)
                .map(|target| target.object_group_id.clone()),
            ReviewTarget::MigrationDisposition {
                object_group_id, ..
            } => Some(object_group_id.clone()),
            _ => None,
        };
        let Some(group_id) = group else {
            return Ok(None);
        };
        let disposition = self
            .migration_dispositions
            .get(task_id)
            .and_then(|items| items.get(&group_id))
            .ok_or_else(|| {
                crate::DomainError::InvalidReviewRevision("migration disposition is missing".into())
            })?;
        let previous = self
            .migration_dependencies
            .get(task_id)
            .and_then(|items| items.get(&group_id));
        Ok(Some((
            group_id,
            crate::MigrationDependencyMarker {
                marker_version: previous.map_or(1, |marker| marker.marker_version + 1),
                kind: crate::MigrationDependencyKind::CorrectionRequired,
                required_disposition_version: disposition.disposition_version,
                event_id: event_id.clone(),
                timestamp,
            },
        )))
    }

    pub fn effective_review_outcome(
        &self,
        task: &TaskDefinition,
    ) -> (crate::TaskStatus, Option<crate::TaskOutcome>) {
        let mut final_decisions = std::collections::BTreeMap::new();
        for review in self
            .effective_reviews_for_task(&task.task_id)
            .filter(|review| {
                matches!(
                    review.target,
                    ReviewTarget::Task { .. } | ReviewTarget::MigrationConfirmation { .. }
                )
            })
        {
            final_decisions.insert(&review.reviewer_user_id, &review.decision);
        }
        if final_decisions
            .values()
            .any(|decision| **decision == crate::ReviewDecision::Rejected)
        {
            (crate::TaskStatus::NeedsCorrection, None)
        } else if final_decisions
            .values()
            .filter(|decision| ***decision == crate::ReviewDecision::Approved)
            .count()
            >= task.review.required_reviews.max(1) as usize
        {
            (
                crate::TaskStatus::Completed,
                Some(crate::TaskOutcome::Approved),
            )
        } else {
            (crate::TaskStatus::Submitted, None)
        }
    }

    pub fn review_round(&self, task_id: &TaskId) -> Option<&ReviewRound> {
        self.review_rounds.get(task_id)
    }

    /// Audit history is retained in `reviews`; this iterator omits explicitly
    /// superseded decisions without conflating separate submission rounds.
    pub fn effective_reviews(&self) -> impl Iterator<Item = &ReviewRecord> {
        self.reviews
            .iter()
            .filter(|review| !self.superseded_review_ids.contains(&review.review_id))
    }

    pub fn effective_reviews_for_task(
        &self,
        task_id: &TaskId,
    ) -> impl Iterator<Item = &ReviewRecord> {
        let task_id = task_id.clone();
        self.effective_reviews().filter(move |review| {
            self.review_target_task(&review.target) == Some(&task_id)
                && match self.review_round(&task_id) {
                    Some(round) => {
                        self.review_record_rounds.get(&review.review_id) == Some(&round.event_id)
                    }
                    None => true,
                }
        })
    }

    pub fn effective_review_for_target(
        &self,
        task_id: &TaskId,
        target: &ReviewTarget,
        reviewer: &UserId,
    ) -> Option<&ReviewRecord> {
        self.effective_reviews_for_task(task_id)
            .filter(|review| review.target == *target && review.reviewer_user_id == *reviewer)
            .last()
    }

    pub fn review_target_task<'a>(&'a self, target: &'a ReviewTarget) -> Option<&'a TaskId> {
        match target {
            ReviewTarget::AnnotationVersion { annotation_id, .. } => self
                .current_annotation(annotation_id)
                .map(|annotation| &annotation.task_id),
            ReviewTarget::Task { task_id }
            | ReviewTarget::MigrationDisposition { task_id, .. }
            | ReviewTarget::MigrationConfirmation { task_id, .. } => Some(task_id),
            ReviewTarget::Image { .. } => None,
        }
    }

    pub fn review_target_fingerprint(&self, task: &TaskDefinition) -> String {
        let mut task_ids = BTreeSet::from([task.task_id.clone()]);
        if let Some(set) = self.migration_target_sets.get(&task.task_id) {
            task_ids.insert(set.guide_task_id.clone());
        }
        let annotations = self
            .annotations
            .values()
            .filter_map(|versions| versions.last())
            .filter(|annotation| task_ids.contains(&annotation.task_id))
            .map(|annotation| {
                (
                    &annotation.annotation_id,
                    annotation.version,
                    annotation.deleted,
                )
            })
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&(
            task,
            annotations,
            self.migration_target_sets.get(&task.task_id),
            self.migration_dispositions.get(&task.task_id),
            self.migration_dependencies.get(&task.task_id),
            self.migration_confirmations.get(&task.task_id),
        ))
        .expect("review context contains serializable domain data");
        blake3::hash(&bytes).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DatasetRole, EventLogEntry, EventPayload, ImageId, ReviewDecision, TaskState, TaskStatus,
        now, rebuild_state,
    };

    #[test]
    fn effective_reviews_follow_submission_event_identity_even_with_equal_timestamps() {
        let image = ImageId::from("image");
        let task = TaskId::from("task");
        let user = UserId::from("reviewer");
        let timestamp = now();
        let target = ReviewTarget::Task {
            task_id: task.clone(),
        };
        let submitted = EventPayload::TaskStateChanged {
            task_state: TaskState {
                task_id: task.clone(),
                status: TaskStatus::Submitted,
                outcome: None,
                assigned_to: None,
                completed_by: None,
                completed_at: Some(timestamp),
                updated_at: timestamp,
            },
        };
        let review = ReviewRecord {
            review_id: ReviewId::from("old"),
            target: target.clone(),
            reviewer_user_id: user.clone(),
            decision: ReviewDecision::Approved,
            timestamp,
            comment: None,
        };
        let events = [
            submitted.clone(),
            EventPayload::ReviewRecorded { review },
            submitted,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            EventLogEntry::new(
                index as u64 + 1,
                image.clone(),
                user.clone(),
                DatasetRole::Reviewer,
                timestamp,
                payload,
            )
        })
        .collect::<Vec<_>>();
        let first = rebuild_state(image.clone(), &events[..2]).unwrap();
        assert!(
            first
                .effective_review_for_target(&task, &target, &user)
                .is_some()
        );
        let second = rebuild_state(image, &events).unwrap();
        assert!(
            second
                .effective_review_for_target(&task, &target, &user)
                .is_none()
        );
        assert_eq!(second.reviews.len(), 1);
        assert_eq!(
            second.review_round(&task).unwrap().event_id,
            events[2].event_id
        );
    }
}
