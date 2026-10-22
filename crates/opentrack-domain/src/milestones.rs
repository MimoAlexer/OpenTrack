use crate::{ProjectProgress, deserialize_present};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneState {
    #[default]
    Planned,
    Completed,
}
impl MilestoneState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Completed => "completed",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Milestone {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    #[schema(required = true)]
    pub target_date: Option<NaiveDate>,
    pub rank: String,
    pub state: MilestoneState,
    #[schema(required = true)]
    pub completed_at: Option<DateTime<Utc>>,
    pub version: String,
    pub etag: String,
    pub can_manage: bool,
    pub can_assign: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MilestoneOverview {
    pub milestone: Milestone,
    pub progress: ProjectProgress,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MilestonePage {
    pub data: Vec<MilestoneOverview>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub project_etag: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMilestone {
    #[schema(min_length = 1, max_length = 160)]
    pub title: String,
    pub target_date: Option<NaiveDate>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMilestone {
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false, min_length = 1, max_length = 160)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub target_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub state: Option<MilestoneState>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MilestoneOrder {
    #[schema(max_items = 500)]
    pub milestone_ids: Vec<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MilestoneAssignment {
    #[serde(deserialize_with = "Option::<Uuid>::deserialize")]
    #[schema(required = true)]
    pub milestone_id: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omission_null_and_invalid_state_are_distinct() {
        let patch: UpdateMilestone = serde_json::from_str("{}").unwrap();
        assert!(patch.target_date.is_none());
        let clear: UpdateMilestone = serde_json::from_str(r#"{"target_date":null}"#).unwrap();
        assert_eq!(clear.target_date, Some(None));
        assert!(serde_json::from_str::<UpdateMilestone>(r#"{"state":null}"#).is_err());
        assert!(serde_json::from_str::<UpdateMilestone>(r#"{"project_id":null}"#).is_err());
        assert!(serde_json::from_str::<MilestoneAssignment>("{}").is_err());
        assert!(
            serde_json::from_str::<MilestoneAssignment>(r#"{"milestone_id":null}"#)
                .unwrap()
                .milestone_id
                .is_none()
        );
    }
}
