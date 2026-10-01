impl LabelloApp {
    fn admin_automation(&mut self, ui: &mut egui::Ui) {
        ui.heading(crate::glossary::AUTOMATION);
        ui.label(
            RichText::new("Configure box visibility, preloading, prelabels, and assignment balancing.").color(theme::TEXT_MUTED),
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
                    ui.heading("Work queues");
                    ui.horizontal_wrapped(|ui| {
                        let label = ui.label("Preloaded items");
                        ui.add(egui::DragValue::new(&mut config.preload_queue_size)
                            .range(1..=labello_domain::MAX_PRELOAD_QUEUE_SIZE))
                            .labelled_by(label.id);
                    });
                    ui.small("Preload upcoming annotation and review work. Larger queues use more memory and reserve more work. Balance limits may keep the queue below this target.");
                    ui.horizontal_wrapped(|ui| {
                        let label = ui.label("Previous items");
                        ui.add(egui::DragValue::new(&mut config.workflow_queue.history_depth).range(0..=labello_domain::MAX_WORKFLOW_HISTORY_DEPTH)).labelled_by(label.id);
                    });
                    let mut limited = config.workflow_queue.max_pending_overviews.is_some();
                    if ui.checkbox(&mut limited, "Limit images awaiting Overview").changed() {
                        config.workflow_queue.max_pending_overviews = limited.then_some(10);
                    }
                    if let Some(limit) = &mut config.workflow_queue.max_pending_overviews {
                        ui.horizontal_wrapped(|ui| {
                            let label = ui.label("Images awaiting Overview");
                            ui.add(egui::DragValue::new(limit).range(1..=100_000)).labelled_by(label.id);
                        });
                    }
                });
                admin_card(ui, "Overlapping boxes card", |ui| {
                    ui.heading("Overlapping boxes");
                    ui.horizontal_wrapped(|ui| {
                        let label = ui.label("IoU threshold");
                        ui.add(egui::DragValue::new(&mut config.bounding_box_visibility.iou_threshold)
                            .range(0.0..=1.0).speed(0.01).max_decimals(2))
                            .labelled_by(label.id);
                    });
                    ui.small("Hide same-class boxes above this overlap within each image. Prefer approved, then unreviewed boxes; use stable object IDs for ties. Stored annotations and exports are preserved. Set 1 to show all boxes.");
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
