impl LabelloApp {
    fn dispatch_prelabel_command(
        &mut self,
        api: Rc<dyn labello_client::LabelloApi>,
        command: UiCommand,
    ) -> Option<UiCommand> {
        let UiCommand::Prelabel {
            request,
            dataset_id,
            action,
        } = command
        else {
            return Some(command);
        };
        use crate::prelabel_flow::{PrelabelAction, PrelabelReply};
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        if matches!(action, PrelabelAction::Load(_) | PrelabelAction::Check(_) | PrelabelAction::Retained(_)) {
            self.work.prelabels.abort = Some(abort);
        }
        self.spawn_message(request.clone(), async move {
            let future = async {
                match action {
                    PrelabelAction::InspectModel { location, .. } => api
                        .inspect_prelabel_model(&dataset_id, labello_client::PrelabelModelCheckRequest { location })
                        .await.map(PrelabelReply::Model),
                    PrelabelAction::Retained(query) => api.retained_prelabels(&dataset_id, query).await
                        .map(|retained| PrelabelReply::Retained(retained.map(Box::new))),
                    PrelabelAction::Load(query) => api
                        .prelabel_suggestions(&dataset_id, query)
                        .await
                        .map(|response| PrelabelReply::Hints(Box::new(response))),
                    PrelabelAction::Check(query) => api
                        .prelabel_generation(&dataset_id, query)
                        .await
                        .map(PrelabelReply::Generation),
                    PrelabelAction::Admin(None) => api
                        .prelabel_admin_state(&dataset_id)
                        .await
                        .map(PrelabelReply::Admin),
                    PrelabelAction::Admin(Some(command)) => api
                        .prelabel_admin_command(&dataset_id, command)
                        .await
                        .map(PrelabelReply::Admin),
                }
                .map_err(|error| error.to_string())
            };
            let result = futures::future::Abortable::new(future, registration)
                .await
                .unwrap_or_else(|_| Err("Hint request cancelled".into()));
            UiMessage::PrelabelFinished {
                request,
                result: Box::new(result),
            }
        });
        None
    }

    fn reduce_prelabel_message(&mut self, message: UiMessage) -> Option<UiMessage> {
        use crate::prelabel_flow::{HintStatus, PrelabelAction, PrelabelReply};
        let (request, result) = match message {
            UiMessage::PrelabelFinished { request, result } => (request, *result),
            UiMessage::RequestFailed { request, error }
                if self
                    .admin
                    .prelabels
                    .pending
                    .as_ref()
                    .is_some_and(|(id, _)| *id == request.request_id)
                    || self.admin.prelabels.model_checks.values().any(|check| check.pending == Some(request.request_id))
                    || self
                        .work
                        .prelabels
                        .pending
                        .as_ref()
                        .is_some_and(|(id, _)| *id == request.request_id) =>
            {
                (request, Err(error))
            }
            other => return Some(other),
        };
        if let Some((id, check)) = self.admin.prelabels.model_checks.iter_mut()
            .find(|(_, check)| check.pending == Some(request.request_id))
        {
            check.pending = None;
            let current = self.datasets.admin_config.as_ref().and_then(|dataset|
                dataset.prelabel_configs.iter().find(|config| &config.config_id == id));
            if current.is_some_and(|config| config.model.location == check.location) {
                check.result = Some(match result {
                    Ok(PrelabelReply::Model(model)) => Ok(model),
                    Err(error) => Err(error),
                    _ => Err("Unexpected model check response".into()),
                });
            }
            return None;
        }
        if self
            .admin
            .prelabels
            .pending
            .as_ref()
            .is_some_and(|(id, _)| *id == request.request_id)
        {
            let (_, action) = self.admin.prelabels.pending.take().unwrap();
            self.admin.prelabels.error = None;
            match result {
                Ok(PrelabelReply::Admin(state)) => {
                    self.admin.prelabels.state = Some(state);
                    if matches!(
                        action,
                        PrelabelAction::Admin(Some(
                            labello_domain::PrelabelAdminCommand::Reset { .. }
                                | labello_domain::PrelabelAdminCommand::Resume { .. }
                        ))
                    ) {
                        self.cancel_prelabel_load();
                        self.work.prelabels.hints.clear();
                        self.work.prelabels.automatic = None;
                        if let Some(current) = &mut self.work.current {
                            current.prelabels.clear();
                        }
                        self.work.queue.clear_prelabels();
                    }
                }
                Err(error) => self.admin.prelabels.error = Some(error),
                _ => {}
            }
            return None;
        }
        if !self
            .work
            .prelabels
            .pending
            .as_ref()
            .is_some_and(|(id, _)| *id == request.request_id)
        {
            return None;
        }
        let (_, action) = self.work.prelabels.pending.take().unwrap();
        self.work.prelabels.abort = None;
        if let PrelabelAction::Retained(query) = action {
            if self.work.current.as_ref().is_none_or(|current| current.image.image_id != query.image_id)
                || self.selected_task().is_none_or(|task| task.task_id != query.task_id)
                || self.work.prelabels.choices.contains_key(&format!("{}/{}", self.config.dataset_id, query.task_id)) {
                return None;
            }
            let retained = match result {
                Ok(PrelabelReply::Retained(retained)) => retained.filter(|retained|
                    !retained.response.suggestions.is_empty() && !retained.response.generation.paused
                    && self.datasets.metadata.as_ref().is_some_and(|metadata|
                        metadata.task(&query.task_id).is_some_and(|task|
                            metadata.prelabel_configs.iter().any(|config| config.config_id == retained.config_id
                                && config.available_to_annotators && config.validate_for_task(task).is_ok())))),
                _ => None,
            };
            self.work.prelabels.automatic = Some(crate::prelabel_flow::AutomaticPrelabels {
                image: query.image_id.clone(), task: query.task_id.clone(),
                config: retained.as_ref().map(|retained| retained.config_id.clone()), checked_at: Instant::now(),
            });
            self.work.prelabels.hints.retain(|(image, task, _), _| image != &query.image_id || task != &query.task_id);
            if let Some(current) = &mut self.work.current {
                current.prelabels = retained.as_ref().map(|retained| retained.response.suggestions.clone()).unwrap_or_default();
            }
            if let Some(retained) = retained {
                self.work.prelabels.hints.insert((query.image_id, query.task_id, retained.config_id), HintStatus {
                    execution: retained.response.execution, generation: Some(retained.response.generation),
                    error: None, from_batch: true, checked_at: Instant::now(),
                });
            }
            return None;
        }
        let (PrelabelAction::Load(query) | PrelabelAction::Check(query)) = action else {
            return None;
        };
        if self.prelabel_choice(&query.task_id).as_ref() != Some(&query.config_id) {
            return None;
        }
        let key = (
            query.image_id.clone(),
            query.task_id.clone(),
            query.config_id.clone(),
        );
        match result {
            Ok(PrelabelReply::Hints(response)) => {
                if let Some(current) = &mut self.work.current
                    && current.image.image_id == query.image_id
                {
                    current.prelabels = response.suggestions.clone();
                }
                self.work
                    .queue
                    .set_prelabels(&query.image_id, response.suggestions);
                self.work.prelabels.hints.insert(
                    key,
                    HintStatus {
                        execution: response.execution,
                        generation: Some(response.generation),
                        error: None,
                        from_batch: response.from_batch,
                        checked_at: Instant::now(),
                    },
                );
            }
            Ok(PrelabelReply::Generation(generation)) => {
                let changed = self
                    .work
                    .prelabels
                    .hints
                    .get(&key)
                    .and_then(|s| s.generation.as_ref())
                    .is_some_and(|old| old != &generation);
                if changed {
                    if let Some(current) = &mut self.work.current {
                        current.prelabels.clear();
                    }
                    self.work.queue.clear_prelabels();
                    self.work.prelabels.hints.clear();
                    if !self.work.prelabels.choices.contains_key(&format!("{}/{}", self.config.dataset_id, query.task_id)) {
                        self.work.prelabels.automatic = None;
                    } else if generation.paused {
                        self.work.prelabels.hints.insert(
                            key,
                            HintStatus {
                                execution: None,
                                generation: Some(generation),
                                error: None,
                                from_batch: false,
                                checked_at: Instant::now(),
                            },
                        );
                    }
                } else if let Some(status) = self.work.prelabels.hints.get_mut(&key) {
                    status.checked_at = Instant::now();
                    // Keep a failed inference actionable until an explicit retry.
                }
            }
            Err(error) => {
                self.work.prelabels.hints.insert(
                    key,
                    HintStatus {
                        execution: None,
                        generation: None,
                        error: Some(format!(
                            "Hints unavailable: {error}. Manual annotation is still available."
                        )),
                        from_batch: false,
                        checked_at: Instant::now(),
                    },
                );
            }
            _ => {}
        }
        None
    }
}
