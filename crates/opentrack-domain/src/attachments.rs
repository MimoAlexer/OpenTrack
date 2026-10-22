use crate::{DiscussionQuery, Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAttachment {
    pub original_name: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub mime_hint: Option<String>,
    pub document_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
}
impl CreateAttachment {
    pub fn validate(&self, limits: &AttachmentLimits) -> Result<()> {
        limits.validate()?;
        if self.original_name.trim().is_empty()
            || self.original_name.len() > 255
            || self.original_name.chars().any(char::is_control)
            || self.original_name.contains(['/', '\\'])
            || matches!(self.original_name.as_str(), "." | "..")
        {
            return Err(Error::Invalid(
                "Use a filename of 1–255 bytes without paths or control characters".into(),
            ));
        }
        if self.size_bytes <= 0 || self.size_bytes > limits.max_upload_bytes {
            return Err(Error::Invalid(
                "This file exceeds the configured upload size limit".into(),
            ));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::Invalid(
                "Provide the file's lowercase SHA-256 checksum".into(),
            ));
        }
        if self.document_id.is_some() == self.team_id.is_some() {
            return Err(Error::Invalid(
                "Select exactly one document or draft team for the attachment".into(),
            ));
        }
        if self
            .mime_hint
            .as_ref()
            .is_some_and(|m| !valid_attachment_mime(m))
        {
            return Err(Error::Invalid("Invalid MIME hint".into()));
        }
        Ok(())
    }
}
pub fn valid_attachment_mime(value: &str) -> bool {
    let Some((major, minor)) = value.split_once('/') else {
        return false;
    };
    !major.is_empty()
        && !minor.is_empty()
        && value.len() <= 127
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-/".contains(&b))
        && !minor.contains('/')
}

#[derive(Debug, Clone, Copy)]
pub struct AttachmentLimits {
    pub max_upload_bytes: i64,
    pub workspace_quota_bytes: i64,
    pub unlinked_ttl_seconds: u32,
    pub upload_lease_seconds: u32,
    pub antivirus_required: bool,
}
impl Default for AttachmentLimits {
    fn default() -> Self {
        Self {
            max_upload_bytes: 25 * 1024 * 1024,
            workspace_quota_bytes: 5 * 1024 * 1024 * 1024,
            unlinked_ttl_seconds: 24 * 60 * 60,
            upload_lease_seconds: 300,
            antivirus_required: false,
        }
    }
}
impl AttachmentLimits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100 * 1024 * 1024).contains(&self.max_upload_bytes)
            || self.workspace_quota_bytes <= 0
            || !(60..=7 * 24 * 60 * 60).contains(&self.unlinked_ttl_seconds)
            || !(5..=900).contains(&self.upload_lease_seconds)
        {
            return Err(Error::Invalid("Invalid attachment limits".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentState {
    Pending,
    Quarantined,
    Ready,
    Rejected,
    Deleted,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentScanStatus {
    Pending,
    Clean,
    NotScanned,
    Infected,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Attachment {
    pub id: Uuid,
    #[schema(required = true)]
    pub document_id: Option<Uuid>,
    pub original_name: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub state: AttachmentState,
    #[schema(required = true)]
    pub detected_mime: Option<String>,
    pub scan_status: AttachmentScanStatus,
    pub version: String,
    pub etag: String,
    #[schema(required = true)]
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub can_delete: bool,
}
impl Attachment {
    pub fn refresh_etag(&mut self) {
        self.etag = format!(
            "\"attachment:{}:{}:{}\"",
            self.id, self.version, self.can_delete
        );
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachmentQuery {
    pub document_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
impl AttachmentQuery {
    pub fn validate(&self) -> Result<()> {
        if self.document_id.is_some() == self.team_id.is_some() {
            return Err(Error::Invalid(
                "Select exactly one document or draft team".into(),
            ));
        }
        DiscussionQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
        }
        .validate()
    }
}
