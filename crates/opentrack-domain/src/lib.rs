mod milestones;
pub use milestones::*;
mod cycles;
pub use cycles::*;
mod projects;
pub use projects::*;
mod preferences;
pub use preferences::*;
mod mentions;
pub use mentions::*;
mod attachments;
pub use attachments::*;
mod templates;
pub use templates::*;
mod documents;
pub use documents::*;
mod deletion;
pub use deletion::*;
mod graph;
mod native;
mod ordering;
use chrono::{DateTime, NaiveDate, Utc};
pub use graph::*;
pub use native::*;
pub use ordering::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("This document schema is unsupported. Update the client before editing")]
    UnsupportedDocumentSchema,
    #[error("This page cursor expired. Reload the list to continue")]
    CursorExpired,
    #[error("This recovery code is invalid or no longer available")]
    InvalidRecovery,
    #[error("This native sign-in request is invalid or no longer available")]
    InvalidNativeAuth,
    #[error("Sign in to continue")]
    Unauthorized,
    #[error("You do not have permission for this action")]
    Forbidden,
    #[error("This item could not be found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("This item changed since you opened it")]
    Stale(Box<Issue>),
    #[error("This item changed since you opened it")]
    VersionConflict(Value),
    #[error("Sign in to your existing account before accepting this invitation")]
    InviteSignInRequired,
    #[error("Sign in again before changing access settings")]
    ReauthenticationRequired,
    #[error("An If-Match version is required")]
    PreconditionRequired,
    #[error("Too many attempts. Try again later")]
    RateLimited,
    #[error("Service temporarily unavailable")]
    Unavailable,
    #[error("An internal error occurred")]
    Internal,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub role: String,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct Session {
    pub audience: SessionAudience,
    pub user: User,
    pub csrf_token: String,
    pub workspaces: Vec<Workspace>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionInfo {
    pub id: Uuid,
    pub audience: SessionAudience,
    #[schema(required = true)]
    pub device_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub auth_time: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
    pub current: bool,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoveryComplete {
    pub token: String,
    pub password: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecoveryReceipt {
    pub id: Uuid,
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionQuery {
    #[schema(minimum = 1, maximum = 100)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
impl SessionQuery {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.limit.unwrap_or(50)) {
            return Err(Error::Invalid("Page size must be between 1 and 100".into()));
        }
        if self
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > 512)
        {
            return Err(Error::Invalid("Invalid session page cursor".into()));
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Actor {
    pub audience: SessionAudience,
    pub user: User,
    pub token_hash: String,
    pub csrf_token: String,
}
impl std::fmt::Debug for Actor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actor")
            .field("user_id", &self.user.id)
            .field("audience", &self.audience)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("user_id", &self.user.id)
            .field("audience", &self.audience)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub key: String,
    pub private: bool,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Status {
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    #[schema(value_type = StatusCategory)]
    pub category: String,
    #[schema(value_type = StatusColor)]
    pub color: String,
    pub position: i32,
    pub is_default: bool,
    #[schema(required = true)]
    pub retiring_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub replacement_status_id: Option<Uuid>,
    #[schema(required = true)]
    pub replacement_job_id: Option<Uuid>,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Issue {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    pub key: String,
    pub title: String,
    pub description: String,
    #[serde(default = "default_body_version")]
    pub body_version: String,
    pub status_id: Uuid,
    pub priority: i16,
    #[schema(required = true)]
    pub assignee_id: Option<Uuid>,
    #[serde(default)]
    #[schema(required = true)]
    pub assignee: Option<IssueAssignee>,
    #[serde(default)]
    #[schema(required = true)]
    pub label_ids: Vec<Uuid>,
    #[serde(default)]
    #[schema(required = true)]
    pub labels: Vec<LabelSummary>,
    #[serde(default)]
    #[schema(required = true, read_only = true)]
    pub etag: String,
    #[schema(required = true)]
    pub estimate: Option<i32>,
    #[schema(required = true)]
    pub due_date: Option<NaiveDate>,
    pub version: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
}
fn default_body_version() -> String {
    "0".into()
}
impl Issue {
    pub fn etag(&self) -> String {
        let references = serde_json::json!({"assignee": self.assignee, "labels": self.labels,"body_version":self.body_version,"description":self.description});
        format!(
            "\"issue:{}:{}:{:x}\"",
            self.id,
            self.version,
            Sha256::digest(references.to_string().as_bytes())
        )
    }
    pub fn refresh_etag(&mut self) {
        self.etag = self.etag();
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LabelSummary {
    pub id: Uuid,
    #[schema(required = true)]
    pub team_id: Option<Uuid>,
    pub name: String,
    #[schema(value_type = StatusColor)]
    pub color: String,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Label {
    pub id: Uuid,
    #[schema(required = true)]
    pub team_id: Option<Uuid>,
    pub name: String,
    #[schema(value_type = StatusColor)]
    pub color: String,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub version: String,
    pub can_manage: bool,
}
impl Label {
    pub fn etag(&self) -> String {
        format!("\"label:{}:{}:{}\"", self.id, self.version, self.can_manage)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateLabel {
    pub team_id: Option<Uuid>,
    #[schema(min_length = 1, max_length = 100)]
    pub name: String,
    #[serde(default = "default_label_color")]
    #[schema(value_type = StatusColor, default = "gray")]
    pub color: String,
}
fn default_label_color() -> String {
    "gray".into()
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateLabel {
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false, min_length = 1, max_length = 100)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(value_type = Option<StatusColor>, nullable = false)]
    pub color: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub archived: Option<bool>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LabelQuery {
    pub scope: Option<String>,
    pub team_id: Option<Uuid>,
    pub state: Option<String>,
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssignableUser {
    pub id: Uuid,
    pub display_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueAssignee {
    pub id: Uuid,
    pub display_name: String,
    pub assignable: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssigneeQuery {
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssigneePage {
    pub data: Vec<AssignableUser>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub self_assignable: bool,
}
pub fn normalize_label_ids(ids: &mut Vec<Uuid>) -> Result<()> {
    if ids.len() > 50 {
        return Err(Error::Invalid("At most 50 labels may be selected".into()));
    }
    ids.sort_unstable();
    ids.dedup();
    if ids.len() > 50 {
        return Err(Error::Invalid("At most 50 labels may be selected".into()));
    }
    Ok(())
}
fn deserialize_label_filter<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Uuid>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Csv(String),
        Ids(Vec<Uuid>),
    }
    let mut ids = match Input::deserialize(deserializer)? {
        Input::Ids(ids) => ids,
        Input::Csv(csv) => {
            if csv.is_empty() || csv.len() > 3700 {
                return Err(serde::de::Error::custom("Invalid label IDs"));
            }
            csv.split(',')
                .map(|id| {
                    id.parse()
                        .map_err(|_| serde::de::Error::custom("Invalid label ID"))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?
        }
    };
    normalize_label_ids(&mut ids).map_err(serde::de::Error::custom)?;
    Ok(ids)
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Bootstrap {
    pub auth_epoch: String,
    pub teams: Vec<Team>,
    pub statuses: Vec<Status>,
    pub members: Vec<User>,
    pub issues: Vec<Issue>,
}
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Page<T> {
    pub data: Vec<T>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub setup_token: String,
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub workspace_name: String,
    pub team_name: String,
    pub team_key: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Login {
    pub email: String,
    pub password: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIssue {
    pub team_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentWrite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<TemplateSelection>,
    pub status_id: Option<Uuid>,
    #[serde(default)]
    pub priority: i16,
    pub assignee_id: Option<Uuid>,
    pub estimate: Option<i32>,
    pub due_date: Option<NaiveDate>,
    pub mutation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schema(max_items = 50)]
    pub label_ids: Vec<Uuid>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schema(nullable = false)]
    pub parent_id: Option<Uuid>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schema(nullable = false, min_length = 1, max_length = 256)]
    pub parent_etag: Option<String>,
}
impl CreateIssue {
    pub fn validate(&mut self) -> Result<()> {
        if let Some(template) = &self.template
            && (template
                .version
                .parse::<i64>()
                .ok()
                .is_none_or(|v| v < 1 || v.to_string() != template.version)
                || self.status_id.is_none()
                || self.document.is_none())
        {
            return Err(Error::Invalid("A template selection requires its canonical positive version and explicit resolved status and document".into()));
        }
        if let Some(document) = &self.document {
            if !self.description.is_empty() {
                return Err(Error::Invalid(
                    "Provide document or legacy description, not both".into(),
                ));
            }
            if document.moderation_reason.is_some() {
                return Err(Error::Invalid(
                    "Moderation reason does not apply to issue creation".into(),
                ));
            }
            document.validate()?;
        }
        self.title = bounded_text(&self.title, "Title", 256)?;
        validate_parent_pair(self.parent_id, self.parent_etag.as_deref())?;
        normalize_label_ids(&mut self.label_ids)?;
        validate_issue_values(&self.description, self.priority, self.estimate)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct IssueFilter {
    pub team_id: Option<Uuid>,
    pub status_id: Option<Uuid>,
    pub assignee_id: Option<Uuid>,
    pub priority: Option<i16>,
    pub q: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, deserialize_with = "deserialize_label_filter")]
    pub label_ids: Vec<Uuid>,
    pub label_mode: Option<String>,
    pub sort: Option<String>,
}
impl IssueFilter {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.limit.unwrap_or(50))
            || self.priority.is_some_and(|p| !(0..=4).contains(&p))
            || self.q.as_ref().is_some_and(|q| q.chars().count() > 256)
            || self.label_ids.len() > 50
            || !["any", "all"].contains(&self.label_mode.as_deref().unwrap_or("any"))
            || !["created", "priority"].contains(&self.sort.as_deref().unwrap_or("created"))
        {
            return Err(Error::Invalid("Invalid issue filter or page size".into()));
        }
        Ok(())
    }
}

pub fn apply_issue_patch(issue: &Issue, patch: &Value) -> Result<Issue> {
    let fields = patch
        .as_object()
        .ok_or_else(|| Error::Invalid("Expected an object".into()))?;
    if fields.is_empty() {
        return Err(Error::Invalid("Provide a field to update".into()));
    }
    let mut result = issue.clone();
    for (key, value) in fields {
        match key.as_str() {
            "title" => {
                result.title = bounded_text(
                    value
                        .as_str()
                        .ok_or_else(|| Error::Invalid("Title must be text".into()))?,
                    "Title",
                    256,
                )?
            }
            "description" => {
                result.description = value
                    .as_str()
                    .ok_or_else(|| Error::Invalid("Description must be text".into()))?
                    .into()
            }
            "status_id" => {
                result.status_id = serde_json::from_value(value.clone())
                    .map_err(|_| Error::Invalid("Invalid status".into()))?
            }
            "priority" => {
                result.priority = serde_json::from_value(value.clone())
                    .map_err(|_| Error::Invalid("Invalid priority".into()))?
            }
            "assignee_id" => {
                result.assignee_id = serde_json::from_value(value.clone())
                    .map_err(|_| Error::Invalid("Invalid assignee".into()))?
            }
            "label_ids" => {
                result.label_ids = serde_json::from_value(value.clone())
                    .map_err(|_| Error::Invalid("Labels must be a list of IDs".into()))?;
                normalize_label_ids(&mut result.label_ids)?;
            }
            "estimate" => {
                result.estimate = serde_json::from_value(value.clone())
                    .map_err(|_| Error::Invalid("Invalid estimate".into()))?
            }
            "due_date" => {
                result.due_date = serde_json::from_value(value.clone())
                    .map_err(|_| Error::Invalid("Invalid due date".into()))?
            }
            "archived" => {
                result.archived_at = if value
                    .as_bool()
                    .ok_or_else(|| Error::Invalid("Archived must be true or false".into()))?
                {
                    Some(Utc::now())
                } else {
                    None
                }
            }
            _ => return Err(Error::Invalid(format!("Unknown field: {key}"))),
        }
    }
    validate_issue_values(&result.description, result.priority, result.estimate)?;
    Ok(result)
}
fn validate_issue_values(description: &str, priority: i16, estimate: Option<i32>) -> Result<()> {
    if description.len() > 262144 {
        return Err(Error::Invalid("Description exceeds 256 KiB".into()));
    }
    if !(0..=4).contains(&priority) {
        return Err(Error::Invalid("Priority must be between 0 and 4".into()));
    }
    if estimate.is_some_and(|v| !(0..=1000).contains(&v)) {
        return Err(Error::Invalid("Estimate must be between 0 and 1000".into()));
    }
    Ok(())
}
pub fn bounded_text(value: &str, name: &str, max: usize) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max || value.chars().any(|c| c.is_control()) {
        Err(Error::Invalid(format!(
            "{name} must contain 1–{max} characters without control characters"
        )))
    } else {
        Ok(value.into())
    }
}
pub fn normalize_email(value: &str) -> Result<String> {
    let email = value.trim().to_lowercase();
    if email.len() > 254
        || !email.contains('@')
        || email.starts_with('@')
        || email.ends_with('@')
        || email.chars().any(char::is_whitespace)
    {
        return Err(Error::Invalid("Enter a valid email address".into()));
    }
    Ok(email)
}
pub fn validate_password(value: &str) -> Result<()> {
    if value.chars().count() < 12 || value.len() > 1024 {
        Err(Error::Invalid(
            "Use a password of at least 12 characters (maximum 1024 bytes)".into(),
        ))
    } else {
        Ok(())
    }
}
pub fn validate_team_key(value: &str) -> Result<String> {
    let value = value.trim().to_uppercase();
    if !(2..=10).contains(&value.len())
        || !value.starts_with(|c: char| c.is_ascii_uppercase())
        || !value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(Error::Invalid(
            "Team key must be 2–10 letters or numbers, starting with a letter".into(),
        ));
    }
    Ok(value)
}
pub fn writable(role: &str) -> bool {
    matches!(role, "owner" | "admin" | "member")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_title_limit_is_characters() {
        assert!(bounded_text(&"é".repeat(256), "Title", 256).is_ok());
        assert!(bounded_text(&"é".repeat(257), "Title", 256).is_err());
    }
    #[test]
    fn password_accepts_spaces_and_paste() {
        assert!(validate_password("a long pasted passphrase 🦀").is_ok());
        assert!(validate_password("short").is_err());
    }
    #[test]
    fn team_keys_and_roles() {
        assert_eq!(validate_team_key("eng").unwrap(), "ENG");
        assert!(validate_team_key("1BAD").is_err());
        assert!(!writable("viewer"));
        assert!(!writable("organization_admin"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OrganizationSummary {
    pub id: Uuid,
    pub name: String,
    pub role: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkspaceDetail {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub slug: String,
    pub role: String,
    pub version: String,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
}
impl WorkspaceDetail {
    pub fn etag(&self) -> String {
        format!("\"workspace:{}:{}\"", self.id, self.version)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeamDetail {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub key: String,
    pub private: bool,
    pub version: String,
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub can_manage: bool,
}
impl TeamDetail {
    pub fn etag(&self) -> String {
        format!("\"team:{}:{}\"", self.id, self.version)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkspaceMember {
    pub user: User,
    pub role: String,
    pub active: bool,
    pub version: String,
}
impl WorkspaceMember {
    pub fn etag(&self, workspace: Uuid) -> String {
        format!("\"member:{}:{}:{}\"", workspace, self.user.id, self.version)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeamMember {
    pub user: User,
    pub role: String,
    pub version: String,
}
impl TeamMember {
    pub fn etag(&self, team: Uuid) -> String {
        format!("\"team-member:{}:{}:{}\"", team, self.user.id, self.version)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Invitation {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub email: String,
    pub role: String,
    pub team_ids: Vec<Uuid>,
    pub expires_at: DateTime<Utc>,
    #[schema(required = true)]
    pub accepted_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub revoked_at: Option<DateTime<Utc>>,
    pub version: String,
}
impl Invitation {
    pub fn etag(&self) -> String {
        format!("\"invitation:{}:{}\"", self.id, self.version)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvitationIssued {
    pub invitation: Invitation,
    /// Secret is returned only at first issuance; idempotent replay returns null.
    #[schema(required = true)]
    pub token: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InvitationPreview {
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub email: String,
    pub role: String,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectoryQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub state: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TeamQuery {
    #[schema(max_length = 100)]
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub state: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkspace {
    pub organization_id: Uuid,
    pub name: String,
    pub team_name: String,
    pub team_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkspace {
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub archived: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTeam {
    pub name: String,
    pub key: String,
    pub private: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTeam {
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub private: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub archived: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMember {
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub role: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub active: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutTeamMember {
    pub role: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateInvitation {
    pub email: String,
    pub role: String,
    #[serde(default)]
    pub team_ids: Vec<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptInvitation {
    pub token: String,
    pub display_name: Option<String>,
    pub password: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InvitationToken {
    pub token: String,
}

// A missing patch field is unchanged. A present non-nullable field must contain
// its actual value; explicit null must not silently become an omitted update.
fn deserialize_present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkspaceAccess {
    pub auth_epoch: String,
    pub role: String,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Workflow {
    pub team_id: Uuid,
    pub version: String,
    pub can_manage: bool,
    pub statuses: Vec<Status>,
}
impl Workflow {
    pub fn etag(&self) -> String {
        format!(
            "\"workflow:{}:{}:{}\"",
            self.team_id, self.version, self.can_manage
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStatus {
    pub name: String,
    #[schema(value_type = StatusCategory)]
    pub category: String,
    #[schema(value_type = StatusColor)]
    pub color: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateStatus {
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    #[schema(value_type = Option<StatusColor>)]
    pub color: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDefaultStatus {
    pub status_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReorderStatuses {
    pub status_ids: Vec<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceStatus {
    pub replacement_status_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowReplacementPreview {
    pub team_id: Uuid,
    pub source_status_id: Uuid,
    pub replacement_status_id: Uuid,
    pub issue_count: String,
    pub source_is_default: bool,
    pub workflow_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowReplacement {
    pub id: Uuid,
    pub team_id: Uuid,
    pub source_status_id: Uuid,
    pub replacement_status_id: Uuid,
    #[schema(value_type = WorkflowJobState)]
    pub state: String,
    pub matched_count: String,
    pub processed_count: String,
    pub remaining_count: String,
    pub version: String,
    #[schema(required = true)]
    #[schema(value_type = Option<WorkflowJobError>)]
    pub error_code: Option<String>,
}
impl WorkflowReplacement {
    pub fn etag(&self) -> String {
        format!("\"workflow-replacement:{}:{}\"", self.id, self.version)
    }
}
pub const STATUS_CATEGORIES: &[&str] = &[
    "triage",
    "backlog",
    "unstarted",
    "started",
    "completed",
    "canceled",
];
pub const STATUS_COLORS: &[&str] = &[
    "gray", "slate", "red", "orange", "amber", "green", "teal", "blue", "indigo", "purple", "pink",
];

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusCategory {
    Triage,
    Backlog,
    Unstarted,
    Started,
    Completed,
    Canceled,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusColor {
    Gray,
    Slate,
    Red,
    Orange,
    Amber,
    Green,
    Teal,
    Blue,
    Indigo,
    Purple,
    Pink,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowJobState {
    Queued,
    Running,
    RetryWait,
    Blocked,
    DeadLetter,
    Succeeded,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowJobError {
    AccessLost,
    ParentArchived,
    TargetUnavailable,
    DependencyUnavailable,
    AttemptAbandoned,
    EffectFailed,
    AttemptLimit,
}

#[cfg(test)]
mod taxonomy_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn taxonomy_wire_rejects_null_and_normalizes_sets_without_changing_old_create_hashes() {
        let team = Uuid::new_v4();
        let label = Uuid::new_v4();
        let mut create: CreateIssue =
            serde_json::from_value(json!({"team_id":team,"title":"Issue"})).unwrap();
        create.validate().unwrap();
        assert!(
            serde_json::to_value(&create)
                .unwrap()
                .get("label_ids")
                .is_none()
        );
        assert!(
            serde_json::from_value::<CreateIssue>(
                json!({"team_id":team,"title":"Issue","label_ids":null})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<UpdateLabel>(json!({"name":null})).is_err());
        assert!(serde_json::from_value::<UpdateLabel>(json!({"color":null})).is_err());
        assert!(serde_json::from_value::<UpdateLabel>(json!({"archived":null})).is_err());
        let csv: IssueFilter =
            serde_json::from_value(json!({"label_ids":format!("{label},{label}")})).unwrap();
        let array: IssueFilter =
            serde_json::from_value(json!({"label_ids":[label,label]})).unwrap();
        assert_eq!(csv.label_ids, array.label_ids);
        assert_eq!(csv.label_ids, vec![label]);
        assert!(serde_json::from_value::<IssueFilter>(json!({"label_ids":null})).is_err());
        assert!(serde_json::from_value::<IssueFilter>(json!({"label_ids":""})).is_err());
        assert!(serde_json::from_value::<IssueFilter>(json!({"label_ids":"invalid"})).is_err());
        assert!(normalize_label_ids(&mut vec![label; 51]).is_err());
    }
    #[test]
    fn taxonomy_issue_etag_changes_with_reference_projection_without_entity_version_change() {
        let id = Uuid::new_v4();
        let user = Uuid::new_v4();
        let now = Utc::now();
        let mut issue:Issue=serde_json::from_value(json!({"id":id,"workspace_id":id,"team_id":id,"key":"T-1","title":"An issue","description":"","status_id":id,"priority":0,"assignee_id":user,"estimate":null,"due_date":null,"version":"1","created_at":now,"updated_at":now,"archived_at":null})).unwrap();
        assert!(issue.etag.is_empty());
        issue.assignee = Some(IssueAssignee {
            id: user,
            display_name: "Historical owner".into(),
            assignable: true,
        });
        issue.refresh_etag();
        let before = issue.etag.clone();
        assert_eq!(issue.etag, issue.etag());
        issue.assignee.as_mut().unwrap().assignable = false;
        issue.refresh_etag();
        assert_ne!(issue.etag, before);
        issue.labels.push(LabelSummary {
            id,
            team_id: None,
            name: "Old label".into(),
            color: "gray".into(),
            archived_at: None,
        });
        issue.label_ids.push(id);
        issue.refresh_etag();
        let before = issue.etag.clone();
        issue.labels[0].name = "New name".into();
        issue.refresh_etag();
        assert_ne!(issue.etag, before);
        let before = issue.etag.clone();
        issue.labels[0].archived_at = Some(now);
        issue.refresh_etag();
        assert_ne!(issue.etag, before);
        assert_eq!(issue.version, "1");
        let value = serde_json::to_value(&issue).unwrap();
        assert_eq!(value["etag"], issue.etag());
        assert!(value["assignee"].get("email").is_none());
    }
}

pub fn validate_parent_pair(parent: Option<Uuid>, etag: Option<&str>) -> Result<()> {
    if parent.is_some() != etag.is_some() || etag.is_some_and(|e| e.is_empty() || e.len() > 256) {
        return Err(Error::Invalid(
            "A parent requires its current ETag; detaching must omit parent_etag".into(),
        ));
    }
    Ok(())
}
