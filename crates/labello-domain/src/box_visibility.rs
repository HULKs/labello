use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationGeometry, AnnotationId, AnnotationVersion, DomainError, DomainResult, ImageState,
    ReviewDecision, ReviewTarget,
};

/// Presentation policy for one image. It never changes annotation or review history.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBoxVisibility {
    pub iou_threshold: f32,
}

impl<'de> Deserialize<'de> for BoundingBoxVisibility {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            iou_threshold: f32,
        }
        let wire = Wire::deserialize(deserializer)?;
        let policy = Self {
            iou_threshold: wire.iou_threshold,
        };
        policy.validate().map_err(serde::de::Error::custom)?;
        Ok(policy)
    }
}

impl Default for BoundingBoxVisibility {
    fn default() -> Self {
        Self { iou_threshold: 0.9 }
    }
}

impl ImageState {
    pub fn bounding_box_exclusions(&self) -> BTreeMap<AnnotationId, AnnotationId> {
        self.bounding_box_visibility
            .map_or_else(BTreeMap::new, |policy| {
                policy
                    .exclusions(self, self.active_annotations())
                    .expect("validated visibility policy")
            })
    }

    /// Current presentation/workflow objects. Export and audit use active_annotations instead.
    pub fn visible_annotations(&self) -> impl Iterator<Item = &AnnotationVersion> {
        let excluded = self.bounding_box_exclusions();
        self.active_annotations()
            .filter(move |annotation| !excluded.contains_key(&annotation.annotation_id))
    }

    pub fn skipped_migration_groups(
        &self,
        task_id: &crate::TaskId,
    ) -> std::collections::BTreeSet<crate::ObjectGroupId> {
        let excluded = self.bounding_box_exclusions();
        self.migration_target_sets
            .get(task_id)
            .into_iter()
            .flat_map(|set| &set.targets)
            .filter(|target| {
                excluded.contains_key(&target.guide_annotation_id)
                    && self
                        .migration_dispositions
                        .get(task_id)
                        .and_then(|items| items.get(&target.object_group_id))
                        .is_some_and(|disposition| {
                            matches!(
                                disposition.status,
                                crate::MigrationDispositionStatus::Pending
                            )
                        })
            })
            .map(|target| target.object_group_id.clone())
            .collect()
    }
}

impl BoundingBoxVisibility {
    pub fn validate(self) -> DomainResult<()> {
        if !self.iou_threshold.is_finite() || !(0.0..=1.0).contains(&self.iou_threshold) {
            return Err(DomainError::InvalidSchemaArtifact(
                "bounding-box visibility IoU threshold must be finite and within [0, 1]".into(),
            ));
        }
        Ok(())
    }

    /// Maps each excluded box to a retained box in the same image and class.
    /// Callers supply the complete image draft before applying workflow/view filters.
    pub fn exclusions<'a>(
        self,
        state: &ImageState,
        annotations: impl IntoIterator<Item = &'a AnnotationVersion>,
    ) -> DomainResult<BTreeMap<AnnotationId, AnnotationId>> {
        self.validate()?;
        let mut candidates = annotations
            .into_iter()
            .filter(|annotation| {
                !annotation.deleted
                    && matches!(annotation.geometry, AnnotationGeometry::BoundingBox(_))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_cached_key(|annotation| {
            (
                std::cmp::Reverse(review_rank(state, annotation)),
                annotation.annotation_id.clone(),
            )
        });
        let mut retained = BTreeMap::<_, Vec<&AnnotationVersion>>::new();
        let mut excluded = BTreeMap::new();
        for candidate in candidates {
            let AnnotationGeometry::BoundingBox(bounds) = candidate.geometry else {
                unreachable!("candidates contain only boxes");
            };
            let class_boxes = retained.entry(&candidate.class_id).or_default();
            let winner = class_boxes.iter().find(|kept| {
                matches!(kept.geometry, AnnotationGeometry::BoundingBox(other)
                    if bounds.iou(other) > self.iou_threshold)
            });
            if let Some(winner) = winner {
                excluded.insert(
                    candidate.annotation_id.clone(),
                    winner.annotation_id.clone(),
                );
            } else {
                class_boxes.push(candidate);
            }
        }
        Ok(excluded)
    }
}

fn review_rank(state: &ImageState, annotation: &AnnotationVersion) -> u8 {
    // Draft geometry may change without a new version until save. Such a draft
    // must not inherit approval of its persisted geometry.
    if state.current_annotation(&annotation.annotation_id) != Some(annotation) {
        return 1;
    }
    let target = ReviewTarget::AnnotationVersion {
        annotation_id: annotation.annotation_id.clone(),
        version: annotation.version,
    };
    match state
        .effective_reviews_for_task(&annotation.task_id)
        .filter(|review| review.target == target)
        .last()
        .map(|review| &review.decision)
    {
        Some(ReviewDecision::Approved) => 2,
        Some(ReviewDecision::Rejected) => 0,
        _ => 1,
    }
}

#[cfg(test)]
mod tests;
