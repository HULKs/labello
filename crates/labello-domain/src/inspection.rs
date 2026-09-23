use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReturnToReviewRequest {
    pub request_id: EventId,
    pub expected_sequence: u64,
    pub task_ids: Vec<TaskId>,
    pub reason: String,
}

impl ReturnToReviewRequest {
    pub fn validate(&self) -> DomainResult<()> {
        self.request_id
            .validate_path_segment()
            .map_err(|_| DomainError::InvalidReviewRevision("invalid identifier".into()))?;
        if self.reason.trim().is_empty()
            || self.reason.len() > 2000
            || self.task_ids.is_empty()
            || self.task_ids.len() > 100
            || self
                .task_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.task_ids.len()
        {
            return Err(DomainError::InvalidReviewRevision(
                "select 1–100 distinct workflows and enter a reason of at most 2000 bytes".into(),
            ));
        }
        for id in &self.task_ids {
            id.validate_path_segment()
                .map_err(|_| DomainError::InvalidReviewRevision("invalid identifier".into()))?;
        }
        Ok(())
    }
}

/// Shared eligibility explanation for the inspector and the validated transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnToReviewBlock {
    Disabled,
    ReviewDisabled,
    NotCompleted,
    Assigned,
    InvalidTargets,
}

impl ReturnToReviewBlock {
    pub fn message(self) -> &'static str {
        match self {
            Self::Disabled => "This workflow is disabled.",
            Self::ReviewDisabled => "Approval review is not enabled for this workflow.",
            Self::NotCompleted => "Only completed workflows can be returned to review.",
            Self::Assigned => "An active assignment must finish or expire first.",
            Self::InvalidTargets => {
                "Review targets are incomplete. Refresh the image or repair the workflow."
            }
        }
    }
}

impl ImageState {
    pub fn return_to_review_block(
        &self,
        task: &TaskDefinition,
        timestamp: Timestamp,
    ) -> Option<ReturnToReviewBlock> {
        if !task.enabled {
            return Some(ReturnToReviewBlock::Disabled);
        }
        if task.review.workflow != ReviewWorkflow::Approval {
            return Some(ReturnToReviewBlock::ReviewDisabled);
        }
        if self
            .task_states
            .get(&task.task_id)
            .is_none_or(|s| s.status != TaskStatus::Completed)
        {
            return Some(ReturnToReviewBlock::NotCompleted);
        }
        if self.assignments.iter().any(|a| {
            a.task_id == task.task_id
                && a.status == AssignmentStatus::Active
                && a.expires_at
                    .unwrap_or(a.updated_at + std::time::Duration::from_secs(1800))
                    > timestamp
        }) {
            return Some(ReturnToReviewBlock::Assigned);
        }
        self.review_targets(task)
            .err()
            .map(|_| ReturnToReviewBlock::InvalidTargets)
    }

    pub fn validate_return_to_review(
        &self,
        request: &ReturnToReviewRequest,
        tasks: &[TaskDefinition],
        timestamp: Timestamp,
    ) -> DomainResult<()> {
        request.validate()?;
        let invalid = || {
            DomainError::InvalidReviewRevision(
                "work changed, is assigned, or is not completed approval work".into(),
            )
        };
        if request.expected_sequence != self.current_sequence
            || tasks.len() != request.task_ids.len()
        {
            return Err(invalid());
        }
        for (task, id) in tasks.iter().zip(&request.task_ids) {
            if task.task_id != *id || self.return_to_review_block(task, timestamp).is_some() {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub(crate) fn apply_return_to_review(
        &mut self,
        request: &ReturnToReviewRequest,
        tasks: &[TaskDefinition],
        event: &EventLogEntry,
    ) -> DomainResult<()> {
        if !matches!(
            event.actor_role,
            DatasetRole::Reviewer | DatasetRole::DataAdmin
        ) {
            return Err(DomainError::InvalidReviewRevision(
                "return to review requires reviewer or data-admin role".into(),
            ));
        }
        self.validate_return_to_review(request, tasks, event.timestamp)?;
        let mut next = self.clone();
        for id in &request.task_ids {
            next.apply_task_state(&TaskState {
                task_id: id.clone(),
                status: TaskStatus::Submitted,
                outcome: None,
                assigned_to: None,
                completed_by: None,
                completed_at: None,
                updated_at: event.timestamp,
            })?;
        }
        *self = next;
        Ok(())
    }
}
