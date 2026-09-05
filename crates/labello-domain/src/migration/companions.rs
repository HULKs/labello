use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationGeometry, AnnotationId, AnnotationOrigin, AnnotationType, AnnotationVersion,
    BoundingBox, ClassId, DomainError, DomainResult, ImageDimensions, ImageState, KeypointState,
    RevisionSource, SkeletonGeometry, TaskId,
};

/// A discovery's stable identities and the exact last automatic derivation.
/// Canonical imported object groups are deliberately not used for this link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MigrationCompanion {
    pub migration_task_id: TaskId,
    pub guide_task_id: TaskId,
    pub class_id: ClassId,
    pub skeleton_annotation_id: AnnotationId,
    pub skeleton_version: u32,
    pub box_annotation_id: AnnotationId,
    pub box_version: u32,
}

pub fn migration_skeleton_bounds(
    skeleton: &SkeletonGeometry,
    dimensions: ImageDimensions,
) -> DomainResult<Option<BoundingBox>> {
    dimensions.validate()?;
    let points = skeleton
        .keypoints
        .iter()
        .filter(|keypoint| {
            matches!(
                keypoint.state,
                KeypointState::Visible | KeypointState::Hidden
            )
        })
        .filter_map(|keypoint| keypoint.point)
        .collect::<Vec<_>>();
    let Some(first) = points.first() else {
        return Ok(None);
    };
    let (mut left, mut right, mut top, mut bottom) = (first.x, first.x, first.y, first.y);
    for point in points {
        point.validate()?;
        left = left.min(point.x);
        right = right.max(point.x);
        top = top.min(point.y);
        bottom = bottom.max(point.y);
    }
    let width = (right - left).max(1.0 / dimensions.width as f32).min(1.0);
    let height = (bottom - top).max(1.0 / dimensions.height as f32).min(1.0);
    let bounds = BoundingBox {
        x: ((left + right - width) / 2.0).clamp(0.0, 1.0 - width),
        y: ((top + bottom - height) / 2.0).clamp(0.0, 1.0 - height),
        width,
        height,
    };
    bounds.validate()?;
    Ok(Some(bounds))
}

impl ImageState {
    pub fn migration_companion_box(
        &self,
        skeleton_id: &AnnotationId,
    ) -> Option<&AnnotationVersion> {
        let companion = self.migration_companions.get(skeleton_id)?;
        let annotation = self.current_annotation(&companion.box_annotation_id)?;
        (!annotation.deleted
            && annotation.task_id == companion.guide_task_id
            && annotation.class_id == companion.class_id
            && annotation.annotation_type == AnnotationType::BoundingBox
            && matches!(annotation.geometry, AnnotationGeometry::BoundingBox(bounds) if bounds.validate().is_ok()))
        .then_some(annotation)
    }

    pub fn migration_companion_is_derived(&self, skeleton_id: &AnnotationId) -> bool {
        let Some(companion) = self.migration_companions.get(skeleton_id) else {
            return false;
        };
        self.migration_companion_box(skeleton_id)
            .is_some_and(|annotation| {
                annotation.version == companion.box_version
                    && matches!(&annotation.revision_source,
                    RevisionSource::MigrationSkeleton { annotation_id, version }
                        if annotation_id == skeleton_id && *version == companion.skeleton_version)
                    && !self.reviews.iter().any(|review| match &review.target {
                        crate::ReviewTarget::AnnotationVersion {
                            annotation_id,
                            version,
                        } => {
                            annotation_id == &companion.box_annotation_id
                                && *version == annotation.version
                        }
                        crate::ReviewTarget::Task { task_id } => {
                            task_id == &companion.guide_task_id
                                && review.timestamp >= annotation.updated_at
                        }
                        _ => false,
                    })
            })
    }

    pub fn migration_discovered_skeletons(&self, task_id: &TaskId) -> Vec<&AnnotationVersion> {
        if !self.migration_target_sets.contains_key(task_id) {
            return Vec::new();
        }
        self.active_annotations()
            .filter(|annotation| {
                annotation.task_id == *task_id
                    && annotation.object_group_id.is_none()
                    && annotation.annotation_type == AnnotationType::Skeleton
                    && matches!(
                        annotation.origin,
                        AnnotationOrigin::Native { legacy_v2: false }
                    )
                    && matches!(annotation.revision_source, RevisionSource::Human { .. })
            })
            .collect()
    }

    pub fn migration_discovery_requires_correction(&self, annotation: &AnnotationVersion) -> bool {
        !annotation.deleted && self.reviews.iter().any(|review| {
            review.decision == crate::ReviewDecision::Rejected
                && matches!(&review.target, crate::ReviewTarget::AnnotationVersion { annotation_id, version }
                    if annotation_id == &annotation.annotation_id && *version == annotation.version)
        })
    }

    pub fn migration_discovery_focus(
        &self,
        skeleton_id: &AnnotationId,
        dimensions: ImageDimensions,
    ) -> Option<BoundingBox> {
        if let Some(annotation) = self.migration_companion_box(skeleton_id)
            && let AnnotationGeometry::BoundingBox(bounds) = annotation.geometry
        {
            return Some(bounds);
        }
        let annotation = self.current_annotation(skeleton_id)?;
        let AnnotationGeometry::Skeleton(skeleton) = &annotation.geometry else {
            return None;
        };
        migration_skeleton_bounds(skeleton, dimensions)
            .ok()
            .flatten()
    }

    pub(crate) fn apply_migration_companion(
        &mut self,
        companion: &MigrationCompanion,
    ) -> DomainResult<()> {
        let invalid = || {
            DomainError::InvalidMigration(
                "discovered-object companion identities or versions are inconsistent".into(),
            )
        };
        let set = self
            .migration_target_sets
            .get(&companion.migration_task_id)
            .ok_or_else(invalid)?;
        let skeleton = self
            .current_annotation(&companion.skeleton_annotation_id)
            .ok_or_else(invalid)?;
        let bounding_box = self
            .current_annotation(&companion.box_annotation_id)
            .ok_or_else(invalid)?;
        if companion.guide_task_id != set.guide_task_id
            || companion.migration_task_id == companion.guide_task_id
            || skeleton.annotation_id == bounding_box.annotation_id
            || skeleton.version != companion.skeleton_version
            || bounding_box.version != companion.box_version
            || skeleton.deleted
            || bounding_box.deleted
            || skeleton.task_id != companion.migration_task_id
            || bounding_box.task_id != companion.guide_task_id
            || skeleton.class_id != companion.class_id
            || bounding_box.class_id != companion.class_id
            || skeleton.annotation_type != AnnotationType::Skeleton
            || bounding_box.annotation_type != AnnotationType::BoundingBox
            || skeleton.object_group_id.is_some()
            || bounding_box.object_group_id.is_some()
            || !matches!(
                skeleton.origin,
                AnnotationOrigin::Native { legacy_v2: false }
            )
            || !matches!(skeleton.revision_source, RevisionSource::Human { .. })
            || !matches!(
                bounding_box.origin,
                AnnotationOrigin::Native { legacy_v2: false }
            )
            || !matches!(&bounding_box.revision_source, RevisionSource::MigrationSkeleton { annotation_id, version }
                if annotation_id == &companion.skeleton_annotation_id && *version == companion.skeleton_version)
            || set.targets.iter().any(|target| {
                target.reserved_skeleton_annotation_id == skeleton.annotation_id
                    || target.guide_annotation_id == bounding_box.annotation_id
            })
            || self.migration_companions.values().any(|existing| {
                existing.box_annotation_id == companion.box_annotation_id
                    && existing.skeleton_annotation_id != companion.skeleton_annotation_id
            })
            || self
                .migration_companions
                .get(&companion.skeleton_annotation_id)
                .is_some_and(|existing| {
                    existing.box_annotation_id != companion.box_annotation_id
                        || existing.guide_task_id != companion.guide_task_id
                        || existing.migration_task_id != companion.migration_task_id
                        || existing.class_id != companion.class_id
                        || existing.skeleton_version > companion.skeleton_version
                        || existing.box_version >= companion.box_version
                })
        {
            return Err(invalid());
        }
        self.migration_companions
            .insert(companion.skeleton_annotation_id.clone(), companion.clone());
        self.migration_confirmations
            .remove(&companion.migration_task_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KeypointAnnotation, NormalizedPoint};

    fn skeleton(points: &[(KeypointState, Option<(f32, f32)>)]) -> SkeletonGeometry {
        SkeletonGeometry {
            keypoints: points
                .iter()
                .enumerate()
                .map(|(index, (state, point))| KeypointAnnotation {
                    name: format!("point_{index}"),
                    state: state.clone(),
                    point: point.map(|(x, y)| NormalizedPoint { x, y }),
                })
                .collect(),
        }
    }

    #[test]
    fn bounds_enclose_positioned_visible_and_occluded_points_only() {
        let shape = skeleton(&[
            (KeypointState::Visible, Some((0.1, 0.2))),
            (KeypointState::Hidden, Some((0.8, 0.9))),
            (KeypointState::Absent, None),
        ]);
        let bounds = migration_skeleton_bounds(
            &shape,
            ImageDimensions {
                width: 640,
                height: 480,
            },
        )
        .unwrap()
        .unwrap();
        assert!((bounds.x - 0.1).abs() < 1e-6);
        assert!((bounds.y - 0.2).abs() < 1e-6);
        assert!((bounds.width - 0.7).abs() < 1e-6);
        assert!((bounds.height - 0.7).abs() < 1e-6);
    }

    #[test]
    fn single_and_collinear_points_have_a_pixel_extent_at_every_image_edge() {
        let dimensions = ImageDimensions {
            width: 100,
            height: 200,
        };
        for (x, y) in [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0), (0.5, 0.5)] {
            let bounds = migration_skeleton_bounds(
                &skeleton(&[(KeypointState::Visible, Some((x, y)))]),
                dimensions,
            )
            .unwrap()
            .unwrap();
            bounds.validate().unwrap();
            assert!(bounds.width >= 1.0 / dimensions.width as f32);
            assert!(bounds.height >= 1.0 / dimensions.height as f32);
            assert!(bounds.x <= x && bounds.y <= y);
            assert!(bounds.x + bounds.width + 1e-6 >= x);
            assert!(bounds.y + bounds.height + 1e-6 >= y);
        }
        for points in [[(0.5, 0.2), (0.5, 0.9)], [(0.2, 0.5), (0.9, 0.5)]] {
            let shape = skeleton(&points.map(|point| (KeypointState::Hidden, Some(point))));
            let bounds = migration_skeleton_bounds(&shape, dimensions)
                .unwrap()
                .unwrap();
            assert!(bounds.width >= 0.01 && bounds.height >= 0.005);
            bounds.validate().unwrap();
        }
    }

    #[test]
    fn coordinate_less_legacy_objects_have_no_invented_box() {
        assert_eq!(
            migration_skeleton_bounds(
                &skeleton(&[(KeypointState::Absent, None)]),
                ImageDimensions {
                    width: 1,
                    height: 1
                }
            )
            .unwrap(),
            None
        );
        assert!(
            migration_skeleton_bounds(
                &skeleton(&[(KeypointState::Visible, Some((f32::NAN, 0.2)))]),
                ImageDimensions {
                    width: 1,
                    height: 1
                }
            )
            .is_err()
        );
        assert!(
            migration_skeleton_bounds(
                &skeleton(&[]),
                ImageDimensions {
                    width: 0,
                    height: 1
                }
            )
            .is_err()
        );
        let edge = migration_skeleton_bounds(
            &skeleton(&[(KeypointState::Visible, Some((1.0, 1.0)))]),
            ImageDimensions {
                width: 1,
                height: 1,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            edge,
            BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0
            }
        );
    }
}
