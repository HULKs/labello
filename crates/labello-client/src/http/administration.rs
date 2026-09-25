impl StatsApi for HttpLabelloApi {
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
    fn get_keybindings<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        _user_id: &'a UserId,
    ) -> crate::ApiFuture<'a, KeybindingSet> {
        Box::pin(async move {
            Self::json(
                self.request(Method::GET, &format!("/datasets/{dataset_id}/keybindings"))?
                    .send()
                    .await?,
            )
            .await
        })
    }

    fn save_keybindings<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        keybindings: KeybindingSet,
    ) -> crate::ApiFuture<'a, KeybindingSet> {
        Box::pin(async move {
            Self::send_json(
                self.request(Method::PUT, &format!("/datasets/{dataset_id}/keybindings"))?,
                &keybindings,
            )
            .await
        })
    }
}

impl PrelabelApi for HttpLabelloApi {
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

    fn prelabel_suggestions<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        request: PrelabelSuggestionRequest,
    ) -> crate::ApiFuture<'a, labello_domain::PrelabelResponse> {
        Box::pin(async move {
            let response: labello_domain::PrelabelResponse = Self::send_json(
                self.request(
                    Method::POST,
                    &format!("/datasets/{dataset_id}/prelabel-suggestions"),
                )?,
                &request,
            )
            .await?;
            if let Some(grant) = response.browser_grant.clone() {
                #[cfg(target_arch = "wasm32")]
                {
                    let metadata = self.get_dataset(dataset_id).await?;
                    let config = metadata
                        .prelabel_configs
                        .iter()
                        .find(|c| c.config_id == request.config_id)
                        .ok_or_else(browser_prelabel_unavailable)?;
                    let task = metadata
                        .task(&request.task_id)
                        .ok_or_else(browser_prelabel_unavailable)?;
                    let model = Self::ensure_success(
                        self.request(
                            Method::GET,
                            &format!(
                                "/datasets/{dataset_id}/prelabels/{}/model",
                                request.config_id
                            ),
                        )?
                        .send()
                        .await?,
                    )
                    .await?;
                    let model =
                        crate::preview::bounded_body(model, labello_inference::MAX_MODEL_BYTES)
                            .await?;
                    let image = Self::ensure_success(
                        self.request(
                            Method::GET,
                            &format!("/datasets/{dataset_id}/images/{}/file", request.image_id),
                        )?
                        .send()
                        .await?,
                    )
                    .await?;
                    let image =
                        crate::preview::bounded_body(image, labello_inference::MAX_IMAGE_BYTES)
                            .await?;
                    if blake3::hash(&model).to_hex().as_str() != grant.model_digest
                        || blake3::hash(&image).to_hex().as_str() != grant.image_hash
                    {
                        return Err(browser_prelabel_unavailable());
                    }
                    let location = web_sys::window()
                        .ok_or_else(browser_prelabel_unavailable)?
                        .location()
                        .href()
                        .map_err(|_| browser_prelabel_unavailable())?;
                    let runtime = url::Url::parse(&location)?.join("onnx/")?;
                    let mut result =
                        labello_inference::infer(&model, &image, config, task, runtime.as_str())
                            .await
                            .map_err(|message| ClientError::Api { status: 0, message })?;
                    result
                        .suggestions
                        .retain(|hint| hint.passes(&config.output_processing));
                    return Self::send_json(
                        self.request(
                            Method::POST,
                            &format!("/datasets/{dataset_id}/prelabel-browser-result"),
                        )?,
                        &labello_domain::BrowserPrelabelResult {
                            grant,
                            execution: result.execution,
                            suggestions: result.suggestions,
                        },
                    )
                    .await;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let _ = grant;
                    return Err(ClientError::Api { status: 0, message: "This model runs in the browser. Select a server model for native annotation.".into() });
                }
            }
            Ok(response)
        })
    }

    fn prelabel_generation<'a>(
        &'a self,
        dataset_id: &'a DatasetId,
        request: PrelabelSuggestionRequest,
    ) -> crate::ApiFuture<'a, labello_domain::PrelabelGeneration> {
        Box::pin(async move {
            Self::json(
                self.request(
                    Method::GET,
                    &format!("/datasets/{dataset_id}/prelabel-generation"),
                )?
                .query(&request)
                .send()
                .await?,
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

#[cfg(target_arch = "wasm32")]
fn browser_prelabel_unavailable() -> ClientError {
    ClientError::Api {
        status: 0,
        message: "The model or image changed. Refresh hints to retry.".into(),
    }
}
