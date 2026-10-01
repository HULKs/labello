use eframe::egui::{self, RichText};

use crate::{
    app::{AppView, LabelloApp, Tool},
    canvas::{CanvasAction, CanvasInteraction, MissingObjectOverlay, show_canvas_with_evidence},
    theme,
};

impl LabelloApp {
    pub(crate) fn is_migration_companion_box(
        &self,
        annotation: &labello_domain::AnnotationVersion,
    ) -> bool {
        !annotation.deleted
            && matches!(
                annotation.geometry,
                labello_domain::AnnotationGeometry::BoundingBox(_)
            )
            && self.work.current_state.as_ref().is_some_and(|state| {
                state.migration_companions.values().any(|link| {
                    link.box_annotation_id == annotation.annotation_id
                        && link.guide_task_id == annotation.task_id
                        && link.class_id == annotation.class_id
                })
            })
    }

    pub(crate) fn review_source_box(&self) -> Option<labello_domain::AnnotationVersion> {
        if self.view != AppView::Review || self.review_overview() {
            return None;
        }
        let state = self.work.current_state.as_ref()?;
        let task = self.selected_task()?;
        let group = match self.focused_review_target()? {
            labello_domain::ReviewTarget::AnnotationVersion { annotation_id, .. } => state
                .current_annotation(&annotation_id)?
                .object_group_id
                .clone()?,
            labello_domain::ReviewTarget::MigrationDisposition {
                object_group_id, ..
            } => object_group_id,
            _ => return None,
        };
        let target = state
            .migration_target_sets
            .get(&task.task_id)?
            .targets
            .iter()
            .find(|target| target.object_group_id == group)?;
        state
            .current_annotation(&target.guide_annotation_id)
            .filter(|guide| !guide.deleted)
            .cloned()
    }

    pub(crate) fn workspace_canvas(&mut self, ui: &mut egui::Ui) {
        if self.workspace_bars_loading() {
            let opacity = ui.opacity();
            ui.disable();
            // Block input without fading the retained image into the canvas background.
            ui.set_opacity(opacity);
        }
        self.work
            .canvas
            .set_pan_drag_modifier(self.work.keybindings.pan_drag_modifier);
        if self.manual_migration_active() {
            self.migration_workspace_canvas(ui);
            return;
        }
        if let Some(current) = self.work.current.clone() {
            let texture = self.work.current_texture.clone();
            let mut annotations = self.annotation_objects();
            if let Some(draft) = self.work.correction_draft.as_ref()
                && let Some(annotation) = annotations
                    .iter_mut()
                    .find(|annotation| annotation.annotation_id == draft.annotation_id)
            {
                annotation.geometry = draft.edited_geometry.clone();
            }
            self.apply_staged_review_previews(&mut annotations);
            if let Some(preview) = self.review_correction_preview() {
                annotations.retain(|annotation| annotation.annotation_id != preview.annotation_id);
                annotations.push(preview);
            }
            self.filter_visible_boxes(&mut annotations);
            let selectable = annotations
                .iter()
                .map(|annotation| annotation.annotation_id.clone())
                .collect();
            if self.view == AppView::Annotate {
                let selected_source = annotations
                    .iter()
                    .find(|annotation| {
                        Some(&annotation.annotation_id) == self.work.selected_annotation.as_ref()
                            && !self.companion_needs_box(annotation)
                    })
                    .and_then(|annotation| self.companion_source(annotation))
                    .cloned();
                annotations = annotations
                    .iter()
                    .map(|annotation| self.companion_guide(annotation))
                    .collect();
                if let Some(source) = selected_source {
                    annotations.push(source);
                }
            }
            let source_box = self.review_source_box();
            let skeleton_edges = self
                .selected_task()
                .and_then(|task| task.skeleton.as_ref())
                .map(|skeleton| {
                    skeleton
                        .edges
                        .iter()
                        .map(|edge| (edge.from.clone(), edge.to.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let prelabels = Vec::new();
            let annotator_editable = self.view == AppView::Annotate
                && self.work.pending_transition.is_none()
                && !self.workspace_bars_loading();
            let correction_interaction = self.work.correction_draft.as_ref().map(|draft| {
                let mut interaction = CanvasInteraction::correction(draft.selected_keypoint);
                interaction.allow_selection =
                    self.view == AppView::Review && self.review_overview();
                interaction.allow_create = matches!(
                    draft.edited_geometry,
                    labello_domain::AnnotationGeometry::Skeleton(_)
                ) || (self.review_overview()
                    && draft.expected_version == 0);
                interaction.editable = !self.loading.saving
                    && !self.loading.image
                    && self.work.pending_transition.is_none();
                interaction
            });
            let overview_editable = self.view == AppView::Review
                && self.review_overview()
                && !self.loading.saving
                && !self.loading.image
                && self.work.pending_transition.is_none()
                && self.work.review_corrections.submission.is_none();
            let mut interaction = correction_interaction.unwrap_or_else(|| {
                CanvasInteraction::annotations(annotator_editable || overview_editable)
            });
            if correction_interaction.is_none()
                && annotator_editable
                && self.work.tool == Tool::Keypoints
            {
                interaction.edit_keypoints = true;
            }
            let bounding_box_tool = self.work.tool == Tool::BoundingBox;
            let selected_annotation = self.work.selected_annotation.clone();
            if self.view == AppView::Review {
                let review_annotation = selected_annotation.as_ref().and_then(|id| {
                    annotations
                        .iter()
                        .find(|annotation| !annotation.deleted && &annotation.annotation_id == id)
                });
                self.work
                    .canvas
                    .set_review_focus(if self.review_overview() {
                        None
                    } else {
                        source_box.as_ref().or(review_annotation)
                    });
            } else if self.view == AppView::Annotate {
                let focus = selected_annotation.as_ref().and_then(|id| {
                    annotations.iter().find(|annotation| {
                        &annotation.annotation_id == id
                            && (self.selected_prelabel_object().is_some()
                                || self.work.prelabel_evidence.contains_key(id)
                                || self.work.annotations.iter().any(|original| {
                                    original.annotation_id == annotation.annotation_id
                                        && (self.is_migration_companion_box(original)
                                            || matches!(
                                                original.origin,
                                                labello_domain::AnnotationOrigin::Prelabel { .. }
                                            ))
                                }))
                    })
                });
                self.work.canvas.set_annotation_edit_focus(focus);
            } else {
                self.work.canvas.clear_review_focus();
            }
            let annotation_color = self
                .selected_class_id()
                .and_then(|class_id| {
                    self.work
                        .classes
                        .iter()
                        .find(|class| &class.class_id == class_id)
                })
                .and_then(|class| parse_class_color(&class.color))
                .unwrap_or(theme::ANNOTATION);
            let locations = self.missing_object_canvas_locations();
            let missing_editable = self.missing_objects_editable();
            if let Some(point) = self.take_missing_object_focus() {
                self.work.canvas.focus_missing_object(point);
            }
            let mut missing_action = None;
            let mut styles = std::collections::BTreeMap::new();
            self.style_review_correction_previews(&annotations, &mut styles);
            if let Some(guide) = source_box {
                styles.insert(
                    guide.annotation_id.clone(),
                    crate::canvas::CanvasAnnotationStyle::dashed(theme::ACCENT_HOVER),
                );
                // Context only: keep the guide outside editable state and selectable IDs.
                annotations.push(guide);
            }
            if self.work.annotations.iter().any(|annotation| {
                Some(&annotation.annotation_id) == selected_annotation.as_ref()
                    && self.companion_needs_box(annotation)
            }) {
                ui.label("Source keypoints are read only. Draw a box for this object.");
            }
            let action = show_canvas_with_evidence(
                ui,
                &mut self.work.canvas,
                texture.as_ref(),
                &annotations,
                [current.image.width, current.image.height],
                bounding_box_tool,
                selected_annotation.as_ref(),
                interaction,
                &skeleton_edges,
                &prelabels,
                annotation_color,
                &styles,
                Some(&selectable),
                Some(MissingObjectOverlay {
                    locations: &locations,
                    selected: self.work.missing_objects.selected,
                    editable: missing_editable,
                    placing: self.work.missing_objects.placing,
                }),
                &mut missing_action,
            );
            if self.workspace_bars_loading() {
                return;
            }
            if let Some(action) = missing_action {
                self.apply_missing_object_action(action);
            }
            if annotator_editable {
                match action {
                    Some(CanvasAction::CreateBoundingBox(bbox)) => self.create_bbox(bbox),
                    Some(CanvasAction::PlaceKeypoint(point)) => self.place_keypoint(point),
                    Some(CanvasAction::Select(id)) => self.work.selected_annotation = Some(id),
                    Some(CanvasAction::EditBoundingBox(edit)) => self.edit_bbox(edit),
                    Some(CanvasAction::EditKeypoint(edit)) => self.edit_keypoint(edit),
                    Some(CanvasAction::SelectKeypoint(_)) => {}
                    None => {}
                }
            } else if overview_editable && self.work.correction_draft.is_none() {
                match action {
                    Some(CanvasAction::CreateBoundingBox(bbox)) => {
                        self.begin_new_review_object(None);
                        if let Some(draft) = self.work.correction_draft.as_mut() {
                            draft.edited_geometry =
                                labello_domain::AnnotationGeometry::BoundingBox(bbox);
                        }
                        self.stage_review_addition_keep_editor();
                    }
                    Some(CanvasAction::PlaceKeypoint(point)) => {
                        self.begin_new_review_object(None);
                        self.place_review_correction_keypoint(point);
                    }
                    Some(CanvasAction::Select(id)) => self.select_review_annotation(&id),
                    _ => {}
                }
            } else if self.work.correction_draft.is_some() {
                match action {
                    Some(CanvasAction::PlaceKeypoint(point)) => {
                        self.place_review_correction_keypoint(point)
                    }
                    Some(CanvasAction::EditBoundingBox(edit)) => self.edit_correction_bbox(edit),
                    Some(CanvasAction::SelectKeypoint(selection)) => {
                        if self
                            .work
                            .correction_draft
                            .as_ref()
                            .is_some_and(|draft| draft.annotation_id == selection.annotation_id)
                        {
                            self.select_correction_keypoint(selection.keypoint_index);
                        }
                    }
                    Some(CanvasAction::EditKeypoint(edit)) => self.edit_correction_keypoint(edit),
                    Some(CanvasAction::Select(id)) => self.select_review_annotation(&id),
                    Some(CanvasAction::CreateBoundingBox(bbox)) if overview_editable => {
                        if self.retain_review_editor() {
                            self.begin_new_review_object(None);
                            if let Some(draft) = self.work.correction_draft.as_mut() {
                                draft.edited_geometry =
                                    labello_domain::AnnotationGeometry::BoundingBox(bbox);
                            }
                            self.stage_review_addition_keep_editor();
                        }
                    }
                    Some(CanvasAction::CreateBoundingBox(_)) | None => {}
                }
            }
        } else {
            let availability_matches = self.assignment_kind().is_some_and(|kind| {
                self.work.availability.dataset_id.as_ref() == Some(&self.config.dataset_id)
                    && self.work.availability.kind.as_ref() == Some(&kind)
            });
            let checking_availability = availability_matches
                && self.work.availability.loading
                && !self.work.availability.resolved;
            let availability_error = availability_matches
                .then(|| self.work.availability.error.clone())
                .flatten();
            egui::ScrollArea::vertical()
                .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                .id_salt("workspace-empty-state")
                .show(ui, |ui| {
                    ui.add_space(((ui.available_height() - 160.0) * 0.5).max(0.0));
                    let width = ui.available_width().min(520.0);
                    let inset = ((ui.available_width() - width) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(inset);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            if self.loading.dataset {
                                theme::inset_frame().show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(RichText::new("Opening dataset").strong());
                                    });
                                    ui.label(
                                        RichText::new("Loading workflows and dataset metadata.")
                                            .color(theme::TEXT_MUTED),
                                    );
                                });
                            } else if self.loading.image {
                                if self.initial_workspace_load() {
                                    theme::inset_frame().show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label(
                                                RichText::new("Loading assignment image").strong(),
                                            );
                                        });
                                        ui.label(
                                            RichText::new(
                                                "Decoding the image preview for the canvas.",
                                            )
                                            .color(theme::TEXT_MUTED),
                                        );
                                    });
                                }
                            } else if let Some(error) = self.runtime.error.clone() {
                                let claimed = self.work.assignment.is_some();
                                let (title, retry) = if claimed {
                                    (
                                        "Assignment image unavailable",
                                        crate::glossary::RETRY_IMAGE_LOAD,
                                    )
                                } else {
                                    ("Assignment unavailable", "Retry assignment")
                                };
                                let shortcut = self.shortcut_text(
                                    ui.ctx(),
                                    labello_domain::UserAction::RetryImageLoad,
                                );
                                if theme::empty_state(
                                    ui,
                                    title,
                                    &error,
                                    Some(
                                        egui::Button::new(retry)
                                            .shortcut_text(crate::theme::button_shortcut(shortcut)),
                                    ),
                                ) {
                                    self.retry_assignment_load();
                                }
                            } else if checking_availability {
                                if self.initial_workspace_load() {
                                    theme::inset_frame().show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label(
                                                RichText::new("Checking assignment availability")
                                                    .strong(),
                                            );
                                        });
                                        ui.label(
                                            RichText::new(
                                                "Looking for work in the selected workflows.",
                                            )
                                            .color(theme::TEXT_MUTED),
                                        );
                                    });
                                }
                            } else if let Some(error) = availability_error {
                                if theme::empty_state(
                                    ui,
                                    "Assignment availability unavailable",
                                    &error,
                                    Some(egui::Button::new("Retry availability")),
                                ) {
                                    self.request_assignment_availability();
                                }
                            } else {
                                let title = match self.view {
                                    AppView::Annotate => "No annotation assignments",
                                    AppView::Review => "No review assignments",
                                    _ => "No assignments",
                                };
                                let shortcut = self.shortcut_text(
                                    ui.ctx(),
                                    labello_domain::UserAction::RetryImageLoad,
                                );
                                if theme::empty_state(
                                    ui,
                                    title,
                                    "No work is available right now. Retry to check again.",
                                    Some(
                                        egui::Button::new(crate::glossary::RETRY_IMAGE_LOAD)
                                            .shortcut_text(crate::theme::button_shortcut(shortcut)),
                                    ),
                                ) {
                                    self.retry_assignment_load();
                                }
                            }
                        });
                    });
                });
        }
    }
}

pub(crate) fn parse_class_color(value: &str) -> Option<egui::Color32> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(egui::Color32::from_rgb(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_colors_parse_with_a_safe_fallback_boundary() {
        assert_eq!(
            parse_class_color("#5eead4"),
            Some(egui::Color32::from_rgb(94, 234, 212))
        );
        assert_eq!(parse_class_color("5eead4"), None);
        assert_eq!(parse_class_color("#invalid"), None);
    }
}
