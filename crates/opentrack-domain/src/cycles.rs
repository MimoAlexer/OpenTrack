use crate::{Error, Result, deserialize_present};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CycleState {
    Planned,
    Active,
    Closed,
}
impl CycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Active => "active",
            Self::Closed => "closed",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Cycle {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub start_date: NaiveDate,
    pub end_date_exclusive: NaiveDate,
    pub timezone: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub state: CycleState,
    pub version: String,
    pub etag: String,
    #[schema(required = true)]
    pub closed_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub latest_report_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub can_plan: bool,
    pub can_manage: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCycle {
    pub name: String,
    pub start_date: NaiveDate,
    pub end_date_exclusive: NaiveDate,
    pub timezone: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCycle {
    #[serde(default, deserialize_with = "deserialize_present")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub start_date: Option<NaiveDate>,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub end_date_exclusive: Option<NaiveDate>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CycleQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CycleMembership {
    pub included: bool,
    pub issue_etag: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReopenCycle {
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleScopeItem {
    pub issue_id: Uuid,
    pub issue_version: String,
    pub category: String,
    pub excluded: bool,
    #[schema(required = true)]
    pub estimate: Option<i32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleSnapshot {
    pub redacted: bool,
    pub id: Uuid,
    pub cycle_id: Uuid,
    pub local_date: NaiveDate,
    pub captured_at: DateTime<Utc>,
    pub kind: String,
    pub definition_version: i32,
    pub scope_count: i64,
    pub completed_count: i64,
    pub canceled_count: i64,
    pub unfinished_count: i64,
    pub excluded_count: i64,
    pub estimate_total: i64,
    pub items: Vec<CycleScopeItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleClosePreview {
    pub cycle: Cycle,
    pub scope: CycleSnapshot,
    pub review_token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseCycle {
    pub review_token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleCloseReport {
    pub id: Uuid,
    pub cycle: Cycle,
    pub scope: CycleSnapshot,
    pub closed_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CarryForwardItem {
    pub issue_id: Uuid,
    pub issue_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CarryForwardCycle {
    pub report_id: Uuid,
    pub destination_cycle_id: Uuid,
    pub destination_etag: String,
    pub items: Vec<CarryForwardItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeCycleCarry {
    pub destination_etag: String,
    pub items: Vec<CarryForwardItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleCarryResult {
    pub issue_id: Uuid,
    pub outcome: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleCarryOperation {
    pub redacted: bool,
    pub id: Uuid,
    pub source_cycle_id: Uuid,
    pub destination_cycle_id: Uuid,
    pub report_id: Uuid,
    pub version: String,
    pub etag: String,
    pub state: String,
    pub results: Vec<CycleCarryResult>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
pub fn validate_cycle_dates(start: NaiveDate, end: NaiveDate) -> Result<()> {
    if start >= end || (end - start).num_days() > 366 {
        return Err(Error::Invalid(
            "Cycle dates require an exclusive end after the start, within 366 days".into(),
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusive_dates_and_strict_payloads() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        assert!(validate_cycle_dates(d, d).is_err());
        assert!(validate_cycle_dates(d, d.succ_opt().unwrap()).is_ok());
        assert!(serde_json::from_str::<UpdateCycle>(r#"{"start_date":null}"#).is_err());
        assert!(serde_json::from_str::<UpdateCycle>(r#"{"state":"closed"}"#).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleIssue {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    pub status_id: Uuid,
    pub status_name: String,
    pub status_category: String,
    pub priority: i16,
    #[schema(required = true)]
    pub estimate: Option<i32>,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub version: String,
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CycleSnapshotPoint {
    pub redacted: bool,
    pub id: Uuid,
    pub cycle_id: Uuid,
    pub local_date: NaiveDate,
    pub captured_at: DateTime<Utc>,
    pub kind: String,
    pub definition_version: i32,
    pub scope_count: i64,
    pub completed_count: i64,
    pub canceled_count: i64,
    pub unfinished_count: i64,
    pub excluded_count: i64,
    pub estimate_total: i64,
}
