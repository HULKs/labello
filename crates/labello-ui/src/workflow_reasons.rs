use crate::{
    app::{AppView, LabelloApp},
    theme,
};
use labello_client::WorkflowReasonEntry;
use labello_domain::{DatasetId, ImageId, TaskId, WorkflowReason, WorkflowReasonAction};

pub(crate) struct ReasonNotice {
    dataset_id: DatasetId,
    image_id: ImageId,
    task_id: TaskId,
    view: AppView,
    reasons: Vec<WorkflowReasonEntry>,
    dismissed: bool,
    compact_details_open: bool,
}

impl LabelloApp {
    pub(crate) fn install_reason_notice(&mut self, reasons: Vec<WorkflowReasonEntry>) {
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
                    .filter(|entry| {
                        entry.reason.image_id == assignment.image_id
                            && entry.reason.task_id.as_ref() == Some(&assignment.task_id)
                    })
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
        self.work
            .reason_notice
            .as_ref()
            .is_some_and(|notice| !notice.dismissed && !notice.reasons.is_empty())
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
        let Some(notice) = self.work.reason_notice.as_ref() else {
            return;
        };
        let reasons: Vec<_> = notice.reasons.iter().rev().cloned().collect();
        let detail_id = (
            "work-feedback",
            notice.dataset_id.clone(),
            notice.image_id.clone(),
            notice.task_id.clone(),
        );
        let short = Self::short_viewport(ui.ctx().content_rect().size());
        let mut details_open = notice.compact_details_open;
        let mut dismiss = false;
        let heading = feedback_heading(&reasons[0].reason);
        let accent = if needs_attention(&reasons[0].reason) {
            theme::AMBER
        } else {
            theme::BORDER
        };
        theme::card_frame()
            .inner_margin(egui::Margin::symmetric(8, if short { 0 } else { 8 }))
            .stroke(egui::Stroke::new(1.0, accent))
            .show(ui, |ui| {
                ui.set_width((width - 16.0).max(100.0));
                // The close target sits beside the content, never between title and explanation.
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width((width - 68.0 - ui.spacing().item_spacing.x).max(44.0));
                        if short {
                            if ui
                                .add_sized(
                                    [ui.available_width(), 44.0],
                                    egui::Button::new(&heading).wrap(),
                                )
                                .on_hover_text("Read image feedback")
                                .clicked()
                            {
                                details_open = true;
                            }
                        } else {
                            egui::ScrollArea::vertical()
                                .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                                .id_salt(&detail_id)
                                .max_height((canvas_height * 0.3 + 44.0).clamp(88.0, 264.0))
                                .show(ui, |ui| self.saved_feedback(ui, &reasons, &detail_id));
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
                if !short && reasons.len() > 1 {
                    ui.label(
                        egui::RichText::new(format!("{} saved messages", reasons.len()))
                            .small()
                            .color(theme::TEXT_MUTED),
                    );
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
                    if ui
                        .add(egui::Button::new("Close feedback").min_size(egui::vec2(44.0, 44.0)))
                        .clicked()
                    {
                        details_open = false;
                    }
                    egui::ScrollArea::vertical()
                        .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                        .id_salt("compact-image-feedback")
                        .max_height((screen.height() - 132.0).max(44.0))
                        .show(ui, |ui| {
                            self.saved_feedback(ui, &reasons, &detail_id);
                            if reasons.len() > 1 {
                                ui.weak(format!("{} saved messages", reasons.len()));
                            }
                        });
                });
        }
        if let Some(notice) = self.work.reason_notice.as_mut() {
            notice.dismissed = dismiss;
            notice.compact_details_open = details_open && !dismiss;
        }
    }

    fn saved_feedback(
        &self,
        ui: &mut egui::Ui,
        reasons: &[WorkflowReasonEntry],
        context: &(impl std::hash::Hash + std::fmt::Debug),
    ) {
        for (index, entry) in reasons.iter().enumerate() {
            let reason = &entry.reason;
            if index > 0 {
                ui.separator();
            }
            ui.push_id(
                (context, &reason.event_id, &reason.annotation_id, index),
                |ui| {
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = theme::SPACE_1;
                        let heading = feedback_heading(reason);
                        let response =
                            ui.add(egui::Label::new(egui::RichText::new(&heading).strong()).wrap());
                        if index == 0 {
                            ui.ctx().accesskit_node_builder(response.id, |node| {
                                node.set_role(egui::accesskit::Role::Status);
                                node.set_label(heading);
                                node.set_live(egui::accesskit::Live::Polite);
                            });
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
                    });
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
                    if reason.superseded {
                        ui.weak("Replaced by a later update");
                    }
                    let name = entry
                        .author
                        .as_ref()
                        .and_then(|author| author.github_login.as_deref())
                        .filter(|login| !login.trim().is_empty())
                        .map(|login| format!("@{login}"))
                        .unwrap_or_else(|| "Unknown author".into());
                    let github_id = entry
                        .author
                        .as_ref()
                        .and_then(|author| author.github_user_id.as_deref());
                    let mut details =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            ui.make_persistent_id("audit-details"),
                            false,
                        );
                    ui.scope(|ui| {
                        ui.spacing_mut().interact_size.y = 24.0;
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let icon_id = ui.id().with("audit-details-icon");
                                    let button = egui::Button::new(egui::Atoms::new((
                                        egui::Atom::custom(icon_id, egui::Vec2::splat(12.0)),
                                        egui::RichText::new("Additional info").small(),
                                    )))
                                    .small()
                                    .frame_when_inactive(false)
                                    .min_size(egui::vec2(0.0, 24.0))
                                    .atom_ui(ui);
                                    let icon_rect = button.rect(icon_id);
                                    let response = button.response;
                                    if let Some(rect) = icon_rect {
                                        egui::collapsing_header::paint_default_icon(
                                            ui,
                                            details.openness(ui.ctx()),
                                            &response.clone().with_new_rect(rect),
                                        );
                                    }
                                    if response.clicked() {
                                        details.toggle(ui);
                                    }
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::CollapsingHeader,
                                            ui.is_enabled(),
                                            "Additional info",
                                        )
                                    });
                                    ui.ctx().accesskit_node_builder(response.id, |node| {
                                        node.set_expanded(details.is_open());
                                    });
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let (rect, _) = ui.allocate_exact_size(
                                                egui::Vec2::splat(24.0),
                                                egui::Sense::hover(),
                                            );
                                            crate::avatar::paint(ui, github_id, &name, rect);
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&name)
                                                        .small()
                                                        .color(theme::TEXT_MUTED),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(&name);
                                        },
                                    );
                                },
                            );
                        });
                    });
                    details.show_body_unindented(ui, |ui| {
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
                                        .monospace(),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        }
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(
                                    reason.timestamp.format("%Y-%m-%d %H:%M UTC").to_string(),
                                )
                                .small()
                                .color(theme::TEXT_MUTED),
                            )
                            .wrap(),
                        );
                    });
                },
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
