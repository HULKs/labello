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
    Load(labello_client::PrelabelSuggestionRequest),
    Check(labello_client::PrelabelSuggestionRequest),
    Admin(Option<PrelabelAdminCommand>),
}
#[derive(Debug)]
pub(crate) enum PrelabelReply {
    Hints(Box<PrelabelResponse>),
    Generation(PrelabelGeneration),
    Admin(PrelabelAdminState),
}
type HintKey = (ImageId, TaskId, PrelabelConfigId);
#[derive(Default)]
pub(crate) struct PrelabelWorkState {
    pub choices: BTreeMap<String, Option<PrelabelConfigId>>,
    pub pending: Option<(u64, PrelabelAction)>,
    pub hints: BTreeMap<HintKey, HintStatus>,
    pub abort: Option<futures::future::AbortHandle>,
}
pub(crate) struct HintStatus {
    pub generation: Option<PrelabelGeneration>,
    pub error: Option<String>,
    pub from_batch: bool,
    pub execution: Option<PrelabelExecutionKind>,
    pub checked_at: Instant,
}
#[derive(Default)]
pub(crate) struct PrelabelAdminUi {
    pub state: Option<PrelabelAdminState>,
    pub pending: Option<(u64, PrelabelAction)>,
    pub error: Option<String>,
    pub last_poll: Option<Instant>,
    pub mappings: BTreeMap<TaskId, PrelabelConfigId>,
    pub reset_scope: PrelabelScope,
    pub confirm_reset: bool,
}

impl LabelloApp {
    fn prelabel_choice_key(&self, task: &TaskId) -> String {
        format!("{}/{task}", self.config.dataset_id)
    }

    pub(crate) fn prelabel_choice(&self, task: &TaskId) -> Option<PrelabelConfigId> {
        let metadata = self.datasets.metadata.as_ref()?;
        let task = metadata.task(task)?;
        let available = |config: &&PrelabelConfig| {
            config.available_to_annotators && config.validate_for_task(task).is_ok()
        };
        if let Some(choice) = self
            .work
            .prelabels
            .choices
            .get(&self.prelabel_choice_key(&task.task_id))
        {
            return choice
                .as_ref()
                .filter(|id| {
                    metadata
                        .prelabel_configs
                        .iter()
                        .filter(available)
                        .any(|c| &c.config_id == *id)
                })
                .cloned();
        }
        metadata
            .prelabel_configs
            .iter()
            .filter(available)
            .map(|c| c.config_id.clone())
            .next()
    }

    pub(crate) fn refresh_prelabels_if_due(&mut self, ctx: &egui::Context) {
        if self.runtime.api.is_none() {
            return;
        }
        if self.view == AppView::Admin && self.admin.section == crate::app::AdminSection::Automation
        {
            let state = &self.admin.prelabels;
            if state.pending.is_none()
                && state
                    .last_poll
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(3))
            {
                self.request_prelabels(PrelabelAction::Admin(None));
            }
            ctx.request_repaint_after(Duration::from_secs(3));
        }
        if self.view != AppView::Annotate {
            self.cancel_prelabel_load();
            return;
        }
        let Some(task) = self.selected_task().map(|task| task.task_id.clone()) else {
            return;
        };
        let Some(config) = self.prelabel_choice(&task) else {
            return;
        };
        let mut images = Vec::new();
        if let Some(current) = &self.work.current {
            images.push(current.image.image_id.clone());
        }
        images.extend(self.work.queue.prepared_image_ids());
        self.work
            .prelabels
            .hints
            .retain(|(image, _, _), _| images.contains(image));
        if let Some((_, PrelabelAction::Load(query) | PrelabelAction::Check(query))) =
            &self.work.prelabels.pending
        {
            let current_needs_hints = images.first().is_some_and(|image| {
                !self.work.prelabels.hints.contains_key(&(
                    image.clone(),
                    task.clone(),
                    config.clone(),
                ))
            });
            if query.task_id != task
                || query.config_id != config
                || !images.contains(&query.image_id)
                || (current_needs_hints && images.first() != Some(&query.image_id))
            {
                self.cancel_prelabel_load();
            }
        }
        if self.work.prelabels.pending.is_some() {
            return;
        }
        for image in &images {
            let key = (image.clone(), task.clone(), config.clone());
            if !self.work.prelabels.hints.contains_key(&key) {
                self.request_prelabels(PrelabelAction::Load(
                    labello_client::PrelabelSuggestionRequest {
                        image_id: image.clone(),
                        task_id: task.clone(),
                        config_id: config.clone(),
                    },
                ));
                return;
            }
        }
        if let Some(image) = images.first()
            && let Some(status) =
                self.work
                    .prelabels
                    .hints
                    .get(&(image.clone(), task.clone(), config.clone()))
            && status.checked_at.elapsed() >= Duration::from_secs(2)
        {
            self.request_prelabels(PrelabelAction::Check(
                labello_client::PrelabelSuggestionRequest {
                    image_id: image.clone(),
                    task_id: task,
                    config_id: config,
                },
            ));
        }
        ctx.request_repaint_after(Duration::from_secs(2));
    }

    pub(crate) fn request_prelabels(&mut self, action: PrelabelAction) {
        if self.runtime.api.is_none() {
            return;
        }
        let pending = if matches!(action, PrelabelAction::Admin(_)) {
            &self.admin.prelabels.pending
        } else {
            &self.work.prelabels.pending
        };
        if pending.is_some() {
            return;
        }
        let request = self.request_identity(Some(self.config.dataset_id.clone()));
        if matches!(action, PrelabelAction::Admin(_)) {
            self.admin.prelabels.pending = Some((request.request_id, action.clone()));
            self.admin.prelabels.last_poll = Some(Instant::now());
        } else {
            self.work.prelabels.pending = Some((request.request_id, action.clone()));
        }
        self.queue_command(UiCommand::Prelabel {
            request,
            dataset_id: self.config.dataset_id.clone(),
            action,
        });
    }

    pub(crate) fn cancel_prelabel_load(&mut self) {
        if let Some(abort) = self.work.prelabels.abort.take() {
            abort.abort();
        }
        if let Some((request, _)) = self.work.prelabels.pending.take() {
            self.runtime.active_requests.remove(&request);
        }
    }

    pub(crate) fn prelabel_selector(&mut self, ui: &mut egui::Ui) {
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let configs: Vec<_> = self
            .datasets
            .metadata
            .as_ref()
            .map(|metadata| {
                metadata
                    .prelabel_configs
                    .iter()
                    .filter(|c| c.available_to_annotators && c.validate_for_task(&task).is_ok())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if configs.is_empty() {
            return;
        }
        let mut choice = self.prelabel_choice(&task.task_id);
        let before = choice.clone();
        egui::ComboBox::from_id_salt("prelabel_model")
            .truncate()
            .selected_text(
                choice
                    .as_ref()
                    .and_then(|id| configs.iter().find(|c| &c.config_id == id))
                    .map_or("No prelabels", |c| c.name.as_str()),
            )
            .width(ui.available_width().max(80.0))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, None, "No prelabels");
                for config in &configs {
                    ui.selectable_value(&mut choice, Some(config.config_id.clone()), &config.name);
                }
            });
        if before != choice {
            self.cancel_prelabel_load();
            self.work
                .prelabels
                .choices
                .insert(self.prelabel_choice_key(&task.task_id), choice.clone());
            self.work.prelabels.hints.clear();
            if let Some(current) = &mut self.work.current {
                current.prelabels.clear();
            }
            self.work.queue.clear_prelabels();
            self.persist_workspace_preference();
        }
        if let Some(config) = choice
            && let Some(current) = &self.work.current
        {
            let key = (current.image.image_id.clone(), task.task_id.clone(), config);
            if let Some(status) = self.work.prelabels.hints.get(&key) {
                if let Some(error) = &status.error {
                    ui.label(error);
                }
                if status
                    .generation
                    .as_ref()
                    .is_some_and(|generation| generation.paused)
                {
                    ui.label("Hints were removed. A dataset administrator can resume generation.");
                }
                if status.from_batch {
                    ui.label("Using dataset hints");
                }
                match status.execution {
                    Some(PrelabelExecutionKind::BrowserWebGpu) => {
                        ui.label("Generated in your browser using WebGPU");
                    }
                    Some(PrelabelExecutionKind::BrowserCpu) => {
                        let fallback =
                            configs
                                .iter()
                                .find(|c| c.config_id == key.2)
                                .is_some_and(|c| {
                                    matches!(
                                        c.execution,
                                        PrelabelExecution::BrowserLocal {
                                            acceleration: BrowserAcceleration::WebGpuPreferred
                                        }
                                    )
                                });
                        ui.label(if fallback {
                            "WebGPU unavailable for this run; used browser CPU fallback"
                        } else {
                            "Generated in your browser using CPU"
                        });
                    }
                    _ => {}
                }
                if ui
                    .add_enabled(
                        self.work.prelabels.pending.is_none(),
                        egui::Button::new("Refresh hints"),
                    )
                    .clicked()
                {
                    self.work.prelabels.hints.remove(&key);
                }
            } else {
                ui.label("Preparing hints… You can annotate while they load.");
            }
        }
    }

    pub(crate) fn prelabel_admin_panel(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Dataset hints");
        ui.label("Generate hints for all remaining box workflows. Save model and workflow changes before starting.");
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
        ui.label("Remove hints");
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
        ui.label("Removal cancels affected runs and pauses new hints. Annotations and drafts are preserved.");
        ui.checkbox(
            &mut self.admin.prelabels.confirm_reset,
            "Confirm hint removal",
        );
        ui.horizontal_wrapped(|ui| {
            if theme::danger_button(
                ui,
                !busy && self.admin.prelabels.confirm_reset,
                egui::Button::new("Remove hints and pause"),
            )
            .clicked()
            {
                self.admin.prelabels.confirm_reset = false;
                self.request_prelabels(PrelabelAction::Admin(Some(PrelabelAdminCommand::Reset {
                    scope: self.admin.prelabels.reset_scope.clone(),
                })));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Resume hints in this scope"))
                .clicked()
            {
                self.request_prelabels(PrelabelAction::Admin(Some(PrelabelAdminCommand::Resume {
                    scope: self.admin.prelabels.reset_scope.clone(),
                })));
            }
        });
    }
}
