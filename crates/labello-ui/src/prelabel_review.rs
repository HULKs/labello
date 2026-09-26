use crate::{LabelloApp, app::AppView};
use labello_domain::{
    AnnotationGeometry, AnnotationId, AnnotationType, AnnotationVersion, PrelabelSuggestion,
};

/// Editable model objects stay outside the annotations sent to the server.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrelabelReview {
    pub objects: Vec<PrelabelObject>,
    pub changed: bool,
    pub started: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrelabelObject {
    pub suggestion: PrelabelSuggestion,
    pub annotation: AnnotationVersion,
}

#[derive(Clone, Copy, Default)]
pub(crate) enum PrelabelPrimaryAction {
    Confirm,
    Focus,
    #[default]
    Submit,
}

impl LabelloApp {
    pub(crate) fn prelabel_primary_action(&self) -> PrelabelPrimaryAction {
        if self.selected_prelabel_object().is_some() {
            PrelabelPrimaryAction::Confirm
        } else if !self.pending_prelabel_objects().is_empty() {
            PrelabelPrimaryAction::Focus
        } else {
            PrelabelPrimaryAction::Submit
        }
    }

    pub(crate) fn sync_prelabel_review(&mut self) {
        if self.view != AppView::Annotate || self.manual_migration_active() || self.loading.image {
            return;
        }
        let hints = self.visible_prelabels();
        let was_pending = self.work.selected_annotation.as_ref().is_some_and(|id| {
            self.work
                .prelabel_review
                .objects
                .iter()
                .any(|item| &item.annotation.annotation_id == id)
        });
        for hint in &hints {
            if let Some(item) = self
                .work
                .prelabel_review
                .objects
                .iter_mut()
                .find(|item| item.suggestion.suggestion_id == hint.suggestion_id)
            {
                // Refresh the signed evidence, retaining any human geometry edits.
                item.suggestion = hint.clone();
                continue;
            }
            self.work.prelabel_review.started = true;
            self.work.prelabel_review.objects.push(PrelabelObject {
                annotation: AnnotationVersion::native(
                    AnnotationId::generate(),
                    hint.task_id.clone(),
                    hint.class_id.clone(),
                    match hint.geometry {
                        AnnotationGeometry::BoundingBox(_) => AnnotationType::BoundingBox,
                        AnnotationGeometry::Skeleton(_) => AnnotationType::Skeleton,
                    },
                    hint.geometry.clone(),
                    self.config.user_id.clone(),
                    labello_domain::now(),
                ),
                suggestion: hint.clone(),
            });
        }
        let pending = self.pending_prelabel_objects();
        let empty = pending.is_empty();
        let selection_visible = pending.iter().any(|item| {
            Some(&item.annotation.annotation_id) == self.work.selected_annotation.as_ref()
        });
        if self.work.selected_annotation.is_none() || was_pending && !selection_visible {
            self.work.selected_annotation = pending
                .first()
                .map(|item| item.annotation.annotation_id.clone());
            if was_pending && empty {
                self.work.canvas.fit_view();
            }
        }
    }

    pub(crate) fn pending_prelabel_objects(&self) -> Vec<&PrelabelObject> {
        self.visible_prelabels()
            .iter()
            .filter_map(|hint| {
                self.work
                    .prelabel_review
                    .objects
                    .iter()
                    .find(|item| item.suggestion.suggestion_id == hint.suggestion_id)
            })
            .collect()
    }

    pub(crate) fn selected_prelabel_object(&self) -> Option<&PrelabelObject> {
        self.pending_prelabel_objects().into_iter().find(|item| {
            Some(&item.annotation.annotation_id) == self.work.selected_annotation.as_ref()
        })
    }

    pub(crate) fn annotation_objects(&self) -> Vec<AnnotationVersion> {
        let mut objects = self
            .work
            .annotations
            .iter()
            .filter(|annotation| {
                !annotation.deleted && self.annotation_matches_selected_workflow(annotation)
            })
            .cloned()
            .collect::<Vec<_>>();
        objects.extend(
            self.pending_prelabel_objects()
                .into_iter()
                .map(|item| item.annotation.clone()),
        );
        objects
    }

    pub(crate) fn cycle_prelabel_object(&mut self, direction: isize) {
        let pending = self.pending_prelabel_objects();
        if pending.is_empty() {
            return;
        }
        let current = pending.iter().position(|item| {
            Some(&item.annotation.annotation_id) == self.work.selected_annotation.as_ref()
        });
        let next = current.map_or_else(
            || if direction < 0 { pending.len() - 1 } else { 0 },
            |index| (index as isize + direction).rem_euclid(pending.len() as isize) as usize,
        );
        self.work.selected_annotation = Some(pending[next].annotation.annotation_id.clone());
    }

    pub(crate) fn advance_prelabel_object(&mut self) {
        self.work.selected_annotation = self
            .pending_prelabel_objects()
            .first()
            .map(|item| item.annotation.annotation_id.clone());
        if self.work.selected_annotation.is_none() {
            self.work.canvas.fit_view();
        }
    }

    pub(crate) fn confirm_prelabel_object(&mut self) -> bool {
        self.sync_prelabel_review();
        let Some(item) = self.selected_prelabel_object().cloned() else {
            if !self.pending_prelabel_objects().is_empty() {
                self.advance_prelabel_object();
                return true;
            }
            return false;
        };
        self.record_edit();
        let id = item.annotation.annotation_id.clone();
        if let Some(evidence) = item.suggestion.evidence {
            self.work.prelabel_evidence.insert(id.clone(), evidence);
        }
        self.work
            .accepted_prelabels
            .push(item.suggestion.suggestion_id);
        self.work
            .prelabel_review
            .objects
            .retain(|item| item.annotation.annotation_id != id);
        self.work.prelabel_review.changed = true;
        let mut annotation = item.annotation;
        if let Some(persisted) = self
            .work
            .current_state
            .as_ref()
            .and_then(|state| state.current_annotation(&id))
        {
            annotation.origin = persisted.origin.clone();
            annotation.created_at = persisted.created_at;
            annotation.object_group_id = persisted.object_group_id.clone();
            annotation.revision_source = labello_domain::RevisionSource::Human {
                action: labello_domain::HumanRevisionKind::Edited,
            };
        }
        self.work
            .annotations
            .retain(|annotation| annotation.annotation_id != id);
        self.work.annotations.push(annotation);
        self.recompute_modified_annotations();
        self.mark_edited();
        self.advance_prelabel_object();
        true
    }

    pub(crate) fn delete_prelabel_object(&mut self) -> bool {
        let Some(item) = self.selected_prelabel_object().cloned() else {
            return false;
        };
        self.record_edit();
        self.work
            .accepted_prelabels
            .push(item.suggestion.suggestion_id);
        self.work.prelabel_review.objects.retain(|candidate| {
            candidate.annotation.annotation_id != item.annotation.annotation_id
        });
        self.work.prelabel_review.changed = true;
        self.mark_edited();
        self.advance_prelabel_object();
        true
    }

    pub(crate) fn edit_prelabel_geometry(
        &mut self,
        id: &AnnotationId,
        geometry: AnnotationGeometry,
    ) -> bool {
        let Some(index) = self
            .work
            .prelabel_review
            .objects
            .iter()
            .position(|item| &item.annotation.annotation_id == id)
        else {
            return false;
        };
        if self.work.prelabel_review.objects[index].annotation.geometry != geometry {
            self.record_edit();
            let item = &mut self.work.prelabel_review.objects[index];
            item.annotation.geometry = geometry;
            item.annotation.updated_at = labello_domain::now();
            self.work.prelabel_review.changed = true;
            self.mark_edited();
        }
        true
    }

    pub(crate) fn prelabel_progress(&self) -> Option<String> {
        if !self.work.prelabel_review.started
            || self.manual_migration_active()
            || self.selected_task().is_none_or(|task| {
                self.runtime.api.is_some() && self.prelabel_choice(&task.task_id).is_none()
            })
        {
            return None;
        }
        let pending = self.pending_prelabel_objects();
        if pending.is_empty() {
            Some("Image overview".into())
        } else {
            let objects = self.annotation_objects();
            let position = objects.iter().position(|annotation| {
                Some(&annotation.annotation_id) == self.work.selected_annotation.as_ref()
            });
            Some(position.map_or_else(
                || format!("{} to confirm", pending.len()),
                |index| format!("Object {} of {}", index + 1, objects.len()),
            ))
        }
    }
}
