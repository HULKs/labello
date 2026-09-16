mod panels;

use crate::{
    app::{LabelloApp, LayoutMode, RequestIdentity, UiCommand, UiMessage, UiRequestError},
    canvas::{CanvasInteraction, CanvasState},
    theme,
};
use eframe::egui;
use labello_client::{ImageExplorerQuery, ImagePreview, LabelloApi};
use labello_domain::{
    AnnotationType, AnnotationVersion, DatasetRole, EventId, ImageExplorerPage, ImageId,
    ImageRecord, ImageState, ReturnToReviewRequest, TaskId, TaskStatus,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

const PAGE_SIZE: usize = 24;
const MAX_THUMBNAILS: usize = 24;
const MAX_IMAGE_REQUESTS: usize = 2;

#[derive(Clone, Debug)]
pub(crate) enum InspectorAction {
    List(ImageExplorerQuery),
    Thumbnail(ImageId),
    Image(ImageId),
    State(ImageId),
    Return(ImageId, ReturnToReviewRequest),
}
#[derive(Debug)]
pub(crate) enum InspectorReply {
    List(ImageExplorerPage),
    Thumbnail(ImageId, ImagePreview),
    Image(ImageId, ImagePreview),
    State(ImageId, Box<ImageState>),
    Returned(Box<ImageState>),
}

pub(crate) struct InspectorState {
    transfers: crate::image_transfer::ImageTransfers,
    query: ImageExplorerQuery,
    page: Option<ImageExplorerPage>,
    pending: BTreeMap<u64, InspectorAction>,
    thumbnails: BTreeMap<ImageId, egui::TextureHandle>,
    thumbnail_errors: BTreeSet<ImageId>,
    selected: Option<ImageRecord>,
    state: Option<ImageState>,
    texture: Option<egui::TextureHandle>,
    canvas: CanvasState,
    hidden_tasks: BTreeSet<TaskId>,
    hidden_annotations: BTreeSet<labello_domain::AnnotationId>,
    hidden_statuses: Vec<TaskStatus>,
    boxes: bool,
    skeletons: bool,
    return_tasks: BTreeSet<TaskId>,
    reason: String,
    retry: Option<ReturnToReviewRequest>,
    error: Option<String>,
    image_error: Option<String>,
    notice: Option<String>,
    scroll: f32,
    images_collapsed: bool,
    overlays_collapsed: bool,
    drawer: Option<bool>,
    drawer_invoker: Option<egui::Id>,
    preview_loaded: bool,
    navigate_page: Option<bool>,
}
impl Drop for InspectorState {
    fn drop(&mut self) {
        self.transfers.cancel_all();
    }
}
impl Default for InspectorState {
    fn default() -> Self {
        Self {
            transfers: Default::default(),
            query: ImageExplorerQuery {
                page: 1,
                page_size: PAGE_SIZE,
                ..Default::default()
            },
            page: None,
            pending: BTreeMap::new(),
            thumbnails: BTreeMap::new(),
            thumbnail_errors: BTreeSet::new(),
            selected: None,
            state: None,
            texture: None,
            canvas: CanvasState::default(),
            hidden_tasks: BTreeSet::new(),
            hidden_annotations: BTreeSet::new(),
            hidden_statuses: Vec::new(),
            boxes: true,
            skeletons: true,
            return_tasks: BTreeSet::new(),
            reason: String::new(),
            retry: None,
            error: None,
            image_error: None,
            notice: None,
            scroll: 0.0,
            images_collapsed: false,
            overlays_collapsed: false,
            drawer: None,
            drawer_invoker: None,
            preview_loaded: false,
            navigate_page: None,
        }
    }
}
impl InspectorState {
    pub(crate) fn suspend_requests(&mut self) {
        self.transfers.cancel_all();
        self.pending.clear();
    }
    fn busy(&self) -> bool {
        self.pending
            .values()
            .any(|a| matches!(a, InspectorAction::Return(..)))
    }
    fn overlays(&self) -> Vec<AnnotationVersion> {
        self.state
            .as_ref()
            .map(|state| {
                state
                    .active_annotations()
                    .filter(|annotation| {
                        !self.hidden_annotations.contains(&annotation.annotation_id)
                            && !self.hidden_tasks.contains(&annotation.task_id)
                            && !self.hidden_statuses.contains(
                                &state
                                    .task_states
                                    .get(&annotation.task_id)
                                    .map(|s| s.status.clone())
                                    .unwrap_or(TaskStatus::Pending),
                            )
                            && match annotation.annotation_type {
                                AnnotationType::BoundingBox => self.boxes,
                                AnnotationType::Skeleton => self.skeletons,
                            }
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl LabelloApp {
    pub(crate) fn inspection_request_pending(&self, id: u64) -> bool {
        self.inspection.pending.contains_key(&id)
    }
    fn inspect_request(&mut self, action: InspectorAction) {
        let request = self.request_identity(Some(self.config.dataset_id.clone()));
        self.inspection
            .pending
            .insert(request.request_id, action.clone());
        self.queue_command(UiCommand::Inspect { request, action });
    }
    pub(crate) fn dispatch_inspection(
        &self,
        api: Rc<dyn LabelloApi>,
        request: RequestIdentity,
        action: InspectorAction,
    ) {
        let transfer = matches!(
            action,
            InspectorAction::Thumbnail(_) | InspectorAction::Image(_)
        )
        .then(|| self.inspection.transfers.transfer(request.request_id));
        self.spawn_message(request.clone(), async move {
            let dataset = request
                .dataset_id
                .as_ref()
                .expect("dataset inspection identity");
            let future = async {
                Ok::<_, labello_client::ClientError>(match action {
                    InspectorAction::List(query) => {
                        InspectorReply::List(api.list_images(dataset, query).await?)
                    }
                    InspectorAction::Thumbnail(id) => {
                        let preview = api
                            .get_encoded_image_preview(
                                dataset,
                                &id,
                                labello_client::ImagePreviewProfile::ThumbnailV1,
                            )
                            .await?
                            .decode()?;
                        InspectorReply::Thumbnail(id, preview)
                    }
                    InspectorAction::Image(id) => {
                        let preview =
                            crate::live_workflow::load_working_preview(api.as_ref(), dataset, &id)
                                .await?;
                        InspectorReply::Image(id, preview)
                    }
                    InspectorAction::State(id) => InspectorReply::State(
                        id.clone(),
                        Box::new(api.get_image_state(dataset, &id).await?),
                    ),
                    InspectorAction::Return(id, body) => InspectorReply::Returned(Box::new(
                        api.return_to_review(dataset, &id, body).await?,
                    )),
                })
            };
            let result = match transfer {
                Some(transfer) => transfer.run(future).await,
                None => future.await,
            }
            .map_err(UiRequestError::from);
            UiMessage::Inspected { request, result }
        });
    }
    pub(crate) fn fail_inspection(&mut self, request: u64, error: String) {
        if let Some(action) = self.inspection.pending.remove(&request) {
            if matches!(&action, InspectorAction::Image(id) | InspectorAction::State(id) if self.inspection.selected.as_ref().is_none_or(|r| r.image_id != *id))
            {
                return;
            }
            if let InspectorAction::Thumbnail(id) = action {
                self.inspection.thumbnail_errors.insert(id);
            } else if matches!(
                action,
                InspectorAction::Image(_) | InspectorAction::State(_)
            ) {
                self.inspection.image_error = Some(error);
            } else {
                if matches!(action, InspectorAction::List(_)) {
                    self.inspection.navigate_page = None;
                }
                self.inspection.error = Some(error);
            }
        }
    }
    pub(crate) fn accept_inspection(
        &mut self,
        ctx: &egui::Context,
        request: RequestIdentity,
        result: Result<InspectorReply, UiRequestError>,
    ) {
        if let Err(error) = result {
            self.fail_inspection(request.request_id, error.to_string());
            return;
        }
        if self
            .inspection
            .pending
            .remove(&request.request_id)
            .is_none()
        {
            return;
        }
        let texture = |preview: ImagePreview| {
            ctx.load_texture(
                "dataset-inspection",
                egui::ColorImage::from_rgba_unmultiplied(
                    [preview.width as usize, preview.height as usize],
                    &preview.rgba,
                ),
                egui::TextureOptions::LINEAR,
            )
        };
        match result.expect("handled failure") {
            InspectorReply::List(page) => {
                self.inspection
                    .thumbnails
                    .retain(|id, _| page.items.iter().any(|item| item.image.image_id == *id));
                self.inspection.thumbnail_errors.clear();
                let destination = self
                    .inspection
                    .navigate_page
                    .take()
                    .and_then(|previous| {
                        if previous {
                            page.items.last()
                        } else {
                            page.items.first()
                        }
                    })
                    .map(|i| i.image.clone());
                self.inspection.page = Some(page);
                if let Some(record) = destination {
                    self.select_inspection_image(record);
                }
                self.inspection.error = None;
            }
            InspectorReply::Thumbnail(id, preview) => {
                if self
                    .inspection
                    .page
                    .as_ref()
                    .is_some_and(|page| page.items.iter().any(|item| item.image.image_id == id))
                    && self.inspection.thumbnails.len() < MAX_THUMBNAILS
                {
                    let texture = texture(preview);
                    if !self.inspection.preview_loaded
                        && self
                            .inspection
                            .selected
                            .as_ref()
                            .is_some_and(|r| r.image_id == id)
                    {
                        self.inspection.texture = Some(texture.clone());
                    }
                    self.inspection.thumbnails.insert(id, texture);
                }
            }
            InspectorReply::State(id, state) => {
                if self
                    .inspection
                    .selected
                    .as_ref()
                    .is_some_and(|r| r.image_id == id)
                {
                    self.inspection.state = Some(*state);
                }
            }
            InspectorReply::Image(id, preview) => {
                if self
                    .inspection
                    .selected
                    .as_ref()
                    .is_some_and(|record| record.image_id == id)
                {
                    self.inspection.preview_loaded = true;
                    self.inspection.texture = Some(texture(preview));
                }
            }
            InspectorReply::Returned(state) => {
                self.inspection.state = Some(*state);
                self.inspection.reason.clear();
                self.inspection.return_tasks.clear();
                self.inspection.retry = None;
                self.inspection.error = None;
                self.inspection.notice = Some("Selected workflows returned to review.".into());
                self.inspection.page = None;
                self.invalidate_assignment_availability(None);
            }
        }
    }
    pub(crate) fn inspection_has_reason(&self) -> bool {
        !self.inspection.reason.is_empty() || self.inspection.busy()
    }
    fn inspection_feedback(&self, ui: &mut egui::Ui) {
        for error in [
            self.inspection.error.clone(),
            self.inspection.image_error.clone(),
        ]
        .into_iter()
        .flatten()
        {
            theme::inline_message(
                ui,
                theme::Intent::Error,
                format!("Could not refresh or save. Displayed data may be stale. {error}"),
            );
        }
        if let Some(notice) = &self.inspection.notice {
            ui.label(notice);
        }
    }
    pub(crate) fn dataset_inspector(&mut self, ui: &mut egui::Ui) {
        if self.inspection.drawer.is_none() {
            self.inspection_feedback(ui);
        }
        if let Some(record) = self.inspection.selected.clone() {
            if !self.inspection.preview_loaded {
                ui.label("Loading image…");
            }
            self.inspection_canvas(ui, &record);
        } else {
            ui.heading("Dataset inspector");
            ui.label("Choose an image from Images to inspect its annotations.");
        }
    }
    fn select_inspection_image(&mut self, record: ImageRecord) {
        if self.inspection.busy() || !self.inspection.reason.is_empty() {
            return;
        }
        self.inspection.texture = self.inspection.thumbnails.get(&record.image_id).cloned();
        self.inspection.preview_loaded = false;
        self.inspection.image_error = None;
        self.inspection.state = None;
        self.inspection.hidden_annotations.clear();
        self.inspection.canvas = CanvasState::default();
        self.inspection.canvas.require_pan_mode(true);
        self.inspection.error = None;
        self.inspection.notice = None;
        self.inspection.return_tasks.clear();
        self.inspection.retry = None;
        self.inspect_request(InspectorAction::State(record.image_id.clone()));
        self.inspection.selected = Some(record);
        self.inspection.drawer = None;
    }
    fn schedule_inspection_image(&mut self) {
        let Some(record) = self.inspection.selected.clone() else {
            return;
        };
        if !self.inspection.preview_loaded
            && self.inspection.texture.is_none()
            && !self.inspection.thumbnail_errors.contains(&record.image_id)
            && !self
                .inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::Thumbnail(id) if *id == record.image_id))
            && self
                .inspection
                .pending
                .values()
                .filter(|a| matches!(a, InspectorAction::Thumbnail(_) | InspectorAction::Image(_)))
                .count()
                < MAX_IMAGE_REQUESTS
        {
            self.inspect_request(InspectorAction::Thumbnail(record.image_id.clone()));
        }
        if !self.inspection.preview_loaded
            && self.inspection.image_error.is_none()
            && !self
                .inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::Image(_)))
            && self
                .inspection
                .pending
                .values()
                .filter(|a| matches!(a, InspectorAction::Thumbnail(_)))
                .count()
                < MAX_IMAGE_REQUESTS
        {
            self.inspect_request(InspectorAction::Image(record.image_id.clone()));
        }
    }
    fn inspection_gallery(&mut self, ui: &mut egui::Ui) {
        if self.inspection.drawer == Some(false) {
            self.inspection_feedback(ui);
        }
        let busy = self.inspection.busy()
            || self
                .inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::List(_)));
        let mut refresh = false;
        let page = self.inspection.page.clone();
        egui::ScrollArea::vertical()
            .id_salt("inspection-controls")
            .max_height((ui.available_height() - 150.0).clamp(80.0, 320.0))
            .show(ui, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    let label = ui.label("Search filename or path");
                    let text = self.inspection.query.search.get_or_insert_default();
                    ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width()))
                        .labelled_by(label.id);
                    egui::CollapsingHeader::new("Filters").show(ui, |ui| {
                        let narrow = ui.available_width() < 1100.0;
                        let filters = |ui: &mut egui::Ui| {
                            egui::ComboBox::from_label("Workflow filter")
                                .width(140.0)
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
                                                .find(|task| task.task_id == *id)
                                                .map(|task| task.name.clone())
                                                .unwrap_or_else(|| id.to_string())
                                        })
                                        .unwrap_or("All workflows".into()),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.inspection.query.task_id,
                                        None,
                                        "All workflows",
                                    );
                                    for task in &self.work.tasks {
                                        ui.selectable_value(
                                            &mut self.inspection.query.task_id,
                                            Some(task.task_id.clone()),
                                            &task.name,
                                        );
                                    }
                                });
                            egui::ComboBox::from_label("Class filter")
                                .width(140.0)
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
                                                .find(|class| class.class_id == *id)
                                                .map(|class| class.name.clone())
                                                .unwrap_or_else(|| id.to_string())
                                        })
                                        .unwrap_or("All classes".into()),
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
                                });
                            egui::ComboBox::from_label("Status filter")
                                .width(140.0)
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .selected_text(
                                    self.inspection
                                        .query
                                        .status
                                        .as_ref()
                                        .map(|s| status_label(s).to_owned())
                                        .unwrap_or("All statuses".into()),
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
                                });
                            if ui.button("Apply filters").clicked() {
                                self.inspection.query.page = 1;
                                self.inspection.scroll = 0.0;
                                refresh = true;
                            }
                            if ui.button("Refresh gallery").clicked() {
                                refresh = true;
                            }
                        };
                        if narrow {
                            ui.vertical(filters);
                        } else {
                            ui.horizontal_wrapped(filters);
                        }
                    });
                    if ui.button("Search images").clicked() {
                        self.inspection.query.page = 1;
                        self.inspection.scroll = 0.0;
                        refresh = true;
                    }
                });
                if self.inspection.page.is_none() && !busy && self.inspection.error.is_none() {
                    refresh = true;
                }
                if refresh {
                    self.inspect_request(InspectorAction::List(self.inspection.query.clone()));
                }
                if busy {
                    ui.label("Loading gallery…");
                }
                if let Some(page) = &page {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} images · Page {} of {}",
                            page.total_items,
                            page.page,
                            page.total_pages.max(1)
                        ));
                        if ui
                            .add_enabled(!busy && page.page > 1, egui::Button::new("Previous page"))
                            .clicked()
                        {
                            self.inspection.query.page = page.page - 1;
                            self.inspection.scroll = 0.0;
                            self.inspect_request(InspectorAction::List(
                                self.inspection.query.clone(),
                            ));
                        }
                        if ui
                            .add_enabled(
                                !busy && page.page < page.total_pages,
                                egui::Button::new("Next page"),
                            )
                            .clicked()
                        {
                            self.inspection.query.page = page.page + 1;
                            self.inspection.scroll = 0.0;
                            self.inspect_request(InspectorAction::List(
                                self.inspection.query.clone(),
                            ));
                        }
                    });
                }
            });
        let Some(page) = page else {
            return;
        };
        if page.items.is_empty() {
            ui.label("No images match these filters.");
        }
        let columns = ((ui.available_width() / 200.0).floor() as usize).max(1);
        let width = ((ui.available_width() - ui.spacing().item_spacing.x * (columns - 1) as f32)
            / columns as f32)
            .max(1.0);
        let mut open = None;
        let mut visible = Vec::new();
        let output = egui::ScrollArea::vertical()
            .id_salt("inspection-gallery")
            .vertical_scroll_offset(self.inspection.scroll)
            .show_rows(ui, 190.0, page.items.len().div_ceil(columns), |ui, rows| {
                for row in rows {
                    ui.horizontal(|ui| {
                        for item in page.items.iter().skip(row * columns).take(columns) {
                            ui.push_id(&item.image.image_id, |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(width, 190.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_min_size(egui::vec2(width, 190.0));
                                        let id = &item.image.image_id;
                                        visible.push(id.clone());
                                        let button = if let Some(texture) =
                                            self.inspection.thumbnails.get(id)
                                        {
                                            egui::Button::image(
                                                egui::Image::new(texture)
                                                    .fit_to_exact_size(egui::vec2(
                                                        width.min(160.0),
                                                        120.0,
                                                    ))
                                                    .alt_text(format!(
                                                        "Inspect {}",
                                                        item.image.file_name
                                                    )),
                                            )
                                        } else {
                                            egui::Button::new(
                                                if self.inspection.thumbnail_errors.contains(id) {
                                                    "Preview unavailable"
                                                } else {
                                                    "Loading preview…"
                                                },
                                            )
                                        };
                                        let response = ui.add_enabled(
                                            !busy && self.inspection.reason.is_empty(),
                                            button
                                                .selected(
                                                    self.inspection
                                                        .selected
                                                        .as_ref()
                                                        .is_some_and(|r| r.image_id == *id),
                                                )
                                                .min_size(egui::vec2(width.min(160.0), 128.0)),
                                        );
                                        response.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::WidgetType::Button,
                                                !busy,
                                                format!("Inspect {}", item.image.file_name),
                                            )
                                        });
                                        if response.clicked() {
                                            open = Some(item.image.clone());
                                        }
                                        ui.add(egui::Label::new(&item.image.file_name).truncate())
                                            .on_hover_text(&item.image.canonical_path);
                                        if self.inspection.thumbnail_errors.contains(id)
                                            && ui.button("Retry preview").clicked()
                                        {
                                            self.inspection.thumbnail_errors.remove(id);
                                        }
                                    },
                                );
                            });
                        }
                    });
                }
            });
        self.inspection.scroll = output.state.offset.y;
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
    fn inspection_canvas(&mut self, ui: &mut egui::Ui, record: &ImageRecord) {
        let annotations = self.inspection.overlays();
        let edges = self
            .work
            .tasks
            .iter()
            .filter_map(|task| {
                task.skeleton.as_ref().map(|skeleton| {
                    (
                        task.task_id.clone(),
                        skeleton
                            .edges
                            .iter()
                            .map(|edge| (edge.from.clone(), edge.to.clone()))
                            .collect(),
                    )
                })
            })
            .collect();
        let styles = annotations
            .iter()
            .filter(|a| a.annotation_type == AnnotationType::Skeleton)
            .map(|annotation| {
                let color = self
                    .work
                    .classes
                    .iter()
                    .find(|class| class.class_id == annotation.class_id)
                    .and_then(|class| crate::workspace_canvas::parse_class_color(&class.color))
                    .unwrap_or(theme::ANNOTATION);
                (
                    annotation.annotation_id.clone(),
                    crate::canvas::CanvasAnnotationStyle::solid(color),
                )
            })
            .collect();
        let mut interaction = CanvasInteraction::annotations(false);
        interaction.allow_selection = false;
        crate::canvas::show_canvas_with_task_edges(
            ui,
            &mut self.inspection.canvas,
            self.inspection.texture.as_ref(),
            &annotations,
            [record.width, record.height],
            false,
            None,
            interaction,
            &[],
            &[],
            theme::ANNOTATION,
            &styles,
            None,
            None,
            &mut None,
            &edges,
        );
    }
    fn inspection_controls(&mut self, ui: &mut egui::Ui, record: &ImageRecord, busy: bool) {
        ui.heading("Annotation overlays");
        let total = self
            .inspection
            .state
            .as_ref()
            .map(|s| s.active_annotations().count())
            .unwrap_or(0);
        ui.label(format!(
            "{} of {total} annotations visible",
            self.inspection.overlays().len()
        ));
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.inspection.boxes, "Bounding boxes");
            ui.checkbox(&mut self.inspection.skeletons, "Skeletons");
        });
        egui::CollapsingHeader::new("Workflow statuses").show(ui, |ui| {
            for status in statuses() {
                let mut visible = !self.inspection.hidden_statuses.contains(&status);
                if ui.checkbox(&mut visible, status_label(&status)).changed() {
                    if visible {
                        self.inspection.hidden_statuses.retain(|old| old != &status);
                    } else {
                        self.inspection.hidden_statuses.push(status);
                    }
                }
            }
        });
        ui.separator();
        if self.inspection.state.is_none() {
            ui.label("Loading annotations…");
        }
        for task in &self.work.tasks {
            ui.push_id(&task.task_id, |ui| {
                let annotations: Vec<_> = self
                    .inspection
                    .state
                    .as_ref()
                    .map(|s| {
                        s.active_annotations()
                            .filter(|a| a.task_id == task.task_id)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                let mut visible = !self.inspection.hidden_tasks.contains(&task.task_id);
                if ui.checkbox(&mut visible, &task.name).changed() {
                    if visible {
                        self.inspection.hidden_tasks.remove(&task.task_id);
                    } else {
                        self.inspection.hidden_tasks.insert(task.task_id.clone());
                    }
                }
                if let Some(state) = &self.inspection.state {
                    let status = state
                        .task_states
                        .get(&task.task_id)
                        .map(|s| s.status.clone())
                        .unwrap_or(TaskStatus::Pending);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · {} annotations",
                            status_label(&status),
                            annotations.len()
                        ))
                        .small()
                        .weak(),
                    );
                }
                if !annotations.is_empty() {
                    egui::CollapsingHeader::new("Annotations")
                        .id_salt("objects")
                        .show(ui, |ui| {
                            ui.add_enabled_ui(visible, |ui| {
                                for (index, annotation) in annotations.iter().enumerate() {
                                    let class = self
                                        .work
                                        .classes
                                        .iter()
                                        .find(|c| c.class_id == annotation.class_id)
                                        .map(|c| c.name.as_str())
                                        .unwrap_or(annotation.class_id.as_str());
                                    let mut shown = !self
                                        .inspection
                                        .hidden_annotations
                                        .contains(&annotation.annotation_id);
                                    ui.push_id(&annotation.annotation_id, |ui| {
                                        if ui
                                            .checkbox(
                                                &mut shown,
                                                format!("{} · {class}", index + 1),
                                            )
                                            .changed()
                                        {
                                            if shown {
                                                self.inspection
                                                    .hidden_annotations
                                                    .remove(&annotation.annotation_id);
                                            } else {
                                                self.inspection
                                                    .hidden_annotations
                                                    .insert(annotation.annotation_id.clone());
                                            }
                                        }
                                    });
                                }
                            });
                        });
                }
                ui.add_space(theme::SPACE_2);
            });
        }
        if self.has_dataset_role(DatasetRole::Reviewer)
            || self.has_dataset_role(DatasetRole::DataAdmin)
        {
            self.inspection_return_controls(ui, record, busy);
        }
    }
    fn inspection_return_controls(&mut self, ui: &mut egui::Ui, record: &ImageRecord, busy: bool) {
        ui.separator();
        ui.heading("Return workflows to review");
        ui.label(
            "Select completed workflows below. Overlay visibility does not select work to return.",
        );
        let mut changed = false;
        ui.add_enabled_ui(!busy, |ui| {
            for task in &self.work.tasks {
                let eligible = task.enabled
                    && task.review.workflow == labello_domain::ReviewWorkflow::Approval
                    && self
                        .inspection
                        .state
                        .as_ref()
                        .and_then(|s| s.task_states.get(&task.task_id))
                        .is_some_and(|s| s.status == TaskStatus::Completed);
                let mut selected = self.inspection.return_tasks.contains(&task.task_id);
                if ui
                    .add_enabled(
                        eligible,
                        egui::Checkbox::new(
                            &mut selected,
                            format!("Return {} to review", task.name),
                        ),
                    )
                    .changed()
                {
                    changed = true;
                    if selected {
                        self.inspection.return_tasks.insert(task.task_id.clone());
                    } else {
                        self.inspection.return_tasks.remove(&task.task_id);
                    }
                }
            }
            let label = ui.label("Reason for returning work");
            changed |= ui
                .add(
                    egui::TextEdit::multiline(&mut self.inspection.reason)
                        .desired_width(ui.available_width())
                        .desired_rows(2),
                )
                .labelled_by(label.id)
                .changed();
            if changed {
                self.inspection.retry = None;
            }
            if self.inspection.reason.len() > 2000 {
                ui.label("Reason must be at most 2000 bytes.");
            }
            ui.horizontal_wrapped(|ui| {
                let valid = !self.inspection.return_tasks.is_empty()
                    && !self.inspection.reason.trim().is_empty()
                    && self.inspection.reason.len() <= 2000
                    && self.inspection.state.is_some();
                if theme::primary_button(ui, valid, egui::Button::new("Return selected workflows"))
                    .clicked()
                {
                    let request = self
                        .inspection
                        .retry
                        .get_or_insert_with(|| ReturnToReviewRequest {
                            request_id: EventId::generate(),
                            expected_sequence: self
                                .inspection
                                .state
                                .as_ref()
                                .unwrap()
                                .current_sequence,
                            task_ids: self.inspection.return_tasks.iter().cloned().collect(),
                            reason: self.inspection.reason.clone(),
                        })
                        .clone();
                    self.inspect_request(InspectorAction::Return(record.image_id.clone(), request));
                }
                if ui.button("Discard return draft").clicked() {
                    self.inspection.reason.clear();
                    self.inspection.return_tasks.clear();
                    self.inspection.retry = None;
                }
            });
        });
    }
}
fn statuses() -> [TaskStatus; 5] {
    [
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Submitted,
        TaskStatus::NeedsCorrection,
        TaskStatus::Completed,
    ]
}

#[cfg(any(test, feature = "inspector-presets"))]
impl LabelloApp {
    pub(crate) fn prepare_inspection_preset(&mut self, detail: bool) {
        let record = self
            .work
            .current
            .as_ref()
            .expect("work preset image")
            .image
            .clone();
        let mut state = ImageState::new(record.image_id.clone());
        for annotation in &self.work.annotations {
            state
                .annotations
                .insert(annotation.annotation_id.clone(), vec![annotation.clone()]);
        }
        for task in &self.work.tasks {
            state.task_states.insert(
                task.task_id.clone(),
                labello_domain::TaskState {
                    task_id: task.task_id.clone(),
                    status: TaskStatus::Completed,
                    outcome: None,
                    assigned_to: None,
                    completed_by: None,
                    completed_at: None,
                    updated_at: labello_domain::now(),
                },
            );
        }
        self.inspection = InspectorState::default();
        self.inspection.page = Some(ImageExplorerPage {
            items: (0..24)
                .map(|index| {
                    let mut image = record.clone();
                    if index > 0 {
                        image.image_id = format!("inspection_{index}").into();
                    }
                    image.file_name = format!("Sample {index}");
                    if let Some(texture) = self.work.current_texture.clone() {
                        self.inspection
                            .thumbnails
                            .insert(image.image_id.clone(), texture);
                    }
                    labello_domain::ImageExplorerItem {
                        image,
                        task_statuses: state
                            .task_states
                            .iter()
                            .map(|(id, s)| (id.clone(), s.status.clone()))
                            .collect(),
                        class_ids: Default::default(),
                    }
                })
                .collect(),
            page: 1,
            page_size: 24,
            total_items: 48,
            total_pages: 2,
        });
        if detail {
            self.inspection.selected = Some(record);
            self.inspection.state = Some(state);
            self.inspection.texture = self.work.current_texture.clone();
            self.inspection.preview_loaded = true;
        }
        self.view = crate::app::AppView::Inspect;
        self.work.assignment = None;
        self.runtime.api = None;
    }
}

fn status_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "Pending",
        TaskStatus::InProgress => "In progress",
        TaskStatus::Submitted => "Submitted",
        TaskStatus::NeedsCorrection => "Needs correction",
        TaskStatus::Completed => "Completed",
        _ => "Historical status",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppView;
    use egui_kittest::{Harness, kittest::Queryable};

    fn app() -> LabelloApp {
        crate::inspector_presets::build(
            crate::inspector_presets::InspectorPreset::DatasetInspection,
            &egui::Context::default(),
        )
    }

    #[test]
    fn inspector_overlay_dimensions_intersect_without_selecting_return_targets() {
        let mut app = app();
        let state = app.inspection.state.as_mut().unwrap();
        let mut skeleton = state.active_annotations().next().unwrap().clone();
        skeleton.annotation_id = "pose-object".into();
        skeleton.task_id = "pose-task".into();
        skeleton.annotation_type = AnnotationType::Skeleton;
        skeleton.geometry =
            labello_domain::AnnotationGeometry::Skeleton(labello_domain::SkeletonGeometry {
                keypoints: vec![],
            });
        let mut submitted = state.task_states.values().next().unwrap().clone();
        submitted.task_id = skeleton.task_id.clone();
        submitted.status = TaskStatus::Submitted;
        state
            .task_states
            .insert(submitted.task_id.clone(), submitted);
        state
            .annotations
            .insert(skeleton.annotation_id.clone(), vec![skeleton]);
        let baseline = app.inspection.overlays();
        assert_eq!(baseline.len(), 2);
        let task = baseline
            .iter()
            .find(|a| a.annotation_type == AnnotationType::BoundingBox)
            .unwrap()
            .task_id
            .clone();
        app.inspection.return_tasks.insert(task.clone());
        app.inspection.hidden_tasks.insert(task.clone());
        assert!(app.inspection.overlays().iter().all(|a| a.task_id != task));
        app.inspection.hidden_tasks.clear();
        app.inspection.hidden_statuses.push(TaskStatus::Completed);
        assert_eq!(app.inspection.overlays().len(), 1);
        app.inspection.hidden_tasks.insert("pose-task".into());
        assert!(app.inspection.overlays().is_empty());
        app.inspection.hidden_tasks.clear();
        app.inspection.hidden_statuses.clear();
        app.inspection.boxes = false;
        assert_eq!(app.inspection.overlays().len(), 1);
        assert!(
            app.inspection
                .overlays()
                .iter()
                .all(|a| a.annotation_type == AnnotationType::Skeleton)
        );
        app.inspection.skeletons = false;
        assert!(app.inspection.overlays().is_empty());
        assert_eq!(app.inspection.return_tasks, BTreeSet::from([task]));
        assert!(app.runtime.commands.is_empty());
    }

    #[test]
    fn inspector_failures_preserve_exact_return_draft_and_block_navigation() {
        let mut app = app();
        app.inspection.reason = "Keep my reason".into();
        let record = app.inspection.selected.clone().unwrap();
        let body = ReturnToReviewRequest {
            request_id: EventId::generate(),
            expected_sequence: 4,
            task_ids: vec![app.work.tasks[0].task_id.clone()],
            reason: app.inspection.reason.clone(),
        };
        app.inspection.return_tasks = body.task_ids.iter().cloned().collect();
        app.inspection.retry = Some(body.clone());
        app.inspection
            .pending
            .insert(42, InspectorAction::Return(record.image_id, body.clone()));
        app.fail_inspection(42, "conflict".into());
        assert_eq!(app.inspection.reason, body.reason);
        assert_eq!(app.inspection.retry, Some(body));
        assert!(app.inspection.notice.is_none());
        app.open_view(AppView::Setup);
        assert_eq!(app.view, AppView::Inspect);
        app.inspection.reason.clear();
        app.begin_auth_epoch();
        assert!(app.inspection.state.is_none());
        assert!(app.inspection.thumbnails.is_empty());
    }

    #[test]
    fn inspector_accessible_read_only_detail_and_gallery_restore() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1288.0, 820.0))
            .build_eframe(|_| app());
        harness.run();
        assert!(harness.query_by_label("Annotation overlays").is_some());
        assert!(harness.query_by_label("Bounding boxes").is_some());
        let before = harness.state().inspection.state.clone();
        harness.get_by_label("Bounding boxes").click();
        harness.run();
        assert_eq!(harness.state().inspection.state, before);
        harness.state_mut().inspection.query.search = Some("retained filter".into());
        harness.state_mut().inspection.scroll = 120.0;
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Images")
            .click();
        harness.run();
        assert!(harness.state().inspection.images_collapsed);
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Images")
            .click();
        harness.run();
        assert!(harness.state().inspection.selected.is_some());
        assert_eq!(
            harness.state().inspection.query.search.as_deref(),
            Some("retained filter")
        );
        assert!(harness.query_by_label("Search filename or path").is_some());
        assert!(harness.state().inspection.thumbnails.len() <= MAX_THUMBNAILS);
    }

    #[test]
    fn inspector_desktop_controls_and_gallery_tiles_fit_the_viewport() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app());
        harness.run();
        let overlays = harness.get_by_label("Annotation overlays").rect();
        let reason = harness.get_by_label("Reason for returning work").rect();
        let submit = harness.get_by_label("Return selected workflows").rect();
        assert!(reason.top() > overlays.bottom());
        assert!(submit.right() <= 1440.0 && submit.bottom() <= 1000.0);

        let preview = harness.get_by_label("Inspect Sample 0").rect();
        let filename = harness.get_by_label("Sample 0").rect();
        assert!(filename.top() >= preview.bottom());
        assert!(harness.get_by_label("Next page").rect().bottom() < preview.top());
        assert!(preview.right() < overlays.left());
        assert!(harness.get_by_label("Fit image").rect().bottom() < preview.top());
    }
    #[test]
    fn inspector_annotation_members_browse_without_return_controls() {
        let mut app = app();
        app.datasets.summaries[0].roles = vec![DatasetRole::Annotator];
        assert!(app.can_open_view(AppView::Inspect));
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1288.0, 820.0))
            .build_eframe(|_| app);
        harness.run();
        assert!(harness.query_by_label("Bounding boxes").is_some());
        assert!(
            harness
                .query_by_label("Return workflows to review")
                .is_none()
        );
        harness.state_mut().datasets.summaries.clear();
        assert!(!harness.state().can_open_view(AppView::Inspect));
    }

    #[test]
    fn inspector_keeps_thumbnail_while_state_and_large_preview_load_independently() {
        let mut app = app();
        let record = app.inspection.page.as_ref().unwrap().items[0].image.clone();
        let proxy = app
            .inspection
            .thumbnails
            .get(&record.image_id)
            .unwrap()
            .id();
        app.select_inspection_image(record.clone());
        assert_eq!(app.inspection.texture.as_ref().unwrap().id(), proxy);
        assert!(app.inspection.state.is_none());
        app.schedule_inspection_image();
        assert!(
            app.inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::Image(id) if *id == record.image_id))
        );
        let request = app.request_identity(Some(app.config.dataset_id.clone()));
        app.inspection.pending.insert(
            request.request_id,
            InspectorAction::State(record.image_id.clone()),
        );
        app.accept_inspection(
            &egui::Context::default(),
            request,
            Ok(InspectorReply::State(
                record.image_id.clone(),
                Box::new(ImageState::new(record.image_id)),
            )),
        );
        assert!(app.inspection.state.is_some());
        assert_eq!(app.inspection.texture.as_ref().unwrap().id(), proxy);
        assert!(!app.inspection.preview_loaded);
    }

    #[test]
    fn inspector_drawers_restore_focus_and_keep_controls_reachable() {
        for size in [egui::vec2(390.0, 844.0), egui::vec2(320.0, 320.0)] {
            let mut harness = Harness::builder().with_size(size).build_eframe(|_| app());
            harness.run();
            harness
                .get_by_role_and_label(egui::accesskit::Role::Button, "Overlays")
                .click();
            harness.run();
            assert!(harness.query_by_label("Bounding boxes").is_some());
            assert!(harness.get_by_label("Close Overlays").rect().bottom() <= size.y);
            harness.get_by_label("Close Overlays").click();
            harness.run();
            assert!(harness.state().inspection.drawer.is_none());
            assert!(
                harness
                    .get_by_role_and_label(egui::accesskit::Role::Button, "Overlays")
                    .is_focused()
            );
            harness
                .get_by_role_and_label(egui::accesskit::Role::Button, "Images")
                .click();
            harness.run();
            assert!(harness.get_by_label("Search images").rect().right() <= size.x);
            harness.key_press(egui::Key::Escape);
            harness.run();
            assert!(harness.state().inspection.drawer.is_none());
        }
    }

    #[test]
    fn inspector_late_preview_cannot_erase_state_failure_or_return_retry() {
        let mut app = app();
        let id = app.inspection.selected.as_ref().unwrap().image_id.clone();
        app.inspection
            .pending
            .insert(98, InspectorAction::State(id.clone()));
        app.fail_inspection(98, "state unavailable".into());
        let retry = ReturnToReviewRequest {
            request_id: EventId::generate(),
            expected_sequence: 1,
            task_ids: vec![app.work.tasks[0].task_id.clone()],
            reason: "Retained reason".into(),
        };
        app.inspection.retry = Some(retry.clone());
        let request = app.request_identity(Some(app.config.dataset_id.clone()));
        app.inspection
            .pending
            .insert(request.request_id, InspectorAction::Image(id.clone()));
        app.accept_inspection(
            &egui::Context::default(),
            request,
            Ok(InspectorReply::Image(
                id.clone(),
                ImagePreview {
                    image_id: id,
                    width: 1,
                    height: 1,
                    rgba: vec![0, 0, 0, 255],
                },
            )),
        );
        assert_eq!(
            app.inspection.image_error.as_deref(),
            Some("state unavailable")
        );
        assert_eq!(app.inspection.retry, Some(retry));
    }

    #[test]
    fn inspector_next_image_crosses_page_boundary_without_changing_filters() {
        let mut app = app();
        let last = app
            .inspection
            .page
            .as_ref()
            .unwrap()
            .items
            .last()
            .unwrap()
            .image
            .clone();
        app.inspection.selected = Some(last);
        app.inspection.query.search = Some("Sample".into());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run();
        harness.get_by_label("Next image").click();
        harness.run();
        assert_eq!(harness.state().inspection.query.page, 2);
        assert_eq!(
            harness.state().inspection.query.search.as_deref(),
            Some("Sample")
        );
        // The preset has no transport: a failed page load keeps the current image.
        assert!(harness.state().inspection.navigate_page.is_none());
        let app = harness.state_mut();
        app.inspection.navigate_page = Some(false);
        let mut page = app.inspection.page.clone().unwrap();
        page.page = 2;
        let expected = page.items[0].image.image_id.clone();
        let request = app.request_identity(Some(app.config.dataset_id.clone()));
        app.inspection.pending.insert(
            request.request_id,
            InspectorAction::List(app.inspection.query.clone()),
        );
        app.accept_inspection(
            &egui::Context::default(),
            request,
            Ok(InspectorReply::List(page)),
        );
        assert_eq!(app.inspection.selected.as_ref().unwrap().image_id, expected);
        assert!(app.inspection.navigate_page.is_none());
    }

    #[test]
    fn inspector_failure_feedback_is_inside_the_active_drawer() {
        let mut app = app();
        app.inspection.error = Some("Synthetic save failure".into());
        app.inspection.reason = "Retained input".into();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(390.0, 844.0))
            .build_eframe(|_| app);
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Overlays")
            .click();
        harness.run();
        let error = harness.get_by_label(
            "Could not refresh or save. Displayed data may be stale. Synthetic save failure",
        );
        assert!(error.rect().top() > harness.get_by_label("Close Overlays").rect().bottom());
        assert!(error.rect().right() <= 390.0);
        assert_eq!(harness.state().inspection.reason, "Retained input");
    }

    #[test]
    fn inspector_auth_recovery_retains_reason_and_cancels_obsolete_requests() {
        let mut app = app();
        app.inspection.reason = "Retained reason".into();
        app.auth.recovery = Some(crate::app::SessionRecovery {
            user_id: app.config.user_id.clone(),
            view: AppView::Inspect,
        });
        app.inspection.pending.insert(
            42,
            InspectorAction::Image(app.inspection.selected.as_ref().unwrap().image_id.clone()),
        );
        app.begin_auth_epoch();
        assert_eq!(app.inspection.reason, "Retained reason");
        assert!(app.inspection.pending.is_empty());
        assert!(app.inspection.selected.is_some());
    }
}
