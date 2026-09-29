use crate::app::{LabelloApp, UiCommand, UiRequestError};
use eframe::egui;
use labello_domain::KeybindingSet;

#[derive(Default)]
pub(crate) struct PreferencesState {
    pub loaded: bool,
    pub pending: Option<u64>,
    pub error: Option<String>,
    pub legacy: Vec<labello_client::LegacyKeybindings>,
}

impl LabelloApp {
    pub(crate) fn request_preferences(&mut self) {
        if self.auth.preferences.loaded
            || self.loading.keybindings
            || self.runtime.api.is_none()
            || self.auth.account.is_none()
            || self.auth.preferences.pending.is_some()
        {
            return;
        }
        let request = self.request_identity(None);
        self.auth.preferences.pending = Some(request.request_id);
        self.auth.preferences.error = None;
        self.queue_command(UiCommand::Preferences {
            request,
            user_id: self.config.user_id.clone(),
        });
    }

    pub(crate) fn accept_preferences(
        &mut self,
        id: u64,
        result: Result<(KeybindingSet, Vec<labello_client::LegacyKeybindings>), UiRequestError>,
    ) {
        if self.auth.preferences.pending != Some(id) {
            return;
        }
        self.auth.preferences.pending = None;
        match result {
            Ok((bindings, legacy)) => {
                self.auth.preferences.loaded = true;
                let untouched =
                    self.work.shortcut_settings.draft == self.work.shortcut_settings.baseline;
                self.work.keybindings = bindings.clone();
                if self.work.show_settings && untouched {
                    self.work.shortcut_settings.draft = Some(bindings.clone());
                    self.work.shortcut_settings.baseline = Some(bindings);
                }
                self.auth.preferences.legacy = legacy;
                self.auth.preferences.error = None;
                if !self.auth.preferences.legacy.is_empty() {
                    self.work.show_settings = true;
                }
            }
            Err(error) => self.auth.preferences.error = Some(error.to_string()),
        }
    }

    pub(crate) fn preference_controls(&mut self, ui: &mut egui::Ui) {
        ui.label("Your shortcuts apply to all datasets.");
        if self.auth.preferences.pending.is_some() {
            ui.label("Loading your shortcuts…");
        }
        if let Some(error) = &self.auth.preferences.error {
            ui.label(format!("Could not load your shortcuts: {error}"));
            if ui.button("Retry shortcuts").clicked() {
                self.request_preferences();
            }
        }
        if !self.auth.preferences.legacy.is_empty() {
            ui.label("Saved dataset shortcuts are available. Choose a set to use everywhere, or save the current settings. Original copies are retained.");
            let mut selected = None;
            egui::ComboBox::from_id_salt("legacy-shortcuts")
                .selected_text("Choose saved shortcuts")
                .width(ui.available_width().min(320.0))
                .truncate()
                .show_ui(ui, |ui| {
                    for (index, entry) in self.auth.preferences.legacy.iter().enumerate() {
                        if ui
                            .selectable_label(
                                false,
                                format!("{} ({})", entry.name, entry.dataset_id),
                            )
                            .clicked()
                        {
                            selected = Some(index);
                        }
                    }
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::ComboBox,
                        true,
                        "Choose saved shortcuts",
                    )
                });
            if let Some(index) = selected {
                self.work.shortcut_settings.draft =
                    Some(self.auth.preferences.legacy[index].bindings.clone());
            }
        }
    }
}
