use egui::{self, RichText};
use labello_domain::{
    AssignmentId, AssignmentKind, AssignmentStatus, ClassId, DatasetId, ImageId,
    MissingObjectLocation, MissingObjectRejection, NormalizedPoint, ReviewDecision, ReviewId,
    ReviewRound, ReviewWorkflow,
};

use crate::{
    app::{AppView, LabelloApp},
    canvas::MissingObjectAction,
    theme,
};

#[derive(Clone, Debug, PartialEq)]
struct MissingObjectScope {
    dataset_id: DatasetId,
    image_id: ImageId,
    assignment_id: AssignmentId,
    round: ReviewRound,
}

#[derive(Default)]
pub(crate) struct MissingObjectDraft {
    scope: Option<MissingObjectScope>,
    pub(crate) locations: Vec<MissingObjectLocation>,
    pub(crate) selected: Option<u32>,
    pub(crate) placing: bool,
    class_id: Option<ClassId>,
    next_id: u32,
    pub(crate) submission: Option<MissingObjectRejection>,
    history: Option<ReviewId>,
    focus: Option<NormalizedPoint>,
}

impl LabelloApp {
    fn missing_object_scope(&self) -> Option<MissingObjectScope> {
        let assignment = self.work.assignment.as_ref()?;
        let state = self.work.current_state.as_ref()?;
        let context = state
            .review_assignment_contexts
            .get(&assignment.assignment_id)?;
        Some(MissingObjectScope {
            dataset_id: self.config.dataset_id.clone(),
            image_id: assignment.image_id.clone(),
            assignment_id: assignment.assignment_id.clone(),
            round: context.round.clone(),
        })
    }

    pub(crate) fn sync_missing_objects(&mut self) {
        let scope = self.missing_object_scope();
        if self.work.missing_objects.scope != scope {
            self.work.missing_objects = MissingObjectDraft {
                scope,
                ..Default::default()
            };
        }
        if !self.missing_objects_editable() {
            self.work.missing_objects.placing = false;
        }
        browser_unload_guard(self.has_missing_object_draft());
    }

    pub(crate) fn has_missing_object_draft(&self) -> bool {
        !self.work.missing_objects.locations.is_empty()
    }

    pub(crate) fn missing_objects_final_phase(&self) -> bool {
        self.view == AppView::Review
            && self.has_dataset_role(labello_domain::DatasetRole::Reviewer)
            && !self.manual_migration_active()
            && self.work.correction_draft.is_none()
            && self.current_review_annotation().is_none()
            && self.selected_task().is_some_and(|task| {
                task.enabled && task.review.workflow == ReviewWorkflow::Approval
            })
            && self.work.assignment.as_ref().is_some_and(|assignment| {
                assignment.kind == AssignmentKind::Review
                    && assignment.status == AssignmentStatus::Active
                    && assignment.assigned_to == self.config.user_id
                    && assignment
                        .expires_at
                        .is_none_or(|expiry| expiry > labello_domain::now())
                    && self.work.current_state.as_ref().is_some_and(|state| {
                        state.image_id == assignment.image_id
                            && state
                                .review_assignment_contexts
                                .get(&assignment.assignment_id)
                                .is_some_and(|context| {
                                    state.review_round(&assignment.task_id) == Some(&context.round)
                                })
                    })
            })
    }

    pub(crate) fn missing_objects_editable(&self) -> bool {
        self.missing_objects_final_phase()
            && !self.loading.saving
            && self.work.pending_transition.is_none()
            && self.work.missing_objects.submission.is_none()
            && self.work.review_revision_commit.is_none()
    }

    pub(crate) fn missing_object_canvas_locations(&self) -> Vec<MissingObjectLocation> {
        if self.has_missing_object_draft() {
            return self.work.missing_objects.locations.clone();
        }
        let Some(state) = self.work.current_state.as_ref() else {
            return Vec::new();
        };
        let Some(task) = self.selected_task() else {
            return Vec::new();
        };
        if self.view == AppView::Annotate {
            state
                .active_missing_object_evidence(&task.task_id)
                .map(|e| e.locations.clone())
                .unwrap_or_default()
        } else if self.view == AppView::Review {
            self.work
                .missing_objects
                .history
                .as_ref()
                .and_then(|id| state.missing_object_evidence.get(id))
                .filter(|e| e.task_id == task.task_id)
                .map(|e| e.locations.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn apply_missing_object_action(&mut self, action: MissingObjectAction) {
        if !self.missing_objects_editable() {
            return;
        }
        let default_class = self
            .selected_task()
            .and_then(|task| task.class_ids.first())
            .cloned();
        let draft = &mut self.work.missing_objects;
        match action {
            MissingObjectAction::Add(point) => {
                if !draft.placing
                    || draft.locations.len() >= labello_domain::MAX_MISSING_OBJECT_LOCATIONS
                    || point.validate().is_err()
                {
                    return;
                }
                let Some(class_id) = draft.class_id.clone().or(default_class) else {
                    return;
                };
                draft.next_id += 1;
                draft.locations.push(MissingObjectLocation {
                    marker_id: draft.next_id,
                    class_id,
                    position: point,
                });
                draft.selected = Some(draft.next_id);
            }
            MissingObjectAction::Select(id) => draft.selected = Some(id),
            MissingObjectAction::Move(id, point) => {
                if point.validate().is_ok()
                    && let Some(marker) = draft
                        .locations
                        .iter_mut()
                        .find(|marker| marker.marker_id == id)
                {
                    marker.position = point;
                    draft.selected = Some(id);
                }
            }
        }
    }

    pub(crate) fn take_missing_object_focus(&mut self) -> Option<NormalizedPoint> {
        self.work.missing_objects.focus.take()
    }

    pub(crate) fn missing_object_panel(&mut self, ui: &mut egui::Ui) {
        let editable = self.missing_objects_editable();
        let final_phase = self.missing_objects_final_phase();
        if final_phase || self.has_missing_object_draft() {
            ui.separator();
            ui.label(RichText::new("Missing objects").strong());
            let count = self.work.missing_objects.locations.len();
            ui.label("Mark missing locations, then send back for annotation.");
            ui.add_enabled_ui(editable, |ui| {
                if ui
                    .add(
                        egui::Button::selectable(self.work.missing_objects.placing, "Mark missing")
                            .min_size(egui::vec2(44.0, 44.0)),
                    )
                    .clicked()
                {
                    self.work.missing_objects.placing = !self.work.missing_objects.placing;
                }
                if self.work.missing_objects.placing
                    && ui
                        .add(
                            egui::Button::new("Add at image center")
                                .min_size(egui::vec2(44.0, 44.0)),
                        )
                        .clicked()
                {
                    self.apply_missing_object_action(MissingObjectAction::Add(NormalizedPoint {
                        x: 0.5,
                        y: 0.5,
                    }));
                }
                if let Some(task) = self.selected_task().cloned() {
                    let type_label = match task.annotation_type {
                        labello_domain::AnnotationType::BoundingBox => "Bounding box",
                        labello_domain::AnnotationType::Skeleton => "Skeleton",
                    };
                    ui.label(format!("Expected annotation: {type_label}"));
                    let selected = self
                        .work
                        .missing_objects
                        .class_id
                        .clone()
                        .or_else(|| task.class_ids.first().cloned());
                    egui::ComboBox::from_id_salt("missing-object-class")
                        .selected_text(
                            selected
                                .as_ref()
                                .map(|id| self.class_name(id))
                                .unwrap_or_default(),
                        )
                        .show_ui(ui, |ui| {
                            for id in &task.class_ids {
                                let label = self.class_name(id);
                                if ui
                                    .selectable_label(selected.as_ref() == Some(id), label)
                                    .clicked()
                                {
                                    self.work.missing_objects.class_id = Some(id.clone());
                                }
                            }
                        });
                }
            });
            if count > 0 {
                ui.label(format!(
                    "{count} missing. Remove all draft locations to enable approval."
                ));
            }
            if self.work.missing_objects.placing {
                ui.label("Click to add; drag to move. Wheel zooms; middle drag pans.");
            }
            ui.label(
                RichText::new("Draft locations are lost on reload or crash.")
                    .small()
                    .color(theme::TEXT_MUTED),
            );
        }
        let history = self
            .selected_task()
            .and_then(|task| {
                self.work.current_state.as_ref().map(|state| {
                    state
                        .missing_object_history(&task.task_id)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        if self.view == AppView::Review && !history.is_empty() && !self.has_missing_object_draft() {
            ui.separator();
            egui::ComboBox::from_id_salt("missing-object-history")
                .selected_text("Missing-object history")
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.work.missing_objects.history,
                        None,
                        "Hide historical locations",
                    );
                    for evidence in history.iter().rev() {
                        let label = format!(
                            "{} · {} · {} locations",
                            evidence.timestamp.format("%Y-%m-%d %H:%M UTC"),
                            evidence.reviewer_user_id,
                            evidence.locations.len()
                        );
                        ui.selectable_value(
                            &mut self.work.missing_objects.history,
                            Some(evidence.review_id.clone()),
                            label,
                        );
                    }
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::ComboBox,
                        true,
                        "Missing-object history",
                    )
                });
        }
        let locations = self.missing_object_canvas_locations();
        if locations.is_empty() {
            return;
        }
        let draft = self.has_missing_object_draft();
        if !draft {
            ui.label(
                RichText::new(if self.view == AppView::Annotate {
                    "Missing-object guidance"
                } else {
                    "Historical missing-object evidence"
                })
                .strong(),
            );
            ui.label(if self.view == AppView::Annotate { "Read-only locations from the latest rejection. Add the missing annotations; guidance remains until task submission." } else { "Read-only history. These locations do not describe the current submission." });
        }
        if draft
            && let Some(selected) = self.work.missing_objects.selected
            && let Some(marker) = self
                .work
                .missing_objects
                .locations
                .iter_mut()
                .find(|marker| marker.marker_id == selected)
        {
            egui::CollapsingHeader::new(format!("Adjust missing {selected} coordinates"))
                .id_salt("missing-object-coordinates")
                .show(ui, |ui| {
                    ui.add_enabled_ui(editable, |ui| {
                        egui::Grid::new("missing-object-coordinate-grid").show(ui, |ui| {
                            let x = ui.label("Across (0–1)");
                            ui.add(
                                egui::DragValue::new(&mut marker.position.x)
                                    .range(0.0..=1.0)
                                    .speed(0.01),
                            )
                            .labelled_by(x.id);
                            ui.end_row();
                            let y = ui.label("Down (0–1)");
                            ui.add(
                                egui::DragValue::new(&mut marker.position.y)
                                    .range(0.0..=1.0)
                                    .speed(0.01),
                            )
                            .labelled_by(y.id);
                            ui.end_row();
                        });
                    });
                });
        }

        for marker in locations {
            let label = format!(
                "Missing {} · {} · {:.0}% across, {:.0}% down",
                marker.marker_id,
                self.class_name(&marker.class_id),
                marker.position.x * 100.0,
                marker.position.y * 100.0
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(
                        egui::Button::selectable(
                            self.work.missing_objects.selected == Some(marker.marker_id),
                            label,
                        )
                        .wrap()
                        .min_size(egui::vec2(44.0, 44.0)),
                    )
                    .clicked()
                {
                    self.work.missing_objects.selected = Some(marker.marker_id);
                    self.work.missing_objects.focus = Some(marker.position);
                }
                if draft
                    && ui
                        .add_enabled(
                            editable,
                            egui::Button::new(format!("Remove missing {}", marker.marker_id))
                                .min_size(egui::vec2(44.0, 44.0)),
                        )
                        .clicked()
                {
                    self.work
                        .missing_objects
                        .locations
                        .retain(|item| item.marker_id != marker.marker_id);
                    if self.work.missing_objects.selected == Some(marker.marker_id) {
                        self.work.missing_objects.selected = None;
                    }
                }
            });
        }
    }

    pub(crate) fn prepare_missing_object_rejection(
        &mut self,
        review: labello_domain::ReviewRecord,
    ) -> Option<MissingObjectRejection> {
        if let Some(submission) = &self.work.missing_objects.submission {
            return Some(submission.clone());
        }
        if !self.missing_objects_final_phase() || review.decision != ReviewDecision::Rejected {
            return None;
        }
        let scope = self.missing_object_scope()?;
        let submission = MissingObjectRejection {
            review,
            round: scope.round,
            locations: self.work.missing_objects.locations.clone(),
        };
        self.work.missing_objects.submission = Some(submission.clone());
        Some(submission)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_unload_guard(_: bool) {}

#[cfg(target_arch = "wasm32")]
fn browser_unload_guard(dirty: bool) {
    use std::cell::Cell;
    use wasm_bindgen::{JsCast, closure::Closure};
    thread_local! {
        static DIRTY: Cell<bool> = const { Cell::new(false) };
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    DIRTY.set(dirty);
    INSTALLED.with(|installed| {
        if installed.get() {
            return;
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| {
            if DIRTY.get() {
                event.prevent_default();
                let _ = js_sys::Reflect::set(event.as_ref(), &"returnValue".into(), &"".into());
            }
        });
        if window
            .add_event_listener_with_callback("beforeunload", handler.as_ref().unchecked_ref())
            .is_ok()
        {
            installed.set(true);
            handler.forget();
        }
    });
}
