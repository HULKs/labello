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
        let eligible = hints.iter().filter(|hint| !labello_domain::filter_prelabels(std::slice::from_ref(*hint), &self.work.annotations, &processing).is_empty());
        let candidates = eligible.map(|hint| {
            let mut candidate = hint.clone();
            if let Some(item) = self.work.prelabel_review.objects.iter()
                .find(|item| item.suggestion.suggestion_id == hint.suggestion_id) {
                candidate.geometry = item.annotation.geometry.clone();
            }
            candidate
        }).collect::<Vec<_>>();
        let kept = labello_domain::filter_prelabels(&candidates, &self.work.annotations, &processing);
        kept.iter().filter_map(|candidate| hints.iter().find(|hint| hint.suggestion_id == candidate.suggestion_id))
            .filter(|suggestion| self.selected_class_id() == Some(&suggestion.class_id))
            .cloned().collect()
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
                    "Objects have been confirmed, deleted, or suppressed by overlapping boxes."
                };
                theme::empty_state(ui, "No remaining suggestions", message, None);
            }
        }
        if !prelabels.is_empty() {
            ui.label(format!("{} objects need confirmation", prelabels.len()));
            ui.label("Edit the selected object on the canvas, then Confirm & next. Delete removes it.");
        }
    }
}
