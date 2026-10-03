impl LabelloApp {
    fn dispatch_auth_command(
        &self,
        api: Rc<dyn labello_client::LabelloApi>,
        command: UiCommand,
    ) -> Option<UiCommand> {
        match command {
            UiCommand::BuildInformation { request } => self.spawn_message(request.clone(), async move {
                UiMessage::BuildInformationLoaded {
                    request,
                    result: api.build_information().await.map_err(UiRequestError::from),
                }
            }),
            UiCommand::InitializeSession { request } => self.spawn_message(request.clone(), async move {
                let (options, session) = futures::join!(api.auth_options(), api.me());
                UiMessage::SessionInitialized {
                    request,
                    options: options.map_err(UiRequestError::from),
                    session: session.map_err(UiRequestError::from),
                }
            }),
            UiCommand::Session { request } => self.spawn_message(request.clone(), async move {
                UiMessage::SessionLoaded {
                    request,
                    result: api.me().await.map_err(UiRequestError::from),
                }
            }),
            UiCommand::LocalAdminLogin { request } => {
                self.spawn_message(request.clone(), async move {
                    UiMessage::SessionLoaded {
                        request,
                        result: api
                            .local_admin_login()
                            .await
                            .map_err(UiRequestError::from),
                    }
                })
            }
            UiCommand::Logout { request } => self.spawn_message(request.clone(), async move {
                UiMessage::LogoutFinished {
                    request,
                    result: api.logout().await.map_err(UiRequestError::from),
                }
            }),
            UiCommand::GithubLogin { request, return_to } => {
                self.spawn_message(request.clone(), async move {
                    UiMessage::GithubLoginUrl {
                        request,
                        result: api
                            .github_login_url(OAuthLoginRequest { return_to })
                            .await
                            .map_err(UiRequestError::from),
                    }
                })
            }
            command => return Some(command),
        }
        None
    }
}
