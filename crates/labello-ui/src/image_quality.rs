use std::{cell::RefCell, collections::BTreeMap, future::Future, rc::Rc};

use eframe::egui;
use futures::future::{AbortHandle, AbortRegistration, Abortable};
use labello_client::{ClientError, ClientResult, ImagePreviewProfile};

use crate::app::{LabelloApp, LoadedImage, UiCommand};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Representation {
    #[default]
    Standard,
    DataSaver,
    Original,
}

#[derive(Default)]
pub(crate) struct ImageQuality {
    pub data_saver: bool,
    pub current: Representation,
    pub requested: Representation,
    pub loading: Option<u64>,
    pub error: Option<String>,
    transfers: Rc<RefCell<BTreeMap<u64, AbortHandle>>>,
}

impl ImageQuality {
    pub fn policy(&self) -> Representation {
        if self.data_saver {
            Representation::DataSaver
        } else {
            Representation::Standard
        }
    }

    pub fn transfer(&self, id: u64, representation: Representation) -> ImageTransfer {
        let (handle, registration) = AbortHandle::new_pair();
        self.transfers.borrow_mut().insert(id, handle);
        ImageTransfer {
            id,
            representation,
            registration: Some(registration),
            transfers: self.transfers.clone(),
        }
    }

    pub fn cancel(&self, id: u64) {
        if let Some(handle) = self.transfers.borrow_mut().remove(&id) {
            handle.abort();
        }
    }

    pub fn cancel_all(&self) {
        for (_, handle) in std::mem::take(&mut *self.transfers.borrow_mut()) {
            handle.abort();
        }
    }
}

pub(crate) struct ImageTransfer {
    id: u64,
    pub representation: Representation,
    registration: Option<AbortRegistration>,
    transfers: Rc<RefCell<BTreeMap<u64, AbortHandle>>>,
}
impl ImageTransfer {
    pub async fn run<T>(
        mut self,
        future: impl Future<Output = ClientResult<T>>,
    ) -> ClientResult<T> {
        Abortable::new(
            future,
            self.registration.take().expect("one image transfer"),
        )
        .await
        .map_err(|_| ClientError::Api {
            status: 0,
            message: "image request superseded".into(),
        })?
    }
}
impl Drop for ImageTransfer {
    fn drop(&mut self) {
        self.transfers.borrow_mut().remove(&self.id);
    }
}

impl LabelloApp {
    pub(crate) fn set_data_saver(&mut self, selected: bool) {
        if self.work.quality.data_saver == selected {
            return;
        }
        self.work.quality.data_saver = selected;
        if let Some(identity) = self.runtime.persistence.identity.as_ref()
            && let Err(error) = crate::persistence::save_data_saver(identity, selected)
        {
            self.runtime.storage_error = Some(error);
        }
        if let Some(id) = self.work.active_prefetch_id.take() {
            self.work.quality.cancel(id);
            self.runtime.active_requests.remove(&id);
        }
        self.work.queue.set_loading(false);
        self.release_prepared_assignments();
        self.work.queue.clear_failure();
        self.request_representation(self.work.quality.policy());
        self.request_prefetch();
    }

    pub(crate) fn request_representation(&mut self, representation: Representation) {
        let Some(assignment) = self.work.assignment.clone() else {
            return;
        };
        if self.runtime.api.is_none() || self.loading.image {
            return;
        }
        if let Some(id) = self.work.quality.loading.take() {
            self.work.quality.cancel(id);
            self.runtime.active_requests.remove(&id);
        }
        let operation_id = self.next_operation();
        let request = self.operation_identity(operation_id, self.config.dataset_id.clone());
        self.work.quality.loading = Some(operation_id);
        self.work.quality.requested = representation;
        self.work.quality.error = None;
        self.queue_command(UiCommand::LoadRepresentation {
            request,
            operation_id,
            dataset_id: self.config.dataset_id.clone(),
            assignment,
            representation,
        });
    }

    pub(crate) fn apply_representation(
        &mut self,
        ctx: &egui::Context,
        operation_id: u64,
        result: Result<LoadedImage, String>,
    ) {
        if self.work.quality.loading != Some(operation_id) {
            return;
        }
        self.work.quality.loading = None;
        match result {
            Ok(loaded)
                if self.work.assignment.as_ref().is_some_and(|assignment| {
                    assignment.assignment_id == loaded.assignment.assignment_id
                        && assignment.image_id == loaded.assignment.image_id
                }) =>
            {
                if let Some(current) = &self.work.current {
                    if current.image.blake3 != loaded.queued.image.blake3
                        || current.image.dimensions() != loaded.queued.image.dimensions()
                    {
                        self.work.quality.error =
                            Some("The original image changed; reopen the assignment.".into());
                        return;
                    }
                    // A representation reply owns pixels only. Drafts, annotations,
                    // selection, save generations and canvas transforms stay owned by work.
                    self.work.current_texture = loaded.color_image.map(|image| {
                        ctx.load_texture(
                            "working-image-detail",
                            image,
                            egui::TextureOptions::LINEAR,
                        )
                    });
                    self.work.quality.current = loaded.representation;
                    self.work.quality.error = None;
                } else {
                    self.runtime.error = None;
                    self.apply_loaded_image(ctx, loaded);
                }
            }
            Ok(_) => (),
            Err(error) => self.work.quality.error = Some(error),
        }
    }

    pub(crate) fn short_image_quality_button(&mut self, ui: &mut egui::Ui) {
        let label = if self.work.quality.loading.is_some() {
            "Loading…"
        } else if self.work.quality.error.is_some() {
            "Image error"
        } else if self.work.quality.current == Representation::Original {
            "Original"
        } else if self.work.quality.data_saver {
            "Data saver"
        } else {
            "Quality"
        };
        let response = ui
            .add_sized([108.0, 44.0], egui::Button::new(label).truncate())
            .on_hover_text("Image quality settings");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Image quality settings")
        });
        if response.clicked() {
            self.open_shortcut_settings();
        }
    }

    pub(crate) fn image_quality_controls(&mut self, ui: &mut egui::Ui) {
        if self.work.assignment.is_none() {
            return;
        }
        let mut selected = self.work.quality.data_saver;
        let busy = self.work.quality.loading.is_some();
        let failed = self.work.quality.error.is_some()
            || (!self.loading.image && self.work.current.is_none() && self.runtime.error.is_some());
        let status = if busy {
            "Loading detail…"
        } else if failed {
            "Image unavailable"
        } else if self.work.quality.current == Representation::Original {
            "Original detail"
        } else if selected {
            "Data saver active"
        } else {
            "Standard detail"
        };
        let mut original = false;
        let mut retry = false;
        let use_preview = self.work.quality.current == Representation::Original && !failed;
        let compact = ui.available_width() < 600.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = 44.0;
            let response = ui.add_enabled_ui(!self.loading.image, |ui| {
                ui.add_sized([108.0, 44.0], egui::Checkbox::new(&mut selected, "Data saver"))
            }).inner;
            response.on_hover_text("Smaller previews use less data and show less detail. Original detail loads only when you request it.");
            let mut actions = |ui: &mut egui::Ui| {
                if compact { ui.label(status); }
                original = ui.add_enabled(!self.loading.image, egui::Button::new(if use_preview { "Use selected preview" } else { "Load original detail" }).min_size(egui::vec2(0.0,44.0))).clicked();
                if failed { retry = ui.add(egui::Button::new("Retry image").min_size(egui::vec2(0.0,44.0))).clicked(); }
                if compact && (original || retry) { ui.close(); }
            };
            if compact {
                ui.menu_button(if busy { "Loading image…" } else if failed { "Image unavailable" } else if use_preview { "Original detail" } else { "Image quality" }, actions);
            } else {
                ui.label(status);
                actions(ui);
            }
        });
        if selected != self.work.quality.data_saver {
            self.set_data_saver(selected);
        }
        if original {
            self.request_representation(if use_preview {
                self.work.quality.policy()
            } else {
                Representation::Original
            });
        }
        if retry {
            self.request_representation(if self.work.quality.error.is_some() {
                self.work.quality.requested
            } else {
                self.work.quality.policy()
            });
        }
    }
}

pub(crate) async fn preview_for_representation(
    api: &dyn labello_client::LabelloApi,
    dataset_id: &labello_domain::DatasetId,
    image_id: &labello_domain::ImageId,
    representation: Representation,
) -> ClientResult<labello_client::ImagePreview> {
    match representation {
        Representation::Standard | Representation::DataSaver => {
            crate::live_workflow::load_working_preview(
                api,
                dataset_id,
                image_id,
                if representation == Representation::DataSaver {
                    ImagePreviewProfile::DataSaverV1
                } else {
                    ImagePreviewProfile::StandardV1
                },
            )
            .await
        }
        Representation::Original => {
            let (record, file) = futures::try_join!(
                api.get_image_record(dataset_id, image_id),
                api.get_original_detail(dataset_id, image_id)
            )?;
            file.decode_original_detail(record.width, record.height)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_drops_the_image_transfer_and_clears_its_registry_entry() {
        let quality = ImageQuality::default();
        let transfer = quality.transfer(1, Representation::DataSaver);
        quality.cancel(1);
        let result: ClientResult<()> =
            futures::executor::block_on(transfer.run(futures::future::pending()));
        assert!(result.is_err());
        assert!(quality.transfers.borrow().is_empty());
    }
}
