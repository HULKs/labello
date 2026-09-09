#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUserActivity {
    pub dataset_id: DatasetId,
    pub user_id: UserId,
    pub window: labello_domain::UtcActivityWindow,
    pub sampled_at: labello_domain::Timestamp,
    pub counts: labello_domain::DailyActivityCounts,
}

/// Server-wide presence. Contains no assignment, image, or session identifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPresence {
    pub users: Vec<PresentUser>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentUser {
    pub user_id: UserId,
    /// Presentation metadata only; identity and ownership use `user_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
    pub datasets: Vec<PresenceDataset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceDataset {
    pub dataset_id: DatasetId,
    pub name: String,
}

impl PresentUser {
    pub fn presence_name(&self) -> String {
        match self.github_login.as_deref().filter(|login| !login.is_empty()) {
            Some(login) => format!("@{login}"),
            None => self.user_id.to_string(),
        }
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    #[test]
    fn presence_names_preserve_identity_and_accept_older_responses() {
        let mut user: PresentUser = serde_json::from_str(
            r#"{"userId":"github_42","datasets":[]}"#,
        ).unwrap();
        assert_eq!(user.presence_name(), "github_42");
        user.github_login = Some("octocat".into());
        assert_eq!(user.presence_name(), "@octocat");
        assert_eq!(user.user_id.as_str(), "github_42");
        let json = serde_json::to_value(&user).unwrap();
        assert_eq!(json["githubLogin"], "octocat");
        user.github_login = Some(String::new());
        assert_eq!(user.presence_name(), "github_42");
    }
}
