use crate::{
    app::{AppView, LabelloApp},
    theme,
};
use labello_domain::{DatasetId, ImageId, TaskId, WorkflowReason, WorkflowReasonAction};

pub(crate) struct ReasonNotice {
    dataset_id: DatasetId,
    image_id: ImageId,
    task_id: TaskId,
    view: AppView,
    reasons: Vec<WorkflowReason>,
    dismissed: bool,
    compact_details_open: bool,
}

impl LabelloApp {
    pub(crate) fn install_reason_notice(&mut self, reasons: Vec<WorkflowReason>) {
        self.work.reason_notice = self
            .work
            .assignment
            .as_ref()
            .map(|assignment| ReasonNotice {
                dataset_id: self.config.dataset_id.clone(),
                image_id: assignment.image_id.clone(),
                task_id: assignment.task_id.clone(),
                view: self.view,
                reasons: reasons
                    .into_iter()
                    .filter(|reason| reason.image_id == assignment.image_id)
                    .collect(),
                dismissed: false,
                compact_details_open: false,
            });
    }

    pub(crate) fn clear_reason_notice_outside_scope(&mut self) {
        if self.work.reason_notice.as_ref().is_some_and(|notice| {
            notice.dataset_id != self.config.dataset_id
                || notice.view != self.view
                || self.work.selected_task_id.as_ref() != Some(&notice.task_id)
                || self
                    .work
                    .current
                    .as_ref()
                    .is_none_or(|current| current.image.image_id != notice.image_id)
                || self.work.assignment.as_ref().is_none_or(|assignment| {
                    assignment.image_id != notice.image_id || assignment.task_id != notice.task_id
                })
        }) {
            self.work.reason_notice = None;
        }
    }

    pub(crate) fn reason_notice_visible(&self) -> bool {
        self.work.reason_notice.as_ref().is_some_and(|notice| {
            !notice.dismissed && (!notice.reasons.is_empty() || self.review_revision_active())
        })
    }

    pub(crate) fn reason_notice_contents(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        canvas_height: f32,
    ) {
        if !self.reason_notice_visible() {
            return;
        }
        let revision = self.review_revision_active();
        let Some(notice) = self.work.reason_notice.as_ref() else {
            return;
        };
        let reasons = notice.reasons.clone();
        let detail_id = (
            "reason-details",
            notice.image_id.clone(),
            notice.task_id.clone(),
        );
        let short = Self::short_viewport(ui.ctx().content_rect().size());
        let mut compact_details_open = notice.compact_details_open;
        let mut dismiss = false;
        let label = if reasons.is_empty() {
            "Previous review".to_owned()
        } else {
            format!("Saved reasons ({})", reasons.len())
        };
        theme::card_frame()
            .inner_margin(egui::Margin::same(6))
            .stroke(egui::Stroke::new(1.0, theme::AMBER))
            .show(ui, |ui| {
                ui.set_width((width - 12.0).max(100.0));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width((width - 66.0 - ui.spacing().item_spacing.x).max(44.0));
                        if short {
                            if ui
                                .add_sized([ui.available_width(), 44.0], egui::Button::new(&label))
                                .on_hover_text("Open reason details")
                                .clicked()
                            {
                                compact_details_open = true;
                            }
                        } else {
                            let response = ui.add_sized(
                                [ui.available_width(), 44.0],
                                egui::Label::new(&label).wrap(),
                            );
                            ui.ctx().accesskit_node_builder(response.id, |node| {
                                node.set_role(egui::accesskit::Role::Status);
                                node.set_label(label.clone());
                                node.set_live(egui::accesskit::Live::Polite);
                            });
                        }
                    });
                    let response = ui
                        .add(egui::Button::new("×").min_size(egui::vec2(44.0, 44.0)))
                        .on_hover_text("Dismiss saved reasons");
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "Dismiss saved reasons",
                        )
                    });
                    dismiss = response.clicked();
                });
                if !short {
                    egui::CollapsingHeader::new("Reason details")
                        .id_salt(&detail_id)
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("saved-reasons-scroll")
                                .max_height((canvas_height * 0.35).clamp(44.0, 240.0))
                                .show(ui, |ui| self.saved_reason_details(ui, &reasons, revision));
                        });
                }
            });
        if short && compact_details_open && !dismiss {
            let screen = ui.ctx().content_rect();
            egui::Window::new("Reason details")
                .id(egui::Id::new(&detail_id))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .constrain_to(screen)
                .title_bar(false)
                .resizable(false)
                .fixed_size(egui::vec2(
                    (screen.width() - 32.0).max(160.0),
                    (screen.height() - 48.0).max(100.0),
                ))
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Reason details").strong());
                        if ui
                            .add(
                                egui::Button::new("Close details").min_size(egui::vec2(44.0, 44.0)),
                            )
                            .clicked()
                        {
                            compact_details_open = false;
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("compact-reason-details")
                        .max_height((screen.height() - 132.0).max(44.0))
                        .show(ui, |ui| self.saved_reason_details(ui, &reasons, revision));
                });
        }
        if let Some(notice) = self.work.reason_notice.as_mut() {
            notice.dismissed = dismiss;
            notice.compact_details_open = compact_details_open && !dismiss;
        }
    }

    fn saved_reason_details(&self, ui: &mut egui::Ui, reasons: &[WorkflowReason], revision: bool) {
        if revision {
            ui.add(egui::Label::new("The previous outcome stays effective until you commit approval or submit corrections. Corrections return the image to a fresh review round.").wrap());
        }
        for reason in reasons {
            ui.separator();
            ui.label(egui::RichText::new(action_label(reason.action)).strong());
            let workflow = reason
                .task_id
                .as_ref()
                .map(|id| {
                    self.work
                        .tasks
                        .iter()
                        .find(|task| &task.task_id == id)
                        .map_or_else(|| id.to_string(), |task| task.name.clone())
                })
                .unwrap_or_else(|| "Whole image".into());
            let relevance = if reason.current_round {
                "Current review round"
            } else if reason.current_exclusion {
                "Current exclusion"
            } else {
                "Historical context"
            };
            ui.add(egui::Label::new(format!("{workflow} · {relevance}")).wrap());
            if let Some(object) = &reason.object_group_id {
                ui.add(egui::Label::new(format!("Object {object}")).wrap());
            } else if let Some(object) = &reason.annotation_id {
                ui.add(egui::Label::new(format!("Object {object}")).wrap());
            }
            ui.add(
                egui::Label::new(format!(
                    "{} · {}",
                    reason.actor_user_id,
                    reason.timestamp.format("%Y-%m-%d %H:%M UTC")
                ))
                .wrap(),
            );
            if let Some(category) = reason.category {
                ui.add(
                    egui::Label::new(format!(
                        "Reason: {}",
                        crate::manual_migration::exclusion_label(category)
                    ))
                    .wrap(),
                );
            }
            if let Some(text) = &reason.text {
                let text = if reason.category.is_some() {
                    format!("Note: {text}")
                } else {
                    text.clone()
                };
                ui.add(egui::Label::new(text).wrap().selectable(true));
            }
        }
    }
}

fn action_label(action: WorkflowReasonAction) -> &'static str {
    match action {
        WorkflowReasonAction::AnnotationEdit => "Annotation edit",
        WorkflowReasonAction::AnnotationDeletion => "Annotation deletion",
        WorkflowReasonAction::ReviewComment => "Review comment",
        WorkflowReasonAction::ReviewRevisionComment => "Revised review comment",
        WorkflowReasonAction::ReviewerCorrection => "Reviewer correction",
        WorkflowReasonAction::MigrationExclusion => "Migration exclusion",
        WorkflowReasonAction::ImportedTaskReopened => "Imported work reopened",
        WorkflowReasonAction::ImportCoverageIncluded => "Imported coverage included",
    }
}
