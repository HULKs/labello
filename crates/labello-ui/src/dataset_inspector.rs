mod gallery;
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

const PAGE_SIZE: usize = 100;
const MAX_THUMBNAILS: usize = 48;
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
    active_query: ImageExplorerQuery,
    page: Option<ImageExplorerPage>,
    pending: BTreeMap<u64, InspectorAction>,
    thumbnails: BTreeMap<ImageId, egui::TextureHandle>,
    thumbnail_errors: BTreeSet<ImageId>,
    selected: Option<ImageRecord>,
    state: Option<ImageState>,
    texture: Option<egui::TextureHandle>,
    canvas: CanvasState,
    hidden_tasks: BTreeSet<TaskId>,
    hidden_statuses: Vec<TaskStatus>,
    boxes: bool,
    skeletons: bool,
    return_tasks: BTreeSet<TaskId>,
    reason: String,
    return_open: bool,
    gallery_error: Option<String>,
    failed_query: Option<ImageExplorerQuery>,
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
            active_query: ImageExplorerQuery {
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
            hidden_statuses: Vec::new(),
            boxes: true,
            skeletons: true,
            return_tasks: BTreeSet::new(),
            reason: String::new(),
            return_open: false,
            gallery_error: None,
            failed_query: None,
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
                        !self.hidden_tasks.contains(&annotation.task_id)
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
            InspectorAction::Thumbnail(_) | InspectorAction::Image(_) | InspectorAction::List(_)
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
                if let InspectorAction::List(query) = action {
                    self.inspection.navigate_page = None;
                    self.inspection.failed_query = Some(query);
                    self.inspection.gallery_error = Some(error);
                } else {
                    self.inspection.error = Some(error);
                }
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
        let Some(action) = self.inspection.pending.remove(&request.request_id) else {
            return;
        };
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
                let InspectorAction::List(query) = action else {
                    return;
                };
                let destination = self
                    .inspection
                    .navigate_page
                    .take()
                    .and_then(|_| page.items.first())
                    .map(|i| i.image.clone());
                if query.page == 1 {
                    self.inspection.active_query = query;
                    self.inspection.scroll = 0.0;
                    self.inspection.page = Some(page);
                    self.inspection.thumbnail_errors.clear();
                } else if let Some(loaded) = &mut self.inspection.page {
                    // Only append the next batch of the active search, never a stale/reset reply.
                    if page.page != loaded.page + 1 {
                        return;
                    }
                    loaded.page = page.page;
                    loaded.total_pages = page.total_pages;
                    loaded.total_items = page.total_items;
                    let known: BTreeSet<_> = loaded
                        .items
                        .iter()
                        .map(|i| i.image.image_id.clone())
                        .collect();
                    loaded.items.extend(
                        page.items
                            .into_iter()
                            .filter(|i| !known.contains(&i.image.image_id)),
                    );
                }
                if let Some(record) = destination {
                    self.select_inspection_image(record);
                }
                self.inspection.gallery_error = None;
                self.inspection.failed_query = None;
                // Fetch metadata independently of scrolling and image decoding.
                self.load_more_inspection_images();
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
                self.inspection.return_open = false;
                self.reset_inspection_gallery();
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
        self.cancel_obsolete_inspection_previews(
            &BTreeSet::from([record.image_id.clone()]),
            Some(&record.image_id),
        );
        self.inspection.texture = self.inspection.thumbnails.get(&record.image_id).cloned();
        self.inspection.preview_loaded = false;
        self.inspection.image_error = None;
        self.inspection.state = None;
        self.inspection.canvas = CanvasState::default();
        self.inspection.canvas.require_pan_mode(true);
        self.inspection.error = None;
        self.inspection.notice = None;
        self.inspection.return_tasks.clear();
        self.inspection.return_open = false;
        self.inspection.retry = None;
        self.inspect_request(InspectorAction::State(record.image_id.clone()));
        self.inspection.selected = Some(record);
        self.inspection.drawer = None;
    }
    fn cancel_obsolete_inspection_previews(
        &mut self,
        visible: &BTreeSet<ImageId>,
        selected: Option<&ImageId>,
    ) {
        let obsolete: Vec<_> = self
            .inspection
            .pending
            .iter()
            .filter_map(|(request, action)| {
                let obsolete = match action {
                    InspectorAction::Thumbnail(id) => !visible.contains(id) && selected != Some(id),
                    InspectorAction::Image(id) => selected != Some(id),
                    _ => false,
                };
                obsolete.then_some(*request)
            })
            .collect();
        for request in obsolete {
            self.inspection.transfers.cancel(request);
            self.inspection.pending.remove(&request);
            self.runtime.active_requests.remove(&request);
        }
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
            .map(|annotation| {
                let color = self
                    .work
                    .classes
                    .iter()
                    .find(|class| class.class_id == annotation.class_id)
                    .and_then(|class| crate::workspace_canvas::parse_class_color(&class.color))
                    .unwrap_or(theme::ANNOTATION);
                let mut style = crate::canvas::CanvasAnnotationStyle::solid(color);
                let labelled_box = annotation.annotation_type == AnnotationType::Skeleton
                    && annotation.object_group_id.as_ref().is_some_and(|group| {
                        annotations.iter().any(|other| {
                            other.annotation_type == AnnotationType::BoundingBox
                                && other.object_group_id.as_ref() == Some(group)
                        })
                    });
                if !labelled_box {
                    style.label = Some(
                        self.work
                            .classes
                            .iter()
                            .find(|c| c.class_id == annotation.class_id)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| annotation.class_id.to_string()),
                    );
                }
                (annotation.annotation_id.clone(), style)
            })
            .collect();
        let mut interaction = CanvasInteraction::annotations(false);
        interaction.allow_selection = false;
        let viewport = ui.available_rect_before_wrap();
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
        if !self.inspection.preview_loaded && self.inspection.image_error.is_none() {
            let rect = egui::Rect::from_center_size(viewport.center(), egui::vec2(32.0, 32.0));
            ui.painter()
                .circle_filled(rect.center(), 22.0, theme::PANEL);
            ui.put(rect, egui::Spinner::new().size(32.0))
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Loading image")
                });
        }
    }
    fn inspection_controls(&mut self, ui: &mut egui::Ui, record: &ImageRecord, busy: bool) {
        ui.horizontal(|ui| {
            panels::annotation_type_toggle(
                ui,
                &mut self.inspection.boxes,
                &AnnotationType::BoundingBox,
                "Bounding boxes",
            );
            panels::annotation_type_toggle(
                ui,
                &mut self.inspection.skeletons,
                &AnnotationType::Skeleton,
                "Skeletons",
            );
            egui::ComboBox::from_id_salt("overlay-statuses")
                .width(ui.available_width())
                .wrap_mode(egui::TextWrapMode::Truncate)
                .selected_text(if self.inspection.hidden_statuses.is_empty() {
                    "All statuses".to_owned()
                } else {
                    format!(
                        "{} statuses shown",
                        5 - self.inspection.hidden_statuses.len()
                    )
                })
                .show_ui(ui, |ui| {
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
                })
                .response
                .on_hover_text("Filter overlays by workflow status")
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Overlay statuses")
                });
        });
        ui.add_space(theme::SPACE_2);
        if let Some(state) = &self.inspection.state {
            for task in &self.work.tasks {
                let count = state
                    .active_annotations()
                    .filter(|a| a.task_id == task.task_id)
                    .count();
                let status = state
                    .task_states
                    .get(&task.task_id)
                    .map(|s| s.status.clone())
                    .unwrap_or(TaskStatus::Pending);
                ui.push_id(&task.task_id, |ui| {
                    let mut visible = !self.inspection.hidden_tasks.contains(&task.task_id);
                    ui.horizontal(|ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        let response = panels::annotation_type_toggle(
                            ui,
                            &mut visible,
                            &task.annotation_type,
                            &task.name,
                        );
                        let width = (ui.available_width() - 40.0).max(1.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 44.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(width);
                                ui.add(egui::Label::new(&task.name).truncate())
                                    .on_hover_text(format!(
                                        "{} · {} · {count} annotations",
                                        task.name,
                                        status_label(&status)
                                    ));
                            },
                        );
                        if response.changed() {
                            if visible {
                                self.inspection.hidden_tasks.remove(&task.task_id);
                            } else {
                                self.inspection.hidden_tasks.insert(task.task_id.clone());
                            }
                        }
                        ui.weak(count.to_string()).on_hover_text(format!(
                            "{count} annotations · {}",
                            status_label(&status)
                        ));
                    });
                });
            }
        } else {
            ui.spinner();
        }
        if self.has_dataset_role(DatasetRole::Reviewer)
            || self.has_dataset_role(DatasetRole::DataAdmin)
        {
            self.inspection_return_controls(ui, record, busy);
        }
    }
    fn inspection_return_controls(&mut self, ui: &mut egui::Ui, record: &ImageRecord, busy: bool) {
        ui.separator();
        if !self.inspection.return_open {
            if ui
                .add_enabled(!busy, egui::Button::new("Return to review"))
                .clicked()
            {
                self.inspection.return_open = true;
            }
            return;
        }
        ui.strong("Return to review");
        ui.weak("Select completed workflows and give a reason.");
        let mut changed = false;
        ui.add_enabled_ui(!busy, |ui| {
            for task in &self.work.tasks {
                let block =
                    self.inspection.state.as_ref().and_then(|state| {
                        state.return_to_review_block(task, labello_domain::now())
                    });
                let eligible = self.inspection.state.is_some() && block.is_none();
                if !eligible {
                    self.inspection.return_tasks.remove(&task.task_id);
                }
                let selected = self.inspection.return_tasks.contains(&task.task_id);
                let mut response = ui.add_enabled(
                    eligible,
                    egui::Button::new(&task.name)
                        .selected(selected)
                        .min_size(egui::vec2(ui.available_width(), 44.0))
                        .wrap(),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        eligible && !busy,
                        selected,
                        format!("Return {} to review", task.name),
                    )
                });
                if let Some(block) = block {
                    response = response.on_disabled_hover_text(block.message());
                    ui.weak(block.message());
                }
                let selected = if response.clicked() {
                    response.mark_changed();
                    !selected
                } else {
                    selected
                };
                if response.changed() {
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
                    self.inspection.return_open = false;
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
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn app() -> LabelloApp {
        crate::inspector_presets::build(
            crate::inspector_presets::InspectorPreset::DatasetInspection,
            &egui::Context::default(),
        )
    }

    #[test]
    fn inspector_metadata_continues_without_scroll_and_replaces_counts() {
        let mut app = app();
        let mut page = app.inspection.page.clone().unwrap();
        page.total_items = 25;
        page.total_pages = 2;
        let request = app.request_identity(Some(app.config.dataset_id.clone()));
        app.inspection.pending.insert(
            request.request_id,
            InspectorAction::List(app.inspection.query.clone()),
        );
        app.accept_inspection(
            &egui::Context::default(),
            request,
            Ok(InspectorReply::List(page.clone())),
        );
        let (id, query) = app
            .inspection
            .pending
            .iter()
            .find_map(|(id, action)| match action {
                InspectorAction::List(q) => Some((*id, q.clone())),
                _ => None,
            })
            .expect("next metadata batch is queued before any scrolling");
        assert_eq!(query.page, 2);
        page.page = 2;
        page.items.truncate(1);
        page.items[0].image.image_id = "last-image".into();
        let request = RequestIdentity {
            request_id: id,
            ..app.request_identity(Some(app.config.dataset_id.clone()))
        };
        app.accept_inspection(
            &egui::Context::default(),
            request,
            Ok(InspectorReply::List(page)),
        );
        assert_eq!(app.inspection.page.as_ref().unwrap().items.len(), 25);
        assert!(
            !app.inspection
                .pending
                .values()
                .any(|a| matches!(a, InspectorAction::List(_)))
        );
        let mut empty = app.inspection.page.clone().unwrap();
        empty.items.clear();
        empty.page = 1;
        empty.total_items = 0;
        empty.total_pages = 0;
        app.reset_inspection_gallery();
        let id = *app.inspection.pending.keys().next().unwrap();
        let request = RequestIdentity {
            request_id: id,
            ..app.request_identity(Some(app.config.dataset_id.clone()))
        };
        app.accept_inspection(
            &egui::Context::default(),
            request,
            Ok(InspectorReply::List(empty)),
        );
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run();
        assert!(harness.query_by_label("0 matching images").is_some());
        assert!(
            harness
                .query_by_label("No images match these filters.")
                .is_some()
        );
    }

    #[test]
    fn inspector_cancels_offscreen_previews_without_cancelling_current_image() {
        let mut app = app();
        let selected = app.inspection.selected.as_ref().unwrap().image_id.clone();
        app.inspection
            .pending
            .insert(80, InspectorAction::Thumbnail("offscreen".into()));
        app.inspection
            .pending
            .insert(81, InspectorAction::Thumbnail("visible".into()));
        app.inspection
            .pending
            .insert(82, InspectorAction::Image(selected.clone()));
        app.inspection
            .pending
            .insert(83, InspectorAction::Image("old-selection".into()));
        app.cancel_obsolete_inspection_previews(
            &BTreeSet::from(["visible".into()]),
            Some(&selected),
        );
        assert_eq!(
            app.inspection.pending.keys().copied().collect::<Vec<_>>(),
            vec![81, 82]
        );
        let late = RequestIdentity {
            request_id: 80,
            ..app.request_identity(Some(app.config.dataset_id.clone()))
        };
        app.accept_inspection(
            &egui::Context::default(),
            late,
            Ok(InspectorReply::Thumbnail(
                "offscreen".into(),
                ImagePreview {
                    image_id: "offscreen".into(),
                    width: 1,
                    height: 1,
                    rgba: vec![0, 0, 0, 255],
                },
            )),
        );
        assert!(
            !app.inspection
                .thumbnails
                .contains_key(&ImageId::from("offscreen"))
        );
    }

    #[test]
    fn inspector_class_labels_follow_visible_groups() {
        use labello_domain::{
            AnnotationGeometry, KeypointAnnotation, KeypointState, NormalizedPoint,
            SkeletonGeometry,
        };
        let mut app = app();
        let state = app.inspection.state.as_mut().unwrap();
        let mut bbox = state.active_annotations().next().unwrap().clone();
        bbox.object_group_id = Some("group".into());
        state
            .annotations
            .insert(bbox.annotation_id.clone(), vec![bbox.clone()]);
        let mut skeleton = bbox.clone();
        skeleton.annotation_id = "skeleton-label".into();
        skeleton.annotation_type = AnnotationType::Skeleton;
        skeleton.geometry = AnnotationGeometry::Skeleton(SkeletonGeometry {
            keypoints: vec![KeypointAnnotation {
                name: "center".into(),
                point: Some(NormalizedPoint { x: 0.5, y: 0.5 }),
                state: KeypointState::Visible,
            }],
        });
        state
            .annotations
            .insert(skeleton.annotation_id.clone(), vec![skeleton]);
        let label = format!(
            "Class: {}",
            app.work
                .classes
                .iter()
                .find(|c| c.class_id == bbox.class_id)
                .unwrap()
                .name
        );
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run();
        // A grouped box and skeleton share one visible label.
        assert!(harness.query_by_label(&label).is_some());
        harness.state_mut().inspection.boxes = false;
        harness.run();
        // Hiding the box gives its now-unboxed keypoint a class label.
        assert!(harness.query_by_label(&label).is_some());
        harness.state_mut().inspection.skeletons = false;
        harness.run();
        assert!(harness.query_by_label(&label).is_none());
    }

    #[test]
    fn inspector_keeps_labels_for_thin_boxes_at_the_viewport_edge() {
        let mut app = app();
        let state = app.inspection.state.as_mut().unwrap();
        let mut annotation = state.active_annotations().next().unwrap().clone();
        let labello_domain::AnnotationGeometry::BoundingBox(bbox) = &mut annotation.geometry else {
            panic!("expected box fixture");
        };
        bbox.x = 0.0;
        bbox.y = 0.4;
        bbox.width = 0.005;
        bbox.height = 0.2;
        let name = app
            .work
            .classes
            .iter()
            .find(|class| class.class_id == annotation.class_id)
            .unwrap()
            .name
            .clone();
        state
            .annotations
            .insert(annotation.annotation_id.clone(), vec![annotation]);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run();
        assert!(harness.query_by_label(&format!("Class: {name}")).is_some());
    }

    #[test]
    fn inspector_completed_boxes_have_full_width_selection_and_explain_exclusions() {
        let mut app = app();
        assert_eq!(
            app.work.tasks[0].annotation_type,
            AnnotationType::BoundingBox
        );
        app.inspection.return_open = true;
        let task = app.work.tasks[0].clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run();
        let button_label = format!("Return {} to review", task.name);
        let button = harness.get_by_label(&button_label);
        assert!(
            button.rect().width() > 200.0,
            "the workflow name belongs to the clickable row"
        );
        button.click();
        harness.run();
        assert!(
            harness
                .state()
                .inspection
                .return_tasks
                .contains(&task.task_id)
        );
        harness.state_mut().work.tasks[0].review.workflow = labello_domain::ReviewWorkflow::None;
        harness.run();
        assert!(
            harness
                .query_by_label("Approval review is not enabled for this workflow.")
                .is_some()
        );
        assert!(harness.state().inspection.return_tasks.is_empty());
        harness.state_mut().work.tasks[0].review.workflow =
            labello_domain::ReviewWorkflow::Approval;
        harness
            .state_mut()
            .inspection
            .state
            .as_mut()
            .unwrap()
            .task_states
            .get_mut(&task.task_id)
            .unwrap()
            .status = TaskStatus::Submitted;
        harness.run();
        assert!(
            harness
                .query_by_label("Only completed workflows can be returned to review.")
                .is_some()
        );
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
        assert!(harness.query_by_label("Annotation overlays").is_none());
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
        assert!(harness.query_by_label("Annotations").is_none());
        assert!(harness.query_by_label("Next page").is_none());
        assert!(harness.query_by_label("Filters").is_none());
        assert!(
            harness
                .query_by_label("Reason for returning work")
                .is_none()
        );
        let preview = harness.get_by_label("Inspect Sample 0").rect();
        let second = harness.get_by_label("Inspect Sample 1").rect();
        let third = harness.get_by_label("Inspect Sample 2").rect();
        let fourth = harness.get_by_label("Inspect Sample 3").rect();
        let filename = harness.get_by_label("Sample 0").rect();
        assert!(filename.top() >= preview.bottom());
        assert!(filename.width() <= preview.width() + 1.0);
        assert_eq!(preview.top(), second.top());
        assert_eq!(preview.top(), third.top());
        assert!(fourth.top() > preview.bottom());
        assert!(third.right() <= 420.0);
        assert!(harness.get_by_label("Fit image").rect().bottom() < preview.top());
        harness.get_by_label("Return to review").click();
        harness.run();
        let submit = harness.get_by_label("Return selected workflows").rect();
        assert!(submit.right() <= 1440.0 && submit.bottom() <= 1000.0);
    }
    #[test]
    fn inspector_icon_toggles_share_a_row_and_keep_return_selection_independent() {
        for size in [
            egui::vec2(1440.0, 1000.0),
            egui::vec2(390.0, 844.0),
            egui::vec2(320.0, 568.0),
        ] {
            let mut harness = Harness::builder().with_size(size).build_eframe(|_| app());
            harness.run();
            if size.x < 1288.0 {
                harness
                    .get_by_role_and_label(egui::accesskit::Role::Button, "Overlays")
                    .click();
                harness.run();
            }
            let boxes =
                harness.get_by_role_and_label(egui::accesskit::Role::Button, "Bounding boxes");
            let skeletons =
                harness.get_by_role_and_label(egui::accesskit::Role::Button, "Skeletons");
            let statuses = harness.get_by_label("Overlay statuses");
            assert_eq!(boxes.rect().center().y, skeletons.rect().center().y);
            assert_eq!(boxes.rect().center().y, statuses.rect().center().y);
            assert!(statuses.rect().right() <= size.x);
            assert!(boxes.rect().width() >= 44.0 && boxes.rect().height() >= 44.0);
            assert_eq!(
                boxes.accesskit_node().toggled(),
                Some(egui::accesskit::Toggled::True)
            );
            boxes.click();
            harness.run();
            assert!(!harness.state().inspection.boxes);
            let task = harness.state().work.tasks[0].clone();
            harness
                .get_by_role_and_label(egui::accesskit::Role::Button, &task.name)
                .click();
            harness.run();
            assert!(
                harness
                    .state()
                    .inspection
                    .hidden_tasks
                    .contains(&task.task_id)
            );
            assert!(harness.state().inspection.return_tasks.is_empty());
            harness.get_by_label("Return to review").click();
            harness.run();
            harness
                .get_by_role_and_label(
                    egui::accesskit::Role::Button,
                    &format!("Return {} to review", task.name),
                )
                .click();
            harness.run();
            assert!(
                harness
                    .state()
                    .inspection
                    .return_tasks
                    .contains(&task.task_id)
            );
            assert!(
                harness
                    .state()
                    .inspection
                    .hidden_tasks
                    .contains(&task.task_id)
            );
            assert!(!harness.state().inspection.boxes);
        }
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
        assert!(harness.query_by_label("Return to review").is_none());
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
        app.inspection.active_query.search = Some("Sample".into());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run();
        harness.get_by_label("Next image").click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .inspection
                .failed_query
                .as_ref()
                .unwrap()
                .page,
            2
        );
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
        page.items[0].image.image_id = "new-image".into();
        let expected = page.items[0].image.image_id.clone();
        let request = app.request_identity(Some(app.config.dataset_id.clone()));
        let mut query = app.inspection.active_query.clone();
        query.page = 2;
        app.inspection
            .pending
            .insert(request.request_id, InspectorAction::List(query));
        app.accept_inspection(
            &egui::Context::default(),
            request,
            Ok(InspectorReply::List(page)),
        );
        assert_eq!(app.inspection.page.as_ref().unwrap().items.len(), 25);
        assert_eq!(app.inspection.selected.as_ref().unwrap().image_id, expected);
        assert!(app.inspection.navigate_page.is_none());
    }

    #[test]
    fn inspector_scroll_requests_coalesce_and_reset_rejects_obsolete_batches() {
        let mut app = app();
        app.inspection.active_query.search = Some("applied".into());
        app.inspection.query.search = Some("unsubmitted".into());
        let selected = app.inspection.selected.clone();
        app.load_more_inspection_images();
        app.load_more_inspection_images();
        let requests: Vec<_> = app
            .inspection
            .pending
            .iter()
            .filter_map(|(id, action)| {
                if let InspectorAction::List(query) = action {
                    Some((*id, query.clone()))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].1.page, 2);
        assert_eq!(requests[0].1.search.as_deref(), Some("applied"));
        let old_request = RequestIdentity {
            request_id: requests[0].0,
            ..app.request_identity(Some(app.config.dataset_id.clone()))
        };
        let mut late = app.inspection.page.clone().unwrap();
        late.page = 2;
        late.items[0].image.image_id = "obsolete".into();
        app.reset_inspection_gallery();
        assert!(!app.inspection.pending.contains_key(&old_request.request_id));
        app.accept_inspection(
            &egui::Context::default(),
            old_request,
            Ok(InspectorReply::List(late)),
        );
        assert_eq!(app.inspection.page.as_ref().unwrap().items.len(), 24);
        assert_eq!(app.inspection.page.as_ref().unwrap().page, 1);
        assert_eq!(app.inspection.selected, selected);
        let request_id = *app.inspection.pending.keys().next().unwrap();
        app.fail_inspection(request_id, "Temporary failure".into());
        assert_eq!(
            app.inspection
                .failed_query
                .as_ref()
                .unwrap()
                .search
                .as_deref(),
            Some("unsubmitted")
        );
        app.load_more_inspection_images();
        assert!(
            app.inspection.pending.is_empty(),
            "failure requires an explicit retry"
        );
    }

    #[test]
    fn inspector_spinner_is_over_the_canvas_and_long_names_stay_within_tiles() {
        let mut app = app();
        let name = "A very long image filename which must remain within the thumbnail width.png";
        app.inspection.page.as_mut().unwrap().items[0]
            .image
            .file_name = name.into();
        app.work.tasks[0].name =
            "A very long workflow name which must not push the count out of its panel".into();
        app.inspection.preview_loaded = false;
        // Hold image loading without a transport while inspecting the shared rendering.
        let id = app.inspection.selected.as_ref().unwrap().image_id.clone();
        app.inspection
            .pending
            .insert(999, InspectorAction::Image(id));
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run_steps(4);
        let canvas = harness.get_by_label("Annotation canvas").rect();
        let spinner = harness.get_by_label("Loading image").rect();
        assert!(canvas.contains_rect(spinner));
        assert!((canvas.center() - spinner.center()).length() < 1.0);
        let preview = harness.get_by_label(&format!("Inspect {name}")).rect();
        let filename = harness.get_by_label(name).rect();
        assert!(filename.width() <= preview.width() + 1.0);
        assert!(filename.right() <= preview.right() + 1.0);
        assert!(harness.state().inspection.texture.is_some());
        assert!(harness.query_by_label("Loading image…").is_none());
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
