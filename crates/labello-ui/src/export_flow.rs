use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;
use labello_client::{ExportCapabilities, ExportJob, ExportPhase};
use labello_domain::{ExportOptions, ExportProfile, ExportSplit};
use web_time::{Duration, Instant};

use crate::{
    LabelloApp,
    app::{AdminSection, AppView, UiCommand},
    theme,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExportAction {
    Load,
    Preflight(ExportOptions),
    Poll(String),
    Start(String),
    Cancel(String),
    Download(String),
}

#[derive(Debug)]
pub(crate) enum ExportReply {
    Loaded {
        capabilities: ExportCapabilities,
        jobs: Vec<ExportJob>,
    },
    Job(Box<ExportJob>),
    Download(String),
}

pub(crate) struct ExportState {
    pub options: ExportOptions,
    pub capabilities: Option<ExportCapabilities>,
    pub jobs: Vec<ExportJob>,
    pub loaded: bool,
    pub selected: Option<String>,
    pub reviewed: bool,
    pub pending: Option<(u64, ExportAction)>,
    pub retry: Option<ExportAction>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub last_poll: Option<Instant>,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            options: ExportOptions {
                profile: ExportProfile::UltralyticsYoloDetectV1,
                classes: BTreeSet::new(),
                fallback_split: ExportSplit::Train,
                split_choices: BTreeMap::new(),
            },
            capabilities: None,
            jobs: Vec::new(),
            loaded: false,
            selected: None,
            reviewed: false,
            pending: None,
            retry: None,
            error: None,
            notice: None,
            last_poll: None,
        }
    }
}

impl ExportState {
    pub fn selected_job(&self) -> Option<&ExportJob> {
        self.jobs
            .iter()
            .find(|job| Some(&job.job_id) == self.selected.as_ref())
    }

    pub fn retained_capture(&self) -> Option<&ExportJob> {
        self.jobs.iter().find(|job| {
            matches!(
                job.phase,
                ExportPhase::Capturing
                    | ExportPhase::Ready
                    | ExportPhase::Blocked
                    | ExportPhase::Building
                    | ExportPhase::Cancelling
            )
        })
    }

    pub fn request_failed(&mut self, error: String) {
        self.retry = self.pending.take().map(|(_, action)| match action {
            ExportAction::Preflight(_) | ExportAction::Start(_) | ExportAction::Cancel(_) => {
                ExportAction::Load
            }
            action => action,
        });
        self.error = Some(error);
        self.notice = None;
    }

    pub fn select_job(&mut self, id: &str) {
        if let Some(job) = self.jobs.iter().find(|job| job.job_id == id) {
            self.options = job.options.clone();
            self.selected = Some(id.to_owned());
            self.reviewed = false;
            self.notice = None;
        }
    }
}

impl LabelloApp {
    fn export_visible(&self) -> bool {
        self.view == AppView::Admin
            && self.admin.section == AdminSection::Export
            && self
                .datasets
                .admin_baseline
                .as_ref()
                .is_some_and(|m| m.dataset_id == self.config.dataset_id)
    }

    fn export_config_saved(&self) -> bool {
        self.datasets.admin_config == self.datasets.admin_baseline
            && self.datasets.users == self.datasets.users_baseline
    }

    pub(crate) fn refresh_export_if_due(&mut self, ctx: &egui::Context) {
        if !self.export_visible()
            || self.runtime.api.is_none()
            || self.admin.export.pending.is_some()
        {
            return;
        }
        let state = &self.admin.export;
        if !state.loaded && state.error.is_none() {
            self.request_export(ExportAction::Load);
            return;
        }
        if state.error.is_some() {
            return;
        }
        if let Some(job) = state.jobs.iter().find(|job| job.phase.is_active()) {
            let remaining = state
                .last_poll
                .map(|last| Duration::from_secs(1).saturating_sub(last.elapsed()))
                .unwrap_or_default();
            if remaining.is_zero() {
                self.request_export(ExportAction::Poll(job.job_id.clone()));
            } else {
                ctx.request_repaint_after(remaining);
            }
        }
    }

    pub(crate) fn request_export(&mut self, action: ExportAction) {
        if self.runtime.api.is_none()
            || self.admin.export.pending.is_some()
            || !self.export_visible()
        {
            return;
        }
        let state = &self.admin.export;
        let enabled = state.capabilities.as_ref().is_some_and(|c| c.available);
        match &action {
            ExportAction::Preflight(options) => {
                let valid = self
                    .datasets
                    .admin_baseline
                    .as_ref()
                    .is_some_and(|metadata| options.class_mapping(metadata).is_ok());
                if !enabled
                    || !self.export_config_saved()
                    || !valid
                    || state.retained_capture().is_some()
                {
                    return;
                }
            }
            ExportAction::Start(id)
                if !enabled
                    || !self.export_config_saved()
                    || !state.reviewed
                    || !state.selected_job().is_some_and(|job| {
                        &job.job_id == id
                            && job.phase == ExportPhase::Ready
                            && job.options == state.options
                            && job
                                .summary
                                .as_ref()
                                .is_some_and(|summary| summary.can_start())
                    }) =>
            {
                return;
            }
            _ => {}
        }
        let dataset_id = self.config.dataset_id.clone();
        let request = self.request_identity(Some(dataset_id.clone()));
        self.admin.export.pending = Some((request.request_id, action.clone()));
        self.admin.export.last_poll = Some(Instant::now());
        self.admin.export.notice = None;
        self.queue_command(UiCommand::Export {
            request,
            dataset_id,
            action,
        });
    }
}

include!("export_flow/views.rs");
