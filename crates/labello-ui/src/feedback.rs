use crate::{
    app::{LabelloApp, UiCommand, UiMessage},
    canvas::{CanvasInteraction, CanvasState},
    theme,
};
use eframe::egui;
use labello_client::{FeedbackDetail, FeedbackEntry, ImagePreview, LabelloApi};
use labello_domain::{DatasetId, EventId, TaskId};
use std::{collections::BTreeMap, rc::Rc};
use web_time::{Duration, Instant};

#[derive(Clone, Debug)]
pub(crate) enum FeedbackAction {
    List,
    Open(EventId),
    Dismiss(EventId, bool),
    Threshold(DatasetId, TaskId, Option<u32>),
}
#[derive(Debug)]
pub(crate) enum FeedbackReply {
    List(Vec<FeedbackEntry>),
    Open(Box<FeedbackDetail>, ImagePreview),
    Dismissed(Vec<FeedbackEntry>),
    Threshold(DatasetId, TaskId, u32),
}
#[derive(Default)]
pub(crate) struct FeedbackState {
    pub(crate) pending: Option<u64>,
    pub(crate) error: Option<String>,
    pub(crate) items: Vec<FeedbackEntry>,
    pub(crate) detail: Option<FeedbackDetail>,
    texture: Option<egui::TextureHandle>,
    dimensions: [u32; 2],
    canvas: CanvasState,
    before: bool,
    selected: usize,
    checked: bool,
    last_attempt: Option<Instant>,
    identity: Option<(String, labello_domain::UserId)>,
    retry: Option<FeedbackAction>,
    acknowledged: bool,
    shown: bool,
    pub(crate) open: bool,
    popup: bool,
    invoker: Option<egui::Id>,
    thresholds: BTreeMap<(DatasetId, TaskId), u32>,
}
impl LabelloApp {
    pub(crate) fn feedback_required(&self) -> bool {
        self.feedback
            .items
            .iter()
            .any(|item| item.summary.mandatory)
    }
    pub(crate) fn feedback_blocks_input(&self) -> bool {
        self.feedback.open || self.feedback.popup || self.work_view() && self.feedback_required()
    }
    pub(crate) fn refresh_feedback(&mut self, ctx: &egui::Context) {
        if self.runtime.api.is_none() || self.auth.account.is_none() || self.auth.recovery.is_some()
        {
            return;
        }
        let identity = (
            self.config.api_base_url.clone(),
            self.config.user_id.clone(),
        );
        if self.feedback.identity.as_ref() != Some(&identity) {
            self.feedback = FeedbackState {
                identity: Some(identity),
                ..Default::default()
            };
        }
        if self.feedback.pending.is_none()
            && self
                .feedback
                .last_attempt
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(5))
        {
            self.request_feedback(FeedbackAction::List);
        }
        ctx.request_repaint_after(Duration::from_secs(5));
        if self.work_view() && self.feedback_required() {
            self.feedback.open = true;
            self.feedback.popup = false;
            if self.feedback.detail.is_none()
                && self.feedback.pending.is_none()
                && self.feedback.error.is_none()
                && let Some(item) = self
                    .feedback
                    .items
                    .iter()
                    .find(|item| item.summary.mandatory)
            {
                self.request_feedback(FeedbackAction::Open(item.summary.event_id.clone()));
            }
        }
    }
    pub(crate) fn request_feedback(&mut self, action: FeedbackAction) {
        if self.feedback.pending.is_some() {
            return;
        }
        let request = self.request_identity(None);
        self.feedback.pending = Some(request.request_id);
        self.feedback.last_attempt = Some(Instant::now());
        self.feedback.error = None;
        self.feedback.retry = Some(action.clone());
        self.queue_command(UiCommand::Feedback { request, action });
    }
    pub(crate) fn dispatch_feedback(
        &mut self,
        api: Rc<dyn LabelloApi>,
        command: UiCommand,
    ) -> Option<UiCommand> {
        let UiCommand::Feedback { request, action } = command else {
            return Some(command);
        };
        self.spawn_message(request.clone(), async move {
            let result = async {
                Ok(match action {
                    FeedbackAction::List => FeedbackReply::List(api.feedback_inbox().await?),
                    FeedbackAction::Open(event) => {
                        let detail = api.feedback_detail(&event).await?;
                        let summary = &detail.feedback.summary;
                        let preview = crate::live_workflow::load_working_preview(
                            api.as_ref(),
                            &summary.dataset_id,
                            &summary.image_id,
                        )
                        .await?;
                        FeedbackReply::Open(Box::new(detail), preview)
                    }
                    FeedbackAction::Dismiss(event, viewed) => {
                        api.dismiss_feedback(&event, viewed).await?;
                        FeedbackReply::Dismissed(api.feedback_inbox().await?)
                    }
                    FeedbackAction::Threshold(dataset, task, value) => {
                        let result = api.feedback_threshold(&dataset, &task, value).await?;
                        FeedbackReply::Threshold(dataset, task, result.threshold)
                    }
                })
            }
            .await;
            UiMessage::Feedback {
                request,
                result: Box::new(result),
            }
        });
        None
    }
    pub(crate) fn reduce_feedback(
        &mut self,
        ctx: &egui::Context,
        message: UiMessage,
    ) -> Option<UiMessage> {
        let UiMessage::Feedback { request, result } = message else {
            return Some(message);
        };
        if self.feedback.pending != Some(request.request_id) {
            return None;
        }
        self.feedback.pending = None;
        match *result {
            Ok(FeedbackReply::List(items)) => {
                self.feedback.items = items;
                self.feedback.checked = true;
            }
            Ok(FeedbackReply::Dismissed(items)) => {
                self.feedback.items = items;
                self.feedback.acknowledged = true;
            }
            Ok(FeedbackReply::Open(detail, preview)) => {
                self.feedback.dimensions = [preview.width, preview.height];
                self.feedback.texture = Some(ctx.load_texture(
                    "feedback-image",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [preview.width as usize, preview.height as usize],
                        &preview.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                ));
                self.feedback.detail = Some(*detail);
                self.feedback.canvas = Default::default();
                self.feedback.before = false;
                self.feedback.selected = 0;
                self.feedback.shown = false;
                self.feedback.acknowledged = false;
                self.feedback.open = true;
                self.feedback.popup = false;
            }
            Ok(FeedbackReply::Threshold(dataset, task, value)) => {
                self.feedback.thresholds.insert((dataset, task), value);
            }
            Err(error) => self.feedback.error = Some(error.to_string()),
        }
        None
    }
    pub(crate) fn feedback_button(&mut self, ui: &mut egui::Ui) {
        let response = ui
            .add_sized(
                [60.0, 44.0],
                egui::Button::new(format!("✉ {}", self.feedback.items.len()))
                    .selected(self.feedback.popup),
            )
            .on_hover_text("Correction feedback");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Open feedback inbox, {} pending", self.feedback.items.len()),
            )
        });
        self.feedback.invoker = Some(response.id);
        if response.clicked() {
            self.feedback.popup = !self.feedback.popup;
            self.request_feedback(FeedbackAction::List);
        }
        let mut open = self.feedback.popup;
        egui::Popup::from_response(&response)
            .open_bool(&mut open)
            .show(|ui| {
                ui.set_width(ui.ctx().content_rect().width().min(380.0) - 32.0);
                ui.heading("Correction feedback");
                if let Some(error) = self.feedback.error.clone() {
                    ui.label(error);
                    if ui.button("Retry feedback").clicked() {
                        self.request_feedback(FeedbackAction::List);
                    }
                }
                if !self.feedback.checked && self.feedback.pending.is_some() {
                    ui.spinner();
                } else if self.feedback.items.is_empty() {
                    ui.label("No pending feedback");
                }
                egui::ScrollArea::vertical()
                    .max_height((ui.ctx().content_rect().height() * 0.55).max(80.0))
                    .show(ui, |ui| {
                        for item in self.feedback.items.clone() {
                            ui.push_id(&item.summary.event_id, |ui| {
                                ui.label(format!(
                                    "{} · {}",
                                    item.summary.dataset_name, item.summary.workflow_name
                                ));
                                ui.small(format!(
                                    "{} · {}",
                                    item.summary.image_id, item.reviewer_name
                                ));
                                ui.small(
                                    item.summary
                                        .timestamp
                                        .format("%Y-%m-%d %H:%M UTC")
                                        .to_string(),
                                );
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(
                                            self.feedback.pending.is_none(),
                                            egui::Button::new("View feedback")
                                                .min_size(egui::vec2(44.0, 44.0)),
                                        )
                                        .clicked()
                                    {
                                        self.request_feedback(FeedbackAction::Open(
                                            item.summary.event_id.clone(),
                                        ));
                                    }
                                    if item.summary.mandatory {
                                        ui.label("Viewing required");
                                    } else if ui
                                        .add_enabled(
                                            self.feedback.pending.is_none(),
                                            egui::Button::new("Dismiss")
                                                .min_size(egui::vec2(44.0, 44.0)),
                                        )
                                        .clicked()
                                    {
                                        self.request_feedback(FeedbackAction::Dismiss(
                                            item.summary.event_id.clone(),
                                            false,
                                        ));
                                    }
                                });
                                ui.separator();
                            });
                        }
                    });
            });
        self.feedback.popup = open && !self.feedback.open;
    }
    pub(crate) fn feedback_overlay(&mut self, ctx: &egui::Context) {
        if !self.feedback.open {
            return;
        }
        let mandatory = self.feedback_required();
        let detail = self.feedback.detail.clone();
        let mut close = false;
        let mut rendered = false;
        let response = egui::Modal::new(egui::Id::new("correction-feedback")).show(ctx, |ui| {
            let size = ctx.content_rect().size();
            ui.set_width((size.x - 48.0).clamp(240.0, 1050.0));
            egui::ScrollArea::vertical().max_height((size.y - 140.0).max(80.0)).show(ui, |ui| {
                ui.heading(if mandatory { "Feedback requires your attention" } else { "Correction feedback" });
                if mandatory { ui.label("View all required feedback before continuing labeling."); }
                if let Some(detail) = &detail {
                    let summary = &detail.feedback.summary;
                    let remaining = self.feedback.items.iter().filter(|item| item.summary.dataset_id == summary.dataset_id && item.summary.task_id == summary.task_id).count();
                    ui.label(format!("{} · {} · {remaining} remaining", summary.dataset_name, summary.workflow_name));
                    ui.label(format!("Corrected by {} · {}", detail.reviewer_name, summary.timestamp.format("%Y-%m-%d %H:%M UTC")));
                    if let Some(reason) = &detail.feedback.reason { ui.label(reason); }
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut self.feedback.before, true, "Before correction");
                        ui.selectable_value(&mut self.feedback.before, false, "After correction");
                        if ui.button("Fit image").clicked() { self.feedback.canvas.fit_view(); }
                    });
                    let annotations = if self.feedback.before { &detail.feedback.before } else { &detail.feedback.after };
                    let edges: Vec<_> = detail.feedback.task.skeleton.as_ref().map(|s| s.edges.iter().map(|edge| (edge.from.clone(), edge.to.clone())).collect()).unwrap_or_default();
                    ui.allocate_ui(egui::vec2(ui.available_width(), (size.y * 0.5).clamp(140.0, 600.0)), |ui| {
                        ui.set_height((size.y * 0.5).clamp(140.0, 600.0));
                        rendered = self.feedback.texture.is_some() && ui.clip_rect().intersect(ui.max_rect()).height() >= 64.0;
                        crate::canvas::show_canvas_configured(ui, &mut self.feedback.canvas, self.feedback.texture.as_ref(), annotations, self.feedback.dimensions, false, detail.feedback.changes.get(self.feedback.selected).and_then(|change| match change {
                            labello_domain::ReviewCorrectionChange::Edit { annotation_id, .. } | labello_domain::ReviewCorrectionChange::Add { annotation_id, .. } | labello_domain::ReviewCorrectionChange::Remove { annotation_id, .. } => Some(annotation_id),
                            labello_domain::ReviewCorrectionChange::MigrationObject { object_group_id, .. } => annotations.iter().find(|a| a.object_group_id.as_ref() == Some(object_group_id)).map(|a| &a.annotation_id),
                        }), CanvasInteraction::annotations(false), &edges, &[]);
                    });
                    for (index, change) in detail.feedback.changes.iter().enumerate() {
                        use labello_domain::{ReviewCorrectionChange as C, MigrationReviewCorrection as M};
                        let label = match change { C::Edit { .. } => "Edited annotation", C::Add { .. } => "Added annotation", C::Remove { .. } => "Removed annotation", C::MigrationObject { replacement: M::Skeleton { .. }, .. } => "Corrected skeleton", C::MigrationObject { replacement: M::Exclude { .. }, .. } => "Excluded object" };
                        if ui.selectable_label(self.feedback.selected == index, format!("{}. {label}", index + 1)).clicked() {
                            self.feedback.selected = index;
                            if let C::Remove { .. } = change { self.feedback.before = true; }
                            if let C::Add { .. } = change { self.feedback.before = false; }
                            let target = match change {
                                C::Edit { annotation_id, .. } | C::Add { annotation_id, .. } | C::Remove { annotation_id, .. } => detail.feedback.after.iter().chain(&detail.feedback.before).find(|a| &a.annotation_id == annotation_id),
                                C::MigrationObject { object_group_id, .. } => detail.feedback.after.iter().chain(&detail.feedback.before).find(|a| a.object_group_id.as_ref() == Some(object_group_id)),
                            };
                            if let Some(target) = target { self.feedback.canvas.focus_annotation(target); }
                        }
                        if let C::MigrationObject { object_group_id, .. } = change {
                            let label = |map: &std::collections::BTreeMap<labello_domain::ObjectGroupId, labello_domain::MigrationDisposition>| match map.get(object_group_id).map(|d| &d.status) {
                                Some(labello_domain::MigrationDispositionStatus::Annotated { .. }) => "Annotated",
                                Some(labello_domain::MigrationDispositionStatus::Excluded { .. }) => crate::glossary::EXCLUDED,
                                _ => crate::glossary::PENDING,
                            };
                            ui.label(format!("{} → {}", label(&detail.feedback.before_dispositions), label(&detail.feedback.after_dispositions)));
                        }
                        if let C::MigrationObject { replacement: M::Exclude { reason, note }, .. } = change {
                            ui.label(match reason {
                                labello_domain::MigrationExclusionReason::NoValidSkeleton => "No valid skeleton",
                                labello_domain::MigrationExclusionReason::InsufficientVisibleFeatures => "Insufficient visible features",
                                labello_domain::MigrationExclusionReason::InvalidSourceBox => "Invalid source box",
                                labello_domain::MigrationExclusionReason::DuplicateSourceObject => "Duplicate source object",
                                labello_domain::MigrationExclusionReason::ObjectNotPresent => "Object not present",
                                labello_domain::MigrationExclusionReason::Other => "Other exclusion",
                            });
                            if let Some(note) = note { ui.label(note); }
                        }
                    }
                } else if self.feedback.pending.is_some() { ui.spinner(); ui.label("Loading feedback…"); }
                if let Some(error) = self.feedback.error.clone() {
                    ui.colored_label(theme::WARNING, error);
                    if ui.add(egui::Button::new("Retry feedback").min_size(egui::vec2(44.0,44.0))).clicked() && let Some(action) = self.feedback.retry.clone() { self.request_feedback(action); }
                }
            });
                ui.horizontal_wrapped(|ui| {
                    let next = self.feedback.items.iter().find(|item| (!mandatory || item.summary.mandatory) && detail.as_ref().is_none_or(|detail| detail.feedback.summary.event_id != item.summary.event_id)).map(|item| item.summary.event_id.clone());
                    if let Some(next) = next
                        && ui.add_enabled(self.feedback.pending.is_none() && self.feedback.acknowledged, egui::Button::new("Next feedback").min_size(egui::vec2(44.0,44.0))).clicked() { self.request_feedback(FeedbackAction::Open(next)); }
                    if !mandatory && ui.add(egui::Button::new("Close feedback").min_size(egui::vec2(44.0,44.0))).clicked() { close = true; }
                });
        });
        if !mandatory && (close || response.should_close()) {
            self.feedback.open = false;
            self.feedback.detail = None;
            self.feedback.texture = None;
            if let Some(id) = self.feedback.invoker {
                ctx.memory_mut(|m| m.request_focus(id));
            }
        } else if rendered
            && self.feedback.shown
            && !self.feedback.acknowledged
            && self.feedback.pending.is_none()
            && self.feedback.error.is_none()
            && let Some(detail) = detail
        {
            self.request_feedback(FeedbackAction::Dismiss(
                detail.feedback.summary.event_id,
                true,
            ));
        }
        self.feedback.shown |= rendered;
        if rendered && !self.feedback.acknowledged {
            ctx.request_repaint();
        }
    }
    pub(crate) fn feedback_settings(&mut self, ui: &mut egui::Ui) {
        let tasks = self
            .datasets
            .metadata
            .as_ref()
            .map(|d| d.tasks.clone())
            .unwrap_or_default();
        ui.collapsing("Mandatory feedback", |ui| {
            ui.label("Block further labeling when a workflow reaches this many pending corrections. Default: 5.");
            for task in tasks {
                let key = (self.config.dataset_id.clone(), task.task_id.clone());
                ui.push_id(&task.task_id, |ui| {
                    ui.label(task.name);
                    if let Some(mut value) = self.feedback.thresholds.get(&key).copied() {
                        let response = ui.add(egui::DragValue::new(&mut value).range(1..=u32::MAX).prefix("Pending items: "));
                        if response.changed() { self.feedback.thresholds.insert(key.clone(), value); }
                        if ui.add_enabled(self.feedback.pending.is_none(), egui::Button::new("Save feedback threshold")).clicked() { self.request_feedback(FeedbackAction::Threshold(key.0, key.1, Some(value))); }
                    } else if ui.add_enabled(self.feedback.pending.is_none(), egui::Button::new("Load feedback threshold")).clicked() { self.request_feedback(FeedbackAction::Threshold(key.0, key.1, None)); }
                });
            }
            if let Some(error) = &self.feedback.error { ui.label(error); }
        });
    }
}

#[cfg(any(test, feature = "inspector-presets"))]
impl LabelloApp {
    pub(crate) fn seed_feedback(&mut self, ctx: &egui::Context, mandatory: bool) {
        let task = self.selected_task().unwrap().clone();
        let summary = labello_domain::FeedbackSummary {
            event_id: EventId::from("synthetic-feedback-0"),
            dataset_id: self.config.dataset_id.clone(),
            dataset_name: "Practice dataset".into(),
            image_id: self.work.current.as_ref().unwrap().image.image_id.clone(),
            task_id: task.task_id.clone(),
            workflow_name: task.name.clone(),
            reviewer: "reviewer".into(),
            timestamp: labello_domain::now(),
            mandatory,
        };
        let before = self.work.annotations.clone();
        let mut after = before.clone();
        let mut changes = Vec::new();
        if let Some(annotation) = after.first_mut() {
            if let labello_domain::AnnotationGeometry::BoundingBox(bbox) = &mut annotation.geometry
            {
                bbox.width = (bbox.width * 0.75).max(0.01);
            }
            annotation.version += 1;
            changes.push(labello_domain::ReviewCorrectionChange::Edit {
                annotation_id: annotation.annotation_id.clone(),
                expected_version: annotation.version - 1,
                geometry: annotation.geometry.clone(),
            });
        }
        self.feedback = FeedbackState {
            checked: true,
            acknowledged: true,
            open: mandatory,
            popup: !mandatory,
            items: (0..5)
                .map(|index| {
                    let mut summary = summary.clone();
                    summary.event_id = EventId::from(format!("synthetic-feedback-{index}"));
                    FeedbackEntry {
                        summary,
                        reviewer_name: "@example-reviewer".into(),
                    }
                })
                .collect(),
            detail: mandatory.then_some(FeedbackDetail {
                feedback: labello_domain::CorrectionFeedback {
                    summary,
                    recipients: Default::default(),
                    task,
                    before,
                    after,
                    changes,
                    before_dispositions: Default::default(),
                    after_dispositions: Default::default(),
                    reason: Some("Tighten the box around the visible object.".into()),
                },
                reviewer_name: "@example-reviewer".into(),
            }),
            texture: Some(self.work.current_texture.clone().unwrap_or_else(|| {
                ctx.load_texture(
                    "feedback-synthetic",
                    egui::ColorImage::filled([100, 100], theme::PANEL),
                    egui::TextureOptions::LINEAR,
                )
            })),
            dimensions: [640, 480],
            ..Default::default()
        };
    }
}
