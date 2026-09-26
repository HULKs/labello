use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrelabelScope {
    pub task_id: Option<TaskId>,
    pub config_id: Option<PrelabelConfigId>,
}

impl PrelabelScope {
    pub fn matches(&self, task: &TaskId, config: &PrelabelConfigId) -> bool {
        self.task_id.as_ref().is_none_or(|id| id == task)
            && self.config_id.as_ref().is_none_or(|id| id == config)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelGeneration {
    pub generation: u64,
    pub scope_generation: u64,
    pub paused: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelResponse {
    pub generation: PrelabelGeneration,
    pub suggestions: Vec<PrelabelSuggestion>,
    pub execution: Option<PrelabelExecutionKind>,
    pub from_batch: bool,
    pub browser_grant: Option<BrowserPrelabelGrant>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserPrelabelGrant {
    pub dataset_id: DatasetId,
    pub image_id: ImageId,
    pub image_hash: String,
    pub task_id: TaskId,
    pub config_id: PrelabelConfigId,
    pub config_digest: String,
    pub model_digest: String,
    pub generation: PrelabelGeneration,
    pub expires_at: crate::Timestamp,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserPrelabelResult {
    pub grant: BrowserPrelabelGrant,
    pub execution: PrelabelExecutionKind,
    pub suggestions: Vec<PrelabelSuggestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrelabelRunPhase {
    Ready,
    Running,
    Completed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrelabelItemOutcome {
    Pending,
    Generated,
    Empty,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelRunSummary {
    pub run_id: String,
    pub phase: PrelabelRunPhase,
    pub created_at: crate::Timestamp,
    pub updated_at: crate::Timestamp,
    pub total: usize,
    #[serde(default)]
    pub reusable: usize,
    #[serde(default)]
    pub ineligible: usize,
    pub generated: usize,
    pub empty: usize,
    pub skipped: usize,
    pub failed: usize,
    pub pending: usize,
    pub blockers: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelAdminState {
    pub runs: Vec<PrelabelRunSummary>,
    pub paused_scopes: Vec<PrelabelScope>,
    pub retained_results: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrelabelAdminCommand {
    Preflight {
        mappings: BTreeMap<TaskId, PrelabelConfigId>,
    },
    Start {
        run_id: String,
    },
    Cancel {
        run_id: String,
    },
    Retry {
        run_id: String,
    },
    Reset {
        scope: PrelabelScope,
    },
    Resume {
        scope: PrelabelScope,
    },
}

/// Remaining work is selected without claiming assignments or changing task state.
pub fn prelabel_task_eligible(task: &TaskDefinition, state: &crate::ImageState) -> bool {
    task.enabled
        && state.import_coverage.get(&task.task_id) != Some(&crate::ImportCoverage::Excluded)
        && state.task_states.get(&task.task_id).is_none_or(|task| {
            matches!(
                task.status,
                crate::TaskStatus::Pending
                    | crate::TaskStatus::InProgress
                    | crate::TaskStatus::NeedsCorrection
            )
        })
}
