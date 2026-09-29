use super::*;
use labello_domain::{
    AnnotationGeometry, AnnotationVersion, AssignmentKind, ReviewCorrectionChange as Change,
    WorkflowEdits,
};

impl LabelloApp {
    pub(crate) fn workflow_review_needs_browser_backup(&self) -> bool {
        let Some(context) = self.workflow_context() else {
            return true;
        };
        let Some(request) = self.current_workflow_draft() else {
            return false;
        };
        let labello_client::WorkflowDraftInput::Edits { edits } = request.draft else {
            return false;
        };
        let saved = self.work.current_state.as_ref().and_then(|state| {
            state.pending_workflow_edits(
                &request.assignment.task_id,
                &context.item,
                &AssignmentKind::Review,
            )
        });
        // Navigation and selection are not unsaved work. Restored server drafts
        // already survive lease release and must not create stale browser conflicts.
        saved.map_or_else(
            || edits != WorkflowEdits::default(),
            |saved| saved.edits != edits,
        )
    }

    pub(super) fn current_workflow_edits(
        &self,
    ) -> Option<labello_client::SaveWorkflowDraftRequest> {
        let context = self.workflow_context()?;
        let assignment = self.work.assignment.as_ref()?;
        let state = self.work.current_state.as_ref()?;
        let mut edits = WorkflowEdits::default();
        if assignment.kind == AssignmentKind::Review {
            edits.changes = self.work.review_corrections.changes.clone();
            let reason = self.correction_reason_text();
            edits.reason = (!reason.is_empty()).then_some(reason);
            if let (Some(editor), Some(draft)) = (
                &self.work.review_corrections.editor,
                &self.work.correction_draft,
            ) {
                let change = if let Some(group) = &editor.object_group_id {
                    let disposition = state
                        .migration_dispositions
                        .get(&assignment.task_id)?
                        .get(group)?;
                    let AnnotationGeometry::Skeleton(skeleton) = &draft.edited_geometry else {
                        return None;
                    };
                    Change::MigrationObject {
                        object_group_id: group.clone(),
                        expected_disposition_version: disposition.disposition_version,
                        replacement: labello_domain::MigrationReviewCorrection::Skeleton {
                            skeleton: skeleton.clone(),
                        },
                    }
                } else if editor.version == 0 {
                    Change::Add {
                        annotation_id: editor.annotation_id.clone(),
                        class_id: editor.class_id.clone(),
                        geometry: draft.edited_geometry.clone(),
                    }
                } else {
                    Change::Edit {
                        annotation_id: editor.annotation_id.clone(),
                        expected_version: editor.version,
                        geometry: draft.edited_geometry.clone(),
                    }
                };
                edits.changes.retain(|old| !same_edit_item(old, &change));
                if editor.version == 0 || draft.geometry_changed() {
                    edits.changes.push(change);
                }
            }
        } else {
            for annotation in self
                .work
                .annotations
                .iter()
                .filter(|a| a.task_id == assignment.task_id)
            {
                let old = state.current_annotation(&annotation.annotation_id);
                let change = match old {
                    Some(old) if annotation.deleted => Change::Remove {
                        annotation_id: old.annotation_id.clone(),
                        expected_version: old.version,
                    },
                    Some(old) if annotation.geometry != old.geometry => Change::Edit {
                        annotation_id: old.annotation_id.clone(),
                        expected_version: old.version,
                        geometry: annotation.geometry.clone(),
                    },
                    None if !annotation.deleted => Change::Add {
                        annotation_id: annotation.annotation_id.clone(),
                        class_id: annotation.class_id.clone(),
                        geometry: annotation.geometry.clone(),
                    },
                    _ => continue,
                };
                edits.changes.push(change);
            }
        }
        if assignment.kind == AssignmentKind::Annotation
            && self.manual_migration_active()
            && self.work.migration.adding_missing_object
            && let Some(mut skeleton) = self.work.migration.draft.clone()
        {
            skeleton
                .keypoints
                .truncate(self.work.migration.keypoint_index);
            let id = self
                .work
                .migration
                .editing_missing_annotation_id
                .clone()
                .unwrap_or_else(|| format!("draft_{}", assignment.assignment_id).into());
            let change = if let Some(annotation) = state.current_annotation(&id) {
                Change::Edit {
                    annotation_id: id,
                    expected_version: annotation.version,
                    geometry: AnnotationGeometry::Skeleton(skeleton),
                }
            } else {
                Change::Add {
                    annotation_id: id,
                    class_id: self.selected_class_id()?.clone(),
                    geometry: AnnotationGeometry::Skeleton(skeleton),
                }
            };
            edits.changes.retain(|old| !same_edit_item(old, &change));
            edits.changes.push(change);
        }
        Some(labello_client::SaveWorkflowDraftRequest {
            assignment: labello_client::AssignmentActionRequest {
                assignment_id: assignment.assignment_id.clone(),
                image_id: assignment.image_id.clone(),
                task_id: assignment.task_id.clone(),
                kind: assignment.kind.clone(),
            },
            expected_sequence: state
                .workflow_edit_draft(&assignment.task_id, &context.item, &assignment.kind)
                .map_or(0, |draft| draft.sequence),
            draft: labello_client::WorkflowDraftInput::Edits { edits },
        })
    }

    pub(crate) fn restore_workflow_edits(&mut self) {
        let Some(context) = self.workflow_context().cloned() else {
            return;
        };
        let Some(assignment) = self.work.assignment.clone() else {
            return;
        };
        let state = self.work.current_state.as_ref().expect("workflow state");
        let Some(draft) = state
            .pending_workflow_edits(&assignment.task_id, &context.item, &assignment.kind)
            .cloned()
        else {
            return;
        };
        // Reacquired work may have changed after its prior owner released it.
        if state
            .validate_workflow_edits(&assignment.task_id, &context.item, &draft.edits)
            .is_err()
        {
            return;
        }
        if assignment.kind == AssignmentKind::Review {
            // Saving the current editor must not fold its per-object reasons
            // back into the aggregate reason and duplicate them on the next save.
            if self.current_workflow_edits().is_some_and(|request| {
                matches!(request.draft, labello_client::WorkflowDraftInput::Edits { edits } if edits == draft.edits)
            }) {
                return;
            }
            self.work.review_corrections.changes = draft.edits.changes;
            self.work.review_corrections.reason = draft.edits.reason.unwrap_or_default();
            let mut previews = self.work.annotations.clone();
            self.apply_staged_review_previews(&mut previews);
            if let Some(annotation) = previews.into_iter().find(|a| {
                self.selected_task().is_some_and(|task| {
                    let mut candidate = a.clone();
                    candidate.version = candidate.version.max(1);
                    candidate
                        .validate_for_task(
                            task,
                            self.work
                                .current
                                .as_ref()
                                .expect("image")
                                .image
                                .dimensions(),
                        )
                        .is_err()
                })
            }) {
                self.begin_review_correction(annotation);
            }
        } else if self.manual_migration_active() {
            if let Some(change) = draft.edits.changes.into_iter().next() {
                let (editing, mut skeleton) = match change {
                    Change::Add {
                        geometry: AnnotationGeometry::Skeleton(skeleton),
                        ..
                    } => (None, skeleton),
                    Change::Edit {
                        annotation_id,
                        geometry: AnnotationGeometry::Skeleton(skeleton),
                        ..
                    } => (Some(annotation_id), skeleton),
                    _ => return,
                };
                let count = skeleton.keypoints.len();
                if let Some(spec) = self.selected_task().and_then(|task| task.skeleton.as_ref()) {
                    for point in spec.keypoints.iter().skip(count) {
                        skeleton.keypoints.push(labello_domain::KeypointAnnotation {
                            name: point.name.clone(),
                            state: labello_domain::KeypointState::Absent,
                            point: None,
                        });
                    }
                }
                self.work.migration.cursor = Some(labello_domain::MigrationCursor::FullImage);
                self.work.migration.adding_missing_object = true;
                self.work.migration.editing_missing_annotation_id = editing;
                self.work.migration.keypoint_index = count;
                self.work.migration.draft = Some(skeleton);
                self.work.migration.draft_dirty = true;
            }
        } else {
            for change in draft.edits.changes {
                match change {
                    Change::Add {
                        annotation_id,
                        class_id,
                        geometry,
                    } => {
                        let task = self.selected_task().expect("workflow task");
                        self.work.annotations.push(AnnotationVersion::native(
                            annotation_id,
                            assignment.task_id.clone(),
                            class_id,
                            task.annotation_type.clone(),
                            geometry,
                            self.config.user_id.clone(),
                            labello_domain::now(),
                        ));
                    }
                    Change::Edit {
                        annotation_id,
                        geometry,
                        ..
                    } => {
                        if let Some(annotation) = self
                            .work
                            .annotations
                            .iter_mut()
                            .find(|a| a.annotation_id == annotation_id)
                        {
                            annotation.geometry = geometry;
                        }
                    }
                    Change::Remove { annotation_id, .. } => {
                        if let Some(annotation) = self
                            .work
                            .annotations
                            .iter_mut()
                            .find(|a| a.annotation_id == annotation_id)
                        {
                            annotation.deleted = true;
                        }
                    }
                    Change::MigrationObject { .. } => {}
                }
            }
            self.recompute_modified_annotations();
            if let Some(annotation) = self.work.annotations.iter().find(|a| !a.deleted && a.task_id == assignment.task_id && matches!(&a.geometry, AnnotationGeometry::Skeleton(skeleton) if self.selected_task().and_then(|task| task.skeleton.as_ref()).is_some_and(|spec| skeleton.keypoints.len() < spec.keypoints.len()))) {
                self.work.selected_annotation = Some(annotation.annotation_id.clone());
                self.work.active_skeleton = Some(annotation.annotation_id.clone());
                if let AnnotationGeometry::Skeleton(skeleton) = &annotation.geometry { self.work.skeleton_keypoint_index = skeleton.keypoints.len(); }
            }
        }
    }
}

fn same_edit_item(left: &Change, right: &Change) -> bool {
    match (left, right) {
        (
            Change::MigrationObject {
                object_group_id: left,
                ..
            },
            Change::MigrationObject {
                object_group_id: right,
                ..
            },
        ) => left == right,
        (
            Change::Add {
                annotation_id: left,
                ..
            }
            | Change::Edit {
                annotation_id: left,
                ..
            }
            | Change::Remove {
                annotation_id: left,
                ..
            },
            Change::Add {
                annotation_id: right,
                ..
            }
            | Change::Edit {
                annotation_id: right,
                ..
            }
            | Change::Remove {
                annotation_id: right,
                ..
            },
        ) => left == right,
        _ => false,
    }
}
