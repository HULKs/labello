impl LabelloApp {
    pub(crate) fn visible_prelabels(&self) -> Vec<labello_domain::PrelabelSuggestion> {
        if self.view != AppView::Annotate || !self.auth.prelabel_available {
            return Vec::new();
        }
        let Some(current) = &self.work.current else {
            return Vec::new();
        };
        let Some(task) = self.selected_task() else {
            return Vec::new();
        };
        let choice = self.prelabel_choice(&task.task_id);
        if self.runtime.api.is_some() && choice.is_none() {
            return Vec::new();
        }
        if let Some(config) = &choice
            && self
                .work
                .prelabels
                .hints
                .get(&(
                    current.image.image_id.clone(),
                    task.task_id.clone(),
                    config.clone(),
                ))
                .is_some_and(|status| {
                    status.error.is_some() || status.generation.as_ref().is_some_and(|g| g.paused)
                })
        {
            return Vec::new();
        }
        let processing = choice
            .as_ref()
            .and_then(|id| {
                self.datasets
                    .metadata
                    .as_ref()?
                    .prelabel_configs
                    .iter()
                    .find(|c| &c.config_id == id)
            })
            .map(|config| config.output_processing.clone())
            .unwrap_or(labello_domain::OutputProcessing {
                confidence_threshold: 0.0,
                suppress_overlaps_iou: None,
            });
        let hints: Vec<_> = current
            .prelabels
            .iter()
            .filter(|suggestion| {
                !self
                    .work
                    .accepted_prelabels
                    .contains(&suggestion.suggestion_id)
                    && suggestion.task_id == task.task_id
                    && choice.as_ref().is_none_or(|id| &suggestion.config_id == id)
            })
            .cloned()
            .collect();
        labello_domain::filter_prelabels(&hints, &self.work.annotations, &processing)
            .into_iter()
            .filter(|suggestion| self.selected_class_id() == Some(&suggestion.class_id))
            .collect()
    }

    fn prelabel_panel(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Prelabels");
        if !self.auth.prelabel_available {
            crate::prelabel_flow::disabled_notice(ui);
            return;
        }
        self.prelabel_selector(ui);
        let prelabels = self.visible_prelabels();
        if self.work.selected_prelabel.as_ref().is_none_or(|selected| {
            !prelabels
                .iter()
                .any(|suggestion| &suggestion.suggestion_id == selected)
        }) {
            self.work.selected_prelabel = prelabels
                .first()
                .map(|suggestion| suggestion.suggestion_id.clone());
        }
        if prelabels.is_empty() {
            let choice = self
                .selected_task()
                .and_then(|task| self.prelabel_choice(&task.task_id));
            let status = self.work.current.as_ref().and_then(|current| {
                let task = self.selected_task()?;
                self.work.prelabels.hints.get(&(
                    current.image.image_id.clone(),
                    task.task_id.clone(),
                    choice.clone()?,
                ))
            });
            if choice.is_none() {
                theme::empty_state(
                    ui,
                    "Prelabels turned off",
                    "Draw annotations manually or select a model.",
                    None,
                );
            } else if status.is_some_and(|status| {
                status.error.is_none()
                    && status
                        .generation
                        .as_ref()
                        .is_some_and(|generation| !generation.paused)
            }) {
                let message = if self
                    .work
                    .current
                    .as_ref()
                    .is_none_or(|current| current.prelabels.is_empty())
                {
                    "No model predictions meet the configured confidence threshold."
                } else {
                    "Hints have been accepted, discarded, or suppressed by overlapping boxes."
                };
                theme::empty_state(ui, "No remaining suggestions", message, None);
            }
        }
        for suggestion in &prelabels {
            let selected = self.work.selected_prelabel.as_ref() == Some(&suggestion.suggestion_id);
            let frame = theme::prelabel_card_frame(selected);
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        theme::badge(
                            ui,
                            &format!("{:.0}%", suggestion.confidence * 100.0),
                            theme::Intent::Accent,
                        );
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        selected,
                                        RichText::new(suggestion.class_id.to_string()).monospace(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(suggestion.class_id.to_string())
                                .clicked()
                            {
                                self.work.selected_prelabel =
                                    Some(suggestion.suggestion_id.clone());
                            }
                        });
                    });
                });
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                    let size = egui::vec2(width, 44.0);
                    if theme::primary_button(
                        ui,
                        !self.loading.saving,
                        egui::Button::new("Approve").min_size(size),
                    )
                    .on_hover_text(format!(
                        "Shortcut: {}",
                        self.shortcut_text(ui.ctx(), labello_domain::UserAction::AcceptPrelabel,)
                    ))
                    .clicked()
                    {
                        self.accept_prelabel(suggestion);
                        self.work.selected_prelabel = self
                            .visible_prelabels()
                            .first()
                            .map(|suggestion| suggestion.suggestion_id.clone());
                    }
                    if theme::danger_button(
                        ui,
                        !self.loading.saving,
                        egui::Button::new("Discard").min_size(size),
                    )
                    .on_hover_text(format!(
                        "Shortcut: {}",
                        self.shortcut_text(ui.ctx(), labello_domain::UserAction::DiscardPrelabel,)
                    ))
                    .clicked()
                    {
                        self.discard_prelabel(suggestion.suggestion_id.clone());
                    }
                });
            });
        }
    }
}
