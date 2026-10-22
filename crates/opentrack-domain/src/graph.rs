use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GraphStatus {
    pub id: Uuid,
    pub name: String,
    #[schema(value_type = StatusColor)]
    pub color: String,
    #[schema(value_type = StatusCategory)]
    pub category: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueGraphTarget {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    pub status: GraphStatus,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub etag: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ParentLink {
    #[schema(required = true)]
    pub target: Option<IssueGraphTarget>,
    pub can_detach: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueParent {
    pub issue_id: Uuid,
    pub issue_etag: String,
    pub graph_version: String,
    #[schema(required = true)]
    pub parent: Option<ParentLink>,
    pub can_change: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Blocks,
    BlockedBy,
    Related,
    DuplicateOf,
    HasDuplicate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CreateRelationKind {
    Blocks,
    BlockedBy,
    Related,
    DuplicateOf,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueRelation {
    pub id: Uuid,
    pub kind: RelationKind,
    #[schema(required = true)]
    pub target: Option<IssueGraphTarget>,
    pub can_remove: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GraphPage<T> {
    pub data: Vec<T>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub issue_etag: String,
    pub graph_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RelationMutation {
    pub issue: Issue,
    pub relation: IssueRelation,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphPageQuery {
    #[schema(minimum = 1, maximum = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueLookupQuery {
    pub team_id: Uuid,
    #[schema(max_length = 100)]
    pub q: Option<String>,
    pub exclude_issue_id: Option<Uuid>,
    #[schema(minimum = 1, maximum = 20)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetIssueParent {
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true)]
    pub parent_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false, min_length = 1, max_length = 256)]
    pub parent_etag: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIssueRelation {
    pub kind: CreateRelationKind,
    pub target_id: Uuid,
    #[schema(min_length = 1, max_length = 256)]
    pub target_etag: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MoveDestination {
    pub id: Uuid,
    pub name: String,
    pub key: String,
    pub private: bool,
    pub can_move_issue: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveDestinationQuery {
    #[schema(max_length = 100)]
    pub q: Option<String>,
    #[schema(minimum = 1, maximum = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MoveBlocker {
    Parent,
    Children,
    Blocks,
    BlockedBy,
    Related,
    DuplicateOf,
    HasDuplicate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MoveAudienceChange {
    None,
    PublicToPrivate,
    PrivateToPublic,
    BetweenPrivateTeams,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueMovePreview {
    pub issue_id: Uuid,
    pub issue_etag: String,
    pub source_team_id: Uuid,
    pub destination: MoveDestination,
    pub destination_workflow_version: String,
    pub destination_default_status_id: Uuid,
    pub audience_change: MoveAudienceChange,
    #[schema(required = true)]
    pub current_assignee: Option<IssueAssignee>,
    pub retained_label_ids: Vec<Uuid>,
    pub removed_label_ids: Vec<Uuid>,
    pub blockers: Vec<MoveBlocker>,
    pub preview_token: String,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveIssue {
    pub destination_team_id: Uuid,
    pub status_id: Uuid,
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true)]
    pub assignee_id: Option<Uuid>,
    #[schema(max_items = 50)]
    pub label_ids: Vec<Uuid>,
    #[schema(min_length = 1, max_length = 4096)]
    pub preview_token: String,
}
/// Strong validator over the complete authorized graph representation.
pub fn graph_etag<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Internal)?;
    Ok(format!("\"graph:{:x}\"", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn graph_requests_distinguish_omission_null_and_explicit_clear() {
        assert!(serde_json::from_value::<SetIssueParent>(json!({})).is_err());
        let clear: SetIssueParent = serde_json::from_value(json!({"parent_id":null})).unwrap();
        validate_parent_pair(clear.parent_id, clear.parent_etag.as_deref()).unwrap();
        assert!(
            serde_json::from_value::<SetIssueParent>(json!({"parent_id":null,"parent_etag":null}))
                .is_err()
        );
        assert!(validate_parent_pair(Some(Uuid::new_v4()), None).is_err());
        assert!(validate_parent_pair(None, Some("etag")).is_err());
        let team = Uuid::new_v4();
        let create: CreateIssue =
            serde_json::from_value(json!({"team_id":team,"title":"Child"})).unwrap();
        let serialized = serde_json::to_value(create).unwrap();
        assert!(serialized.get("parent_id").is_none());
        assert!(serialized.get("parent_etag").is_none());
        assert!(
            serde_json::from_value::<CreateIssue>(
                json!({"team_id":team,"title":"Child","parent_id":null})
            )
            .is_err()
        );
        let mut movement = json!({"destination_team_id":team,"status_id":Uuid::new_v4(),"assignee_id":null,"label_ids":[],"preview_token":"token"});
        assert!(serde_json::from_value::<MoveIssue>(movement.clone()).is_ok());
        movement.as_object_mut().unwrap().remove("assignee_id");
        assert!(serde_json::from_value::<MoveIssue>(movement).is_err());
    }
    #[test]
    fn graph_validator_covers_authorized_target_projection_and_permissions() {
        let mut representation = json!({"issue_etag":"issue:1","parent":{"target":{"title":"Before"},"can_detach":true}});
        let first = graph_etag(&representation).unwrap();
        representation["parent"]["target"]["title"] = json!("After");
        let second = graph_etag(&representation).unwrap();
        assert_ne!(first, second);
        representation["parent"]["can_detach"] = json!(false);
        assert_ne!(second, graph_etag(&representation).unwrap());
    }
}
