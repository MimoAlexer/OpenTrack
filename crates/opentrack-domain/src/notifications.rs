use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationReason {
    Assignment,
    Mention,
    SubscribedComment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSourceKind {
    Issue,
    Project,
}

/// Source labels and routing metadata are resolved under current access on every read.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NotificationSource {
    pub kind: NotificationSourceKind,
    pub id: Uuid,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NotificationItem {
    pub id: Uuid,
    /// Recipient inbox sequence, independent of when the source event was written.
    pub seq: String,
    pub reason: NotificationReason,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<DateTime<Utc>>,
    pub version: String,
    pub etag: String,
    pub source: NotificationSource,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationFilter {
    #[default]
    Unread,
    All,
    Archived,
    Snoozed,
}
impl NotificationFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::All => "all",
            Self::Archived => "archived",
            Self::Snoozed => "snoozed",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationQuery {
    pub filter: Option<NotificationFilter>,
    #[schema(minimum = 1, maximum = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NotificationPage {
    pub items: Vec<NotificationItem>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
    /// Signed, recipient-scoped upper bound for mark-read. Treat as opaque.
    pub snapshot: String,
    pub unread_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAction {
    Read,
    Unread,
    Archive,
    Unarchive,
    Snooze,
    Unsnooze,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNotification {
    pub action: NotificationAction,
    /// Required only for snooze; UTC instant within the next 90 days.
    pub until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkNotificationsRead {
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NotificationsMarkedRead {
    pub updated_count: u64,
    pub unread_count: i64,
    /// More delivered items remain below the same snapshot; retry that snapshot.
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NotificationPreferences {
    pub assignments: bool,
    pub mentions: bool,
    pub subscribed_comments: bool,
    pub version: String,
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNotificationPreferences {
    pub assignments: bool,
    pub mentions: bool,
    pub subscribed_comments: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn notification_commands_reject_recipient_overrides_and_unknown_actions() {
        for value in [
            json!({"action":"read","user_id":Uuid::nil()}),
            json!({"action":"delete"}),
            json!({"action":"snooze","until":"later"}),
        ] {
            assert!(serde_json::from_value::<UpdateNotification>(value).is_err());
        }
        assert!(
            serde_json::from_value::<MarkNotificationsRead>(
                json!({"snapshot":"x","user_id":Uuid::nil()})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<NotificationQuery>(json!({"user_id":Uuid::nil()})).is_err()
        );
        assert!(serde_json::from_value::<UpdateNotificationPreferences>(json!({"assignments":true,"mentions":true,"subscribed_comments":true,"user_id":Uuid::nil()})).is_err());
    }
}
