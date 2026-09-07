use crate::{
    AnnotationGeometry, AnnotationType, AnnotationVersion, ExportPolicyError, ExportPoseBox,
    ImageDimensions, ImageState, RevisionSource, export_pose_bounds,
};

impl ImageState {
    /// Resolve explicit identities only. Nearby boxes and display names are not links.
    pub fn export_pose_box(
        &self,
        pose: &AnnotationVersion,
        dimensions: ImageDimensions,
    ) -> Result<ExportPoseBox, ExportPolicyError> {
        if pose.deleted || self.current_annotation(&pose.annotation_id) != Some(pose) {
            return Err(ExportPolicyError::InvalidGeometry);
        }
        let AnnotationGeometry::Skeleton(geometry) = &pose.geometry else {
            return Err(ExportPolicyError::InvalidGeometry);
        };
        let usable = |annotation: &&AnnotationVersion| {
            !annotation.deleted
                && annotation.class_id == pose.class_id
                && annotation.annotation_type == AnnotationType::BoundingBox
                && !self.effective_reviews_for_task(&annotation.task_id).any(|review| {
                    review.decision == crate::ReviewDecision::Rejected
                        && matches!(&review.target, crate::ReviewTarget::AnnotationVersion { annotation_id, version }
                            if annotation_id == &annotation.annotation_id && *version == annotation.version)
                })
                && matches!(annotation.geometry, AnnotationGeometry::BoundingBox(bounds) if bounds.validate().is_ok())
                && match &annotation.revision_source {
                    RevisionSource::PrelabelSuggestion { .. } => false,
                    RevisionSource::MigrationSkeleton {
                        annotation_id,
                        version,
                    } => annotation_id == &pose.annotation_id && *version == pose.version,
                    _ => true,
                }
        };
        let linked = if let Some(set) = self.migration_target_sets.get(&pose.task_id) {
            // Confirmation binds the current guide, disposition and skeleton versions.
            self.validate_migration_terminal(&pose.task_id)
                .map_err(|_| ExportPolicyError::InvalidGeometry)?;
            if let Some(target) = set.targets.iter().find(|target| {
                target.reserved_skeleton_annotation_id == pose.annotation_id
                    && pose.object_group_id.as_ref() == Some(&target.object_group_id)
            }) {
                self.current_annotation(&target.guide_annotation_id)
                    .filter(usable)
            } else {
                self.migration_companion_box(&pose.annotation_id)
                    .filter(usable)
            }
        } else if let Some(group) = &pose.object_group_id {
            let mut candidates = self.active_annotations().filter(|annotation| {
                annotation.object_group_id.as_ref() == Some(group) && usable(annotation)
            });
            let first = candidates.next();
            if candidates.next().is_some() {
                return Err(ExportPolicyError::AmbiguousBox);
            }
            first
        } else {
            None
        };
        let linked_bounds = linked.and_then(|annotation| match annotation.geometry {
            AnnotationGeometry::BoundingBox(bounds) => Some(bounds),
            _ => None,
        });
        let (bounds, derived) = export_pose_bounds(geometry, dimensions, linked_bounds)?;
        Ok(ExportPoseBox {
            bounds,
            derived,
            linked_annotation: linked
                .map(|annotation| (annotation.annotation_id.clone(), annotation.version)),
        })
    }
}

#[cfg(test)]
mod tests;
