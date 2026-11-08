use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectScopeAction {
    Added,
    Removed,
    Postponed,
}
impl ProjectScopeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Postponed => "postponed",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectScopeIssue {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectScopeEvent {
    pub id: Uuid,
    pub action: ProjectScopeAction,
    pub reason: String,
    pub issue: ProjectScopeIssue,
    #[schema(required = true)]
    pub actor_id: Option<Uuid>,
    #[schema(required = true)]
    pub actor_name: Option<String>,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectScopeBaseline {
    pub id: Uuid,
    pub label: String,
    pub reason: String,
    #[schema(required = true)]
    pub author_id: Option<Uuid>,
    #[schema(required = true)]
    pub author_name: Option<String>,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectScopeComparison {
    pub unchanged: usize,
    pub added: Vec<ProjectScopeIssue>,
    pub removed: Vec<ProjectScopeIssue>,
    pub postponed: Vec<ProjectScopeIssue>,
}
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectScopeHistory {
    pub capture_limit: u32,
    pub items_has_more: bool,
    #[schema(required = true)]
    pub items_next_cursor: Option<String>,
    pub baselines_has_more: bool,
    #[schema(required = true)]
    pub baselines_next_cursor: Option<String>,
    pub scope_etag: String,
    pub history_started_at: DateTime<Utc>,
    pub legacy_history_unknown: bool,
    pub can_agree: bool,
    pub baselines: Vec<ProjectScopeBaseline>,
    #[schema(required = true)]
    pub baseline: Option<ProjectScopeBaseline>,
    #[schema(required = true)]
    pub comparison: Option<ProjectScopeComparison>,
    pub current: Vec<ProjectScopeIssue>,
    pub events: crate::Page<ProjectScopeEvent>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectScopeQuery {
    pub items_cursor: Option<String>,
    pub baseline_cursor: Option<String>,
    pub baseline_id: Option<Uuid>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgreeProjectScope {
    pub label: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeProjectScope {
    pub issue_id: Uuid,
    pub action: ProjectScopeAction,
    pub reason: String,
}
