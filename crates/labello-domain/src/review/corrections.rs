use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationGeometry, AnnotationId, ClassId, CorrectionId, MigrationExclusionReason,
    ObjectGroupId, ReviewRound,
};

/// A draft is bound to the captured submission and its complete target set.
/// Its correction ID and contents remain unchanged while retrying publication.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewCorrectionSubmission {
    pub correction_id: CorrectionId,
    pub round: ReviewRound,
    pub target_fingerprint: String,
    pub changes: Vec<ReviewCorrectionChange>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewCorrectionChange {
    Edit {
        annotation_id: AnnotationId,
        expected_version: u32,
        geometry: AnnotationGeometry,
    },
    Add {
        annotation_id: AnnotationId,
        class_id: ClassId,
        geometry: AnnotationGeometry,
    },
    Remove {
        annotation_id: AnnotationId,
        expected_version: u32,
    },
    MigrationObject {
        object_group_id: ObjectGroupId,
        expected_disposition_version: u32,
        replacement: MigrationReviewCorrection,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MigrationReviewCorrection {
    Skeleton {
        skeleton: crate::SkeletonGeometry,
    },
    Exclude {
        reason: MigrationExclusionReason,
        note: Option<String>,
    },
}

impl ReviewCorrectionSubmission {
    pub fn validate(&self) -> crate::DomainResult<()> {
        self.correction_id
            .validate_path_segment()
            .map_err(|error| crate::DomainError::InvalidReviewerCorrection(error.to_string()))?;
        if self.changes.is_empty()
            || self.changes.len() > 10_000
            || self.target_fingerprint.len() > 256
            || self
                .reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 2_000)
        {
            return Err(crate::DomainError::InvalidReviewerCorrection(
                "correction submission must contain 1 to 10000 bounded changes".into(),
            ));
        }
        Ok(())
    }
}

impl ReviewCorrectionSubmission {
    pub(crate) fn matches_applied_changes(
        &self,
        state: &crate::ImageState,
        task: &crate::TaskId,
        actor: &crate::UserId,
        timestamp: crate::Timestamp,
    ) -> bool {
        let authored = |annotation: &crate::AnnotationVersion| {
            !annotation.deleted
                && &annotation.task_id == task
                && &annotation.author_user_id == actor
                && annotation.updated_at == timestamp
                && matches!(&annotation.revision_source, crate::RevisionSource::ReviewerCorrection { correction_id } if correction_id == &self.correction_id)
        };
        self.changes.iter().all(|change| match change {
            ReviewCorrectionChange::Edit {
                annotation_id,
                expected_version,
                geometry,
            } => state
                .current_annotation(annotation_id)
                .is_some_and(|annotation| {
                    authored(annotation)
                        && expected_version.checked_add(1) == Some(annotation.version)
                        && &annotation.geometry == geometry
                }),
            ReviewCorrectionChange::Add {
                annotation_id,
                class_id,
                geometry,
            } => state
                .current_annotation(annotation_id)
                .is_some_and(|annotation| {
                    authored(annotation)
                        && annotation.version == 1
                        && &annotation.class_id == class_id
                        && &annotation.geometry == geometry
                }),
            ReviewCorrectionChange::Remove {
                annotation_id,
                expected_version,
            } => state
                .current_annotation(annotation_id)
                .is_some_and(|annotation| {
                    &annotation.task_id == task
                        && annotation.deleted
                        && annotation.version == *expected_version
                }),
            ReviewCorrectionChange::MigrationObject {
                object_group_id,
                expected_disposition_version,
                replacement,
            } => {
                let Some(disposition) = state
                    .migration_dispositions
                    .get(task)
                    .and_then(|items| items.get(object_group_id))
                else {
                    return false;
                };
                // Deleting a canonical skeleton first reopens its disposition;
                // recording the exclusion then advances it once more.
                let advances = disposition
                    .disposition_version
                    .checked_sub(*expected_disposition_version);
                if advances != Some(1)
                    && !(advances == Some(2)
                        && matches!(replacement, MigrationReviewCorrection::Exclude { .. }))
                {
                    return false;
                }
                match (&disposition.status, replacement) {
                    (
                        crate::MigrationDispositionStatus::Annotated {
                            skeleton_annotation_id,
                            skeleton_version,
                        },
                        MigrationReviewCorrection::Skeleton { skeleton },
                    ) => state
                        .current_annotation(skeleton_annotation_id)
                        .is_some_and(|annotation| {
                            authored(annotation)
                                && annotation.object_group_id.as_ref() == Some(object_group_id)
                                && annotation.version == *skeleton_version
                                && annotation.geometry
                                    == AnnotationGeometry::Skeleton(skeleton.clone())
                        }),
                    (
                        crate::MigrationDispositionStatus::Excluded { exclusion },
                        MigrationReviewCorrection::Exclude { reason, note },
                    ) => {
                        exclusion.reason == *reason
                            && exclusion.note == *note
                            && &exclusion.actor_user_id == actor
                            && exclusion.timestamp == timestamp
                    }
                    _ => false,
                }
            }
        })
    }
}
