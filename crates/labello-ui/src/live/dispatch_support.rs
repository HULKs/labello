impl LabelloApp {
    fn dispatch_support_command(
        &self,
        api: Rc<dyn labello_client::LabelloApi>,
        command: UiCommand,
    ) -> Option<UiCommand> {
        match command {
            UiCommand::Inspect { request, action } => self.dispatch_inspection(api, request, action),
            UiCommand::Presence { request } => self.spawn_message(request.clone(), async move {
                let result = api.server_presence().await.map_err(UiRequestError::from);
                UiMessage::PresenceLoaded { request, result }
            }),
            UiCommand::Overview { request } => self.spawn_message(request.clone(), async move {
                UiMessage::OverviewLoaded { request, result: api.statistics_overview().await.map_err(UiRequestError::from) }
            }),
            UiCommand::Preferences { request, user_id } => self.spawn_message(request.clone(), async move {
                let result: Result<_, labello_client::ClientError> = async { Ok((api.get_keybindings(&user_id).await?, api.legacy_keybindings().await?)) }.await;
                UiMessage::PreferencesLoaded { request, result: result.map_err(UiRequestError::from) }
            }),
            UiCommand::Stats {
                request,
                dataset_id,
            } => self.spawn_message(request.clone(), async move {
                let result = api
                    .dataset_stats(&dataset_id)
                    .await
                    .map_err(UiRequestError::from);
                UiMessage::StatsLoaded { request, result }
            }),
            UiCommand::AssignmentAvailability {
                request,
                dataset_id,
                kind,
                checked_assignments,
            } => self.spawn_message(request.clone(), async move {
                let result = api
                    .assignment_availability(
                        &dataset_id,
                        labello_client::AssignmentAvailabilityRequest { kind },
                    )
                    .await
                    .map_err(UiRequestError::from);
                UiMessage::AssignmentAvailabilityLoaded { request, result, checked_assignments }
            }),
            UiCommand::SaveKeybindings {
                request,
                keybindings,
            } => self.spawn_message(request.clone(), async move {
                UiMessage::KeybindingsSaved {
                    request,
                    result: api
                        .save_keybindings(keybindings)
                        .await
                        .map_err(UiRequestError::from),
                }
            }),
            command => return Some(command),
        }
        None
    }
}
