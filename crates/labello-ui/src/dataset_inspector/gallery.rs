use super::*;

impl LabelloApp {
    pub(super) fn reset_inspection_gallery(&mut self) {
        self.inspection
            .pending
            .retain(|_, a| !matches!(a, InspectorAction::List(_)));
        self.inspection.navigate_page = None;
        self.inspection.query.page = 1;
        self.inspection.gallery_error = None;
        self.inspect_request(InspectorAction::List(self.inspection.query.clone()));
    }

    pub(super) fn load_more_inspection_images(&mut self) {
        if self.inspection.gallery_error.is_some()
            || self
                .inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::List(_)))
        {
            return;
        }
        if let Some(page) = &self.inspection.page
            && page.page < page.total_pages
        {
            let mut query = self.inspection.active_query.clone();
            query.page = page.page + 1;
            self.inspect_request(InspectorAction::List(query));
        }
    }

    pub(super) fn inspection_gallery(&mut self, ui: &mut egui::Ui) {
        if self.inspection.drawer == Some(false) {
            self.inspection_feedback(ui);
        }
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let busy = self.inspection.busy();
        let loading = self
            .inspection
            .pending
            .values()
            .any(|a| matches!(a, InspectorAction::List(_)));
        let mut refresh = false;
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 82.0).max(44.0);
                let text = self.inspection.query.search.get_or_insert_default();
                let response = ui.add_sized(
                    [width, 44.0],
                    theme::singleline_text_edit(text)
                        .hint_text("Search images")
                        .desired_width(width),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        "Search filename or path",
                    )
                });
                refresh |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let response = ui.add_sized([76.0, 44.0], egui::Button::new("Search"));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Search images")
                });
                refresh |= response.on_hover_text("Search images").clicked();
            });
            let previous = (
                self.inspection.query.task_id.clone(),
                self.inspection.query.class_id.clone(),
                self.inspection.query.status.clone(),
            );
            let width = ui.available_width();
            egui::ComboBox::from_id_salt("gallery-workflow")
                .width(width)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .selected_text(
                    self.inspection
                        .query
                        .task_id
                        .as_ref()
                        .map(|id| {
                            self.work
                                .tasks
                                .iter()
                                .find(|t| t.task_id == *id)
                                .map(|t| t.name.as_str())
                                .unwrap_or(id.as_str())
                        })
                        .unwrap_or("All workflows"),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.inspection.query.task_id, None, "All workflows");
                    for task in &self.work.tasks {
                        ui.selectable_value(
                            &mut self.inspection.query.task_id,
                            Some(task.task_id.clone()),
                            &task.name,
                        );
                    }
                })
                .response
                .on_hover_text("Filter images by workflow");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("gallery-class")
                    .width((width - 6.0) / 2.0)
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .selected_text(
                        self.inspection
                            .query
                            .class_id
                            .as_ref()
                            .map(|id| {
                                self.work
                                    .classes
                                    .iter()
                                    .find(|c| c.class_id == *id)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or(id.as_str())
                            })
                            .unwrap_or("All classes"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.inspection.query.class_id,
                            None,
                            "All classes",
                        );
                        for class in &self.work.classes {
                            ui.selectable_value(
                                &mut self.inspection.query.class_id,
                                Some(class.class_id.clone()),
                                &class.name,
                            );
                        }
                    })
                    .response
                    .on_hover_text("Filter images by class");
                egui::ComboBox::from_id_salt("gallery-status")
                    .width((width - 6.0) / 2.0)
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .selected_text(
                        self.inspection
                            .query
                            .status
                            .as_ref()
                            .map(status_label)
                            .unwrap_or("All statuses"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.inspection.query.status,
                            None,
                            "All statuses",
                        );
                        for status in statuses() {
                            ui.selectable_value(
                                &mut self.inspection.query.status,
                                Some(status.clone()),
                                status_label(&status),
                            );
                        }
                    })
                    .response
                    .on_hover_text("Filter images by status");
            });
            refresh |= previous
                != (
                    self.inspection.query.task_id.clone(),
                    self.inspection.query.class_id.clone(),
                    self.inspection.query.status.clone(),
                );
        });
        if refresh {
            self.reset_inspection_gallery();
        }
        if let Some(error) = self.inspection.gallery_error.clone() {
            ui.colored_label(theme::DANGER, format!("Could not load images. {error}"));
            if ui.button("Retry loading images").clicked() {
                self.inspection.gallery_error = None;
                if let Some(query) = self.inspection.failed_query.take() {
                    self.inspect_request(InspectorAction::List(query));
                }
            }
        }
        let Some(page) = self.inspection.page.as_ref() else {
            if loading {
                ui.spinner();
            }
            return;
        };
        ui.horizontal(|ui| {
            ui.weak(format!("{} images", page.total_items));
            if loading {
                ui.spinner();
            }
        });
        if page.items.is_empty() {
            ui.label("No images match these filters.");
        }
        let columns = 3;
        // Reserve the scrollbar width even before it becomes necessary.
        let width =
            ((ui.available_width() - ui.spacing().scroll.allocated_width() - 12.0) / 3.0).max(1.0);
        let image_height = width * 0.75;
        let row_height = image_height + ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let mut open = None;
        let mut visible = BTreeSet::new();
        let output = egui::ScrollArea::vertical()
            .id_salt("inspection-gallery")
            .vertical_scroll_offset(self.inspection.scroll)
            .show_rows(
                ui,
                row_height,
                page.items.len().div_ceil(columns),
                |ui, rows| {
                    for row in rows {
                        ui.horizontal(|ui| {
                            for item in page.items.iter().skip(row * columns).take(columns) {
                                ui.push_id(&item.image.image_id, |ui| {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(width, row_height),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_width(width);
                                            ui.spacing_mut().button_padding = egui::Vec2::ZERO;
                                            let id = &item.image.image_id;
                                            visible.insert(id.clone());
                                            let image_size = egui::vec2(width, image_height);
                                            let button = if let Some(texture) =
                                                self.inspection.thumbnails.get(id)
                                            {
                                                egui::Button::image(
                                                    egui::Image::new(texture)
                                                        .fit_to_exact_size(image_size)
                                                        .alt_text(format!(
                                                            "Inspect {}",
                                                            item.image.file_name
                                                        )),
                                                )
                                            } else {
                                                egui::Button::new(
                                                    if self.inspection.thumbnail_errors.contains(id)
                                                    {
                                                        "Retry"
                                                    } else {
                                                        "…"
                                                    },
                                                )
                                            };
                                            let enabled =
                                                !busy && self.inspection.reason.is_empty();
                                            let response = ui
                                                .add_enabled_ui(enabled, |ui| {
                                                    ui.add_sized(
                                                        image_size,
                                                        egui::Button::selected(
                                                            button,
                                                            self.inspection
                                                                .selected
                                                                .as_ref()
                                                                .is_some_and(|r| r.image_id == *id),
                                                        ),
                                                    )
                                                })
                                                .inner;
                                            response.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::WidgetType::Button,
                                                    enabled,
                                                    format!("Inspect {}", item.image.file_name),
                                                )
                                            });
                                            if response.clicked() && enabled {
                                                self.inspection.thumbnail_errors.remove(id);
                                                open = Some(item.image.clone());
                                            }
                                            ui.add_sized(
                                                [
                                                    width,
                                                    ui.text_style_height(&egui::TextStyle::Body),
                                                ],
                                                egui::Label::new(&item.image.file_name).truncate(),
                                            )
                                            .on_hover_text(&item.image.canonical_path);
                                        },
                                    );
                                });
                            }
                        });
                    }
                },
            );
        self.inspection.scroll = output.state.offset.y;
        if output.state.offset.y + output.inner_rect.height() >= output.content_size.y - row_height
        {
            self.load_more_inspection_images();
        }
        // Texture memory follows the visible rows, not the ever-growing result list.
        self.inspection.thumbnails.retain(|id, _| {
            visible.contains(id)
                || self
                    .inspection
                    .selected
                    .as_ref()
                    .is_some_and(|r| r.image_id == *id)
        });
        self.schedule_inspection_image();
        for id in visible {
            if self
                .inspection
                .pending
                .values()
                .filter(|a| matches!(a, InspectorAction::Thumbnail(_) | InspectorAction::Image(_)))
                .count()
                >= MAX_IMAGE_REQUESTS
            {
                break;
            }
            if !self.inspection.thumbnails.contains_key(&id)
                && !self.inspection.thumbnail_errors.contains(&id)
                && !self
                    .inspection
                    .pending
                    .values()
                    .any(|a| matches!(a, InspectorAction::Thumbnail(pending) if *pending == id))
            {
                self.inspect_request(InspectorAction::Thumbnail(id));
            }
        }
        if let Some(record) = open {
            self.select_inspection_image(record);
        }
    }
}
