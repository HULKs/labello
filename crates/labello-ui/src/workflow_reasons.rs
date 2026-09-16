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
        // Recent feedback should not be buried below earlier explanations.
        let reasons: Vec<_> = notice.reasons.iter().rev().cloned().collect();
        let detail_id = (
            "work-feedback",
            notice.image_id.clone(),
            notice.task_id.clone(),
        );
        let short = Self::short_viewport(ui.ctx().content_rect().size());
        let mut details_open = notice.compact_details_open;
        let mut dismiss = false;
        let heading = if revision {
            "Revisiting a completed review".to_owned()
        } else {
            reasons
                .first()
                .map_or_else(|| "Image feedback".into(), feedback_heading)
        };
        let accent = if !revision && reasons.first().is_some_and(needs_attention) {
            theme::AMBER
        } else {
            theme::BORDER
        };
        theme::card_frame()
            .inner_margin(egui::Margin::symmetric(8, if short { 0 } else { 8 }))
            .stroke(egui::Stroke::new(1.0, accent))
            .show(ui, |ui| {
                ui.set_width((width - 16.0).max(100.0));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width((width - 68.0 - ui.spacing().item_spacing.x).max(44.0));
                        if short {
                            let label = heading.clone();
                            if ui
                                .add_sized(
                                    [ui.available_width(), 44.0],
                                    egui::Button::new(&label).wrap(),
                                )
                                .on_hover_text("Read image feedback")
                                .clicked()
                            {
                                details_open = true;
                            }
                        } else {
                            let response = ui.add(
                                egui::Label::new(egui::RichText::new(&heading).strong()).wrap(),
                            );
                            ui.ctx().accesskit_node_builder(response.id, |node| {
                                node.set_role(egui::accesskit::Role::Status);
                                node.set_label(heading.clone());
                                node.set_live(egui::accesskit::Live::Polite);
                            });
                            if reasons.len() > 1 {
                                ui.weak(format!("{} saved messages", reasons.len()));
                            }
                        }
                    });
                    let response = ui
                        .add(egui::Button::new("×").min_size(egui::vec2(44.0, 44.0)))
                        .on_hover_text("Dismiss image feedback");
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "Dismiss image feedback",
                        )
                    });
                    dismiss = response.clicked();
                });
                if !short {
                    egui::ScrollArea::vertical()
                        .id_salt(&detail_id)
                        .max_height((canvas_height * 0.3).clamp(44.0, 220.0))
                        .show(ui, |ui| self.saved_feedback(ui, &reasons, revision));
                }
            });
        if short && details_open && !dismiss {
            let screen = ui.ctx().content_rect();
            egui::Window::new(&heading)
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
                    // Put Close first so even a long heading cannot push it out of reach.
                    if ui
                        .add(egui::Button::new("Close feedback").min_size(egui::vec2(44.0, 44.0)))
                        .clicked()
                    {
                        details_open = false;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("compact-image-feedback")
                        .max_height((screen.height() - 132.0).max(44.0))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&heading).strong());
                            self.saved_feedback(ui, &reasons, revision);
                        });
                });
        }
        if let Some(notice) = self.work.reason_notice.as_mut() {
            notice.dismissed = dismiss;
            notice.compact_details_open = details_open && !dismiss;
        }
    }

    fn saved_feedback(&self, ui: &mut egui::Ui, reasons: &[WorkflowReason], revision: bool) {
        if revision {
            ui.add(egui::Label::new("The saved decision stays in effect until you submit your review. Submitting corrections starts a new review round.").wrap());
        }
        for (index, reason) in reasons.iter().enumerate() {
            if index > 0 || revision {
                ui.add_space(theme::SPACE_2);
                ui.separator();
                ui.label(egui::RichText::new(feedback_heading(reason)).strong());
            }
            if reason.superseded {
                ui.weak("Replaced by a later update");
            }
            if let Some(category) = reason.category {
                ui.add(
                    egui::Label::new(format!(
                        "Exclusion: {}",
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
            let relevance = if reason.current_exclusion {
                " · Active exclusion"
            } else if reason.current_round {
                " · Current review round"
            } else {
                ""
            };
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{workflow}{relevance}"))
                        .small()
                        .color(theme::TEXT_MUTED),
                )
                .wrap(),
            );
            if let Some(object) = reason
                .object_group_id
                .as_ref()
                .map(ToString::to_string)
                .or_else(|| reason.annotation_id.as_ref().map(ToString::to_string))
            {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("Object {object}"))
                            .small()
                            .monospace()
                            .color(theme::TEXT_MUTED),
                    )
                    .truncate(),
                )
                .on_hover_text(format!("Object {object}"));
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{} · {}",
                        reason.actor_user_id,
                        reason.timestamp.format("%Y-%m-%d %H:%M UTC")
                    ))
                    .small()
                    .color(theme::TEXT_MUTED),
                )
                .wrap(),
            );
        }
    }
}

fn is_historical(reason: &WorkflowReason) -> bool {
    reason.superseded || (!reason.current_round && !reason.current_exclusion)
}

fn needs_attention(reason: &WorkflowReason) -> bool {
    !is_historical(reason)
        && (reason.review_decision == Some(labello_domain::ReviewDecision::Rejected)
            || reason.action == WorkflowReasonAction::MigrationExclusion)
}

fn feedback_heading(reason: &WorkflowReason) -> String {
    let event = match reason.action {
        WorkflowReasonAction::AnnotationEdit => "Annotation changed",
        WorkflowReasonAction::AnnotationDeletion => "Annotation removed",
        WorkflowReasonAction::ReviewComment => match reason.review_decision {
            Some(labello_domain::ReviewDecision::Rejected) => "Review rejected",
            Some(labello_domain::ReviewDecision::Approved) => "Review approved",
            None => "Reviewer comment",
        },
        WorkflowReasonAction::ReviewRevisionComment => match reason.review_decision {
            Some(labello_domain::ReviewDecision::Rejected) => "Review changed to rejected",
            Some(labello_domain::ReviewDecision::Approved) => "Review changed to approved",
            None => "Review comment updated",
        },
        WorkflowReasonAction::ReviewerCorrection => "Reviewer corrections saved",
        WorkflowReasonAction::MigrationExclusion => "Object excluded from migration",
        WorkflowReasonAction::ImportedTaskReopened => "Imported work reopened",
        WorkflowReasonAction::ImportCoverageIncluded => "Imported coverage included",
    };
    if is_historical(reason) {
        format!("Earlier · {event}")
    } else {
        event.into()
    }
}
