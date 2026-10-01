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
        self.spawn_message(request.clone(), async move {
            let future = async {
                match action {
                    PrelabelAction::InspectModel { location, .. } => api
                        .inspect_prelabel_model(&dataset_id, labello_client::PrelabelModelCheckRequest { location })
                        .await.map(PrelabelReply::Model),
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
            let result = future.await;
            UiMessage::PrelabelFinished {
                request,
                result: Box::new(result),
            }
        });
        None
    }

    fn reduce_prelabel_message(&mut self, message: UiMessage) -> Option<UiMessage> {
        use crate::prelabel_flow::PrelabelReply;
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
 =>
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
            self.admin.prelabels.pending = None;
            self.admin.prelabels.error = None;
            match result {
                Ok(PrelabelReply::Admin(state)) => {
                    self.admin.prelabels.state = Some(state);
                }
                Err(error) => self.admin.prelabels.error = Some(error),
                _ => {}
            }
            return None;
        }
        Some(UiMessage::PrelabelFinished { request, result: Box::new(result) })
    }
}
