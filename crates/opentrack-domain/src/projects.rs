use crate::{
    DocumentWrite, Error, IssueAssignee, Result, TemplateDocument, bounded_text,
    deserialize_present,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectState {
    #[default]
    Planned,
    Active,
    Paused,
    Completed,
    Canceled,
}
impl ProjectState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Canceled => "canceled",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectHealth {
    #[default]
    OnTrack,
    AtRisk,
    OffTrack,
}
impl ProjectHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnTrack => "on_track",
            Self::AtRisk => "at_risk",
            Self::OffTrack => "off_track",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Project {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub summary: String,
    pub narrative: TemplateDocument,
    /// Brief document version; null only for creation receipts predating independent document versioning.
    #[serde(default)]
    #[schema(required = true, read_only = true)]
    pub narrative_version: Option<String>,
    #[schema(required = true)]
    pub lead_id: Option<Uuid>,
    #[schema(required = true)]
    pub lead: Option<IssueAssignee>,
    #[schema(required = true)]
    pub start_date: Option<NaiveDate>,
    #[schema(required = true)]
    pub target_date: Option<NaiveDate>,
    pub state: ProjectState,
    pub health: ProjectHealth,
    pub version: String,
    pub etag: String,
    #[serde(default)]
    #[schema(required = true, read_only = true)]
    pub metadata_etag: String,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub can_edit: bool,
    pub can_manage: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub summary: String,
    #[schema(required = true)]
    pub lead_id: Option<Uuid>,
    #[schema(required = true)]
    pub lead: Option<IssueAssignee>,
    #[schema(required = true)]
    pub start_date: Option<NaiveDate>,
    #[schema(required = true)]
    pub target_date: Option<NaiveDate>,
    pub state: ProjectState,
    pub health: ProjectHealth,
    pub version: String,
    pub etag: String,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub can_edit: bool,
    pub can_manage: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectProgress {
    pub completed: i64,
    pub eligible: i64,
    pub canceled: i64,
    pub open: i64,
    pub total: i64,
    pub unscoped: bool,
    pub as_of: DateTime<Utc>,
    #[serde(default)]
    #[schema(required = true)]
    pub calculation_source: crate::PlanningCalculationSource,
    #[serde(default)]
    #[schema(required = true)]
    pub facts_as_of: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProject {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub narrative: Option<DocumentWrite>,
    pub lead_id: Option<Uuid>,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProject {
    #[serde(default, deserialize_with = "deserialize_present")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub narrative: Option<DocumentWrite>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub lead_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub start_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub target_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub state: Option<ProjectState>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub health: Option<ProjectHealth>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub archived: Option<bool>,
    #[serde(default)]
    pub acknowledge_open_issues: bool,
}
impl UpdateProject {
    pub fn changes_metadata(&self) -> bool {
        self.name.is_some()
            || self.summary.is_some()
            || self.narrative.is_some()
            || self.lead_id.is_some()
            || self.start_date.is_some()
            || self.target_date.is_some()
            || self.state.is_some()
            || self.health.is_some()
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectQuery {
    pub q: Option<String>,
    pub archived: Option<bool>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectPageQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectMembership {
    pub included: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectIssue {
    pub id: Uuid,
    #[schema(required = true)]
    pub milestone_id: Option<Uuid>,
    pub key: String,
    pub title: String,
    pub status_id: Uuid,
    pub status_name: String,
    pub status_category: String,
    pub priority: i16,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectUpdate {
    pub body: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectUpdate {
    pub id: Uuid,
    pub author_id: Uuid,
    pub author_name: String,
    pub body: String,
    pub state: ProjectState,
    pub health: ProjectHealth,
    pub completed: i64,
    pub eligible: i64,
    pub created_at: DateTime<Utc>,
}
pub fn validate_project_dates(start: Option<NaiveDate>, target: Option<NaiveDate>) -> Result<()> {
    if start.zip(target).is_some_and(|(a, b)| a > b) {
        return Err(Error::Invalid(
            "Project target date cannot precede its start date".into(),
        ));
    }
    Ok(())
}
pub fn project_summary(value: &str) -> Result<String> {
    if value.trim().is_empty() {
        Ok(String::new())
    } else {
        bounded_text(value, "Project summary", 500)
    }
}
pub fn validate_project_narrative(value: &DocumentWrite) -> Result<String> {
    let plain = value.validate()?;
    if value.moderation_reason.is_some() {
        return Err(Error::Invalid(
            "Moderation reasons apply only to comments".into(),
        ));
    }
    Ok(plain)
}

impl ProjectUpdate {
    pub fn etag(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Internal)?;
        let digest = Sha256::digest(bytes);
        Ok(format!("\"project-update:{}:{digest:x}\"", self.id))
    }
}

impl From<&Project> for ProjectSummary {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id,
            workspace_id: p.workspace_id,
            team_id: p.team_id,
            name: p.name.clone(),
            summary: p.summary.clone(),
            lead_id: p.lead_id,
            lead: p.lead.clone(),
            start_date: p.start_date,
            target_date: p.target_date,
            state: p.state,
            health: p.health,
            version: p.version.clone(),
            etag: p.etag.clone(),
            archived_at: p.archived_at,
            created_at: p.created_at,
            updated_at: p.updated_at,
            can_edit: p.can_edit,
            can_manage: p.can_manage,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patch_clear_is_distinct_from_omission_and_unknown_fields_fail() {
        let empty: UpdateProject = serde_json::from_str("{}").unwrap();
        let clear: UpdateProject =
            serde_json::from_str(r#"{"lead_id":null,"target_date":null}"#).unwrap();
        assert!(empty.lead_id.is_none());
        assert_eq!(clear.lead_id, Some(None));
        assert_eq!(clear.target_date, Some(None));
        assert!(serde_json::from_str::<UpdateProject>(r#"{"state":null}"#).is_err());
        assert!(serde_json::from_str::<UpdateProject>(r#"{"team_id":null}"#).is_err());
    }
    #[test]
    fn date_order_and_summary_are_bounded() {
        let a = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let b = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        assert!(validate_project_dates(Some(b), Some(a)).is_err());
        assert!(validate_project_dates(Some(a), Some(a)).is_ok());
        assert!(project_summary(&"x".repeat(501)).is_err());
        assert_eq!(project_summary("  ").unwrap(), "");
    }
}
