#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUserActivity {
    pub dataset_id: DatasetId,
    pub user_id: UserId,
    pub window: labello_domain::UtcActivityWindow,
    pub sampled_at: labello_domain::Timestamp,
    pub counts: labello_domain::DailyActivityCounts,
}
