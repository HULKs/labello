mod interaction;

use crate::app::{AppView, CorrectionDraft, LabelloApp, UiCommand};
use labello_domain::{
    AnnotationGeometry, AnnotationId, AnnotationType, AnnotationVersion, AssignmentKind,
    BoundingBox, CorrectionId, KeypointAnnotation, KeypointState, MigrationDispositionStatus,
    MigrationExclusionReason, MigrationReviewCorrection, NormalizedPoint, ReviewCorrectionChange,
    ReviewCorrectionSubmission, SkeletonGeometry,
};

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReviewCorrectionsDraft {
    pub(crate) changes: Vec<ReviewCorrectionChange>,
    #[serde(default)]
    pub(crate) reviewed: Vec<labello_domain::ReviewTarget>,
    #[serde(default)]
    pub(crate) needs_review: Vec<labello_domain::ReviewTarget>,
    #[serde(default)]
    pub(crate) position: Option<usize>,
    #[serde(default)]
    pub(crate) reason: String,
    pub(crate) editor: Option<AnnotationVersion>,
    pub(crate) submission: Option<ReviewCorrectionSubmission>,
}

impl LabelloApp {
    pub(crate) fn has_review_corrections(&self) -> bool {
        !self.work.review_corrections.changes.is_empty() || self.review_editor_changed()
    }

    pub(crate) fn can_submit_review_corrections(&self) -> bool {
        self.review_overview()
            && self.all_review_items_decided()
            && self.has_review_corrections()
            && self.review_editor_valid()
    }

    pub(crate) fn begin_review_correction(&mut self, annotation: AnnotationVersion) {
        if self.loading.saving || self.work.review_corrections.submission.is_some() {
            return;
        }
        self.work.selected_annotation = Some(annotation.annotation_id.clone());
        let geometry = self
            .work
            .review_corrections
            .changes
            .iter()
            .find_map(|change| match change {
                ReviewCorrectionChange::Edit {
                    annotation_id,
                    geometry,
                    ..
                }
                | ReviewCorrectionChange::Add {
                    annotation_id,
                    geometry,
                    ..
                } if annotation_id == &annotation.annotation_id => Some(geometry.clone()),
                ReviewCorrectionChange::MigrationObject {
                    object_group_id,
                    replacement: MigrationReviewCorrection::Skeleton { skeleton },
                    ..
                } if annotation.object_group_id.as_ref() == Some(object_group_id) => {
                    Some(AnnotationGeometry::Skeleton(skeleton.clone()))
                }
                _ => None,
            })
            .unwrap_or_else(|| annotation.geometry.clone());
        self.work.correction_draft = Some(CorrectionDraft {
            correction_id: CorrectionId::generate(),
            annotation_id: annotation.annotation_id.clone(),
            expected_version: annotation.version,
            original_geometry: annotation.geometry.clone(),
            edited_geometry: geometry,
            reason: String::new(),
            geometry_history: Vec::new(),
            selected_keypoint: (annotation.annotation_type == AnnotationType::Skeleton)
                .then_some(0),
        });
        self.work.review_corrections.editor = Some(annotation);
        self.runtime.error = None;
    }

    pub(crate) fn stage_review_correction(&mut self) {
        let (Some(editor), Some(draft), Some(task)) = (
            self.work.review_corrections.editor.clone(),
            self.work.correction_draft.clone(),
            self.selected_task().cloned(),
        ) else {
            return;
        };
        if self.loading.saving || self.work.review_corrections.submission.is_some() {
            return;
        }
        if editor.version > 0 && !draft.geometry_changed() {
            let target = self.focused_review_target();
            if let Some(target) = target {
                let changes = self
                    .work
                    .review_corrections
                    .changes
                    .iter()
                    .filter(|change| !self.change_matches_target(change, &target))
                    .cloned()
                    .collect();
                if self.work.review_corrections.changes != changes {
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
                self.work.review_corrections.changes = changes;
            }
            self.discard_correction();
            return;
        }
        let mut annotation = editor.clone();
        annotation.version = editor.version.max(1);
        annotation.geometry = draft.edited_geometry.clone();
        let Some(image) = self.work.current.as_ref() else {
            return;
        };
        if let Err(error) = annotation.validate_for_task(&task, image.image.dimensions()) {
            self.runtime.error = Some(error.to_string());
            return;
        }
        if task.manual_box_guide_migration.is_some()
            && let AnnotationGeometry::Skeleton(skeleton) = &annotation.geometry
            && let Err(error) = labello_domain::validate_manual_migration_skeleton(skeleton)
        {
            self.runtime.error = Some(error.to_string());
            return;
        }
        let change = if let Some(group) = editor
            .object_group_id
            .as_ref()
            .filter(|_| task.manual_box_guide_migration.is_some())
        {
            let Some(disposition) = self
                .work
                .current_state
                .as_ref()
                .and_then(|state| state.migration_dispositions.get(&task.task_id)?.get(group))
            else {
                return;
            };
            let AnnotationGeometry::Skeleton(skeleton) = annotation.geometry else {
                return;
            };
            ReviewCorrectionChange::MigrationObject {
                object_group_id: group.clone(),
                expected_disposition_version: disposition.disposition_version,
                replacement: MigrationReviewCorrection::Skeleton { skeleton },
            }
        } else if editor.version == 0 {
            ReviewCorrectionChange::Add {
                annotation_id: editor.annotation_id.clone(),
                class_id: editor.class_id,
                geometry: annotation.geometry,
            }
        } else {
            ReviewCorrectionChange::Edit {
                annotation_id: editor.annotation_id.clone(),
                expected_version: editor.version,
                geometry: annotation.geometry,
            }
        };
        if editor.version > 0 && !draft.geometry_changed() {
            self.remove_staged_change(&change);
        } else {
            self.keep_review_change(change);
        }
        if !draft.reason.trim().is_empty() {
            self.work.review_corrections.reason = draft.reason.trim().to_owned();
        }
        self.discard_correction();
        self.runtime.error = None;
    }

    fn remove_staged_change(&mut self, change: &ReviewCorrectionChange) {
        self.work
            .review_corrections
            .changes
            .retain(|old| !same_target(old, change));
    }

    fn keep_review_change(&mut self, change: ReviewCorrectionChange) {
        if self.work.review_corrections.changes.contains(&change) {
            return;
        }
        self.remove_staged_change(&change);
        if let Some(target) = self
            .review_object_targets()
            .into_iter()
            .find(|target| self.change_matches_target(&change, target))
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
        self.work.review_corrections.changes.push(change);
        self.work.assignment_touched = true;
    }

    pub(crate) fn submit_staged_review_corrections(&mut self) -> bool {
        if !self.retain_review_editor() {
            return false;
        }
        if !self.can_submit_review_corrections()
            || self.loading.saving
            || self.loading.image
            || self.work.pending_transition.is_some()
            || self.runtime.api.is_none()
        {
            return false;
        }
        let Some(assignment) = self
            .work
            .assignment
            .clone()
            .filter(|assignment| assignment.kind == AssignmentKind::Review)
        else {
            return false;
        };
        let Some(captured) = self.work.current_state.as_ref().and_then(|state| {
            state
                .review_assignment_contexts
                .get(&assignment.assignment_id)
        }) else {
            return false;
        };
        let correction = self
            .work
            .review_corrections
            .submission
            .get_or_insert_with(|| ReviewCorrectionSubmission {
                correction_id: CorrectionId::generate(),
                round: captured.round.clone(),
                target_fingerprint: captured.target_fingerprint.clone(),
                changes: self.work.review_corrections.changes.clone(),
                reason: (!self.work.review_corrections.reason.is_empty())
                    .then(|| self.work.review_corrections.reason.clone()),
            })
            .clone();
        let operation_id = self.begin_operation();
        let request = self.operation_identity(operation_id, self.config.dataset_id.clone());
        self.queue_command(UiCommand::Correction {
            request,
            operation_id,
            dataset_id: self.config.dataset_id.clone(),
            assignment,
            correction,
        })
    }

    pub(crate) fn review_corrections_panel(&mut self, ui: &mut egui::Ui) {
        if self.view != AppView::Review || self.work.assignment.is_none() {
            return;
        }
        let ready = !self.loading.saving
            && !self.loading.image
            && !self.work.migration.busy
            && self.work.pending_transition.is_none()
            && self.work.review_corrections.submission.is_none();
        let position = self.review_position();
        let count = self.review_object_targets().len();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(ready && position > 0, egui::Button::new("Previous item"))
                .clicked()
            {
                self.cycle_review_item(-1);
            }
            if ui
                .add_enabled(
                    ready && position < count,
                    egui::Button::new(if position + 1 == count {
                        "Overview"
                    } else {
                        "Next item"
                    }),
                )
                .clicked()
            {
                self.cycle_review_item(1);
            }
        });
        if self.work.review_corrections.submission.is_some() {
            ui.label("Submission pending. Retry with the same corrections.");
        }
        if self.work.correction_draft.is_some() {
            self.correction_actions(ui, ready);
        }
        if let Some(target) = self.focused_review_target() {
            match target {
                labello_domain::ReviewTarget::AnnotationVersion {
                    annotation_id,
                    version,
                } => {
                    if let Some(group) = self
                        .work
                        .current_state
                        .as_ref()
                        .and_then(|state| state.current_annotation(&annotation_id))
                        .and_then(|annotation| annotation.object_group_id.clone())
                        .filter(|_| self.manual_migration_active())
                    {
                        self.review_exclusion_menu(ui, group, ready);
                    } else if ui
                        .add_enabled(ready, egui::Button::new("Remove item"))
                        .clicked()
                    {
                        self.discard_correction();
                        self.keep_review_change(ReviewCorrectionChange::Remove {
                            annotation_id,
                            expected_version: version,
                        });
                    }
                }
                labello_domain::ReviewTarget::MigrationDisposition {
                    object_group_id, ..
                } => {
                    self.review_exclusion_menu(ui, object_group_id.clone(), ready);
                    if self.work.correction_draft.is_none()
                        && let Some(target) = self
                            .selected_task()
                            .and_then(|task| {
                                self.work
                                    .current_state
                                    .as_ref()?
                                    .migration_target_sets
                                    .get(&task.task_id)
                            })
                            .and_then(|set| {
                                set.targets
                                    .iter()
                                    .find(|target| target.object_group_id == object_group_id)
                            })
                            .cloned()
                        && ui
                            .add_enabled(
                                ready,
                                egui::Button::new("Create skeleton for excluded object"),
                            )
                            .clicked()
                    {
                        self.begin_new_review_object(Some((
                            target.reserved_skeleton_annotation_id,
                            object_group_id,
                        )));
                    }
                }
                _ => {}
            }
            if self.work.correction_draft.is_none()
                && ui
                    .add_enabled(ready, egui::Button::new("Reset item"))
                    .clicked()
            {
                self.reset_review_item();
            }
        } else {
            ui.label("Draw a missing box or click to begin a skeleton. Select an existing item to review it again.");
            if !self.all_review_items_decided() {
                ui.label("Review every item before submitting this image.");
            }
            if ui
                .add_enabled(ready, egui::Button::new("New annotation"))
                .on_hover_text("Start an annotation using keyboard-accessible controls.")
                .clicked()
                && self.retain_review_editor()
            {
                self.begin_new_review_object(None);
            }
            egui::CollapsingHeader::new("Review items").show(ui, |ui| {
                for index in 0..count {
                    if ui
                        .add_enabled(
                            ready,
                            egui::Button::new(format!("Review item {}", index + 1)),
                        )
                        .clicked()
                    {
                        self.navigate_review_item(index);
                    }
                }
            });
        }
        if self.has_review_corrections() {
            ui.label("Corrections remain unsaved until you reject and submit from the overview.");
        }
    }

    fn review_exclusion_menu(
        &mut self,
        ui: &mut egui::Ui,
        group: labello_domain::ObjectGroupId,
        ready: bool,
    ) {
        ui.add_enabled_ui(ready, |ui| {
            ui.menu_button("Set exclusion", |ui| {
                for (reason, label) in [
                    (
                        MigrationExclusionReason::NoValidSkeleton,
                        "No valid skeleton",
                    ),
                    (
                        MigrationExclusionReason::InsufficientVisibleFeatures,
                        "Insufficient visible features",
                    ),
                    (
                        MigrationExclusionReason::InvalidSourceBox,
                        "Invalid source box",
                    ),
                    (
                        MigrationExclusionReason::DuplicateSourceObject,
                        "Duplicate source object",
                    ),
                    (
                        MigrationExclusionReason::ObjectNotPresent,
                        "Object not present",
                    ),
                ] {
                    if ui.button(label).clicked() {
                        self.stage_review_exclusion(group.clone(), reason);
                        ui.close();
                    }
                }
            });
        });
    }

    fn stage_review_exclusion(
        &mut self,
        group: labello_domain::ObjectGroupId,
        reason: MigrationExclusionReason,
    ) {
        self.discard_correction();
        let Some(task) = self.selected_task() else {
            return;
        };
        let Some(disposition) = self
            .work
            .current_state
            .as_ref()
            .and_then(|state| state.migration_dispositions.get(&task.task_id)?.get(&group))
        else {
            return;
        };
        let unchanged = matches!(&disposition.status, MigrationDispositionStatus::Excluded { exclusion } if exclusion.reason == reason);
        let change = ReviewCorrectionChange::MigrationObject {
            object_group_id: group,
            expected_disposition_version: disposition.disposition_version,
            replacement: MigrationReviewCorrection::Exclude { reason, note: None },
        };
        if unchanged {
            self.remove_staged_change(&change);
        } else {
            self.keep_review_change(change);
        }
    }

    pub(crate) fn begin_new_review_object(
        &mut self,
        canonical: Option<(AnnotationId, labello_domain::ObjectGroupId)>,
    ) {
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let Some(class_id) = self.selected_class_id().cloned() else {
            return;
        };
        let geometry = match task.annotation_type {
            AnnotationType::BoundingBox => AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.4,
                y: 0.4,
                width: 0.2,
                height: 0.2,
            }),
            AnnotationType::Skeleton => AnnotationGeometry::Skeleton(SkeletonGeometry {
                keypoints: task
                    .skeleton
                    .as_ref()
                    .map(|spec| {
                        spec.keypoints
                            .iter()
                            .map(|keypoint| KeypointAnnotation {
                                name: keypoint.name.clone(),
                                state: KeypointState::Absent,
                                point: None,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }),
        };
        let (id, group) = canonical.map_or_else(
            || (AnnotationId::generate(), None),
            |(id, group)| (id, Some(group)),
        );
        let now = labello_domain::now();
        let mut annotation = self
            .work
            .current_state
            .as_ref()
            .and_then(|state| state.current_annotation(&id))
            .cloned()
            .unwrap_or(AnnotationVersion {
                annotation_id: id,
                version: 0,
                object_group_id: group,
                origin: labello_domain::AnnotationOrigin::native(),
                task_id: task.task_id,
                class_id,
                annotation_type: task.annotation_type,
                revision_source: labello_domain::RevisionSource::Human {
                    action: labello_domain::HumanRevisionKind::Authored,
                },
                geometry: geometry.clone(),
                author_user_id: self.config.user_id.clone(),
                created_at: now,
                updated_at: now,
                deleted: false,
            });
        annotation.geometry = geometry;
        annotation.deleted = false;
        self.begin_review_correction(annotation);
    }

    pub(crate) fn style_review_correction_previews(
        &self,
        annotations: &[AnnotationVersion],
        styles: &mut std::collections::BTreeMap<AnnotationId, crate::canvas::CanvasAnnotationStyle>,
    ) {
        for annotation in annotations {
            let changed = self.work.correction_draft.as_ref().is_some_and(|draft| {
                draft.annotation_id == annotation.annotation_id
                    && (draft.expected_version == 0 || draft.geometry_changed())
            }) || self.work.review_corrections.changes.iter().any(|change| {
                match change {
                    ReviewCorrectionChange::Edit { annotation_id, .. }
                    | ReviewCorrectionChange::Add { annotation_id, .. }
                    | ReviewCorrectionChange::Remove { annotation_id, .. } => {
                        annotation_id == &annotation.annotation_id
                    }
                    ReviewCorrectionChange::MigrationObject {
                        object_group_id, ..
                    } => {
                        annotation.annotation_type == AnnotationType::Skeleton
                            && annotation.object_group_id.as_ref() == Some(object_group_id)
                    }
                }
            });
            if changed {
                styles.insert(
                    annotation.annotation_id.clone(),
                    crate::canvas::CanvasAnnotationStyle::dashed(crate::theme::WARNING),
                );
            }
        }
    }

    pub(crate) fn apply_staged_review_previews(&self, annotations: &mut Vec<AnnotationVersion>) {
        let Some(task) = self.selected_task() else {
            return;
        };
        let Some(state) = self.work.current_state.as_ref() else {
            return;
        };
        for change in &self.work.review_corrections.changes {
            let (id, geometry, class, group, remove) = match change {
                ReviewCorrectionChange::Edit {
                    annotation_id,
                    geometry,
                    ..
                } => (
                    annotation_id.clone(),
                    Some(geometry.clone()),
                    None,
                    None,
                    false,
                ),
                ReviewCorrectionChange::Add {
                    annotation_id,
                    class_id,
                    geometry,
                } => (
                    annotation_id.clone(),
                    Some(geometry.clone()),
                    Some(class_id.clone()),
                    None,
                    false,
                ),
                ReviewCorrectionChange::Remove { annotation_id, .. } => {
                    (annotation_id.clone(), None, None, None, true)
                }
                ReviewCorrectionChange::MigrationObject {
                    object_group_id,
                    replacement,
                    ..
                } => {
                    let Some(target) =
                        state
                            .migration_target_sets
                            .get(&task.task_id)
                            .and_then(|set| {
                                set.targets
                                    .iter()
                                    .find(|target| target.object_group_id == *object_group_id)
                            })
                    else {
                        continue;
                    };
                    let geometry = match replacement {
                        MigrationReviewCorrection::Skeleton { skeleton } => {
                            Some(AnnotationGeometry::Skeleton(skeleton.clone()))
                        }
                        MigrationReviewCorrection::Exclude { .. } => None,
                    };
                    (
                        target.reserved_skeleton_annotation_id.clone(),
                        geometry.clone(),
                        task.class_ids.first().cloned(),
                        Some(object_group_id.clone()),
                        geometry.is_none(),
                    )
                }
            };
            if remove {
                annotations.retain(|annotation| annotation.annotation_id != id);
                continue;
            }
            if let Some(annotation) = annotations
                .iter_mut()
                .find(|annotation| annotation.annotation_id == id)
            {
                annotation.geometry = geometry.unwrap();
            } else if let (Some(class_id), Some(geometry)) = (class, geometry) {
                let now = labello_domain::now();
                annotations.push(AnnotationVersion {
                    annotation_id: id,
                    version: 0,
                    object_group_id: group,
                    origin: labello_domain::AnnotationOrigin::native(),
                    task_id: task.task_id.clone(),
                    class_id,
                    annotation_type: task.annotation_type.clone(),
                    revision_source: labello_domain::RevisionSource::Human {
                        action: labello_domain::HumanRevisionKind::Authored,
                    },
                    geometry,
                    author_user_id: self.config.user_id.clone(),
                    created_at: now,
                    updated_at: now,
                    deleted: false,
                });
            }
        }
    }

    pub(crate) fn review_correction_preview(&self) -> Option<AnnotationVersion> {
        let mut annotation = self.work.review_corrections.editor.clone()?;
        annotation.geometry = self.work.correction_draft.as_ref()?.edited_geometry.clone();
        Some(annotation)
    }

    pub(crate) fn place_review_correction_keypoint(&mut self, point: NormalizedPoint) {
        let Some(draft) = self.work.correction_draft.as_mut() else {
            return;
        };
        let Some(index) = draft.selected_keypoint else {
            return;
        };
        let mut geometry = draft.edited_geometry.clone();
        let AnnotationGeometry::Skeleton(skeleton) = &mut geometry else {
            return;
        };
        let Some(keypoint) = skeleton.keypoints.get_mut(index) else {
            return;
        };
        keypoint.point = Some(point);
        keypoint.state = KeypointState::Visible;
        draft.geometry_history.push(draft.edited_geometry.clone());
        draft.edited_geometry = geometry;
        if let AnnotationGeometry::Skeleton(skeleton) = &draft.edited_geometry {
            draft.selected_keypoint = skeleton
                .keypoints
                .iter()
                .enumerate()
                .skip(index + 1)
                .find(|(_, keypoint)| keypoint.point.is_none())
                .map(|(index, _)| index)
                .or(Some(index));
        }
        self.work.assignment_touched = true;
    }
}

fn same_target(a: &ReviewCorrectionChange, b: &ReviewCorrectionChange) -> bool {
    fn annotation(change: &ReviewCorrectionChange) -> Option<&AnnotationId> {
        match change {
            ReviewCorrectionChange::Edit { annotation_id, .. }
            | ReviewCorrectionChange::Add { annotation_id, .. }
            | ReviewCorrectionChange::Remove { annotation_id, .. } => Some(annotation_id),
            _ => None,
        }
    }
    match (a, b) {
        (
            ReviewCorrectionChange::MigrationObject {
                object_group_id: a, ..
            },
            ReviewCorrectionChange::MigrationObject {
                object_group_id: b, ..
            },
        ) => a == b,
        _ => annotation(a).is_some() && annotation(a) == annotation(b),
    }
}
