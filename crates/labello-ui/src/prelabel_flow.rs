use crate::{
    LabelloApp,
    app::{AppView, UiCommand},
    theme,
};
use eframe::egui;
use labello_domain::*;
use std::collections::BTreeMap;
use web_time::{Duration, Instant};

#[derive(Clone, Debug)]
pub(crate) enum PrelabelAction {
    Admin(Option<PrelabelAdminCommand>),
    InspectModel {
        config_id: PrelabelConfigId,
        location: String,
    },
}
#[derive(Debug)]
pub(crate) enum PrelabelReply {
    Admin(PrelabelAdminState),
    Model(PrelabelModelInspection),
}
#[derive(Default)]
pub(crate) struct PrelabelAdminUi {
    pub model_checks: BTreeMap<PrelabelConfigId, ModelCheckUi>,
    pub state: Option<PrelabelAdminState>,
    pub pending: Option<(u64, PrelabelAction)>,
    pub error: Option<String>,
    pub last_poll: Option<Instant>,
    pub mappings: BTreeMap<TaskId, PrelabelConfigId>,
    pub reset_scope: PrelabelScope,
    pub confirm_reset: bool,
}

#[derive(Default)]
pub(crate) struct ModelCheckUi {
    pub location: String,
    pub pending: Option<u64>,
    pub result: Option<Result<PrelabelModelInspection, String>>,
}

impl LabelloApp {
    pub(crate) fn refresh_prelabels_if_due(&mut self, ctx: &egui::Context) {
        if self.runtime.api.is_none()
            || !self.auth.prelabel_available
            || self.view != AppView::Admin
            || self.admin.section != crate::app::AdminSection::Automation
        {
            return;
        }
        if self.admin.prelabels.pending.is_none()
            && self
                .admin
                .prelabels
                .last_poll
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(3))
        {
            self.request_prelabels(PrelabelAction::Admin(None));
        }
        ctx.request_repaint_after(Duration::from_secs(3));
    }

    pub(crate) fn request_prelabels(&mut self, action: PrelabelAction) {
        if self.runtime.api.is_none() || !self.auth.prelabel_available {
            return;
        }
        if let PrelabelAction::InspectModel {
            config_id,
            location,
        } = &action
        {
            if self
                .admin
                .prelabels
                .model_checks
                .get(config_id)
                .is_some_and(|check| check.pending.is_some())
            {
                return;
            }
            let request = self.request_identity(Some(self.config.dataset_id.clone()));
            self.admin.prelabels.model_checks.insert(
                config_id.clone(),
                ModelCheckUi {
                    location: location.clone(),
                    pending: Some(request.request_id),
                    result: None,
                },
            );
            self.queue_command(UiCommand::Prelabel {
                request,
                dataset_id: self.config.dataset_id.clone(),
                action,
            });
            return;
        }
        if self.admin.prelabels.pending.is_some() {
            return;
        }
        let request = self.request_identity(Some(self.config.dataset_id.clone()));
        self.admin.prelabels.pending = Some((request.request_id, action.clone()));
        self.admin.prelabels.last_poll = Some(Instant::now());
        self.queue_command(UiCommand::Prelabel {
            request,
            dataset_id: self.config.dataset_id.clone(),
            action,
        });
    }

    pub(crate) fn prelabel_admin_panel(&mut self, ui: &mut egui::Ui) {
        if !self.auth.prelabel_available {
            return;
        }
        ui.separator();
        ui.heading(crate::glossary::DATASET_PRELABELS);
        ui.label("Generate prelabels for all remaining box workflows. Save model and workflow changes before starting.");
        let busy = self.admin.prelabels.pending.is_some();
        if let Some(error) = &self.admin.prelabels.error {
            ui.colored_label(theme::DANGER, error);
        }
        if let Some(metadata) = &self.datasets.admin_baseline {
            for task in metadata
                .tasks
                .iter()
                .filter(|t| t.enabled && t.annotation_type == AnnotationType::BoundingBox)
            {
                let models: Vec<_> = metadata
                    .prelabel_configs
                    .iter()
                    .filter(|c| {
                        c.validate_for_task(task).is_ok()
                            && matches!(c.execution, PrelabelExecution::ServerSide { .. })
                    })
                    .collect();
                let mut selected = self.admin.prelabels.mappings.get(&task.task_id).cloned();
                egui::ComboBox::from_id_salt(("batch_model", &task.task_id))
                    .width(ui.available_width().min(420.0))
                    .truncate()
                    .selected_text(
                        selected
                            .as_ref()
                            .and_then(|id| models.iter().find(|c| &c.config_id == id))
                            .map_or("Automatic (one compatible model)", |c| c.name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut selected,
                            None,
                            "Automatic (one compatible model)",
                        );
                        for config in models {
                            ui.selectable_value(
                                &mut selected,
                                Some(config.config_id.clone()),
                                &config.name,
                            );
                        }
                    })
                    .response
                    .on_hover_text(&task.name);
                ui.label(&task.name);
                if let Some(id) = selected {
                    self.admin
                        .prelabels
                        .mappings
                        .insert(task.task_id.clone(), id);
                } else {
                    self.admin.prelabels.mappings.remove(&task.task_id);
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("Check remaining box workflows"))
                .clicked()
            {
                self.request_prelabels(PrelabelAction::Admin(Some(
                    PrelabelAdminCommand::Preflight {
                        mappings: self.admin.prelabels.mappings.clone(),
                    },
                )));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Refresh runs"))
                .clicked()
            {
                self.request_prelabels(PrelabelAction::Admin(None));
            }
        });
        if let Some(state) = self.admin.prelabels.state.clone() {
            ui.label(format!(
                "{} image/workflow results retained",
                state.retained_results
            ));
            for run in state.runs {
                ui.group(|ui| {
                    ui.label(format!(
                        "{:?} · {} of {} processed",
                        run.phase,
                        run.total - run.pending,
                        run.total
                    ));
                    ui.label(format!(
                        "{} reusable · {} ineligible pairs omitted at preflight",
                        run.reusable, run.ineligible
                    ));
                    ui.label(format!(
                        "{} generated · {} empty · {} skipped · {} failed",
                        run.generated, run.empty, run.skipped, run.failed
                    ));
                    for blocker in &run.blockers {
                        ui.label(blocker);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if run.phase == PrelabelRunPhase::Ready
                            && ui
                                .add_enabled(
                                    !busy && run.blockers.is_empty(),
                                    egui::Button::new("Start generation"),
                                )
                                .clicked()
                        {
                            self.request_prelabels(PrelabelAction::Admin(Some(
                                PrelabelAdminCommand::Start {
                                    run_id: run.run_id.clone(),
                                },
                            )));
                        }
                        if run.phase == PrelabelRunPhase::Running
                            && ui
                                .add_enabled(!busy, egui::Button::new("Cancel generation"))
                                .clicked()
                        {
                            self.request_prelabels(PrelabelAction::Admin(Some(
                                PrelabelAdminCommand::Cancel {
                                    run_id: run.run_id.clone(),
                                },
                            )));
                        }
                        if (matches!(
                            run.phase,
                            PrelabelRunPhase::Cancelled | PrelabelRunPhase::Interrupted
                        ) || run.failed > 0)
                            && ui
                                .add_enabled(!busy, egui::Button::new("Retry remaining items"))
                                .clicked()
                        {
                            self.request_prelabels(PrelabelAction::Admin(Some(
                                PrelabelAdminCommand::Retry {
                                    run_id: run.run_id.clone(),
                                },
                            )));
                        }
                    });
                });
            }
        }
        ui.separator();
        ui.label("Remove prelabels");
        let previous_scope = self.admin.prelabels.reset_scope.clone();
        if let Some(metadata) = &self.datasets.admin_baseline {
            egui::ComboBox::from_id_salt("reset_workflow")
                .width(ui.available_width().min(420.0))
                .truncate()
                .selected_text(
                    self.admin
                        .prelabels
                        .reset_scope
                        .task_id
                        .as_ref()
                        .map_or("All workflows", |id| id.as_str()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.admin.prelabels.reset_scope.task_id,
                        None,
                        "All workflows",
                    );
                    for task in &metadata.tasks {
                        ui.selectable_value(
                            &mut self.admin.prelabels.reset_scope.task_id,
                            Some(task.task_id.clone()),
                            &task.name,
                        );
                    }
                });
            egui::ComboBox::from_id_salt("reset_model")
                .width(ui.available_width().min(420.0))
                .truncate()
                .selected_text(
                    self.admin
                        .prelabels
                        .reset_scope
                        .config_id
                        .as_ref()
                        .map_or("All models", |id| id.as_str()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.admin.prelabels.reset_scope.config_id,
                        None,
                        "All models",
                    );
                    for config in &metadata.prelabel_configs {
                        ui.selectable_value(
                            &mut self.admin.prelabels.reset_scope.config_id,
                            Some(config.config_id.clone()),
                            &config.name,
                        );
                    }
                });
        }
        if previous_scope != self.admin.prelabels.reset_scope {
            self.admin.prelabels.confirm_reset = false;
        }
        ui.label("Removal cancels affected runs and pauses new prelabels. Annotations and drafts are preserved.");
        ui.checkbox(
            &mut self.admin.prelabels.confirm_reset,
            "Confirm prelabel removal",
        );
        ui.horizontal_wrapped(|ui| {
            if theme::danger_button(
                ui,
                !busy && self.admin.prelabels.confirm_reset,
                egui::Button::new(crate::glossary::REMOVE_PRELABELS_AND_PAUSE),
            )
            .clicked()
            {
                self.admin.prelabels.confirm_reset = false;
                self.request_prelabels(PrelabelAction::Admin(Some(PrelabelAdminCommand::Reset {
                    scope: self.admin.prelabels.reset_scope.clone(),
                })));
            }
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(crate::glossary::RESUME_PRELABELS_IN_THIS_SCOPE),
                )
                .clicked()
            {
                self.request_prelabels(PrelabelAction::Admin(Some(PrelabelAdminCommand::Resume {
                    scope: self.admin.prelabels.reset_scope.clone(),
                })));
            }
        });
    }
}

pub(crate) fn disabled_notice(ui: &mut egui::Ui) {
    ui.label("Prelabeling is disabled by server configuration.");
}
