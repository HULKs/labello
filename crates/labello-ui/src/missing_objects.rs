use crate::{
    app::{AppView, LabelloApp},
    canvas::MissingObjectAction,
};
use labello_domain::{MissingObjectLocation, MissingObjectRejection, NormalizedPoint, ReviewId};

#[derive(Default)]
pub(crate) struct MissingObjectDraft {
    pub(crate) locations: Vec<MissingObjectLocation>,
    pub(crate) selected: Option<u32>,
    pub(crate) placing: bool,
    history: Option<ReviewId>,
}

impl LabelloApp {
    pub(crate) fn sync_missing_objects(&mut self) {
        self.work.missing_objects.placing = false;
        browser_unload_guard(self.has_review_corrections());
    }
    pub(crate) fn has_missing_object_draft(&self) -> bool {
        false
    }
    pub(crate) fn missing_objects_final_phase(&self) -> bool {
        false
    }
    pub(crate) fn missing_objects_editable(&self) -> bool {
        false
    }
    pub(crate) fn apply_missing_object_action(&mut self, _: MissingObjectAction) {}
    pub(crate) fn take_missing_object_focus(&mut self) -> Option<NormalizedPoint> {
        None
    }
    pub(crate) fn prepare_missing_object_rejection(
        &mut self,
        _: labello_domain::ReviewRecord,
    ) -> Option<MissingObjectRejection> {
        None
    }

    pub(crate) fn missing_object_canvas_locations(&self) -> Vec<MissingObjectLocation> {
        if self.view != AppView::Review {
            return Vec::new();
        }
        self.work
            .current_state
            .as_ref()
            .and_then(|state| {
                let task = self.selected_task()?;
                state
                    .missing_object_evidence
                    .get(self.work.missing_objects.history.as_ref()?)
                    .filter(|evidence| evidence.task_id == task.task_id)
                    .map(|evidence| evidence.locations.clone())
            })
            .unwrap_or_default()
    }

    pub(crate) fn missing_object_panel(&mut self, ui: &mut egui::Ui) {
        if self.view != AppView::Review {
            return;
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
        if history.is_empty() {
            return;
        }
        egui::CollapsingHeader::new("Historical missing-object evidence").show(ui, |ui| {
            ui.label("Read-only history. These locations do not describe the current submission.");
            ui.selectable_value(
                &mut self.work.missing_objects.history,
                None,
                "Hide historical locations",
            );
            for evidence in history.iter().rev() {
                ui.selectable_value(
                    &mut self.work.missing_objects.history,
                    Some(evidence.review_id.clone()),
                    format!(
                        "{} · {} · {} locations",
                        evidence.timestamp.format("%Y-%m-%d %H:%M UTC"),
                        evidence.reviewer_user_id,
                        evidence.locations.len()
                    ),
                );
            }
        });
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
