use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MentionCandidate {
    pub id: Uuid,
    pub display_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MentionReference {
    pub id: Uuid,
    #[schema(required = true, nullable = true)]
    pub display_name: Option<String>,
    pub available: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MentionQuery {
    pub team_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
    pub document_id: Option<Uuid>,
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
impl MentionQuery {
    pub fn validate(&self) -> Result<()> {
        if [self.team_id, self.issue_id, self.document_id]
            .iter()
            .flatten()
            .count()
            != 1
            || !(1..=50).contains(&self.limit.unwrap_or(20))
            || self.q.as_ref().is_some_and(|s| s.chars().count() > 100)
            || self.cursor.as_ref().is_some_and(|s| s.len() > 2048)
        {
            return Err(Error::Invalid(
                "Choose one mention scope, a page size of 1–50, and search at most 100 characters"
                    .into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionPreference {
    Automatic,
    Subscribed,
    Unsubscribed,
}
impl SubscriptionPreference {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Subscribed => "subscribed",
            Self::Unsubscribed => "unsubscribed",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSource {
    Creator,
    Assignment,
    Mention,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueSubscription {
    pub issue_id: Uuid,
    pub preference: SubscriptionPreference,
    pub subscribed: bool,
    pub sources: Vec<SubscriptionSource>,
    pub version: String,
    pub etag: String,
    pub can_change: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIssueSubscription {
    pub preference: SubscriptionPreference,
}
