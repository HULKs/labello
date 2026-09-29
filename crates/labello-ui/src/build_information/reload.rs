/// Browser-owned asset refresh and navigation. Preparation must not navigate;
/// the shared owner rechecks current drafts after the asynchronous operation.
pub type BuildReloadPreparer = Rc<dyn Fn(String, bool) -> Pin<Box<dyn Future<Output = Result<(), String>>>>>;

pub struct BuildReloadAdapter {
    pub prepare: BuildReloadPreparer,
    pub navigate: Rc<dyn Fn() -> Result<(), String>>,
}

#[derive(Default)]
pub(crate) struct BuildReloadState {
    pub(crate) adapter: Option<Rc<BuildReloadAdapter>>,
    pub(crate) phase: ReloadPhase,
    generation: u64,
    pub(crate) target: Option<BuildIdentity>,
    manual: bool,
    waiting: Option<&'static str>,
}

#[derive(Default)]
pub(crate) enum ReloadPhase {
    #[default]
    Idle,
    Preparing,
    Prepared,
    Navigating,
    Failed(String),
}

impl LabelloApp {
    pub fn set_build_reload_adapter(&mut self, adapter: BuildReloadAdapter) {
        self.builds.reload.adapter = Some(Rc::new(adapter));
    }

    pub(crate) fn reset_build_reload(&mut self) {
        let reload = &mut self.builds.reload;
        reload.generation = reload.generation.wrapping_add(1);
        reload.phase = ReloadPhase::Idle;
        reload.target = None;
        reload.waiting = None;
        reload.manual = false;
    }

    pub(crate) fn advance_build_reload(&mut self, ctx: &egui::Context) {
        let Some(adapter) = self.builds.reload.adapter.clone() else { return };
        // Retained identity is display data until the refresh validates it.
        if self.builds.loading { return; }
        if !self.builds_differ() {
            self.reset_build_reload();
            return;
        }
        if self.builds.reload.target != self.builds.server {
            self.reset_build_reload();
            self.builds.reload.target = self.builds.server.clone();
        }
        if matches!(self.builds.reload.phase, ReloadPhase::Failed(_) | ReloadPhase::Navigating) {
            return;
        }
        self.builds.reload.waiting = self.build_reload_blocker();
        if self.builds.reload.waiting.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        match self.builds.reload.phase {
            ReloadPhase::Idle => {
                let key = serde_json::to_string(&(&self.config.api_base_url, &self.builds.server))
                    .expect("build identity is serializable");
                let operation = (adapter.prepare)(key, self.builds.reload.manual);
                self.builds.reload.manual = false;
                self.builds.reload.phase = ReloadPhase::Preparing;
                let generation = self.builds.reload.generation;
                let request = self.request_identity(None);
                self.spawn_message(request, async move {
                    UiMessage::BuildReloadPrepared { generation, result: operation.await }
                });
            }
            ReloadPhase::Prepared => {
                crate::missing_objects::browser_unload_guard(false);
                self.builds.reload.phase = match (adapter.navigate)() {
                    Ok(()) => ReloadPhase::Navigating,
                    Err(message) => ReloadPhase::Failed(message),
                };
            }
            _ => {}
        }
    }

    fn build_reload_blocker(&self) -> Option<&'static str> {
        if self.migration_has_unsaved_input()
            || !self.work.staged_review_decisions.is_empty()
            || self.inspection_has_reason()
            || self.import.open
            || self.work.shortcut_settings.draft != self.work.shortcut_settings.baseline
            || self.datasets.users != self.datasets.users_baseline
            || self.auth.recovery.is_some()
            || (self.view == AppView::Setup && self.setup.section == SetupSection::Create)
            || self.setup.api_base_url_draft != self.config.api_base_url
        {
            return Some("Update waiting. Finish or discard the current staged operation first.");
        }
        if self.loading.saving || self.loading.image || self.loading.dataset
            || self.loading.admin || self.loading.session || self.loading.logout
            || self.loading.uploading || self.loading.ingesting || self.loading.creating_snapshot
            || self.work.migration.busy || self.work.canvas.is_dragging()
            || self.work.pending_transition.is_some() || self.import.busy
            || !self.runtime.active_requests.is_empty() || !self.runtime.commands.is_empty()
        {
            return Some("Update waiting for the current operation to finish.");
        }
        if !self.browser_drafts_ready_for_reload() {
            return Some("Update waiting for browser storage. Keep this tab open until your draft is saved.");
        }
        None
    }

    fn retry_build_reload(&mut self) {
        self.reset_build_reload();
        self.builds.reload.target = self.builds.server.clone();
        self.builds.reload.manual = true;
        self.request_build_information();
    }

    fn build_reload_controls(&mut self, ui: &mut egui::Ui) {
        if self.builds.reload.adapter.is_none() || !self.builds_differ() { return; }
        let message = match &self.builds.reload.phase {
            ReloadPhase::Failed(message) => message.as_str(),
            _ => self.builds.reload.waiting.unwrap_or("Updating Labello. Your saved browser drafts will be kept."),
        };
        ui.add_space(theme::SPACE_3);
        let response = ui.label(message);
        ui.ctx().accesskit_node_builder(response.id, |node| node.set_live(egui::accesskit::Live::Polite));
        if matches!(self.builds.reload.phase, ReloadPhase::Failed(_))
            && focus_action(theme::quiet_button(ui, true,
                egui::Button::new("Retry app update").min_size(egui::vec2(44.0, 44.0))))
        {
            self.retry_build_reload();
        }
    }
}

