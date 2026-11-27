impl AuthApi for HttpLabelloApi {
    fn csrf_token(&self) -> Option<String> {
        self.csrf_token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn auth_options<'a>(&'a self) -> crate::ApiFuture<'a, AuthOptions> {
        Box::pin(async move {
            self.discovery_get("/auth/options").await
        })
    }

    fn local_admin_login<'a>(&'a self) -> crate::ApiFuture<'a, SessionInfo> {
        Box::pin(async move {
            self.discard_discovery();
            self.session(
                self.request(Method::POST, "/auth/local-admin")?
                    .send()
                    .await?,
            )
            .await
        })
    }

    fn github_login_url<'a>(&'a self, request: OAuthLoginRequest) -> crate::ApiFuture<'a, String> {
        Box::pin(async move {
            let mut url = self.endpoint("/auth/github/login")?;
            if let Some(return_to) = request.return_to {
                url.query_pairs_mut().append_pair("returnTo", &return_to);
            }
            Ok(url.to_string())
        })
    }

    fn github_callback<'a>(
        &'a self,
        request: OAuthCallbackRequest,
    ) -> crate::ApiFuture<'a, UserAccount> {
        Box::pin(async move {
            self.discard_discovery();
            Self::json(
                self.request(
                    Method::GET,
                    &format!(
                        "/auth/github/callback?code={}&state={}",
                        urlencoding::encode(&request.code),
                        urlencoding::encode(&request.state)
                    ),
                )?
                .send()
                .await?,
            )
            .await
        })
    }

    fn me<'a>(&'a self) -> crate::ApiFuture<'a, SessionInfo> {
        Box::pin(async move {
            let session = self.discovery_get("/me").await?;
            self.remember_session(&session);
            Ok(session)
        })
    }

    fn logout<'a>(&'a self) -> crate::ApiFuture<'a, ()> {
        Box::pin(async move {
            self.discard_discovery();
            let response = self.request(Method::POST, "/logout")?.send().await?;
            Self::ensure_success(response).await?;
            self.clear_csrf_token();
            Ok(())
        })
    }
}

fn response_request_id(response: &Response) -> Option<String> {
    response
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
