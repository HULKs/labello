//! Object and overview work share a task, but have separate leases and completion.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationId, Assignment, AssignmentId, ObjectGroupId, PrelabelSuggestion, ReviewRecord,
    ReviewTarget, TaskId,
};

mod edits;
mod policy;
pub use edits::*;
#[cfg(test)]
mod tests;

pub const DEFAULT_WORKFLOW_HISTORY_DEPTH: usize = 5;
pub const MAX_WORKFLOW_HISTORY_DEPTH: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct WorkflowQueueConfig {
    pub history_depth: usize,
    /// Images awaiting Overview, not the number of objects they contain.
    pub max_pending_overviews: Option<usize>,
}

impl Default for WorkflowQueueConfig {
    fn default() -> Self {
        Self {
            history_depth: DEFAULT_WORKFLOW_HISTORY_DEPTH,
            max_pending_overviews: None,
        }
    }
}

impl WorkflowQueueConfig {
    pub fn validate(&self) -> crate::DomainResult<()> {
        if self.history_depth > MAX_WORKFLOW_HISTORY_DEPTH || self.max_pending_overviews == Some(0)
        {
            return Err(crate::DomainError::InvalidWorkflow(
                "invalid workflow queue limits".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSelection {
    pub task_id: TaskId,
    pub kind: crate::AssignmentKind,
    pub variant: WorkflowVariant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowItemRef {
    pub image_id: crate::ImageId,
    pub item: WorkflowItem,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowVariant {
    Objects,
    #[default]
    Overview,
}

/// Stable object identity; a skeleton is one object regardless of keypoint count.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowObject {
    Annotation { annotation_id: AnnotationId },
    Migration { object_group_id: ObjectGroupId },
    Prelabel { suggestion_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowItem {
    Object { object: WorkflowObject },
    Overview,
}

impl WorkflowItem {
    pub fn variant(&self) -> WorkflowVariant {
        match self {
            Self::Object { .. } => WorkflowVariant::Objects,
            Self::Overview => WorkflowVariant::Overview,
        }
    }
}

/// Predictions are captured before presentation. Seen entries survive later model changes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPreparation {
    pub objects: Vec<WorkflowObject>,
    pub prelabels: Vec<PrelabelSuggestion>,
    pub config_digest: Option<String>,
    #[serde(default)]
    pub generation_key: Option<String>,
    #[serde(default)]
    pub status: WorkflowPreparationStatus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPreparationStatus {
    #[default]
    Ready,
    Pending,
    Failed,
}

/// Historical multi-model lists resolve in their configured order. New admin
/// edits select a single active model; annotators never select a model.
pub fn workflow_prelabel_config<'a>(
    metadata: &'a crate::DatasetMetadata,
    task: &crate::TaskDefinition,
) -> Option<&'a crate::PrelabelConfig> {
    task.prelabel_config_ids.iter().find_map(|id| {
        metadata.prelabel_configs.iter().find(|c| {
            c.config_id == *id && c.available_to_annotators && c.validate_for_task(task).is_ok()
        })
    })
}

pub fn workflow_prelabel_digest(
    metadata: &crate::DatasetMetadata,
    task: &crate::TaskDefinition,
) -> Option<String> {
    workflow_prelabel_config(metadata, task).map(|config| {
        blake3::hash(&serde_json::to_vec(&(task, config)).expect("serializable workflow model"))
            .to_hex()
            .to_string()
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAssignmentContext {
    pub item: WorkflowItem,
    pub task_fingerprint: String,
    pub overview_fingerprint: Option<String>,
    pub review_target: Option<ReviewTarget>,
    /// The dataset-wide independent-work check authorized this exception.
    pub review_exception: bool,
    pub source_assignment_id: Option<AssignmentId>,
}

/// Model selection changes unused predictions, never the contract of already seen work.
pub fn workflow_task_fingerprint(task: &crate::TaskDefinition) -> String {
    let mut definition = task.clone();
    definition.prelabel_config_ids.clear();
    blake3::hash(&serde_json::to_vec(&definition).expect("serializable task definition"))
        .to_hex()
        .to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAnnotation {
    pub annotation_id: AnnotationId,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowConfirmation {
    pub assignment_id: AssignmentId,
    pub task_id: TaskId,
    /// Exact final geometry, absent for an exclusion or a dismissed prediction.
    pub annotation: Option<WorkflowAnnotation>,
    pub review: Option<ReviewRecord>,
    /// Corrections satisfy object review for the corrected version; Overview still follows.
    pub reviewed_targets: Vec<ReviewTarget>,
}

/// Partial geometry is durable work, but is not yet a confirmed annotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDraft {
    pub task_id: TaskId,
    pub item: WorkflowItem,
    pub geometry: crate::AnnotationGeometry,
    pub sequence: u64,
    pub previous_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSeen {
    pub sequence: u64,
    pub timestamp: crate::Timestamp,
    pub event_id: crate::EventId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowHistoryEntry {
    pub image_id: crate::ImageId,
    pub assignment_id: AssignmentId,
    pub seen_at: crate::Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAvailability {
    pub selection: WorkflowSelection,
    pub available: bool,
    pub reason: Option<crate::WorkflowUnavailableReason>,
    pub split: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowEvent {
    Prepared {
        task_id: TaskId,
        preparation: WorkflowPreparation,
    },
    AssignmentOpened {
        assignment: Assignment,
        context: WorkflowAssignmentContext,
    },
    ItemSeen {
        task_id: TaskId,
        assignment_id: AssignmentId,
    },
    ItemConfirmed {
        confirmation: WorkflowConfirmation,
    },
    DraftSaved {
        task_id: TaskId,
        assignment_id: AssignmentId,
        geometry: crate::AnnotationGeometry,
        expected_sequence: u64,
    },
    EditsSaved {
        task_id: TaskId,
        assignment_id: AssignmentId,
        edits: WorkflowEdits,
        expected_sequence: u64,
    },
}

impl WorkflowEvent {
    pub fn task_id(&self) -> &TaskId {
        match self {
            Self::Prepared { task_id, .. }
            | Self::ItemSeen { task_id, .. }
            | Self::EditsSaved { task_id, .. }
            | Self::DraftSaved { task_id, .. } => task_id,
            Self::AssignmentOpened { assignment, .. } => &assignment.task_id,
            Self::ItemConfirmed { confirmation } => &confirmation.task_id,
        }
    }
}
