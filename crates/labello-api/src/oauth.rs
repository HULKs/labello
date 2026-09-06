use labello_domain::{UserAccount, UserId, now};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{ApiError, ApiResult};

#[derive(Clone, Debug)]
pub(crate) struct GithubOAuthEndpoints {
    pub token_url: String,
    pub user_url: String,
}

impl Default for GithubOAuthEndpoints {
    fn default() -> Self {
        Self {
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            user_url: "https://api.github.com/user".to_string(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl GithubOAuthConfig {
    pub(crate) fn flow_cookie_path(&self) -> ApiResult<String> {
        let invalid = || {
            ApiError::BadRequest("invalid GitHub OAuth redirect URI: expected an HTTP(S) URL with a plain safe path ending in /auth/github/callback".to_string())
        };
        let uri = &self.redirect_uri;
        let url = Url::parse(uri).map_err(|_| invalid())?;
        // Reject parser normalization so configuration and browser cookie scope agree.
        let raw_path = uri
            .split_once("://")
            .and_then(|(_, authority_and_path)| {
                authority_and_path
                    .find('/')
                    .map(|start| &authority_and_path[start..])
            })
            .ok_or_else(invalid)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !uri.is_ascii()
            || uri
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control() || byte == b'\\')
            || raw_path != url.path()
            || !raw_path.ends_with("/auth/github/callback")
            || raw_path.split('/').skip(1).any(|segment| {
                segment.is_empty()
                    || matches!(segment, "." | "..")
                    || !segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte))
            })
        {
            return Err(invalid());
        }
        Ok(raw_path
            .strip_suffix("/callback")
            .ok_or_else(invalid)?
            .to_string())
    }

    pub fn authorization_url(&self, state: &str) -> ApiResult<String> {
        let mut url = Url::parse("https://github.com/login/oauth/authorize")
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("scope", "read:user user:email")
            .append_pair("state", state);
        Ok(url.to_string())
    }
}

#[derive(Deserialize)]
struct GithubTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GithubUserResponse {
    id: u64,
    login: String,
    name: Option<String>,
}

pub(crate) async fn exchange_code(
    client: &reqwest::Client,
    config: &GithubOAuthConfig,
    endpoints: &GithubOAuthEndpoints,
    code: &str,
) -> ApiResult<UserAccount> {
    let token: GithubTokenResponse = client
        .post(&endpoints.token_url)
        .header("Accept", "application/json")
        .form(&serde_json::json!({
            "client_id": config.client_id,
            "client_secret": config.client_secret,
            "code": code,
            "redirect_uri": config.redirect_uri,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let user: GithubUserResponse = client
        .get(&endpoints.user_url)
        .bearer_auth(token.access_token)
        .header("User-Agent", "labello")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let timestamp = now();
    Ok(UserAccount {
        user_id: UserId::from(format!("github_{}", user.id)),
        display_name: user.name.unwrap_or_else(|| user.login.clone()),
        github_user_id: Some(user.id.to_string()),
        github_login: Some(user.login),
        created_at: timestamp,
        updated_at: timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(redirect_uri: &str) -> GithubOAuthConfig {
        GithubOAuthConfig {
            client_id: String::new(),
            client_secret: String::new(),
            redirect_uri: redirect_uri.to_string(),
        }
    }

    #[test]
    fn callback_path_validation_preserves_only_safe_literal_paths() {
        for prefix in ["", "/api", "/labello/api", "/v1.2/a_b-c~d"] {
            for scheme in ["http", "https"] {
                let config = config(&format!(
                    "{scheme}://example.com{prefix}/auth/github/callback"
                ));
                assert_eq!(
                    config.flow_cookie_path().unwrap(),
                    format!("{prefix}/auth/github")
                );
            }
        }
        for path in [
            "/callback",
            "/api/auth/github/callback/",
            "/api/auth/github/callback-extra",
            "/api/auth/github/callback?extra=1",
            "/api/auth/github/callback#fragment",
            "/api;Secure/auth/github/callback",
            "/api,evil/auth/github/callback",
            "/api=evil/auth/github/callback",
            "/api%2Fother/auth/github/callback",
            "/%61pi/auth/github/callback",
            "/%2e%2e/auth/github/callback",
            "/api%3B/auth/github/callback",
            "/api%00/auth/github/callback",
            "/api%ZZ/auth/github/callback",
            "/api%/auth/github/callback",
            "/api/../auth/github/callback",
            "/api/./auth/github/callback",
            "//api/auth/github/callback",
            "/api//auth/github/callback",
            "/api\\auth/github/callback",
            "/api\t/auth/github/callback",
            "/api\r\n/auth/github/callback",
            "/api /auth/github/callback",
            "/äpi/auth/github/callback",
            "/api\0/auth/github/callback",
        ] {
            assert!(
                config(&format!("https://example.com{path}"))
                    .flow_cookie_path()
                    .is_err(),
                "accepted unsafe callback path"
            );
        }
        for uri in [
            "/auth/github/callback",
            "ftp://example.com/auth/github/callback",
            "https://user@example.com/auth/github/callback",
            "https:///auth/github/callback",
            " https://example.com/auth/github/callback",
        ] {
            assert!(
                config(uri).flow_cookie_path().is_err(),
                "accepted malformed callback URI"
            );
        }
    }
}
