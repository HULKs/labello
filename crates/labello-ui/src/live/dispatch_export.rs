impl LabelloApp {
    fn dispatch_export_command(
        &self,
        api: Rc<dyn labello_client::LabelloApi>,
        command: UiCommand,
    ) -> Option<UiCommand> {
        let UiCommand::Export {
            request,
            dataset_id,
            action,
        } = command
        else {
            return Some(command);
        };
        self.spawn_message(request.clone(), async move {
            use crate::export_flow::{ExportAction, ExportReply};
            let result = async {
                match action {
                    ExportAction::Load => {
                        let capabilities = api.export_capabilities(&dataset_id).await?;
                        let jobs = if capabilities.available {
                            api.list_exports(&dataset_id).await?
                        } else {
                            Vec::new()
                        };
                        Ok(ExportReply::Loaded { capabilities, jobs })
                    }
                    ExportAction::Preflight(options) => api
                        .preflight_export(&dataset_id, options)
                        .await
                        .map(|job| ExportReply::Job(Box::new(job))),
                    ExportAction::Poll(id) => api
                        .get_export(&dataset_id, &id)
                        .await
                        .map(|job| ExportReply::Job(Box::new(job))),
                    ExportAction::Start(id) => api
                        .start_export(&dataset_id, &id)
                        .await
                        .map(|job| ExportReply::Job(Box::new(job))),
                    ExportAction::Cancel(id) => api
                        .cancel_export(&dataset_id, &id)
                        .await
                        .map(|job| ExportReply::Job(Box::new(job))),
                    ExportAction::Download(id) => api
                        .export_download_url(&dataset_id, &id)
                        .await
                        .map(ExportReply::Download),
                }
            }
            .await
            .map_err(|error: labello_client::ClientError| error.to_string());
            UiMessage::ExportFinished {
                request,
                result: Box::new(result),
            }
        });
        None
    }
}
