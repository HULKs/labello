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
    pub datasets: Vec<PresenceDataset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceDataset {
    pub dataset_id: DatasetId,
    pub name: String,
}
