impl LabelloApp {
    fn reduce_export_message(
        &mut self,
        ctx: &egui::Context,
        message: UiMessage,
    ) -> Option<UiMessage> {
        use crate::export_flow::{ExportAction, ExportReply};
        let (request, result) = match message {
            UiMessage::ExportFinished { request, result } => (request, *result),
            UiMessage::RequestFailed { request, error }
                if self
                    .admin
                    .export
                    .pending
                    .as_ref()
                    .is_some_and(|(id, _)| *id == request.request_id) =>
            {
                (request, Err(error))
            }
            message => return Some(message),
        };
        let state = &mut self.admin.export;
        if !state
            .pending
            .as_ref()
            .is_some_and(|(id, _)| *id == request.request_id)
        {
            return None;
        }
        if let Err(error) = result {
            state.request_failed(error);
            return None;
        }
        let (_, action) = state.pending.take().expect("checked export request");
        state.error = None;
        state.retry = None;
        match result.expect("handled export failure") {
            ExportReply::Loaded {
                capabilities,
                mut jobs,
            } => {
                jobs.retain(|job| job.dataset_id == self.config.dataset_id);
                jobs.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| b.job_id.cmp(&a.job_id))
                });
                let restore = !state.loaded;
                state.capabilities = Some(capabilities);
                state.jobs = jobs;
                state.loaded = true;
                if restore {
                    if let Some(id) = state.jobs.first().map(|job| job.job_id.clone()) {
                        state.select_job(&id);
                    }
                } else if !state
                    .jobs
                    .iter()
                    .any(|job| Some(&job.job_id) == state.selected.as_ref())
                {
                    state.selected = None;
                    state.reviewed = false;
                }
            }
            ExportReply::Job(job) => {
                let job = *job;
                if job.dataset_id != self.config.dataset_id {
                    return None;
                }
                if matches!(action, ExportAction::Preflight(_)) {
                    state.selected = Some(job.job_id.clone());
                    state.reviewed = false;
                }
                if let Some(existing) = state.jobs.iter_mut().find(|old| old.job_id == job.job_id) {
                    *existing = job;
                } else {
                    state.jobs.insert(0, job);
                }
            }
            ExportReply::Download(url) => {
                ctx.open_url(egui::OpenUrl::same_tab(url));
                state.notice = Some(
                    "Download requested. Check your browser's downloads for transfer status."
                        .into(),
                );
            }
        }
        None
    }
}
