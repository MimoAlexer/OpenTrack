use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeletionPreview {
    pub issue: Issue,
    pub can_delete: bool,
    pub child_count: u64,
    pub relationship_count: u64,
    pub has_parent: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeletedIssue {
    pub issue: Issue,
    pub deleted_at: DateTime<Utc>,
    pub restore_until: DateTime<Utc>,
    pub can_restore: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletedIssueQuery {
    pub team_id: Option<Uuid>,
    #[schema(max_length = 256)]
    pub q: Option<String>,
    #[schema(minimum = 1, maximum = 100)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreIssue {
    pub status_id: Uuid,
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true)]
    pub assignee_id: Option<Uuid>,
    #[schema(max_items = 50)]
    pub label_ids: Vec<Uuid>,
}
fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}
