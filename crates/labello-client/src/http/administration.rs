impl StatsApi for HttpLabelloApi {
    fn statistics_overview(&self) -> crate::ApiFuture<'_, Vec<crate::DatasetStatistics>> {
        Box::pin(async move { Self::json(self.request(Method::GET, "/stats")?.timeout(STATS_REQUEST_TIMEOUT).send().await?).await })
    }

    fn server_presence(&self) -> crate::ApiFuture<'_, crate::ServerPresence> {
        Box::pin(async move {
            Self::json(
                self.request(Method::GET, "/presence")?
                    .timeout(std::time::Duration::from_secs(8))
                    .send()
                    .await?,
            )
            .await
        })
    }

    fn current_user_activity<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
    ) -> crate::ApiFuture<'a, crate::CurrentUserActivity> {
        Box::pin(async move {
            Self::json(
                self.request(Method::GET, &format!("/datasets/{dataset_id}/stats/me"))?
                    .timeout(STATS_REQUEST_TIMEOUT)
                    .send()
                    .await?,
            )
            .await
        })
    }

    fn dataset_stats<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
    ) -> crate::ApiFuture<'a, DatasetStats> {
        Box::pin(async move {
            Self::json(
                self.request(Method::GET, &format!("/datasets/{dataset_id}/stats"))?
                    .timeout(STATS_REQUEST_TIMEOUT)
                    .send()
                    .await?,
            )
            .await
        })
    }
}

impl KeybindingApi for HttpLabelloApi {
    fn legacy_keybindings(&self) -> crate::ApiFuture<'_, Vec<crate::LegacyKeybindings>> {
        Box::pin(async move { Self::json(self.request(Method::GET, "/keybindings/legacy")?.send().await?).await })
    }

    fn get_keybindings<'a>(
        &'a self,
        _user_id: &'a UserId,
    ) -> crate::ApiFuture<'a, KeybindingSet> {
        Box::pin(async move {
            Self::json(
                self.request(Method::GET, "/keybindings")?
                    .send()
                    .await?,
            )
            .await
        })
    }

    fn save_keybindings<'a>(
        &'a self,
        keybindings: KeybindingSet,
    ) -> crate::ApiFuture<'a, KeybindingSet> {
        Box::pin(async move {
            Self::send_json(
                self.request(Method::PUT, "/keybindings")?,
                &keybindings,
            )
            .await
        })
    }
}

impl PrelabelApi for HttpLabelloApi {
    fn inspect_prelabel_model<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        request: crate::PrelabelModelCheckRequest,
    ) -> crate::ApiFuture<'a, labello_domain::PrelabelModelInspection> {
        Box::pin(async move {
            Self::send_json(self.request(Method::POST, &format!("/datasets/{dataset_id}/prelabel-model-check"))?, &request).await
        })
    }

    fn list_prelabel_configs<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
    ) -> crate::ApiFuture<'a, Vec<PrelabelConfig>> {
        Box::pin(async move {
            Self::json(
                self.request(Method::GET, &format!("/datasets/{dataset_id}/prelabels"))?
                    .send()
                    .await?,
            )
            .await
        })
    }

    fn add_prelabel_config<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        config: PrelabelConfig,
    ) -> crate::ApiFuture<'a, PrelabelConfig> {
        Box::pin(async move {
            Self::send_json(
                self.request(Method::POST, &format!("/datasets/{dataset_id}/prelabels"))?,
                &config,
            )
            .await
        })
    }

    fn prelabel_admin_state<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
    ) -> crate::ApiFuture<'a, labello_domain::PrelabelAdminState> {
        Box::pin(async move {
            Self::json(
                self.request(
                    Method::GET,
                    &format!("/datasets/{dataset_id}/prelabel-management"),
                )?
                .send()
                .await?,
            )
            .await
        })
    }
    fn prelabel_admin_command<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        command: labello_domain::PrelabelAdminCommand,
    ) -> crate::ApiFuture<'a, labello_domain::PrelabelAdminState> {
        Box::pin(async move {
            Self::send_json(
                self.request(
                    Method::POST,
                    &format!("/datasets/{dataset_id}/prelabel-management"),
                )?,
                &command,
            )
            .await
        })
    }
}

impl UserApi for HttpLabelloApi {
    fn list_dataset_users<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
    ) -> crate::ApiFuture<'a, Vec<DatasetUser>> {
        Box::pin(async move {
            Self::json(
                self.request(Method::GET, &format!("/datasets/{dataset_id}/users"))?
                    .send()
                    .await?,
            )
            .await
        })
    }

    fn set_dataset_roles<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        request: SetDatasetRolesRequest,
    ) -> crate::ApiFuture<'a, DatasetUser> {
        Box::pin(async move {
            Self::send_json(
                self.request(Method::PUT, &format!("/datasets/{dataset_id}/roles"))?,
                &request,
            )
            .await
        })
    }
}
