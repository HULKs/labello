use super::*;

impl LabelloApp {
    pub(super) fn reset_inspection_gallery(&mut self) {
        let obsolete: Vec<_> = self
            .inspection
            .pending
            .iter()
            .filter_map(|(id, action)| matches!(action, InspectorAction::List(_)).then_some(*id))
            .collect();
        for id in obsolete {
            self.inspection.transfers.cancel(id);
            self.inspection.pending.remove(&id);
            self.runtime.active_requests.remove(&id);
        }
        let selected = self
            .inspection
            .selected
            .as_ref()
            .map(|r| r.image_id.clone());
        self.cancel_obsolete_inspection_previews(&BTreeSet::new(), selected.as_ref());
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
            filter_menu(
                ui,
                "gallery-workflow",
                width,
                (self
                    .inspection
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
                    .unwrap_or("All workflows"))
                .to_owned(),
                |ui| {
                    filter_menu_width(
                        ui,
                        std::iter::once("All workflows")
                            .chain(self.work.tasks.iter().map(|task| task.name.as_str())),
                    );
                    filter_choice(
                        ui,
                        &mut self.inspection.query.task_id,
                        None,
                        "All workflows",
                        FilterIcon::All,
                    );
                    for task in &self.work.tasks {
                        filter_choice(
                            ui,
                            &mut self.inspection.query.task_id,
                            Some(task.task_id.clone()),
                            &task.name,
                            FilterIcon::Workflow(&task.annotation_type),
                        );
                    }
                },
            )
            .on_hover_text("Filter images by workflow");
            ui.horizontal(|ui| {
                filter_menu(
                    ui,
                    "gallery-class",
                    (width - 6.0) / 2.0,
                    (self
                        .inspection
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
                        .unwrap_or("All classes"))
                    .to_owned(),
                    |ui| {
                        filter_menu_width(
                            ui,
                            std::iter::once("All classes")
                                .chain(self.work.classes.iter().map(|class| class.name.as_str())),
                        );
                        filter_choice(
                            ui,
                            &mut self.inspection.query.class_id,
                            None,
                            "All classes",
                            FilterIcon::All,
                        );
                        for class in &self.work.classes {
                            filter_choice(
                                ui,
                                &mut self.inspection.query.class_id,
                                Some(class.class_id.clone()),
                                &class.name,
                                FilterIcon::Class(
                                    crate::workspace_canvas::parse_class_color(&class.color)
                                        .unwrap_or(theme::INFO),
                                ),
                            );
                        }
                    },
                )
                .on_hover_text("Filter images by class");
                filter_menu(
                    ui,
                    "gallery-status",
                    (width - 6.0) / 2.0,
                    (self
                        .inspection
                        .query
                        .status
                        .as_ref()
                        .map(status_label)
                        .unwrap_or("All statuses"))
                    .to_owned(),
                    |ui| {
                        filter_menu_width(
                            ui,
                            std::iter::once("All statuses")
                                .chain(statuses().iter().map(status_label)),
                        );
                        filter_choice(
                            ui,
                            &mut self.inspection.query.status,
                            None,
                            "All statuses",
                            FilterIcon::All,
                        );
                        for status in statuses() {
                            filter_choice(
                                ui,
                                &mut self.inspection.query.status,
                                Some(status.clone()),
                                status_label(&status),
                                FilterIcon::Status(&status),
                            );
                        }
                    },
                )
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
        let replacing = self
            .inspection
            .pending
            .values()
            .any(|a| matches!(a, InspectorAction::List(q) if q.page == 1));
        if replacing {
            ui.label("Applying filters… Previous results remain visible.");
        }
        let Some(page) = self.inspection.page.as_ref() else {
            if loading {
                ui.spinner();
            }
            return;
        };
        ui.horizontal(|ui| {
            ui.weak(format!("{} matching images", page.total_items));
            if page.items.len() < page.total_items {
                ui.weak(format!("{} listed", page.items.len()));
            }
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
        let selected = self
            .inspection
            .selected
            .as_ref()
            .map(|r| r.image_id.clone());
        self.cancel_obsolete_inspection_previews(&visible, selected.as_ref());
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

fn compact_menu_style(style: &mut egui::Style) {
    style.spacing.interact_size.y = 32.0;
    style.spacing.item_spacing.y = 2.0;
    style.spacing.button_padding.y = 2.0;
}

pub(super) enum FilterIcon<'a> {
    All,
    Workflow(&'a AnnotationType),
    Class(egui::Color32),
    Status(&'a TaskStatus),
}

fn filter_choice<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
    icon: FilterIcon<'_>,
) {
    let selected = *current == value;
    if filter_option(ui, selected, label, icon).clicked() {
        *current = value;
    }
}

pub(super) fn filter_option(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    icon: FilterIcon<'_>,
) -> egui::Response {
    let icon_id = ui.id().with(("filter-icon", label));
    let icon_width = if matches!(icon, FilterIcon::Workflow(_)) {
        28.0
    } else {
        20.0
    };
    let atoms = (
        egui::Atom::custom(icon_id, egui::vec2(icon_width, 28.0)),
        label,
    );
    let choice = egui::Button::new(atoms)
        .min_size(egui::vec2(ui.available_width(), 32.0))
        .selected(selected)
        .wrap_mode(egui::TextWrapMode::Truncate)
        .atom_ui(ui);
    if let Some(rect) = choice.rect(icon_id) {
        let center = rect.center();
        let stroke = ui.style().interact(&choice.response).fg_stroke;
        match icon {
            FilterIcon::Workflow(kind) => {
                crate::panels::workflow_type_icon(ui, icon_id, rect, kind)
            }
            FilterIcon::Class(color) => {
                ui.painter().circle_filled(center, 6.0, color);
            }
            FilterIcon::All => {
                for y in [-4.0, 4.0] {
                    for x in [-4.0, 4.0] {
                        ui.painter()
                            .circle_filled(center + egui::vec2(x, y), 2.0, stroke.color);
                    }
                }
            }
            FilterIcon::Status(status) => {
                let painter = ui.painter();
                match status {
                    TaskStatus::Completed => {
                        painter.line_segment(
                            [
                                center + egui::vec2(-6.0, 0.0),
                                center + egui::vec2(-2.0, 4.0),
                            ],
                            egui::Stroke::new(2.0, theme::SUCCESS),
                        );
                        painter.line_segment(
                            [
                                center + egui::vec2(-2.0, 4.0),
                                center + egui::vec2(7.0, -5.0),
                            ],
                            egui::Stroke::new(2.0, theme::SUCCESS),
                        );
                    }
                    TaskStatus::NeedsCorrection => {
                        painter.line_segment(
                            [
                                center + egui::vec2(0.0, -6.0),
                                center + egui::vec2(0.0, 2.0),
                            ],
                            egui::Stroke::new(2.0, theme::DANGER),
                        );
                        painter.circle_filled(center + egui::vec2(0.0, 6.0), 1.5, theme::DANGER);
                    }
                    TaskStatus::Submitted => {
                        painter.line_segment(
                            [
                                center + egui::vec2(-7.0, 0.0),
                                center + egui::vec2(7.0, 0.0),
                            ],
                            stroke,
                        );
                        painter.line_segment(
                            [
                                center + egui::vec2(2.0, -5.0),
                                center + egui::vec2(7.0, 0.0),
                            ],
                            stroke,
                        );
                        painter.line_segment(
                            [center + egui::vec2(2.0, 5.0), center + egui::vec2(7.0, 0.0)],
                            stroke,
                        );
                    }
                    _ => {
                        painter.circle_stroke(center, 7.0, stroke);
                        if *status == TaskStatus::InProgress {
                            painter.line_segment([center, center + egui::vec2(0.0, -5.0)], stroke);
                            painter.line_segment([center, center + egui::vec2(4.0, 0.0)], stroke);
                        }
                    }
                }
            }
        }
    }
    choice.response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    choice.response
}

pub(super) fn filter_menu_width<'a>(ui: &mut egui::Ui, labels: impl Iterator<Item = &'a str>) {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text_width = ui.fonts_mut(|fonts| {
        labels
            .map(|label| {
                fonts
                    .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
            .fold(0.0, f32::max)
    });
    // Let choices grow wider than their trigger, retaining bounded truncation
    // only when the full label cannot fit across the viewport.
    ui.set_min_width((text_width + 60.0).min(ui.ctx().content_rect().width() - 32.0));
}

pub(super) fn filter_menu(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    selected: String,
    choices: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    ui.push_id(id, |ui| {
        let response = ui.add(
            egui::Button::new(&selected)
                .right_text("▼")
                .min_size(egui::vec2(width, 44.0))
                .truncate(),
        );
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::new(egui::WidgetType::ComboBox);
            info.enabled = ui.is_enabled();
            info.current_text_value = Some(selected.clone());
            info
        });
        let popup = egui::Popup::menu(&response)
            .width(response.rect.width())
            .style(compact_menu_style);
        let was_open = popup.is_open();
        popup.show(|ui| {
            let margin = egui::Frame::popup(ui.style()).total_margin().sum().y;
            let height = (ui.ctx().content_rect().height() - margin).max(1.0);
            // ScrollArea also clamps to its parent's available height. Reset the
            // popup's cached/default area height before creating the scroll area.
            ui.set_max_height(height);
            egui::ScrollArea::vertical()
                .max_height(height)
                .show(ui, choices);
        });
        if was_open
            && !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response))
        {
            response.request_focus();
        }
        response
    })
    .inner
}
