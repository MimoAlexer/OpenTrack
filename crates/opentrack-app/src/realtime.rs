use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use chrono::Utc;
use opentrack_domain::{RealtimeInvalidation, RealtimePage, RealtimeResetReason as Reset};
use serde::{Deserialize, Serialize};
use sqlx::Acquire;

const CURSOR_TTL: i64 = 7 * 24 * 60 * 60;
const CURSOR_PLAINTEXT_BYTES: usize = 512;
pub struct RealtimeFrame {
    pub cursor: String,
    pub event: RealtimeInvalidation,
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentrack_domain::User;

    fn fixture() -> (App, Actor, Uuid) {
        let app = App {
            pool: PgPoolOptions::new()
                .connect_lazy("postgres://example:example@127.0.0.1/example_test")
                .unwrap(),
            setup_token_hash: digest(b"realtime-test-installation-key"),
            hashes: Arc::new(Semaphore::new(1)),
        };
        let actor = Actor {
            audience: SessionAudience::Web,
            user: User {
                id: Uuid::new_v4(),
                email: "member@example.test".into(),
                display_name: "Example Member".into(),
            },
            token_hash: digest(b"example-session"),
            csrf_token: "example-csrf".into(),
        };
        (app, actor, Uuid::new_v4())
    }
    #[tokio::test]
    async fn encrypted_cursor_is_randomized_and_shared_by_matching_installation_nodes() {
        let (app, actor, workspace) = fixture();
        let now = 2_000_000_000;
        let first = app
            .realtime_cursor_at(&actor, workspace, 17, 4321, now)
            .unwrap();
        let second = app
            .realtime_cursor_at(&actor, workspace, 17, 4321, now)
            .unwrap();
        assert_ne!(first, second);
        let larger_position = app
            .realtime_cursor_at(&actor, workspace, i64::MAX, i64::MAX, now)
            .unwrap();
        assert_eq!(
            first.len(),
            larger_position.len(),
            "Hidden stream positions must not change token length"
        );
        assert!(!first.contains(&workspace.to_string()));
        assert!(!first.contains(&actor.user.id.to_string()));
        let other_node = app.clone();
        let decoded = other_node
            .realtime_decode(&actor, workspace, &first, now)
            .unwrap();
        assert_eq!(
            (decoded.epoch, decoded.sequence, decoded.expires_at),
            (17, 4321, now + CURSOR_TTL)
        );
        let bytes = URL_SAFE_NO_PAD
            .decode(first.strip_prefix("rt1.").unwrap())
            .unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&bytes[24..]).is_err());
    }
    #[tokio::test]
    async fn cursor_rejects_other_principals_sessions_workspaces_audiences_and_keys() {
        let (app, actor, workspace) = fixture();
        let now = 2_000_000_000;
        let token = app
            .realtime_cursor_at(&actor, workspace, 1, 0, now)
            .unwrap();
        let mut changed = actor.clone();
        changed.user.id = Uuid::new_v4();
        assert!(
            app.realtime_decode(&changed, workspace, &token, now)
                .is_err()
        );
        let mut changed = actor.clone();
        changed.token_hash = digest(b"rotated-session");
        assert!(
            app.realtime_decode(&changed, workspace, &token, now)
                .is_err()
        );
        let mut changed = actor.clone();
        changed.audience = SessionAudience::Desktop;
        assert!(
            app.realtime_decode(&changed, workspace, &token, now)
                .is_err()
        );
        assert!(
            app.realtime_decode(&actor, Uuid::new_v4(), &token, now)
                .is_err()
        );
        let mut changed = app.clone();
        changed.setup_token_hash = digest(b"different-installation");
        assert!(
            changed
                .realtime_decode(&actor, workspace, &token, now)
                .is_err()
        );
    }
    #[tokio::test]
    async fn cursor_rejects_tampering_malformed_future_and_expired_positions() {
        let (app, actor, workspace) = fixture();
        let now = 2_000_000_000;
        let token = app
            .realtime_cursor_at(&actor, workspace, 1, 1, now)
            .unwrap();
        for invalid in [
            String::new(),
            "rt1.invalid=".into(),
            "rt2.invalid".into(),
            "x".repeat(4097),
        ] {
            assert!(matches!(
                app.realtime_decode(&actor, workspace, &invalid, now),
                Err(Error::RealtimeReset(Reset::ResetRequired))
            ));
        }
        let mut bytes = URL_SAFE_NO_PAD
            .decode(token.strip_prefix("rt1.").unwrap())
            .unwrap();
        bytes[24] ^= 1;
        assert!(
            app.realtime_decode(
                &actor,
                workspace,
                &format!("rt1.{}", URL_SAFE_NO_PAD.encode(bytes)),
                now
            )
            .is_err()
        );
        assert!(matches!(
            app.realtime_decode(&actor, workspace, &token, now + CURSOR_TTL),
            Err(Error::RealtimeReset(Reset::CursorExpired))
        ));
        assert!(
            app.realtime_decode(&actor, workspace, &token, now + CURSOR_TTL - 1)
                .is_ok()
        );
        let future = app
            .realtime_cursor_at(&actor, workspace, 1, 1, now + 31)
            .unwrap();
        assert!(
            app.realtime_decode(&actor, workspace, &future, now)
                .is_err()
        );
        for (epoch, sequence) in [(-1, 1), (1, -1)] {
            let invalid = app
                .realtime_cursor_at(&actor, workspace, epoch, sequence, now)
                .unwrap();
            assert!(
                app.realtime_decode(&actor, workspace, &invalid, now)
                    .is_err()
            );
        }
    }
}
pub struct RealtimeBatch {
    pub frames: Vec<RealtimeFrame>,
    pub cursor: String,
    pub has_more: bool,
    pub epoch: i64,
    pub advanced: bool,
}
pub struct RealtimeListener(sqlx::postgres::PgListener);
impl RealtimeListener {
    pub async fn recv(&mut self) -> Result<()> {
        self.0
            .recv()
            .await
            .map(|_| ())
            .map_err(|_| Error::Unavailable)
    }

    /// Fault injection targets only this listener's acquired connection. It
    /// accepts no process identifier and grants no cross-session authority.
    #[doc(hidden)]
    pub async fn interrupt(&mut self) -> Result<()> {
        let connection = (&mut self.0)
            .acquire()
            .await
            .map_err(|_| Error::Unavailable)?;
        sqlx::query("SELECT pg_terminate_backend(pg_backend_pid())")
            .execute(connection)
            .await
            .map(|_| ())
            .map_err(|_| Error::Unavailable)
    }
}
pub(crate) fn snapshot_error(error: sqlx::Error) -> Error {
    if matches!(&error, sqlx::Error::Database(error) if error.code().as_deref() == Some("40001")) {
        Error::Unavailable
    } else {
        db_error(error)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    workspace: Uuid,
    user: Uuid,
    epoch: i64,
    sequence: i64,
    issued_at: i64,
    expires_at: i64,
}
impl App {
    pub async fn listen_realtime(&self) -> Result<RealtimeListener> {
        let mut listener = sqlx::postgres::PgListener::connect_with(&self.pool)
            .await
            .map_err(|_| Error::Unavailable)?;
        listener
            .listen("opentrack_realtime")
            .await
            .map_err(|_| Error::Unavailable)?;
        Ok(RealtimeListener(listener))
    }
    pub(crate) async fn realtime_snapshot_scope(
        &self,
        actor: &Actor,
        workspace: Uuid,
    ) -> Result<store::Tx> {
        let connection = self.pool.acquire().await.map_err(snapshot_error)?;
        let mut tx = store::Tx::begin_with(connection, "BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .map_err(snapshot_error)?;
        store::context(&mut tx, actor.user.id, Some(workspace))
            .await
            .map_err(snapshot_error)?;
        if store::workspace_role(&mut tx, actor, workspace)
            .await
            .map_err(snapshot_error)?
            .is_none()
        {
            return Err(Error::NotFound);
        }
        if !store::active_session_snapshot(&mut tx, actor)
            .await
            .map_err(snapshot_error)?
        {
            return Err(Error::Unauthorized);
        }
        Ok(tx)
    }
    fn realtime_cipher(&self) -> XChaCha20Poly1305 {
        let mut key = Sha256::new();
        key.update(b"opentrack.realtime.cursor.key.v1\0");
        key.update(self.setup_token_hash.as_bytes());
        XChaCha20Poly1305::new_from_slice(&key.finalize()).expect("SHA-256 derives a 32-byte key")
    }
    fn realtime_binding(actor: &Actor, workspace: Uuid) -> Vec<u8> {
        let mut aad = b"opentrack.realtime.cursor.v1\0".to_vec();
        aad.extend_from_slice(workspace.as_bytes());
        aad.extend_from_slice(actor.user.id.as_bytes());
        aad.extend_from_slice(actor.token_hash.as_bytes());
        aad.extend_from_slice(actor.audience.as_str().as_bytes());
        aad
    }
    pub(crate) fn realtime_cursor(
        &self,
        actor: &Actor,
        workspace: Uuid,
        epoch: i64,
        sequence: i64,
    ) -> Result<String> {
        self.realtime_cursor_at(actor, workspace, epoch, sequence, Utc::now().timestamp())
    }
    fn realtime_cursor_at(
        &self,
        actor: &Actor,
        workspace: Uuid,
        epoch: i64,
        sequence: i64,
        now: i64,
    ) -> Result<String> {
        let mut payload = serde_json::to_vec(&Cursor {
            version: 1,
            workspace,
            user: actor.user.id,
            epoch,
            sequence,
            issued_at: now,
            expires_at: now + CURSOR_TTL,
        })
        .map_err(|_| Error::Internal)?;
        if payload.len() > CURSOR_PLAINTEXT_BYTES {
            return Err(Error::Internal);
        }
        // Fixed-size authenticated plaintext hides sequence/epoch digit lengths.
        // JSON whitespace remains inside the ciphertext and is discarded on read.
        payload.resize(CURSOR_PLAINTEXT_BYTES, b' ');
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = self
            .realtime_cipher()
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &payload,
                    aad: &Self::realtime_binding(actor, workspace),
                },
            )
            .map_err(|_| Error::Internal)?;
        let mut token = nonce.to_vec();
        token.extend_from_slice(&ciphertext);
        Ok(format!("rt1.{}", URL_SAFE_NO_PAD.encode(token)))
    }
    fn realtime_decode(
        &self,
        actor: &Actor,
        workspace: Uuid,
        cursor: &str,
        now: i64,
    ) -> Result<Cursor> {
        let invalid = || Error::RealtimeReset(Reset::ResetRequired);
        if cursor.len() > 4096 {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(cursor.strip_prefix("rt1.").ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        if bytes.len() != 24 + CURSOR_PLAINTEXT_BYTES + 16 {
            return Err(invalid());
        }
        let payload = self
            .realtime_cipher()
            .decrypt(
                &XNonce::try_from(&bytes[..24]).map_err(|_| invalid())?,
                Payload {
                    msg: &bytes[24..],
                    aad: &Self::realtime_binding(actor, workspace),
                },
            )
            .map_err(|_| invalid())?;
        let cursor: Cursor = serde_json::from_slice(&payload).map_err(|_| invalid())?;
        if cursor.version != 1
            || cursor.workspace != workspace
            || cursor.user != actor.user.id
            || cursor.epoch < 0
            || cursor.sequence < 0
            || cursor.issued_at > now + 30
            || cursor.expires_at != cursor.issued_at + CURSOR_TTL
        {
            return Err(invalid());
        }
        if now >= cursor.expires_at {
            return Err(Error::RealtimeReset(Reset::CursorExpired));
        }
        Ok(cursor)
    }
    pub async fn realtime_authority(
        &self,
        actor: &Actor,
        workspace: Uuid,
        epoch: i64,
    ) -> Result<()> {
        let mut tx = self.scoped(actor, workspace, false).await?;
        if store::auth_epoch(&mut tx, workspace)
            .await
            .map_err(db_error)?
            != epoch
        {
            return Err(Error::RealtimeReset(Reset::AccessChanged));
        }
        tx.commit().await.map_err(db_error)
    }
    pub async fn realtime_replay(
        &self,
        actor: &Actor,
        workspace: Uuid,
        cursor: &str,
        limit: u32,
    ) -> Result<RealtimeBatch> {
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid(
                "Realtime pages contain between 1 and 100 events".into(),
            ));
        }
        let cursor = self.realtime_decode(actor, workspace, cursor, Utc::now().timestamp())?;
        let mut tx = self.scoped(actor, workspace, false).await?;
        let epoch = store::auth_epoch(&mut tx, workspace)
            .await
            .map_err(db_error)?;
        if cursor.epoch != epoch {
            return Err(Error::RealtimeReset(Reset::AccessChanged));
        }
        let head = store::stream_head(&mut tx, workspace)
            .await
            .map_err(db_error)?;
        if cursor.sequence > head || head - cursor.sequence > 10_000 {
            return Err(Error::RealtimeReset(Reset::ResetRequired));
        }
        let rows = store::stream_page(&mut tx, workspace, cursor.sequence, limit)
            .await
            .map_err(db_error)?;
        let mut sequence = cursor.sequence;
        let mut frames = Vec::new();
        for row in rows {
            if row.seq != sequence + 1 {
                return Err(Error::RealtimeReset(Reset::ResetRequired));
            }
            sequence = row.seq;
            match store::stream_source(&mut tx, workspace, actor.user.id, &row)
                .await
                .map_err(db_error)?
            {
                store::StreamSource::Hidden => {}
                store::StreamSource::ScopeReset => {
                    return Err(Error::RealtimeReset(Reset::ScopeChanged));
                }
                store::StreamSource::Visible(entity_type) => frames.push(RealtimeFrame {
                    cursor: self.realtime_cursor(actor, workspace, epoch, row.seq)?,
                    event: RealtimeInvalidation {
                        event_id: row.event_id,
                        schema_version: 1,
                        event_type: row.event_type,
                        workspace_id: workspace,
                        entity_type: entity_type.into(),
                        entity_id: row.entity_id,
                        entity_version: row.entity_version.to_string(),
                        actor_id: None,
                        occurred_at: row.occurred_at,
                        mutation_id: row.mutation_id,
                        changed_fields: Vec::new(),
                    },
                }),
            }
        }
        if sequence == cursor.sequence && head > sequence {
            return Err(Error::RealtimeReset(Reset::ResetRequired));
        }
        tx.commit().await.map_err(db_error)?;
        Ok(RealtimeBatch {
            frames,
            cursor: self.realtime_cursor(actor, workspace, epoch, sequence)?,
            has_more: sequence < head,
            epoch,
            advanced: sequence != cursor.sequence,
        })
    }
    pub async fn realtime_page(
        &self,
        actor: &Actor,
        workspace: Uuid,
        cursor: &str,
        limit: u32,
    ) -> Result<RealtimePage> {
        let batch = self
            .realtime_replay(actor, workspace, cursor, limit)
            .await?;
        Ok(RealtimePage {
            events: batch.frames.into_iter().map(|frame| frame.event).collect(),
            cursor: batch.cursor,
            has_more: batch.has_more,
        })
    }
    /// Reauthorize a queued frame at emission time, after any period of network
    /// backpressure. A previously authorized page is not a lasting read grant.
    pub async fn realtime_emission(
        &self,
        actor: &Actor,
        workspace: Uuid,
        cursor: &str,
        event: Option<&RealtimeInvalidation>,
    ) -> Result<bool> {
        let position = self.realtime_decode(actor, workspace, cursor, Utc::now().timestamp())?;
        let mut tx = self.scoped(actor, workspace, false).await?;
        if store::auth_epoch(&mut tx, workspace)
            .await
            .map_err(db_error)?
            != position.epoch
        {
            return Err(Error::RealtimeReset(Reset::AccessChanged));
        }
        let mut allowed = true;
        if let Some(event) = event {
            if position.sequence == 0 {
                return Err(Error::RealtimeReset(Reset::ResetRequired));
            }
            let rows = store::stream_page(&mut tx, workspace, position.sequence - 1, 1)
                .await
                .map_err(db_error)?;
            let Some(row) = rows.into_iter().next().filter(|row| {
                row.seq == position.sequence
                    && row.event_id == event.event_id
                    && row.entity_id == event.entity_id
            }) else {
                return Err(Error::RealtimeReset(Reset::ResetRequired));
            };
            match store::stream_source(&mut tx, workspace, actor.user.id, &row)
                .await
                .map_err(db_error)?
            {
                store::StreamSource::Visible(kind) => allowed = kind == event.entity_type,
                store::StreamSource::Hidden => allowed = false,
                store::StreamSource::ScopeReset => {
                    return Err(Error::RealtimeReset(Reset::ScopeChanged));
                }
            }
        }
        tx.commit().await.map_err(db_error)?;
        Ok(allowed)
    }
    pub async fn realtime_reserve(&self, actor: &Actor, workspace: Uuid) -> Result<Uuid> {
        let mut tx = self.scoped(actor, workspace, false).await?;
        // This dedicated operational lock never precedes an aggregate/event
        // mutation, and never survives the short registration transaction.
        sqlx::query("SELECT pg_advisory_xact_lock(1480340562,hashtext($1))")
            .bind(actor.user.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query(
            "DELETE FROM realtime_stream_leases WHERE user_id=$1 AND expires_at<=clock_timestamp()",
        )
        .bind(actor.user.id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        let session: Uuid = sqlx::query_scalar(
            "SELECT id FROM sessions WHERE user_id=$1 AND token_hash=$2 AND audience=$3",
        )
        .bind(actor.user.id)
        .bind(&actor.token_hash)
        .bind(actor.audience.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let (account_count, session_count): (i64,i64) = sqlx::query_as("SELECT count(*),count(*) FILTER(WHERE session_id=$2) FROM realtime_stream_leases WHERE user_id=$1 AND expires_at>clock_timestamp()").bind(actor.user.id).bind(session).fetch_one(&mut *tx).await.map_err(db_error)?;
        if account_count >= 12 || session_count >= 3 {
            return Err(Error::RateLimited);
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO realtime_stream_leases(id,workspace_id,user_id,session_id,expires_at) VALUES($1,$2,$3,$4,clock_timestamp()+interval '90 seconds')").bind(id).bind(workspace).bind(actor.user.id).bind(session).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(id)
    }
    pub async fn realtime_renew(&self, actor: &Actor, workspace: Uuid, lease: Uuid) -> Result<()> {
        let mut tx = self.scoped(actor, workspace, false).await?;
        let count = sqlx::query("UPDATE realtime_stream_leases SET expires_at=clock_timestamp()+interval '90 seconds' WHERE id=$1 AND workspace_id=$2 AND user_id=$3 AND session_id=(SELECT id FROM sessions WHERE token_hash=$4 AND user_id=$3) AND expires_at>clock_timestamp()")
            .bind(lease).bind(workspace).bind(actor.user.id).bind(&actor.token_hash).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
        if count != 1 {
            return Err(Error::Unauthorized);
        }
        tx.commit().await.map_err(db_error)
    }
    pub async fn realtime_release(
        &self,
        actor: &Actor,
        workspace: Uuid,
        lease: Uuid,
    ) -> Result<()> {
        // Closing an already-owned operational lease remains possible after
        // membership/session revocation; it never returns workspace data.
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, actor.user.id, None)
            .await
            .map_err(db_error)?;
        sqlx::query("DELETE FROM realtime_stream_leases WHERE id=$1 AND workspace_id=$2 AND user_id=$3 AND session_id=(SELECT id FROM sessions WHERE token_hash=$4 AND user_id=$3)")
            .bind(lease).bind(workspace).bind(actor.user.id).bind(&actor.token_hash).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }
}
