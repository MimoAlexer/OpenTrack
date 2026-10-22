use super::*;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use opentrack_domain::{Page, SessionInfo, SessionQuery};
use sqlx::Row;

type CursorMac = Hmac<Sha256>;

pub(crate) async fn security_event(
    tx: &mut store::Tx,
    user: Uuid,
    session: Uuid,
    related: Option<Uuid>,
    action: &str,
) -> Result<()> {
    security_event_correlated(tx, user, session, related, action, Uuid::new_v4()).await
}
pub(crate) async fn security_event_correlated(
    tx: &mut store::Tx,
    user: Uuid,
    session: Uuid,
    related: Option<Uuid>,
    action: &str,
    correlation: Uuid,
) -> Result<()> {
    let appended = sqlx::query("INSERT INTO account_security_events(user_id,session_id,related_session_id,action,correlation_id) VALUES($1,$2,$3,$4,$5)")
        .bind(user).bind(session).bind(related).bind(action).bind(correlation)
        .execute(&mut **tx).await.map_err(db_error)?;
    if appended.rows_affected() != 1 {
        return Err(Error::Internal);
    }
    Ok(())
}

/// Caller holds the exclusive account guard, or has just inserted this account.
pub(crate) async fn mint_session(tx: &mut store::Tx, user: Uuid) -> Result<(Uuid, String, String)> {
    insert_session(tx, user, None, None).await
}

/// Rotating a cookie without password verification must not extend the previous
/// authentication window or the session's absolute lifetime.
pub(crate) async fn mint_rotated_session(
    tx: &mut store::Tx,
    actor: &Actor,
) -> Result<(Uuid, String, String)> {
    if actor.audience != SessionAudience::Web {
        return Err(Error::Forbidden);
    }
    let prior = sqlx::query("SELECT auth_time,expires_at FROM sessions WHERE user_id=$1 AND token_hash=$2 AND audience='web' AND revoked_at IS NULL")
        .bind(actor.user.id).bind(&actor.token_hash).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::Unauthorized)?;
    insert_session(
        tx,
        actor.user.id,
        Some(prior.try_get("auth_time").map_err(db_error)?),
        Some(prior.try_get("expires_at").map_err(db_error)?),
    )
    .await
}

async fn insert_session(
    tx: &mut store::Tx,
    user: Uuid,
    auth_time: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(Uuid, String, String)> {
    let token = random_secret();
    let csrf = random_secret();
    let id: Uuid = sqlx::query_scalar("INSERT INTO sessions(token_hash,user_id,csrf_token,auth_time,expires_at) VALUES($1,$2,$3,COALESCE($4::timestamptz,now()),COALESCE($5::timestamptz,now()+interval '7 days')) RETURNING id")
        .bind(digest(token.as_bytes())).bind(user).bind(&csrf).bind(auth_time).bind(expires_at)
        .fetch_one(&mut **tx).await.map_err(db_error)?;
    security_event(tx, user, id, None, "session.created").await?;
    Ok((id, token, csrf))
}

/// Hash lookup is also account-bound: signing into another account cannot revoke it.
pub(crate) async fn rotate_previous(
    tx: &mut store::Tx,
    user: Uuid,
    previous_hash: &str,
    replacement: Uuid,
    action: &str,
) -> Result<()> {
    rotate_previous_correlated(tx, user, previous_hash, replacement, action, Uuid::new_v4()).await
}
pub(crate) async fn rotate_previous_correlated(
    tx: &mut store::Tx,
    user: Uuid,
    previous_hash: &str,
    replacement: Uuid,
    action: &str,
    correlation: Uuid,
) -> Result<()> {
    let previous: Option<Uuid> = sqlx::query_scalar("UPDATE sessions SET revoked_at=clock_timestamp() WHERE token_hash=$1 AND user_id=$2 AND audience='web' AND revoked_at IS NULL RETURNING id")
        .bind(previous_hash).bind(user).fetch_optional(&mut **tx).await.map_err(db_error)?;
    if let Some(previous) = previous {
        security_event_correlated(tx, user, previous, Some(replacement), action, correlation)
            .await?;
    }
    Ok(())
}

/// Both account guards and the workspace access guard are already held.
/// user_id in the account audit is the affected account, not the initiator:
/// correlation links these rows to the workspace audit's actual acting user.
pub(crate) async fn revoke_for_elevation(
    tx: &mut store::Tx,
    actor: &Actor,
    target: Uuid,
    keep_session: Option<Uuid>,
    correlation: Uuid,
) -> Result<bool> {
    // Change only the account audit RLS context, without acquiring a later user
    // lock or changing workspace scope. Restore it before workspace audit/events.
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true)")
        .bind(target.to_string())
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    let result = sqlx::query(
        "WITH revoked AS (
           UPDATE sessions SET revoked_at=clock_timestamp()
           WHERE user_id=$1 AND ($2::uuid IS NULL OR id<>$2) AND revoked_at IS NULL
           AND expires_at>clock_timestamp() AND last_seen_at>clock_timestamp()-interval '12 hours'
           RETURNING id,token_hash
         ), audited AS (
           INSERT INTO account_security_events(user_id,session_id,action,correlation_id)
           SELECT $1,id,'session.revoked',$3 FROM revoked RETURNING id
         ) SELECT (SELECT count(*) FROM revoked) AS revoked_count,
           (SELECT count(*) FROM audited) AS audited_count,
           EXISTS(SELECT 1 FROM revoked WHERE token_hash=$4) AS current_revoked",
    )
    .bind(target)
    .bind(keep_session)
    .bind(correlation)
    .bind(&actor.token_hash)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if result
        .try_get::<i64, _>("revoked_count")
        .map_err(db_error)?
        != result
            .try_get::<i64, _>("audited_count")
            .map_err(db_error)?
    {
        return Err(Error::Internal);
    }
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true)")
        .bind(actor.user.id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    result.try_get("current_revoked").map_err(db_error)
}

impl App {
    async fn account_scope(&self, actor: &Actor, exclusive: bool) -> Result<store::Tx> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        if exclusive {
            store::context_exclusive(&mut tx, actor.user.id, None)
                .await
                .map_err(db_error)?;
        } else {
            store::context(&mut tx, actor.user.id, None)
                .await
                .map_err(db_error)?;
        }
        if !store::active_session(&mut tx, actor)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Unauthorized);
        }
        Ok(tx)
    }

    pub async fn sessions(&self, actor: &Actor, query: SessionQuery) -> Result<Page<SessionInfo>> {
        query.validate()?;
        let limit = query.limit.unwrap_or(50);
        let after = query
            .cursor
            .as_deref()
            .map(|cursor| self.session_cursor(actor, limit, cursor))
            .transpose()?;
        let mut tx = self.account_scope(actor, false).await?;
        let rows = sqlx::query("SELECT id,audience,device_name,created_at,auth_time,last_seen_at,expires_at,LEAST(expires_at,last_seen_at+interval '12 hours') AS idle_expires_at,token_hash=$2 AS current FROM sessions WHERE user_id=$1 AND revoked_at IS NULL AND expires_at>clock_timestamp() AND last_seen_at>clock_timestamp()-interval '12 hours' AND ($3::timestamptz IS NULL OR (created_at,id)<($3,$4)) ORDER BY created_at DESC,id DESC LIMIT $5")
            .bind(actor.user.id).bind(&actor.token_hash).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(i64::from(limit)+1)
            .fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut data = rows
            .into_iter()
            .map(|row| {
                Ok(SessionInfo {
                    audience: SessionAudience::parse(
                        row.try_get::<&str, _>("audience").map_err(db_error)?,
                    )
                    .ok_or(Error::Internal)?,
                    device_name: row.try_get("device_name").map_err(db_error)?,
                    id: row.try_get("id").map_err(db_error)?,
                    created_at: row.try_get("created_at").map_err(db_error)?,
                    auth_time: row.try_get("auth_time").map_err(db_error)?,
                    last_seen_at: row.try_get("last_seen_at").map_err(db_error)?,
                    expires_at: row.try_get("expires_at").map_err(db_error)?,
                    idle_expires_at: row.try_get("idle_expires_at").map_err(db_error)?,
                    current: row.try_get("current").map_err(db_error)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let has_more = data.len() > limit as usize;
        data.truncate(limit as usize);
        let next_cursor = if has_more {
            data.last()
                .map(|row| self.encode_session_cursor(actor, limit, row))
                .transpose()?
        } else {
            None
        };
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }

    /// Returns whether the selected row is the caller's current session.
    pub async fn revoke_session(&self, actor: &Actor, id: Uuid) -> Result<bool> {
        let mut tx = self.account_scope(actor, true).await?;
        let row = sqlx::query("SELECT token_hash=$3 AS current,revoked_at IS NOT NULL AS revoked FROM sessions WHERE user_id=$1 AND id=$2 FOR UPDATE")
            .bind(actor.user.id).bind(id).bind(&actor.token_hash).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        let current: bool = row.try_get("current").map_err(db_error)?;
        if !current {
            crate::lifecycle::recent(&mut tx, actor).await?;
        }
        if !row.try_get::<bool, _>("revoked").map_err(db_error)? {
            sqlx::query(
                "UPDATE sessions SET revoked_at=clock_timestamp() WHERE user_id=$1 AND id=$2",
            )
            .bind(actor.user.id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
            security_event(
                &mut tx,
                actor.user.id,
                id,
                None,
                if current {
                    "session.logged_out"
                } else {
                    "session.revoked"
                },
            )
            .await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(current)
    }

    pub async fn revoke_other_sessions(&self, actor: &Actor) -> Result<()> {
        let mut tx = self.account_scope(actor, true).await?;
        crate::lifecycle::recent(&mut tx, actor).await?;
        // The account guard stabilizes the candidate set. Check both the effect
        // and its mandatory audit, including BEFORE triggers that return NULL.
        let (expected, revoked, audited): (i64, i64, i64) = sqlx::query_as(
            "WITH eligible AS MATERIALIZED (
               SELECT id FROM sessions WHERE user_id=$1 AND token_hash<>$2
               AND revoked_at IS NULL AND expires_at>clock_timestamp()
               AND last_seen_at>clock_timestamp()-interval '12 hours'
             ), revoked AS (
               UPDATE sessions s SET revoked_at=clock_timestamp() FROM eligible e
               WHERE s.id=e.id RETURNING s.id
             ), audited AS (
               INSERT INTO account_security_events(user_id,session_id,action)
               SELECT $1,id,'session.revoked' FROM revoked RETURNING id
             ) SELECT (SELECT count(*) FROM eligible),(SELECT count(*) FROM revoked),
               (SELECT count(*) FROM audited)",
        )
        .bind(actor.user.id)
        .bind(&actor.token_hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if expected != revoked || revoked != audited {
            return Err(Error::Internal);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    pub async fn logout(&self, actor: &Actor) -> Result<()> {
        let mut tx = self.account_scope(actor, true).await?;
        let id: Uuid = sqlx::query_scalar("UPDATE sessions SET revoked_at=clock_timestamp() WHERE user_id=$1 AND token_hash=$2 AND revoked_at IS NULL RETURNING id")
            .bind(actor.user.id).bind(&actor.token_hash).fetch_one(&mut *tx).await.map_err(db_error)?;
        security_event(&mut tx, actor.user.id, id, None, "session.logged_out").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    fn cursor_mac(&self, actor: &Actor, limit: u32, payload: &str) -> Result<CursorMac> {
        let mut mac = CursorMac::new_from_slice(self.setup_token_hash.as_bytes())
            .map_err(|_| Error::Internal)?;
        // This cursor key is stable across restarts. Rotating the setup token
        // deliberately invalidates pagination cursors, not live sessions.
        mac.update(b"opentrack.session-directory.v1\0");
        mac.update(actor.user.id.as_bytes());
        mac.update(actor.token_hash.as_bytes());
        mac.update(&limit.to_be_bytes());
        mac.update(payload.as_bytes());
        Ok(mac)
    }
    fn encode_session_cursor(
        &self,
        actor: &Actor,
        limit: u32,
        row: &SessionInfo,
    ) -> Result<String> {
        self.encode_session_cursor_at(actor, limit, row, Utc::now().timestamp())
    }
    fn encode_session_cursor_at(
        &self,
        actor: &Actor,
        limit: u32,
        row: &SessionInfo,
        issued: i64,
    ) -> Result<String> {
        let expires = issued.checked_add(24 * 60 * 60).ok_or(Error::Internal)?;
        let payload = format!(
            "1|{}|{}|{issued}|{expires}",
            row.created_at.to_rfc3339(),
            row.id
        );
        let signature = self
            .cursor_mac(actor, limit, &payload)?
            .finalize()
            .into_bytes();
        let encoded: String = signature.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(format!("{payload}|{encoded}"))
    }
    fn session_cursor(
        &self,
        actor: &Actor,
        limit: u32,
        cursor: &str,
    ) -> Result<(DateTime<Utc>, Uuid)> {
        self.session_cursor_at(actor, limit, cursor, Utc::now().timestamp())
    }
    fn session_cursor_at(
        &self,
        actor: &Actor,
        limit: u32,
        cursor: &str,
        now: i64,
    ) -> Result<(DateTime<Utc>, Uuid)> {
        let invalid = || {
            Error::Invalid(
                "Invalid session page cursor for this account, session or page size".into(),
            )
        };
        let (payload, signature) = cursor.rsplit_once('|').ok_or_else(invalid)?;
        if signature.len() != 64 || !signature.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let mut decoded = [0_u8; 32];
        for (index, byte) in decoded.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&signature[index * 2..index * 2 + 2], 16)
                .map_err(|_| invalid())?;
        }
        self.cursor_mac(actor, limit, payload)?
            .verify_slice(&decoded)
            .map_err(|_| invalid())?;
        let parts: Vec<&str> = payload.split('|').collect();
        if parts.len() != 5 || parts[0] != "1" {
            return Err(invalid());
        }
        let issued = parts[3].parse::<i64>().map_err(|_| invalid())?;
        let expires = parts[4].parse::<i64>().map_err(|_| invalid())?;
        if issued > now.saturating_add(60) || issued.checked_add(24 * 60 * 60) != Some(expires) {
            return Err(invalid());
        }
        if expires <= now {
            return Err(Error::CursorExpired);
        }
        Ok((
            parts[1].parse().map_err(|_| invalid())?,
            parts[2].parse().map_err(|_| invalid())?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentrack_domain::User;

    #[tokio::test]
    async fn session_cursors_are_authenticated_scoped_and_expire_after_24_hours() {
        // Lazy pool: this test exercises only cursor cryptography, never PostgreSQL.
        let app = App {
            pool: PgPoolOptions::new()
                .connect_lazy("postgres://unused@localhost/unused")
                .unwrap(),
            setup_token_hash: digest(b"cursor-test-key"),
            hashes: Arc::new(Semaphore::new(1)),
        };
        let actor = Actor {
            audience: SessionAudience::Web,
            user: User {
                id: Uuid::new_v4(),
                email: "cursor@example.invalid".into(),
                display_name: "Cursor".into(),
            },
            token_hash: digest(b"test-cookie"),
            csrf_token: "unused".into(),
        };
        let now = 1_800_000_000;
        let at = DateTime::from_timestamp(now, 0).unwrap();
        let row = SessionInfo {
            audience: SessionAudience::Web,
            device_name: None,
            id: Uuid::new_v4(),
            created_at: at,
            auth_time: at,
            last_seen_at: at,
            expires_at: at,
            idle_expires_at: at,
            current: false,
        };
        let cursor = app.encode_session_cursor_at(&actor, 50, &row, now).unwrap();
        assert_eq!(
            app.session_cursor_at(&actor, 50, &cursor, now).unwrap(),
            (at, row.id)
        );
        assert!(matches!(
            app.session_cursor_at(&actor, 50, &cursor, now + 86400),
            Err(Error::CursorExpired)
        ));
        let expired = app
            .encode_session_cursor_at(&actor, 50, &row, now - 86401)
            .unwrap();
        assert!(matches!(
            app.session_cursor_at(&actor, 50, &expired, now),
            Err(Error::CursorExpired)
        ));
        let future = app
            .encode_session_cursor_at(&actor, 50, &row, now + 61)
            .unwrap();
        assert!(matches!(
            app.session_cursor_at(&actor, 50, &future, now),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            app.session_cursor_at(&actor, 1, &cursor, now),
            Err(Error::Invalid(_))
        ));
        let mut other = actor.clone();
        other.user.id = Uuid::new_v4();
        assert!(matches!(
            app.session_cursor_at(&other, 50, &cursor, now),
            Err(Error::Invalid(_))
        ));
        other = actor.clone();
        other.token_hash = digest(b"another-cookie");
        assert!(matches!(
            app.session_cursor_at(&other, 50, &cursor, now),
            Err(Error::Invalid(_))
        ));
        let mut changed_key = app.clone();
        changed_key.setup_token_hash = digest(b"rotated-setup-token");
        assert!(matches!(
            changed_key.session_cursor_at(&actor, 50, &cursor, now),
            Err(Error::Invalid(_))
        ));
        let tampered = cursor.replacen(&row.id.to_string(), &Uuid::new_v4().to_string(), 1);
        assert!(matches!(
            app.session_cursor_at(&actor, 50, &tampered, now),
            Err(Error::Invalid(_))
        ));
        app.pool.close().await;
    }
}
