use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationOrigin, AnnotationVersion, ClassId, HumanRevisionKind, ImportGeometryProvenance,
    RevisionSource, TaskId, UserId,
};

mod activity;
pub use activity::{DailyActivityCounts, UtcActivityWindow, daily_activity_from_events};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetStats {
    pub total_images: usize,
    pub completed_tasks: usize,
    pub pending_tasks: usize,
    pub in_progress_tasks: usize,
    pub awaiting_review_tasks: usize,
    pub needs_correction_tasks: usize,
    pub per_task: BTreeMap<TaskId, TaskStats>,
    pub per_class: BTreeMap<ClassId, ClassStats>,
    pub throughput: Vec<ThroughputPoint>,
    #[serde(default)]
    pub provenance: ProvenanceStats,
    #[serde(default)]
    pub migration: MigrationStats,
    #[serde(default)]
    pub import_coverage: ImportCoverageStats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_balance: Option<AssignmentBalanceStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contributors: Option<BTreeMap<UserId, ContributorStats>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContributorStats {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_user_id: Option<String>,
    pub history: Vec<ContributorDay>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContributorDay {
    pub day: String,
    pub labeled: usize,
    pub reviewed: usize,
    pub accepted: usize,
    pub rejected: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentBalanceStats {
    pub annotation_counts: BTreeMap<TaskId, usize>,
    pub review_counts: BTreeMap<TaskId, usize>,
    pub annotation_blocked_tasks: BTreeSet<TaskId>,
    pub review_blocked_tasks: BTreeSet<TaskId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskStats {
    pub completed: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub awaiting_review: usize,
    pub needs_correction: usize,
    #[serde(default)]
    pub provenance: ProvenanceStats,
    #[serde(default)]
    pub migration: MigrationStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassStats {
    pub annotations: usize,
    pub completed_tasks: usize,
    #[serde(default)]
    pub provenance: ProvenanceStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceStats {
    pub imported_direct_annotations: usize,
    pub imported_derived_annotations: usize,
    pub human_authored_annotations: usize,
    pub human_accepted_imports: usize,
    pub reviewer_corrections: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MigrationStats {
    pub expected: usize,
    pub annotated: usize,
    pub excluded: usize,
    pub pending: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportCoverageStats {
    pub complete: usize,
    pub verified_empty: usize,
    pub incomplete: usize,
    pub excluded: usize,
}

impl ProvenanceStats {
    pub fn record_annotation(&mut self, annotation: &AnnotationVersion) {
        match &annotation.origin {
            AnnotationOrigin::Imported { imported } => match &imported.geometry_provenance {
                ImportGeometryProvenance::Direct => self.imported_direct_annotations += 1,
                ImportGeometryProvenance::Derived { .. } => {
                    self.imported_derived_annotations += 1;
                }
            },
            AnnotationOrigin::Native { .. } => {
                if matches!(annotation.revision_source, RevisionSource::Human { .. }) {
                    self.human_authored_annotations += 1;
                }
            }
        }
        match annotation.revision_source {
            RevisionSource::Human {
                action: HumanRevisionKind::AcceptedUnchanged,
            } if matches!(&annotation.origin, AnnotationOrigin::Imported { .. }) => {
                self.human_accepted_imports += 1;
            }
            RevisionSource::ReviewerCorrection { .. } => self.reviewer_corrections += 1,
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThroughputPoint {
    pub day: String,
    pub annotations: usize,
    pub reviews: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_from_older_servers_decode_without_contributor_support() {
        let legacy = serde_json::to_value(DatasetStats::default()).unwrap();
        assert!(legacy.get("contributors").is_none());
        let decoded: DatasetStats = serde_json::from_value(legacy).unwrap();
        assert!(decoded.contributors.is_none());
        let contributor: ContributorStats = serde_json::from_value(serde_json::json!({
            "displayName": "Older server", "history": []
        }))
        .unwrap();
        assert!(contributor.github_user_id.is_none());
    }
}
