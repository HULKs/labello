use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ClassId, DomainError, DomainResult, ManualBoxGuideMigration, MigrationCardinality,
    MigrationSequence, PrelabelConfigId, TaskId, Timestamp, UserId,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationType {
    BoundingBox,
    Skeleton,
}

impl std::fmt::Display for AnnotationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BoundingBox => "bounding_box",
            Self::Skeleton => "skeleton",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelClass {
    pub class_id: ClassId,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TutorialContent {
    pub title: String,
    pub example_text: String,
    pub example_images: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeypointSpec {
    pub name: String,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonEdge {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonSpec {
    pub keypoints: Vec<KeypointSpec>,
    pub edges: Vec<SkeletonEdge>,
    pub allow_hidden: bool,
    pub allow_absent: bool,
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct ReviewConfig {
    pub workflow: ReviewWorkflow,
    pub allow_reviewer_corrections: bool,
    /// Decoding and fingerprint reproduction only. Current configuration never uses this policy.
    #[schemars(skip)]
    pub legacy: Option<Box<LegacyReviewConfig>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LegacyReviewConfig {
    pub required_reviews: u32,
    pub agreement_threshold: Option<AgreementThreshold>,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            workflow: ReviewWorkflow::Approval,
            allow_reviewer_corrections: false,
            legacy: None,
        }
    }
}

impl Serialize for ReviewConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut fields = serializer
            .serialize_struct("ReviewConfig", if self.legacy.is_some() { 4 } else { 2 })?;
        if let Some(legacy) = &self.legacy {
            fields.serialize_field("requiredReviews", &legacy.required_reviews)?;
        }
        fields.serialize_field("workflow", &self.workflow)?;
        fields.serialize_field("allowReviewerCorrections", &self.allow_reviewer_corrections)?;
        if let Some(legacy) = &self.legacy {
            fields.serialize_field("agreementThreshold", &legacy.agreement_threshold)?;
        }
        fields.end()
    }
}

impl<'de> Deserialize<'de> for ReviewConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            workflow: ReviewWorkflow,
            allow_reviewer_corrections: bool,
            required_reviews: Option<u32>,
            agreement_threshold: Option<AgreementThreshold>,
        }
        let value = serde_json::Value::deserialize(deserializer)?;
        let historical =
            value.get("requiredReviews").is_some() || value.get("agreementThreshold").is_some();
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        let legacy = if historical {
            Some(Box::new(LegacyReviewConfig {
                required_reviews: wire
                    .required_reviews
                    .ok_or_else(|| D::Error::custom("historical review count is missing"))?,
                agreement_threshold: wire.agreement_threshold,
            }))
        } else {
            None
        };
        Ok(Self {
            workflow: wire.workflow,
            allow_reviewer_corrections: wire.allow_reviewer_corrections,
            legacy,
        })
    }
}

impl ReviewConfig {
    pub fn is_current(&self) -> bool {
        self.legacy.is_none()
            && matches!(
                self.workflow,
                ReviewWorkflow::None | ReviewWorkflow::Approval
            )
    }

    pub fn upgrade(&mut self) {
        self.legacy = None;
        if self.workflow == ReviewWorkflow::LegacyIndependentAgreement {
            self.workflow = ReviewWorkflow::Approval;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewWorkflow {
    None,
    Approval,
    #[serde(rename = "independent_agreement")]
    LegacyIndependentAgreement,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgreementThreshold {
    pub metric: AgreementMetric,
    pub threshold: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgreementMetric {
    Iou,
    KeypointMeanDistance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    pub task_id: TaskId,
    pub name: String,
    pub annotation_type: AnnotationType,
    pub class_ids: Vec<ClassId>,
    pub instructions: TutorialContent,
    pub skeleton: Option<SkeletonSpec>,
    pub review: ReviewConfig,
    pub prelabel_config_ids: Vec<PrelabelConfigId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_box_guide_migration: Option<ManualBoxGuideMigration>,
    pub enabled: bool,
}

impl TaskDefinition {
    pub fn allows_class(&self, class_id: &ClassId) -> bool {
        self.class_ids.iter().any(|candidate| candidate == class_id)
    }

    pub fn validate_manual_migration(&self, guide: &TaskDefinition) -> DomainResult<()> {
        let Some(config) = &self.manual_box_guide_migration else {
            return Ok(());
        };
        let valid = self.annotation_type == AnnotationType::Skeleton
            && guide.annotation_type == AnnotationType::BoundingBox
            && config.guide_task_id == guide.task_id
            && self.task_id != guide.task_id
            && self.class_ids.len() == 1
            && self.class_ids == guide.class_ids
            && config.cardinality == MigrationCardinality::ExactlyOne
            && config.sequence == MigrationSequence::ImportedSpatialOrderV1
            && config.allow_exclusion
            && matches!(
                self.review.workflow,
                ReviewWorkflow::None | ReviewWorkflow::Approval
            );
        if valid {
            Ok(())
        } else {
            Err(DomainError::InvalidMigration(format!(
                "task {} has an invalid manual box-guide migration configuration",
                self.task_id
            )))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Submitted,
    Completed,
    NeedsCorrection,
    #[serde(rename = "adjudication_required")]
    LegacyAdjudicationRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    AnnotationCompleted,
    ImportedGroundTruth,
    Approved,
    ReviewerCorrected,
    #[serde(rename = "adjudicated")]
    LegacyAdjudicated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskState {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub outcome: Option<TaskOutcome>,
    pub assigned_to: Option<UserId>,
    pub completed_by: Option<UserId>,
    pub completed_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

impl TaskState {
    pub fn new(task_id: TaskId, timestamp: Timestamp) -> Self {
        Self {
            task_id,
            status: TaskStatus::Pending,
            outcome: None,
            assigned_to: None,
            completed_by: None,
            completed_at: None,
            updated_at: timestamp,
        }
    }
}

#[cfg(test)]
mod review_config_tests {
    use super::*;

    #[test]
    fn historical_review_configuration_preserves_fingerprint_bytes_and_normalizes() {
        for workflow in ["none", "approval", "independent_agreement"] {
            let wire = format!(
                r#"{{"requiredReviews":3,"workflow":"{workflow}","allowReviewerCorrections":false,"agreementThreshold":null}}"#
            );
            let mut config: ReviewConfig = serde_json::from_str(&wire).unwrap();
            assert!(!config.is_current());
            assert_eq!(serde_json::to_string(&config).unwrap(), wire);
            config.upgrade();
            assert!(config.is_current());
            assert_eq!(
                config.workflow,
                if workflow == "none" {
                    ReviewWorkflow::None
                } else {
                    ReviewWorkflow::Approval
                }
            );
            let current = serde_json::to_value(&config).unwrap();
            assert_eq!(current.as_object().unwrap().len(), 2);
            assert!(current.get("requiredReviews").is_none());
            assert!(current.get("agreementThreshold").is_none());
        }
    }
}
