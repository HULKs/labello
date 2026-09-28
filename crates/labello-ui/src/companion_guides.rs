use labello_domain::{AnnotationGeometry, AnnotationType, AnnotationVersion, RevisionSource};

use crate::app::{AppView, LabelloApp};

impl LabelloApp {
    pub(crate) fn companion_source(
        &self,
        annotation: &AnnotationVersion,
    ) -> Option<&AnnotationVersion> {
        if !self.is_migration_companion_box(annotation) {
            return None;
        }
        let state = self.work.current_state.as_ref()?;
        let link = state
            .migration_companions
            .values()
            .find(|link| link.box_annotation_id == annotation.annotation_id)?;
        // Show the exact source that generated this companion, even if the source
        // has since changed and needs explicit reconciliation.
        state
            .annotations
            .get(&link.skeleton_annotation_id)?
            .iter()
            .find(|source| {
                source.version == link.skeleton_version
                    && !source.deleted
                    && source.task_id == link.migration_task_id
                    && source.class_id == link.class_id
                    && matches!(&source.geometry, AnnotationGeometry::Skeleton(skeleton)
                    if skeleton.keypoints.iter().any(|point| point.point.is_some()))
            })
    }

    pub(crate) fn companion_needs_box(&self, annotation: &AnnotationVersion) -> bool {
        if self.view != AppView::Annotate
            || !self.annotation_matches_selected_workflow(annotation)
            || self
                .work_box_exclusions()
                .contains_key(&annotation.annotation_id)
        {
            return false;
        }
        let Some(source) = self.companion_source(annotation) else {
            return false;
        };
        let Some(state) = self.work.current_state.as_ref() else {
            return false;
        };
        state.migration_companion_is_derived(&source.annotation_id)
            && state.current_annotation(&annotation.annotation_id) == Some(annotation)
            && matches!(
                annotation.revision_source,
                RevisionSource::MigrationSkeleton { .. }
            )
    }

    pub(crate) fn advance_companion_guide(&mut self) -> bool {
        let selected_pending = self.work.annotations.iter().any(|annotation| {
            Some(&annotation.annotation_id) == self.work.selected_annotation.as_ref()
                && self.companion_needs_box(annotation)
        });
        if selected_pending {
            self.runtime.notice =
                Some("Draw a bounding box for this guide before continuing.".into());
            return true;
        }
        let next = self
            .annotation_objects()
            .iter()
            .find(|annotation| self.companion_needs_box(annotation))
            .map(|annotation| annotation.annotation_id.clone());
        let Some(next) = next else {
            return false;
        };
        self.work.selected_annotation = Some(next);
        self.runtime.error = None;
        self.runtime.notice = None;
        if matches!(
            self.work.save_status,
            crate::app::SaveStatus::Dirty | crate::app::SaveStatus::Retry
        ) {
            self.request_save(false);
        }
        true
    }

    pub(crate) fn companion_guide(&self, annotation: &AnnotationVersion) -> AnnotationVersion {
        if self.companion_needs_box(annotation)
            && let Some(source) = self.companion_source(annotation)
        {
            let mut guide = annotation.clone();
            // Projection only: selection retains the box identity; saving still
            // operates on the original bounding-box annotation in work state.
            guide.geometry = source.geometry.clone();
            guide.annotation_type = AnnotationType::Skeleton;
            return guide;
        }
        annotation.clone()
    }
}
