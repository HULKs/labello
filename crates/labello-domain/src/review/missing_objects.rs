use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationType, AssignmentId, AssignmentKind, AssignmentStatus, ClassId, DatasetId,
    DatasetRole, DomainError, DomainResult, EventLogEntry, ImageId, ImageState, NormalizedPoint,
    ReviewDecision, ReviewId, ReviewRecord, ReviewRound, ReviewTarget, TaskDefinition, TaskId,
    TaskStatus, Timestamp, UserId,
};

pub const MAX_MISSING_OBJECT_LOCATIONS: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingObjectLocation {
    pub marker_id: u32,
    pub class_id: ClassId,
    pub position: NormalizedPoint,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingObjectRejection {
    pub review: ReviewRecord,
    pub round: ReviewRound,
    pub locations: Vec<MissingObjectLocation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingObjectEvidence {
    pub dataset_id: DatasetId,
    pub image_id: ImageId,
    pub task_id: TaskId,
    pub assignment_id: AssignmentId,
    pub review_id: ReviewId,
    pub reviewer_user_id: UserId,
    pub timestamp: Timestamp,
    pub round: ReviewRound,
    pub annotation_type: AnnotationType,
    pub locations: Vec<MissingObjectLocation>,
}

fn invalid(message: &str) -> DomainError {
    DomainError::InvalidMissingObjectEvidence(message.into())
}

pub fn validate_missing_object_locations(
    locations: &[MissingObjectLocation],
    task: &TaskDefinition,
) -> DomainResult<()> {
    if locations.is_empty() || locations.len() > MAX_MISSING_OBJECT_LOCATIONS {
        return Err(invalid(
            "missing-object evidence must contain between 1 and 64 locations",
        ));
    }
    let mut ids = BTreeSet::new();
    for location in locations {
        location.position.validate()?;
        if location.marker_id == 0
            || !ids.insert(location.marker_id)
            || !task.class_ids.contains(&location.class_id)
        {
            return Err(invalid(
                "missing-object location identity or class is invalid",
            ));
        }
    }
    Ok(())
}

impl ImageState {
    pub fn active_missing_object_evidence(
        &self,
        task_id: &TaskId,
    ) -> Option<&MissingObjectEvidence> {
        let review = self
            .effective_reviews_for_task(task_id)
            .filter(|review| {
                review.target
                    == (ReviewTarget::Task {
                        task_id: task_id.clone(),
                    })
                    && review.decision == ReviewDecision::Rejected
            })
            .last()?;
        self.missing_object_evidence.get(&review.review_id)
    }

    pub fn missing_object_history(&self, task_id: &TaskId) -> Vec<&MissingObjectEvidence> {
        self.reviews
            .iter()
            .filter_map(|review| self.missing_object_evidence.get(&review.review_id))
            .filter(|evidence| &evidence.task_id == task_id)
            .collect()
    }

    pub fn missing_object_evidence_for_submission(
        &self,
        dataset_id: &DatasetId,
        assignment_id: &AssignmentId,
        submission: &MissingObjectRejection,
        timestamp: Timestamp,
    ) -> DomainResult<MissingObjectEvidence> {
        let context = self
            .review_assignment_contexts
            .get(assignment_id)
            .ok_or_else(|| invalid("review assignment context is missing"))?;
        if !context.task.enabled
            || context.task.review.workflow != crate::ReviewWorkflow::Approval
            || context.task.manual_box_guide_migration.is_some()
            || submission.round != context.round
            || self.review_round(&context.task.task_id) != Some(&context.round)
            || submission.review.target
                != (ReviewTarget::Task {
                    task_id: context.task.task_id.clone(),
                })
            || submission.review.decision != ReviewDecision::Rejected
        {
            return Err(invalid(
                "missing-object evidence requires the current ordinary final review",
            ));
        }
        validate_missing_object_locations(&submission.locations, &context.task)?;
        dataset_id
            .validate_path_segment()
            .map_err(|_| invalid("invalid evidence dataset"))?;
        Ok(MissingObjectEvidence {
            dataset_id: dataset_id.clone(),
            image_id: self.image_id.clone(),
            task_id: context.task.task_id.clone(),
            assignment_id: assignment_id.clone(),
            review_id: submission.review.review_id.clone(),
            reviewer_user_id: submission.review.reviewer_user_id.clone(),
            timestamp,
            round: context.round.clone(),
            annotation_type: context.task.annotation_type.clone(),
            locations: submission.locations.clone(),
        })
    }

    pub(crate) fn apply_missing_object_evidence(
        &mut self,
        evidence: &MissingObjectEvidence,
        submission: &MissingObjectRejection,
        event: &EventLogEntry,
    ) -> DomainResult<()> {
        let expected = self.missing_object_evidence_for_submission(
            &evidence.dataset_id,
            &evidence.assignment_id,
            submission,
            event.timestamp,
        )?;
        let assignment = self
            .assignments
            .iter()
            .find(|assignment| assignment.assignment_id == evidence.assignment_id)
            .ok_or_else(|| invalid("evidence assignment is missing"))?;
        if &expected != evidence
            || assignment.kind != AssignmentKind::Review
            || assignment.status != AssignmentStatus::Completed
            || assignment.assigned_to != event.actor_user_id
            || event.actor_role != DatasetRole::Reviewer
            || evidence.reviewer_user_id != event.actor_user_id
            || assignment.task_id != evidence.task_id
            || self
                .review_finished_sequences
                .contains_key(&assignment.assignment_id)
            || self
                .missing_object_submissions
                .contains_key(&assignment.assignment_id)
            || self
                .missing_object_evidence
                .contains_key(&evidence.review_id)
            || !self.reviews.contains(&submission.review)
            || self
                .task_states
                .get(&evidence.task_id)
                .is_none_or(|task| task.status != TaskStatus::NeedsCorrection)
            || self.effective_review_for_target(
                &evidence.task_id,
                &submission.review.target,
                &event.actor_user_id,
            ) != Some(&submission.review)
        {
            return Err(invalid(
                "evidence does not belong to this final rejection transaction",
            ));
        }
        let context = &self.review_assignment_contexts[&evidence.assignment_id];
        if self
            .review_object_targets(&context.task)?
            .iter()
            .any(|target| {
                self.effective_review_for_target(&evidence.task_id, target, &event.actor_user_id)
                    .is_none()
            })
            || self
                .review_revision_commits
                .get(&evidence.assignment_id)
                .is_some_and(|commit| {
                    commit.missing_objects != submission.locations
                        || commit.reviews.last() != Some(&submission.review)
                })
        {
            return Err(invalid(
                "evidence requires the complete final review target set",
            ));
        }
        self.missing_object_evidence
            .insert(evidence.review_id.clone(), evidence.clone());
        self.missing_object_submissions
            .insert(evidence.assignment_id.clone(), submission.clone());
        Ok(())
    }
}
