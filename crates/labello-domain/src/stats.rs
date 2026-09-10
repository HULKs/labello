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

pub const DAILY_LABEL_GOAL: usize = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabelStreak {
    pub days: usize,
    pub labeled_today: usize,
}

impl LabelStreak {
    pub fn lit(self) -> bool {
        self.labeled_today >= DAILY_LABEL_GOAL
    }
}

impl ContributorStats {
    /// UTC submission days; yesterday's streak remains available to extend today.
    pub fn label_streak(&self, today: chrono::NaiveDate) -> LabelStreak {
        let days: BTreeMap<chrono::NaiveDate, usize> = self
            .history
            .iter()
            .filter_map(|day| day.day.parse().ok().map(|date| (date, day.labeled)))
            .collect();
        let mut streak = LabelStreak {
            labeled_today: days.get(&today).copied().unwrap_or_default(),
            ..Default::default()
        };
        let mut cursor = if streak.lit() {
            Some(today)
        } else {
            today.pred_opt()
        };
        while let Some(day) = cursor {
            if days.get(&day).copied().unwrap_or_default() < DAILY_LABEL_GOAL {
                break;
            }
            streak.days += 1;
            cursor = day.pred_opt();
        }
        streak
    }
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
    fn streak_requires_twenty_and_expires_only_after_a_missed_utc_day() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let mut person = ContributorStats {
            history: [
                ("2025-12-31", 20),
                ("2025-12-30", 25),
                ("2025-12-28", 30),
                ("2026-01-02", 50),
                ("invalid", 50),
                ("2026-01-01", 19),
            ]
            .into_iter()
            .map(|(day, labeled)| ContributorDay {
                day: day.into(),
                labeled,
                ..Default::default()
            })
            .collect(),
            ..Default::default()
        };
        person.history.last_mut().unwrap().reviewed = 100;
        assert_eq!(
            person.label_streak(today),
            LabelStreak {
                days: 2,
                labeled_today: 19
            }
        );
        assert!(!person.label_streak(today).lit());
        person.history.last_mut().unwrap().labeled = 20;
        assert_eq!(
            person.label_streak(today),
            LabelStreak {
                days: 3,
                labeled_today: 20
            }
        );
        assert!(person.label_streak(today).lit());
        assert_eq!(
            person.label_streak(today + chrono::Days::new(4)),
            LabelStreak::default()
        );
        assert_eq!(
            ContributorStats::default().label_streak(today),
            LabelStreak::default()
        );
    }

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
