use super::*;
use chrono::{DateTime, Duration, Utc};
use opentrack_domain::*;
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};

#[derive(Serialize, Deserialize)]
struct InboxCursor {
    snapshot: String,
    before: i64,
}

fn notification_row(w: Uuid, u: Uuid, row: &PgRow) -> Result<NotificationItem> {
    let id: Uuid = row.try_get("id").map_err(db_error)?;
    let version: i64 = row.try_get("version").map_err(db_error)?;
    Ok(NotificationItem {
        id,
        seq: row
            .try_get::<i64, _>("inbox_seq")
            .map_err(db_error)?
            .to_string(),
        reason: match row.try_get::<&str, _>("reason").map_err(db_error)? {
            "assignment" => NotificationReason::Assignment,
            "mention" => NotificationReason::Mention,
            "subscribed_comment" => NotificationReason::SubscribedComment,
            _ => return Err(Error::Internal),
        },
        created_at: row.try_get("created_at").map_err(db_error)?,
        read_at: row.try_get("read_at").map_err(db_error)?,
        archived_at: row.try_get("archived_at").map_err(db_error)?,
        snoozed_until: row.try_get("snoozed_until").map_err(db_error)?,
        version: version.to_string(),
        etag: format!("\"notification:{w}:{u}:{id}:{version}\""),
        source: NotificationSource {
            kind: match row.try_get::<&str, _>("entity_kind").map_err(db_error)? {
                "issue" => NotificationSourceKind::Issue,
                "project" => NotificationSourceKind::Project,
                _ => return Err(Error::Internal),
            },
            id: row.try_get("entity_id").map_err(db_error)?,
            available: false,
            team_id: None,
            document_id: None,
            comment_id: None,
            key: None,
            title: None,
            actor_display_name: None,
        },
    })
}

/// No body/history copies: source labels are selected only after both the original
/// audience and the current source audience pass their ordinary access rules.
async fn project_source(
    tx: &mut store::Tx,
    w: Uuid,
    u: Uuid,
    item: &mut NotificationItem,
) -> Result<()> {
    let source = sqlx::query(
        "SELECT t.id AS team_id,
                CASE WHEN n.entity_kind='issue' THEN t.key||'-'||i.number ELSE NULL END AS key,
                CASE WHEN n.entity_kind='issue' THEN i.title ELSE p.name END AS title,
                a.display_name,n.document_id,n.comment_id
         FROM notifications n
         LEFT JOIN issues i ON n.entity_kind='issue' AND i.workspace_id=n.workspace_id AND i.id=n.entity_id AND i.deleted_at IS NULL
         LEFT JOIN projects p ON n.entity_kind='project' AND p.workspace_id=n.workspace_id AND p.id=n.entity_id
         JOIN teams t ON t.workspace_id=n.workspace_id AND t.id=coalesce(i.team_id,p.team_id)
         JOIN teams origin ON origin.workspace_id=n.workspace_id AND origin.id=n.origin_team_id
         LEFT JOIN users a ON a.id=n.actor_id
         WHERE n.workspace_id=$1 AND n.user_id=$2 AND n.id=$3
           AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=t.id AND tm.user_id=$2))
           AND (NOT origin.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=origin.id AND tm.user_id=$2))
           AND (n.comment_id IS NULL OR EXISTS(
             SELECT 1 FROM comments c JOIN teams ct ON ct.workspace_id=c.workspace_id AND ct.id=c.origin_team_id
             WHERE c.workspace_id=$1 AND c.id=n.comment_id AND c.issue_id=i.id AND c.deleted_at IS NULL
               AND (NOT ct.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=ct.id AND tm.user_id=$2))))
           AND (n.document_id IS NULL OR EXISTS(
             SELECT 1 FROM documents d WHERE d.workspace_id=$1 AND d.id=n.document_id AND d.deleted_at IS NULL
               AND ((n.entity_kind='issue' AND (d.issue_id=i.id OR d.comment_id=n.comment_id))
                 OR (n.entity_kind='project' AND d.project_id=p.id))))",
    ).bind(w).bind(u).bind(item.id).fetch_optional(&mut **tx).await.map_err(db_error)?;
    if let Some(row) = source {
        item.source.available = true;
        item.source.team_id = Some(row.try_get("team_id").map_err(db_error)?);
        item.source.document_id = row.try_get("document_id").map_err(db_error)?;
        item.source.comment_id = row.try_get("comment_id").map_err(db_error)?;
        item.source.key = row.try_get("key").map_err(db_error)?;
        item.source.title = row.try_get("title").map_err(db_error)?;
        item.source.actor_display_name = row.try_get("display_name").map_err(db_error)?;
    }
    Ok(())
}

async fn inbox_count(tx: &mut store::Tx, w: Uuid, u: Uuid, now: DateTime<Utc>) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND read_at IS NULL AND archived_at IS NULL AND (snoozed_until IS NULL OR snoozed_until<=$3)")
        .bind(w).bind(u).bind(now).fetch_one(&mut **tx).await.map_err(db_error)
}

/// Persist the private stream intent with the inbox change. Admission/backlog can
/// delay publication without blocking personal inbox operations.
async fn append_inbox_invalidation(
    tx: &mut store::Tx,
    w: Uuid,
    u: Uuid,
    id: Uuid,
    version: i64,
) -> Result<()> {
    graph::one(sqlx::query("INSERT INTO notification_invalidations(workspace_id,user_id,notification_id,notification_version,event_kind) VALUES($1,$2,$3,$4,'notification.updated') ON CONFLICT DO NOTHING")
        .bind(w).bind(u).bind(id).bind(version).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}

async fn preferences(tx: &mut store::Tx, w: Uuid, u: Uuid) -> Result<NotificationPreferences> {
    let row = sqlx::query("SELECT assignments,mentions,subscribed_comments,version FROM notification_preferences WHERE workspace_id=$1 AND user_id=$2")
        .bind(w).bind(u).fetch_optional(&mut **tx).await.map_err(db_error)?;
    let (assignments, mentions, subscribed_comments, version) = match row {
        Some(row) => (
            row.try_get("assignments").map_err(db_error)?,
            row.try_get("mentions").map_err(db_error)?,
            row.try_get("subscribed_comments").map_err(db_error)?,
            row.try_get::<i64, _>("version").map_err(db_error)?,
        ),
        None => (true, true, true, 0),
    };
    Ok(NotificationPreferences {
        assignments,
        mentions,
        subscribed_comments,
        version: version.to_string(),
        etag: format!("\"notification-preferences:{w}:{u}:{version}\""),
    })
}

impl App {
    pub async fn notifications(
        &self,
        actor: &Actor,
        w: Uuid,
        query: NotificationQuery,
    ) -> Result<NotificationPage> {
        let limit = query.limit.unwrap_or(50);
        if !(1..=50).contains(&limit) {
            return Err(Error::Invalid(
                "Notification page size must be between 1 and 50".into(),
            ));
        }
        let filter = query.filter.unwrap_or_default();
        let mut tx = self.scoped(actor, w, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let scope = format!("{w}:{}:{epoch}", actor.user.id);
        let binding = format!("{scope}:{}:{limit}", filter.as_str());
        let cursor: Option<InboxCursor> = query
            .cursor
            .as_deref()
            .map(|value| self.taxonomy_cursor_decode("notification-page", &binding, value))
            .transpose()?;
        let (snapshot, upper, before) = match cursor {
            Some(cursor) => {
                let upper: i64 =
                    self.taxonomy_cursor_decode("notification-snapshot", &scope, &cursor.snapshot)?;
                if upper < 0 || cursor.before <= 0 || cursor.before > upper {
                    return Err(Error::Invalid("Invalid notification cursor".into()));
                }
                (cursor.snapshot, upper, Some(cursor.before))
            }
            None => {
                let upper: i64 = sqlx::query_scalar("SELECT coalesce((SELECT last_seq FROM notification_heads WHERE workspace_id=$1 AND user_id=$2),0)")
                    .bind(w).bind(actor.user.id).fetch_one(&mut *tx).await.map_err(db_error)?;
                (
                    self.taxonomy_cursor_encode("notification-snapshot", &scope, &upper)?,
                    upper,
                    None,
                )
            }
        };
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let mut rows = sqlx::query("SELECT * FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND inbox_seq<=$3 AND ($4::bigint IS NULL OR inbox_seq<$4) AND CASE $5::text WHEN 'archived' THEN archived_at IS NOT NULL WHEN 'snoozed' THEN archived_at IS NULL AND snoozed_until>$6 ELSE archived_at IS NULL AND (snoozed_until IS NULL OR snoozed_until<=$6) AND ($5='all' OR read_at IS NULL) END ORDER BY inbox_seq DESC LIMIT $7")
            .bind(w).bind(actor.user.id).bind(upper).bind(before).bind(filter.as_str()).bind(now).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let mut item = notification_row(w, actor.user.id, &row)?;
            project_source(&mut tx, w, actor.user.id, &mut item).await?;
            items.push(item);
        }
        let next_cursor = if has_more {
            items
                .last()
                .map(|last| {
                    self.taxonomy_cursor_encode(
                        "notification-page",
                        &binding,
                        &InboxCursor {
                            snapshot: snapshot.clone(),
                            before: last.seq.parse().map_err(|_| Error::Internal)?,
                        },
                    )
                })
                .transpose()?
        } else {
            None
        };
        let unread_count = inbox_count(&mut tx, w, actor.user.id, now).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(NotificationPage {
            items,
            next_cursor,
            snapshot,
            unread_count,
        })
    }

    pub async fn update_notification(
        &self,
        actor: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        request: UpdateNotification,
    ) -> Result<NotificationItem> {
        if request.action != NotificationAction::Snooze && request.until.is_some() {
            return Err(Error::Invalid(
                "Only snooze accepts an until timestamp".into(),
            ));
        }
        let mut tx = self.scoped(actor, w, false).await?;
        let row = sqlx::query(
            "SELECT * FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND id=$3 FOR UPDATE",
        )
        .bind(w)
        .bind(actor.user.id)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
        let mut old = notification_row(w, actor.user.id, &row)?;
        project_source(&mut tx, w, actor.user.id, &mut old).await?;
        lifecycle::version(etag, &old.etag, &old)?;
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let mut read = old.read_at;
        let mut archive = old.archived_at;
        let mut snooze = old.snoozed_until;
        match request.action {
            NotificationAction::Read => {
                read.get_or_insert(now);
            }
            NotificationAction::Unread => read = None,
            NotificationAction::Archive => {
                archive.get_or_insert(now);
            }
            NotificationAction::Unarchive => archive = None,
            NotificationAction::Unsnooze => snooze = None,
            NotificationAction::Snooze => {
                let until = request
                    .until
                    .ok_or_else(|| Error::Invalid("Snooze requires an until timestamp".into()))?;
                // PostgreSQL timestamps retain microseconds. Compare the stored
                // precision so retries with RFC3339 nanoseconds remain no-ops.
                let until = DateTime::from_timestamp_micros(until.timestamp_micros())
                    .ok_or_else(|| Error::Invalid("Invalid snooze timestamp".into()))?;
                if snooze != Some(until) && (until <= now || until > now + Duration::days(90)) {
                    return Err(Error::Invalid(
                        "Snooze must be in the future and within 90 days".into(),
                    ));
                }
                snooze = Some(until);
            }
        }
        if (read, archive, snooze) == (old.read_at, old.archived_at, old.snoozed_until) {
            tx.commit().await.map_err(db_error)?;
            return Ok(old);
        }
        let row = sqlx::query("UPDATE notifications SET read_at=$4,archived_at=$5,snoozed_until=$6,version=version+1 WHERE workspace_id=$1 AND user_id=$2 AND id=$3 RETURNING *")
            .bind(w).bind(actor.user.id).bind(id).bind(read).bind(archive).bind(snooze).fetch_one(&mut *tx).await.map_err(db_error)?;
        let mut result = notification_row(w, actor.user.id, &row)?;
        project_source(&mut tx, w, actor.user.id, &mut result).await?;
        append_inbox_invalidation(
            &mut tx,
            w,
            actor.user.id,
            id,
            result.version.parse().map_err(|_| Error::Internal)?,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }

    pub async fn mark_notifications_read(
        &self,
        actor: &Actor,
        w: Uuid,
        request: MarkNotificationsRead,
    ) -> Result<NotificationsMarkedRead> {
        let mut tx = self.scoped(actor, w, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let scope = format!("{w}:{}:{epoch}", actor.user.id);
        let upper: i64 =
            self.taxonomy_cursor_decode("notification-snapshot", &scope, &request.snapshot)?;
        if upper < 0 {
            return Err(Error::Invalid("Invalid notification snapshot".into()));
        }
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let changed = sqlx::query("WITH batch AS (SELECT id FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND inbox_seq<=$3 AND read_at IS NULL ORDER BY inbox_seq LIMIT 100 FOR UPDATE) UPDATE notifications n SET read_at=$4,version=version+1 FROM batch WHERE n.workspace_id=$1 AND n.user_id=$2 AND n.id=batch.id AND n.read_at IS NULL RETURNING n.id,n.version")
            .bind(w).bind(actor.user.id).bind(upper).bind(now).fetch_all(&mut *tx).await.map_err(db_error)?;
        let unread_count = inbox_count(&mut tx, w, actor.user.id, now).await?;
        let has_more: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND inbox_seq<=$3 AND read_at IS NULL)")
            .bind(w).bind(actor.user.id).bind(upper).fetch_one(&mut *tx).await.map_err(db_error)?;
        if let Some(first) = changed.first() {
            append_inbox_invalidation(
                &mut tx,
                w,
                actor.user.id,
                first.try_get("id").map_err(db_error)?,
                first.try_get("version").map_err(db_error)?,
            )
            .await?;
        }
        let updated_count = changed.len() as u64;
        tx.commit().await.map_err(db_error)?;
        Ok(NotificationsMarkedRead {
            updated_count,
            unread_count,
            has_more,
        })
    }

    pub async fn notification_preferences(
        &self,
        actor: &Actor,
        w: Uuid,
    ) -> Result<NotificationPreferences> {
        let mut tx = self.scoped(actor, w, false).await?;
        let result = preferences(&mut tx, w, actor.user.id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }

    pub async fn update_notification_preferences(
        &self,
        actor: &Actor,
        w: Uuid,
        etag: &str,
        request: UpdateNotificationPreferences,
    ) -> Result<NotificationPreferences> {
        let mut tx = self.scoped(actor, w, false).await?;
        store::lock_user_preferences(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?;
        let old = preferences(&mut tx, w, actor.user.id).await?;
        lifecycle::version(etag, &old.etag, &old)?;
        if (old.assignments, old.mentions, old.subscribed_comments)
            == (
                request.assignments,
                request.mentions,
                request.subscribed_comments,
            )
        {
            tx.commit().await.map_err(db_error)?;
            return Ok(old);
        }
        graph::one(sqlx::query("INSERT INTO notification_preferences(workspace_id,user_id,assignments,mentions,subscribed_comments) VALUES($1,$2,$3,$4,$5) ON CONFLICT(workspace_id,user_id) DO UPDATE SET assignments=EXCLUDED.assignments,mentions=EXCLUDED.mentions,subscribed_comments=EXCLUDED.subscribed_comments,version=notification_preferences.version+1,updated_at=clock_timestamp()")
            .bind(w).bind(actor.user.id).bind(request.assignments).bind(request.mentions).bind(request.subscribed_comments).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let result = preferences(&mut tx, w, actor.user.id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}
