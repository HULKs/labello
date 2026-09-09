use super::*;
use labello_domain::{ReviewDecision, ReviewTarget};

impl LabelloApp {
    pub(crate) fn review_object_targets(&self) -> Vec<ReviewTarget> {
        self.selected_task()
            .and_then(|task| {
                self.work
                    .current_state
                    .as_ref()?
                    .review_object_targets(task)
                    .ok()
            })
            .unwrap_or_default()
    }

    pub(crate) fn review_position(&self) -> usize {
        if self.manual_migration_active() {
            self.work.migration.review_index
        } else {
            self.work.review_index
        }
    }

    pub(crate) fn focused_review_target(&self) -> Option<ReviewTarget> {
        self.review_object_targets()
            .get(self.review_position())
            .cloned()
    }

    pub(crate) fn review_overview(&self) -> bool {
        self.review_position() == self.review_object_targets().len()
    }

    fn review_target_decided(&self, target: &ReviewTarget) -> bool {
        if self
            .work
            .review_corrections
            .changes
            .iter()
            .any(|change| self.change_matches_target(change, target))
        {
            return true;
        }
        if self.work.review_corrections.needs_review.contains(target) {
            return false;
        }
        if self.work.review_corrections.reviewed.contains(target) {
            return true;
        }
        if self.review_revision_active() {
            return self.staged_review_decision(target).is_some();
        }
        self.selected_task()
            .and_then(|task| {
                self.work
                    .current_state
                    .as_ref()?
                    .effective_review_for_target(&task.task_id, target, &self.config.user_id)
            })
            .is_some_and(|review| review.decision == ReviewDecision::Approved)
    }

    pub(crate) fn all_review_items_decided(&self) -> bool {
        self.review_object_targets()
            .iter()
            .all(|target| self.review_target_decided(target))
    }

    pub(crate) fn next_review_position(&self) -> usize {
        let targets = self.review_object_targets();
        targets
            .iter()
            .position(|target| !self.review_target_decided(target))
            .unwrap_or(targets.len())
    }

    pub(crate) fn finish_local_review_item(&mut self) {
        if let Some(target) = self.focused_review_target() {
            self.work
                .review_corrections
                .needs_review
                .retain(|old| old != &target);
            if !self.work.review_corrections.reviewed.contains(&target) {
                self.work.review_corrections.reviewed.push(target);
            }
        }
        self.discard_correction();
        self.set_review_position(self.next_review_position());
    }

    fn set_review_position(&mut self, position: usize) {
        self.work.review_index = position;
        self.work.migration.review_index = position;
        self.work.review_corrections.position = Some(position);
        self.work.selected_annotation = None;
        self.work.canvas.exit_pan_mode();
    }

    pub(crate) fn navigate_review_item(&mut self, position: usize) {
        if self.loading.saving
            || self.loading.image
            || self.work.migration.busy
            || self.work.pending_transition.is_some()
            || self.work.review_corrections.submission.is_some()
        {
            return;
        }
        if !self.retain_review_editor() {
            return;
        }
        self.set_review_position(position.min(self.review_object_targets().len()));
        self.sync_review_editor();
    }

    pub(crate) fn cycle_review_item(&mut self, delta: isize) {
        self.navigate_review_item(self.review_position().saturating_add_signed(delta));
    }

    pub(crate) fn review_editor_changed(&self) -> bool {
        self.work
            .correction_draft
            .as_ref()
            .is_some_and(|draft| draft.expected_version == 0 || draft.geometry_changed())
    }

    pub(crate) fn change_matches_target(
        &self,
        change: &ReviewCorrectionChange,
        target: &ReviewTarget,
    ) -> bool {
        match (change, target) {
            (
                ReviewCorrectionChange::Edit {
                    annotation_id: a, ..
                }
                | ReviewCorrectionChange::Add {
                    annotation_id: a, ..
                }
                | ReviewCorrectionChange::Remove {
                    annotation_id: a, ..
                },
                ReviewTarget::AnnotationVersion {
                    annotation_id: b, ..
                },
            ) => a == b,
            (
                ReviewCorrectionChange::MigrationObject {
                    object_group_id: a, ..
                },
                ReviewTarget::MigrationDisposition {
                    object_group_id: b, ..
                },
            ) => a == b,
            (
                ReviewCorrectionChange::MigrationObject {
                    object_group_id, ..
                },
                ReviewTarget::AnnotationVersion { annotation_id, .. },
            ) => self
                .work
                .current_state
                .as_ref()
                .and_then(|state| state.current_annotation(annotation_id))
                .is_some_and(|annotation| {
                    annotation.object_group_id.as_ref() == Some(object_group_id)
                }),
            _ => false,
        }
    }

    pub(crate) fn focused_review_changed(&self) -> bool {
        if self.review_overview() {
            return self.has_review_corrections();
        }
        if self.work.correction_draft.is_some() {
            return self.review_editor_changed();
        }
        self.focused_review_target().is_some_and(|target| {
            self.work
                .review_corrections
                .changes
                .iter()
                .any(|change| self.change_matches_target(change, &target))
        })
    }

    pub(crate) fn review_editor_valid(&self) -> bool {
        if !self.review_editor_changed() {
            return true;
        }
        let Some(editor) = &self.work.review_corrections.editor else {
            return true;
        };
        let (Some(draft), Some(task), Some(image)) = (
            &self.work.correction_draft,
            self.selected_task(),
            &self.work.current,
        ) else {
            return false;
        };
        let mut annotation = editor.clone();
        annotation.version = annotation.version.max(1);
        annotation.geometry = draft.edited_geometry.clone();
        annotation
            .validate_for_task(task, image.image.dimensions())
            .is_ok()
            && (task.manual_box_guide_migration.is_none()
                || match &annotation.geometry {
                    AnnotationGeometry::Skeleton(skeleton) => {
                        labello_domain::validate_manual_migration_skeleton(skeleton).is_ok()
                    }
                    _ => false,
                })
    }

    pub(crate) fn review_can_approve(&self) -> bool {
        !self.focused_review_changed()
            && self.review_editor_valid()
            && (!self.review_overview() || self.all_review_items_decided())
            && self.work.review_corrections.submission.is_none()
    }

    pub(crate) fn review_can_reject(&self) -> bool {
        self.focused_review_changed()
            && self.review_editor_valid()
            && (!self.review_overview() || self.all_review_items_decided())
    }

    pub(crate) fn retain_review_editor(&mut self) -> bool {
        if !self.review_editor_valid() {
            self.runtime.error = Some("Finish or reset this annotation before continuing.".into());
            return false;
        }
        if self.work.correction_draft.is_some() {
            self.stage_review_correction();
        }
        self.work.correction_draft.is_none()
    }

    pub(crate) fn reject_review_item(&mut self) -> bool {
        if !self.review_can_reject()
            || self.loading.saving
            || self.loading.image
            || self.work.migration.busy
            || self.work.pending_transition.is_some()
        {
            return false;
        }
        let overview = self.review_overview();
        if !self.retain_review_editor() {
            return false;
        }
        if overview {
            self.submit_staged_review_corrections()
        } else {
            self.finish_local_review_item();
            true
        }
    }

    pub(crate) fn reset_review_item(&mut self) {
        if self.work.review_corrections.submission.is_some()
            || self.loading.saving
            || self.work.migration.busy
        {
            return;
        }
        if let Some(target) = self.focused_review_target() {
            let retained = self
                .work
                .review_corrections
                .changes
                .iter()
                .filter(|change| !self.change_matches_target(change, &target))
                .cloned()
                .collect();
            self.work.review_corrections.changes = retained;
            self.work
                .review_corrections
                .reviewed
                .retain(|old| old != &target);
            if !self.work.review_corrections.needs_review.contains(&target) {
                self.work
                    .review_corrections
                    .needs_review
                    .push(target.clone());
            }
            self.work
                .staged_review_decisions
                .retain(|review| review.target != target);
        } else if let Some(editor) = &self.work.review_corrections.editor {
            let id = editor.annotation_id.clone();
            self.work.review_corrections.changes.retain(|change| !matches!(change, ReviewCorrectionChange::Add { annotation_id, .. } if annotation_id == &id));
        }
        self.discard_correction();
        self.sync_review_editor();
    }

    pub(crate) fn discard_all_review_corrections(&mut self) {
        if self.work.review_corrections.submission.is_some() {
            return;
        }
        let targets = self.review_object_targets();
        for target in targets {
            if self
                .work
                .review_corrections
                .changes
                .iter()
                .any(|change| self.change_matches_target(change, &target))
                || self.focused_review_target().as_ref() == Some(&target)
                    && self.review_editor_changed()
            {
                self.work
                    .review_corrections
                    .reviewed
                    .retain(|old| old != &target);
                self.work
                    .staged_review_decisions
                    .retain(|review| review.target != target);
                if !self.work.review_corrections.needs_review.contains(&target) {
                    self.work.review_corrections.needs_review.push(target);
                }
            }
        }
        self.work.review_corrections.changes.clear();
        self.work.review_corrections.reason.clear();
        self.discard_correction();
        self.set_review_position(self.next_review_position());
        self.sync_review_editor();
    }

    pub(crate) fn sync_review_editor(&mut self) {
        if self.view != AppView::Review
            || self.work.assignment.is_none()
            || self.work.correction_draft.is_some()
            || self.work.review_corrections.submission.is_some()
            || self.loading.image
            || self.loading.saving
            || self.work.migration.busy
        {
            return;
        }
        let Some(target) = self.focused_review_target() else {
            return;
        };
        if self.work.review_corrections.changes.iter().any(|change| {
            self.change_matches_target(change, &target)
                && matches!(
                    change,
                    ReviewCorrectionChange::Remove { .. }
                        | ReviewCorrectionChange::MigrationObject {
                            replacement: MigrationReviewCorrection::Exclude { .. },
                            ..
                        }
                )
        }) {
            return;
        }
        let annotation = self
            .work
            .current_state
            .as_ref()
            .and_then(|state| match &target {
                ReviewTarget::AnnotationVersion { annotation_id, .. } => {
                    state.current_annotation(annotation_id)
                }
                ReviewTarget::MigrationDisposition {
                    task_id,
                    object_group_id,
                    ..
                } => {
                    let target = state
                        .migration_target_sets
                        .get(task_id)?
                        .targets
                        .iter()
                        .find(|target| &target.object_group_id == object_group_id)?;
                    state
                        .current_annotation(&target.reserved_skeleton_annotation_id)
                        .filter(|annotation| !annotation.deleted)
                }
                _ => None,
            })
            .cloned();
        if let Some(annotation) = annotation {
            self.begin_review_correction(annotation);
        }
    }

    pub(crate) fn select_review_annotation(&mut self, id: &AnnotationId) {
        let target_index = self
            .review_object_targets()
            .iter()
            .position(|target| match target {
                ReviewTarget::AnnotationVersion { annotation_id, .. } => annotation_id == id,
                ReviewTarget::MigrationDisposition {
                    task_id,
                    object_group_id,
                    ..
                } => self
                    .work
                    .current_state
                    .as_ref()
                    .and_then(|state| state.migration_target_sets.get(task_id))
                    .is_some_and(|set| {
                        set.targets.iter().any(|target| {
                            &target.object_group_id == object_group_id
                                && (&target.reserved_skeleton_annotation_id == id
                                    || &target.guide_annotation_id == id)
                        })
                    }),
                _ => false,
            });
        if let Some(index) = target_index {
            self.navigate_review_item(index);
        } else if self.retain_review_editor() {
            let mut previews = Vec::new();
            self.apply_staged_review_previews(&mut previews);
            if let Some(annotation) = previews
                .into_iter()
                .find(|annotation| &annotation.annotation_id == id)
            {
                self.begin_review_correction(annotation);
            }
        }
    }
}
