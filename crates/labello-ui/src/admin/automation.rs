impl LabelloApp {
    fn admin_automation(&mut self, ui: &mut egui::Ui) {
        ui.heading("Automation");
        ui.label(
            RichText::new("Configure preloading, prelabels, and assignment balancing.").color(theme::TEXT_MUTED),
        );
        let enabled = !self.loading.admin
            && self.loading.roles_user.is_none()
            && !self.loading.uploading
            && !self.loading.ingesting;
        let prelabel_available = self.auth.prelabel_available;
        if !prelabel_available {
            admin_card(ui, "Prelabels card", |ui| {
                ui.heading("Prelabels");
                crate::prelabel_flow::disabled_notice(ui);
            });
        }
        let mut model_checks = Vec::new();
        if let Some(config) = self.datasets.admin_config.as_mut() {
            ui.add_enabled_ui(enabled, |ui| {
                admin_card(ui, "Image preloading card", |ui| {
                    ui.heading("Image preloading");
                    ui.horizontal_wrapped(|ui| {
                        let label = ui.label("Upcoming images");
                        ui.add(egui::DragValue::new(&mut config.preload_queue_size)
                            .range(1..=labello_domain::MAX_PRELOAD_QUEUE_SIZE))
                            .labelled_by(label.id);
                    });
                    ui.small("Preload upcoming annotation and review work. Larger queues use more memory and reserve more work. Balance limits may keep the queue below this target.");
                });
                if prelabel_available {
                    edit_prelabels(ui, &mut config.prelabel_configs, &mut config.tasks, &config.label_classes, &mut self.admin.prelabels.model_checks, &mut model_checks);
                }
                edit_imbalance(ui, &mut config.imbalance);
            });
        }
        for action in model_checks { self.request_prelabels(action); }
        self.prelabel_admin_panel(ui);
    }
}
