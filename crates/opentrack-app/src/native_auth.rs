use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::*;
use sqlx::{Row, postgres::PgRow};

const MAX_REQUESTS: i64 = 2000;
const CONFIRMATION_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

fn one(count: u64) -> Result<()> {
    if count == 1 {
        Ok(())
    } else {
        Err(Error::Internal)
    }
}
fn require_audience(actor: &Actor, audience: SessionAudience) -> Result<()> {
    if actor.audience == audience {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}
fn confirmation_code() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    // The alphabet has exactly 32 values, so each symbol is unbiased.
    bytes
        .into_iter()
        .map(|b| CONFIRMATION_ALPHABET[(b & 31) as usize] as char)
        .collect()
}
async fn account_scope(app: &App, actor: &Actor, exclusive: bool) -> Result<store::Tx> {
    let mut tx = store::begin(&app.pool).await.map_err(db_error)?;
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
async fn public_session_id(tx: &mut store::Tx, actor: &Actor) -> Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 AND token_hash=$2 AND audience=$3")
        .bind(actor.user.id)
        .bind(&actor.token_hash)
        .bind(actor.audience.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::Unauthorized)
}
async fn active_reference(
    tx: &mut store::Tx,
    user: Uuid,
    id: Uuid,
    audience: SessionAudience,
) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.user_id=$1 AND s.id=$2 AND s.audience=$3 AND u.disabled_at IS NULL AND s.revoked_at IS NULL AND s.expires_at>clock_timestamp() AND s.last_seen_at>clock_timestamp()-interval '12 hours')")
        .bind(user).bind(id).bind(audience.as_str()).fetch_one(&mut **tx).await.map_err(db_error)
}
async fn binding_valid(tx: &mut store::Tx, row: &PgRow, user: Uuid) -> Result<bool> {
    let bound: Option<Uuid> = row.try_get("bound_user_id").map_err(db_error)?;
    let previous: Option<Uuid> = row
        .try_get("previous_native_session_id")
        .map_err(db_error)?;
    match (bound, previous) {
        (None, None) => Ok(true),
        (Some(bound), Some(previous)) if bound == user => {
            active_reference(tx, user, previous, SessionAudience::Desktop).await
        }
        _ => Ok(false),
    }
}

impl App {
    pub async fn start_native_auth(
        &self,
        previous: Option<&Actor>,
        mut request: NativeAuthStart,
    ) -> Result<NativeAuthStartReceipt> {
        request.validate()?;
        let mut tx;
        let (bound_user, previous_id) = if let Some(actor) = previous {
            require_audience(actor, SessionAudience::Desktop)?;
            tx = account_scope(self, actor, false).await?;
            (
                Some(actor.user.id),
                Some(public_session_id(&mut tx, actor).await?),
            )
        } else {
            tx = store::begin(&self.pool).await.map_err(db_error)?;
            // Anonymous identity ingress has no existing account to lock. The
            // nil context still applies the same transaction-local timeouts.
            store::context(&mut tx, Uuid::nil(), None)
                .await
                .map_err(db_error)?;
            (None, None)
        };
        let admission: Option<bool> = sqlx::query_scalar(
            "SELECT singleton FROM native_auth_admission WHERE singleton FOR UPDATE",
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        if admission != Some(true) {
            return Err(Error::Internal);
        }
        // One admission lock covers cleanup and insertion across all processes.
        // Limit both retained state and the work performed by a single request.
        let counts: (i64, i64) = sqlx::query_as("WITH expired AS MATERIALIZED (SELECT id FROM native_auth_requests WHERE expires_at<=clock_timestamp() ORDER BY expires_at,id LIMIT 100 FOR UPDATE), removed AS (DELETE FROM native_auth_requests r USING expired e WHERE r.id=e.id RETURNING r.id) SELECT (SELECT count(*) FROM expired),(SELECT count(*) FROM removed)")
            .fetch_one(&mut *tx).await.map_err(db_error)?;
        if counts.0 != counts.1 {
            return Err(Error::Internal);
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM (SELECT 1 FROM native_auth_requests LIMIT 2000) bounded",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if count >= MAX_REQUESTS {
            return Err(Error::RateLimited);
        }
        let id = Uuid::new_v4();
        let confirmation = confirmation_code();
        let expires_at: DateTime<Utc> = sqlx::query_scalar("WITH at AS MATERIALIZED (SELECT clock_timestamp() AS now) INSERT INTO native_auth_requests(id,code_challenge,callback_uri,state,confirmation_code,device_name,created_at,expires_at,bound_user_id,previous_native_session_id) SELECT $1,$2,$3,$4,$5,$6,at.now,at.now+interval '10 minutes',$7,$8 FROM at RETURNING expires_at")
            .bind(id).bind(request.code_challenge).bind(request.callback_uri).bind(request.state)
            .bind(&confirmation).bind(request.device_name).bind(bound_user).bind(previous_id)
            .fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::Internal)?;
        tx.commit().await.map_err(db_error)?;
        Ok(NativeAuthStartReceipt {
            id,
            authorization_path: format!("/auth/native/approve?request_id={id}"),
            confirmation_code: confirmation,
            expires_at,
        })
    }

    pub async fn native_auth_consent(
        &self,
        browser: &Actor,
        request_id: Uuid,
    ) -> Result<NativeAuthConsent> {
        require_audience(browser, SessionAudience::Web)?;
        let mut tx = account_scope(self, browser, false).await?;
        let row = sqlx::query("SELECT id,device_name,confirmation_code,expires_at,bound_user_id,previous_native_session_id FROM native_auth_requests WHERE id=$1 AND approved_at IS NULL AND expires_at>clock_timestamp()")
            .bind(request_id).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        if !binding_valid(&mut tx, &row, browser.user.id).await? {
            return Err(Error::NotFound);
        }
        let consent = NativeAuthConsent {
            id: request_id,
            device_name: row.try_get("device_name").map_err(db_error)?,
            confirmation_code: row.try_get("confirmation_code").map_err(db_error)?,
            expires_at: row.try_get("expires_at").map_err(db_error)?,
            reauthentication: row
                .try_get::<Option<Uuid>, _>("bound_user_id")
                .map_err(db_error)?
                .is_some(),
        };
        tx.commit().await.map_err(db_error)?;
        Ok(consent)
    }

    pub async fn approve_native_auth(
        &self,
        browser: &Actor,
        request_id: Uuid,
        request: NativeAuthApprove,
    ) -> Result<NativeAuthApproval> {
        require_audience(browser, SessionAudience::Web)?;
        if request.confirmation_code.len() != 8
            || !request
                .confirmation_code
                .bytes()
                .all(|b| CONFIRMATION_ALPHABET.contains(&b))
        {
            return Err(Error::InvalidNativeAuth);
        }
        let mut tx = account_scope(self, browser, true).await?;
        let row = sqlx::query("SELECT callback_uri,state,confirmation_code,bound_user_id,previous_native_session_id FROM native_auth_requests WHERE id=$1 AND approved_at IS NULL AND expires_at>clock_timestamp() FOR UPDATE")
            .bind(request_id).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::InvalidNativeAuth)?;
        let expected: String = row.try_get("confirmation_code").map_err(db_error)?;
        if !secret_matches(&expected, &request.confirmation_code)
            || !binding_valid(&mut tx, &row, browser.user.id).await?
        {
            return Err(Error::InvalidNativeAuth);
        }
        let callback: String = row.try_get("callback_uri").map_err(db_error)?;
        if !valid_native_callback(&callback) {
            return Err(Error::InvalidNativeAuth);
        }
        let state: String = row.try_get("state").map_err(db_error)?;
        crate::lifecycle::recent(&mut tx, browser).await?;
        let browser_id = public_session_id(&mut tx, browser).await?;
        let code = random_secret();
        let changed = sqlx::query("WITH at AS MATERIALIZED (SELECT clock_timestamp() AS now) UPDATE native_auth_requests r SET approved_user_id=$2,approving_web_session_id=$3,approved_at=at.now,code_hash=$4,code_expires_at=LEAST(at.now+interval '60 seconds',r.expires_at) FROM at WHERE r.id=$1 AND r.approved_at IS NULL AND r.expires_at>at.now AND EXISTS(SELECT 1 FROM sessions s WHERE s.user_id=$2 AND s.id=$3 AND s.audience='web' AND s.revoked_at IS NULL AND s.expires_at>at.now AND s.last_seen_at>at.now-interval '12 hours') AND (r.previous_native_session_id IS NULL OR EXISTS(SELECT 1 FROM sessions s WHERE s.user_id=$2 AND s.id=r.previous_native_session_id AND s.audience='desktop' AND s.revoked_at IS NULL AND s.expires_at>at.now AND s.last_seen_at>at.now-interval '12 hours'))")
            .bind(request_id).bind(browser.user.id).bind(browser_id).bind(digest(code.as_bytes()))
            .execute(&mut *tx).await.map_err(db_error)?;
        // A timeout crossing before the conditional effect is invalid, while a
        // suppressing trigger also fails closed without returning a callback.
        if changed.rows_affected() != 1 {
            return Err(Error::InvalidNativeAuth);
        }
        crate::sessions::security_event_correlated(
            &mut tx,
            browser.user.id,
            browser_id,
            None,
            "session.native_authorized",
            request_id,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(NativeAuthApproval {
            callback_uri: format!("{callback}?request_id={request_id}&code={code}&state={state}"),
        })
    }

    pub async fn exchange_native_auth(
        &self,
        request: NativeAuthExchange,
    ) -> Result<NativeSessionGrant> {
        if !native_hex_secret(&request.code) {
            return Err(Error::InvalidNativeAuth);
        }
        let challenge = native_code_challenge(&request.code_verifier)?;
        let code_hash = digest(request.code.as_bytes());
        // Discover only the guard key before opening the transaction. All grant
        // checks, including enabled account and live source, repeat under lock.
        let user: Uuid = sqlx::query_scalar("SELECT approved_user_id FROM native_auth_requests WHERE id=$1 AND code_hash=$2 AND approved_user_id IS NOT NULL AND consumed_at IS NULL AND code_expires_at>clock_timestamp() AND expires_at>clock_timestamp()")
            .bind(request.request_id).bind(&code_hash).fetch_optional(&self.pool).await.map_err(db_error)?.ok_or(Error::InvalidNativeAuth)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context_exclusive(&mut tx, user, None)
            .await
            .map_err(db_error)?;
        let row = sqlx::query("SELECT code_challenge,device_name,bound_user_id,previous_native_session_id,approving_web_session_id FROM native_auth_requests WHERE id=$1 AND approved_user_id=$2 AND code_hash=$3 AND consumed_at IS NULL AND code_expires_at>clock_timestamp() AND expires_at>clock_timestamp() FOR UPDATE")
            .bind(request.request_id).bind(user).bind(&code_hash).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::InvalidNativeAuth)?;
        let expected_challenge: String = row.try_get("code_challenge").map_err(db_error)?;
        if !secret_matches(&expected_challenge, &challenge)
            || !binding_valid(&mut tx, &row, user).await?
        {
            return Err(Error::InvalidNativeAuth);
        }
        let browser: Uuid = row.try_get("approving_web_session_id").map_err(db_error)?;
        let previous: Option<Uuid> = row
            .try_get("previous_native_session_id")
            .map_err(db_error)?;
        let device: String = row.try_get("device_name").map_err(db_error)?;
        let source = sqlx::query("SELECT s.auth_time,LEAST(s.expires_at,clock_timestamp()+interval '7 days') AS expires_at,u.email,u.display_name FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.user_id=$1 AND s.id=$2 AND s.audience='web' AND s.revoked_at IS NULL AND s.expires_at>clock_timestamp() AND s.last_seen_at>clock_timestamp()-interval '12 hours' AND u.disabled_at IS NULL")
            .bind(user).bind(browser).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::InvalidNativeAuth)?;
        let auth_time: DateTime<Utc> = source.try_get("auth_time").map_err(db_error)?;
        let expires_at: DateTime<Utc> = source.try_get("expires_at").map_err(db_error)?;
        let public_user = User {
            id: user,
            email: source.try_get("email").map_err(db_error)?,
            display_name: source.try_get("display_name").map_err(db_error)?,
        };
        let token = random_secret();
        let session_id: Uuid = sqlx::query_scalar("INSERT INTO sessions(token_hash,user_id,csrf_token,auth_time,expires_at,audience,device_name,created_at,last_seen_at) VALUES($1,$2,'',$3,$4,'desktop',$5,clock_timestamp(),clock_timestamp()) RETURNING id")
            .bind(digest(token.as_bytes())).bind(user).bind(auth_time).bind(expires_at).bind(device)
            .fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::Internal)?;
        crate::sessions::security_event_correlated(
            &mut tx,
            user,
            session_id,
            None,
            "session.created",
            request.request_id,
        )
        .await?;
        // Recheck the wall clock after every earlier effect. A grant cannot be
        // consumed after expiry merely because the transaction began in time.
        let consumed = sqlx::query("WITH at AS MATERIALIZED (SELECT clock_timestamp() AS now) UPDATE native_auth_requests r SET consumed_at=at.now,issued_session_id=$4 FROM at WHERE r.id=$1 AND r.approved_user_id=$2 AND r.code_hash=$3 AND r.consumed_at IS NULL AND r.code_expires_at>at.now AND r.expires_at>at.now AND EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.user_id=$2 AND s.id=r.approving_web_session_id AND s.audience='web' AND u.disabled_at IS NULL AND s.revoked_at IS NULL AND s.expires_at>at.now AND s.last_seen_at>at.now-interval '12 hours') AND (r.previous_native_session_id IS NULL OR EXISTS(SELECT 1 FROM sessions s WHERE s.user_id=$2 AND s.id=r.previous_native_session_id AND s.audience='desktop' AND s.revoked_at IS NULL AND s.expires_at>at.now AND s.last_seen_at>at.now-interval '12 hours'))")
            .bind(request.request_id).bind(user).bind(&code_hash).bind(session_id).execute(&mut *tx).await.map_err(db_error)?;
        if consumed.rows_affected() != 1 {
            return Err(Error::InvalidNativeAuth);
        }
        if let Some(previous) = previous {
            one(sqlx::query("UPDATE sessions SET revoked_at=clock_timestamp() WHERE user_id=$1 AND id=$2 AND audience='desktop' AND revoked_at IS NULL")
                .bind(user).bind(previous).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            crate::sessions::security_event_correlated(
                &mut tx,
                user,
                previous,
                Some(session_id),
                "session.native_rotated",
                request.request_id,
            )
            .await?;
        }
        crate::sessions::security_event_correlated(
            &mut tx,
            user,
            session_id,
            Some(browser),
            "session.native_exchanged",
            request.request_id,
        )
        .await?;
        let workspaces = store::workspaces(&mut tx, user).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(NativeSessionGrant {
            token_type: "Bearer".into(),
            access_token: token,
            session_id,
            expires_at,
            session: Session {
                audience: SessionAudience::Desktop,
                user: public_user,
                csrf_token: String::new(),
                workspaces,
            },
        })
    }
}
