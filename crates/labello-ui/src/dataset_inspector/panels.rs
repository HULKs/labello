use super::*;

impl LabelloApp {
    pub(crate) fn inspection_panels(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        if self.inspection.drawer.is_none() && self.inspection.drawer_invoker.is_some() {
            if ui.ctx().memory(|m| m.top_modal_layer().is_none()) {
                let id = self.inspection.drawer_invoker.take().unwrap();
                ui.ctx().memory_mut(|m| m.request_focus(id));
            } else {
                ui.ctx().request_repaint();
            }
        }
        self.schedule_inspection_image();
        if self.inspection.page.is_none()
            && self.inspection.error.is_none()
            && !self
                .inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::List(_)))
        {
            self.inspect_request(InspectorAction::List(self.inspection.query.clone()));
        }
        let available = (ui.available_width() - 30.0).max(1.0);
        let labels = if layout == LayoutMode::Compact {
            ["Images", "Overlays", "Fit image", "‹", "›", "Refresh image"]
        } else {
            [
                "Images",
                "Overlays",
                "Fit image",
                "Previous image",
                "Next image",
                "Refresh image",
            ]
        };
        let mut rows = 1;
        let mut used = 0.0;
        for label in labels {
            let galley = egui::WidgetText::from(label).into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            );
            let width = (galley.size().x + 2.0 * ui.spacing().button_padding.x).max(44.0);
            if used > 0.0 && used + width > available {
                rows += 1;
                used = 0.0;
            }
            used += width + ui.spacing().item_spacing.x;
        }
        let identity_height = if self.inspection.selected.is_some() {
            ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y
        } else {
            0.0
        };
        let height = 18.0
            + rows as f32 * 44.0
            + (rows - 1) as f32 * ui.spacing().item_spacing.y
            + identity_height;
        egui::Panel::top("inspection-context")
            .min_size(height)
            .frame(theme::top_bar_frame().fill(theme::PANEL))
            .show(ui, |ui| self.inspection_context(ui, layout));
        if layout == LayoutMode::Wide {
            self.inspection.drawer = None;
            if !self.inspection.images_collapsed {
                egui::Panel::left("inspection-images")
                    .exact_size(280.0)
                    .resizable(false)
                    .frame(theme::side_frame())
                    .show(ui, |ui| {
                        ui.heading("Images");
                        self.inspection_gallery(ui);
                    });
            } else {
                ui.skip_ahead_auto_ids(1);
            }
            if !self.inspection.overlays_collapsed {
                egui::Panel::right("inspection-overlays")
                    .exact_size(315.0)
                    .resizable(false)
                    .frame(theme::side_frame())
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("inspection-overlays-scroll")
                            .show(ui, |ui| self.inspection_sidebar(ui));
                    });
            } else {
                ui.skip_ahead_auto_ids(1);
            }
        }
    }

    fn inspection_context(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        ui.spacing_mut().interact_size.x = ui.spacing().interact_size.x.max(44.0);
        let navigation_allowed = self.inspection.reason.is_empty()
            && !self.inspection.busy()
            && !self
                .inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::List(_)));
        ui.horizontal_wrapped(|ui| {
            for (right, title) in [(false, "Images"), (true, "Overlays")] {
                let selected = if layout == LayoutMode::Wide {
                    if right {
                        !self.inspection.overlays_collapsed
                    } else {
                        !self.inspection.images_collapsed
                    }
                } else {
                    self.inspection.drawer == Some(right)
                };
                let response = ui.add(egui::Button::new(title).selected(selected));
                if response.clicked() {
                    if layout == LayoutMode::Wide {
                        if right {
                            self.inspection.overlays_collapsed = selected;
                        } else {
                            self.inspection.images_collapsed = selected;
                        }
                    } else {
                        self.inspection.drawer = if selected { None } else { Some(right) };
                        self.inspection.drawer_invoker = Some(response.id);
                    }
                }
            }
            if ui.button("Fit image").clicked() {
                self.inspection.canvas.fit_view();
            }
            let items = self
                .inspection
                .page
                .as_ref()
                .map(|p| p.items.clone())
                .unwrap_or_default();
            let position = self
                .inspection
                .selected
                .as_ref()
                .and_then(|r| items.iter().position(|i| i.image.image_id == r.image_id));
            for (previous, label, icon) in
                [(true, "Previous image", "‹"), (false, "Next image", "›")]
            {
                let target = position
                    .and_then(|p| {
                        if previous {
                            p.checked_sub(1)
                        } else {
                            p.checked_add(1)
                        }
                    })
                    .and_then(|p| items.get(p));
                let page_target = position.and(self.inspection.page.as_ref()).and_then(|p| {
                    if previous && p.page > 1 {
                        Some(p.page - 1)
                    } else if !previous && p.page < p.total_pages {
                        Some(p.page + 1)
                    } else {
                        None
                    }
                });
                let enabled = navigation_allowed && (target.is_some() || page_target.is_some());
                let response = ui.add_enabled(
                    enabled,
                    egui::Button::new(if layout == LayoutMode::Compact {
                        icon
                    } else {
                        label
                    })
                    .min_size(egui::vec2(44.0, 44.0)),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label)
                });
                if response.on_hover_text(label).clicked() {
                    if let Some(target) = target {
                        self.select_inspection_image(target.image.clone());
                    } else if let Some(page) = page_target {
                        self.inspection.navigate_page = Some(previous);
                        self.inspection.query.page = page;
                        self.inspection.scroll = 0.0;
                        self.inspect_request(InspectorAction::List(self.inspection.query.clone()));
                    }
                }
            }
            if ui
                .add_enabled(
                    !self.inspection.busy()
                        && self.inspection.selected.is_some()
                        && !self
                            .inspection
                            .pending
                            .values()
                            .any(|a| matches!(a, InspectorAction::State(_))),
                    egui::Button::new("Refresh image"),
                )
                .clicked()
            {
                let record = self.inspection.selected.clone().unwrap();
                self.inspection.error = None;
                self.inspection.preview_loaded = false;
                self.inspection.image_error = None;
                self.inspection.retry = None;
                self.inspect_request(InspectorAction::State(record.image_id));
            }
        });
        if let Some(record) = &self.inspection.selected {
            ui.add(egui::Label::new(&record.file_name).truncate())
                .on_hover_text(&record.canonical_path);
        }
    }

    fn inspection_sidebar(&mut self, ui: &mut egui::Ui) {
        if let Some(record) = self.inspection.selected.clone() {
            let busy = self.inspection.busy()
                || self.inspection.state.is_none()
                || self
                    .inspection
                    .pending
                    .values()
                    .any(|a| matches!(a, InspectorAction::State(id) if *id == record.image_id));
            self.inspection_controls(ui, &record, busy);
        } else {
            ui.heading("Annotation overlays");
            ui.label("Choose an image to see its annotations.");
        }
    }

    pub(crate) fn inspection_drawer(&mut self, ctx: &egui::Context, layout: LayoutMode) {
        if layout == LayoutMode::Wide {
            return;
        }
        let Some(right) = self.inspection.drawer else {
            return;
        };
        let screen = ctx.content_rect();
        let width = 315.0_f32.min(screen.width() - 48.0);
        let height = (screen.height() - 64.0).max(120.0);
        let title = if right { "Overlays" } else { "Images" };
        let id = egui::Id::new("inspection-drawer");
        let area = egui::Modal::default_area(id)
            .anchor(
                if right {
                    egui::Align2::RIGHT_CENTER
                } else {
                    egui::Align2::LEFT_CENTER
                },
                egui::vec2(if right { -12.0 } else { 12.0 }, 0.0),
            )
            .default_width(width)
            .constrain_to(screen);
        let mut close = false;
        let response = theme::modal(ctx, id).area(area).show(ctx, |ui| {
            ui.set_width(width);
            ui.set_max_height(height);
            let button = ui.button(format!("Close {title}"));
            if self
                .inspection
                .drawer_invoker
                .is_some_and(|id| ctx.memory(|m| m.focused()) == Some(id))
            {
                button.request_focus();
            }
            close = button.clicked();
            if right {
                egui::ScrollArea::vertical()
                    .id_salt("inspection-drawer-scroll")
                    .max_height((height - 54.0).max(60.0))
                    .show(ui, |ui| self.inspection_sidebar(ui));
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("inspection-images-drawer-scroll")
                    .max_height((height - 54.0).max(60.0))
                    .show(ui, |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, (height - 54.0).max(480.0)),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| self.inspection_gallery(ui),
                        );
                    });
            }
        });
        response
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Window, true, title));
        if close || response.should_close() || self.inspection.drawer.is_none() {
            self.inspection.drawer = None;
            ctx.request_repaint();
        }
    }
}
