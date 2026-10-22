mod activity;
mod assignment_cleanup;
mod attachments;
mod milestones;
mod preferences;
mod projects;
pub use attachments::{
    AttachmentGcClaim, DownloadDescriptor, ScanOutcome, UploadFailure, UploadPermit,
};
mod comments;
pub mod consumer;
mod documents;
mod graph;
mod identity;
mod invitations;
mod issue_deletion;
mod issue_moves;
mod issue_references;
mod labels;
mod lifecycle;
mod mentions;
mod native_auth;
mod recovery;
mod sessions;
mod templates;
mod tracking;
pub mod worker;
mod workflow;
use opentrack_domain::{Actor, Error, Result, SessionAudience, native_hex_secret};
use opentrack_store as store;
use rand::{RngCore, rngs::OsRng};
pub use recovery::{AccountLifecycleOutcome, Operator, generate_recovery_code};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

/// The entity mutation succeeded. A self-elevation can additionally end the
/// caller's current session; transports must report success and clear its cookie.
#[derive(Debug)]
pub struct AccessMutation<T> {
    pub entity: T,
    pub current_session_revoked: bool,
}

#[derive(Clone)]
pub struct App {
    pub pool: PgPool,
    pub(crate) setup_token_hash: String,
    pub(crate) hashes: Arc<Semaphore>,
}
impl App {
    pub async fn connect(
        url: &str,
        setup_token: &str,
        connections: u32,
        hash_concurrency: usize,
    ) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(connections)
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(url)
            .await
            .map_err(db_error)?;
        let elevated: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_roles
             WHERE (rolname=current_user OR pg_has_role(current_user,oid,'MEMBER'))
             AND (rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb OR rolreplication))",
        )
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
        if elevated {
            tracing::error!(
                "Runtime database role has prohibited direct or reachable superuser, BYPASSRLS, role creation, database creation, or replication privileges"
            );
            return Err(Error::Internal);
        }
        let owns_tables: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=current_schema() AND c.relkind='r' AND pg_has_role(current_user,c.relowner,'MEMBER'))").fetch_one(&pool).await.map_err(db_error)?;
        if owns_tables {
            tracing::error!("Runtime database role must not own application tables");
            return Err(Error::Internal);
        }
        // Operator commands are installation authority, never a capability of the serving
        // credential. Check SET ROLE reachability as well as inherited rights.
        // Missing functions are expected when checking an older schema.
        let operator_authority: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_roles r CROSS JOIN pg_proc p
             WHERE (r.rolname=current_user OR pg_has_role(current_user,r.oid,'MEMBER'))
               AND p.pronamespace=current_schema()::regnamespace
               AND left(p.proname,9)='operator_'
               AND has_function_privilege(r.oid,p.oid,'EXECUTE'))",
        )
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
        let worker_authority: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_roles r CROSS JOIN pg_proc p
             WHERE (r.rolname=current_user OR pg_has_role(current_user,r.oid,'MEMBER'))
               AND p.pronamespace=current_schema()::regnamespace
               AND left(p.proname,7)='worker_'
               AND has_function_privilege(r.oid,p.oid,'EXECUTE'))",
        )
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
        let consumer_table_authority: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_roles r CROSS JOIN pg_class c
             WHERE (r.rolname=current_user OR pg_has_role(current_user,r.oid,'MEMBER'))
               AND c.relnamespace=current_schema()::regnamespace AND c.relkind IN ('r','p')
               AND c.relname IN ('assignment_reconciliations','consumer_registrations','consumer_checkpoints','consumer_receipts','consumer_dead_letters','consumer_alerts','event_retention_state','consumer_operations','document_retention_operations','attachment_storage_operations','document_mentions','mention_intents','discussion_sources')
               AND (has_any_column_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,REFERENCES')
                 OR has_table_privilege(r.oid,c.oid,'DELETE,TRUNCATE,TRIGGER')))",
        )
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
        if worker_authority || consumer_table_authority {
            tracing::error!(
                "Runtime database role must not have direct or reachable worker authority"
            );
            return Err(Error::Internal);
        }
        if operator_authority {
            tracing::error!(
                "Runtime database role must not have direct or reachable operator authority"
            );
            return Err(Error::Internal);
        }
        Ok(Self {
            pool,
            setup_token_hash: digest(setup_token.as_bytes()),
            hashes: Arc::new(Semaphore::new(hash_concurrency)),
        })
    }
    pub async fn ready(&self) -> bool {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }
    pub async fn authenticate(&self, token: &str) -> Result<Actor> {
        self.authenticate_for_audience(token, SessionAudience::Web)
            .await
    }
    pub async fn authenticate_native(&self, token: &str) -> Result<Actor> {
        self.authenticate_for_audience(token, SessionAudience::Desktop)
            .await
    }
    async fn authenticate_for_audience(
        &self,
        token: &str,
        audience: SessionAudience,
    ) -> Result<Actor> {
        if !native_hex_secret(token) {
            return Err(Error::Unauthorized);
        }
        store::find_actor_for_audience(&self.pool, &digest(token.as_bytes()), audience)
            .await
            .map_err(db_error)?
            .ok_or(Error::Unauthorized)
    }
    pub(crate) async fn scoped(
        &self,
        actor: &Actor,
        workspace: Uuid,
        write: bool,
    ) -> Result<store::Tx> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, actor.user.id, Some(workspace))
            .await
            .map_err(db_error)?;
        let role = store::workspace_role(&mut tx, actor, workspace)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !store::active_session(&mut tx, actor)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Unauthorized);
        }
        if write
            && sqlx::query_scalar::<_, bool>(
                "SELECT archived_at IS NOT NULL FROM workspaces WHERE id=$1",
            )
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Conflict(
                "Restore the workspace before making changes".into(),
            ));
        }
        if write && !opentrack_domain::writable(&role) {
            return Err(Error::Forbidden);
        }
        Ok(tx)
    }
}
pub(crate) fn db_error(e: sqlx::Error) -> Error {
    if let sqlx::Error::Database(ref d) = e {
        if d.constraint() == Some("issue_template_capacity") {
            return Error::Conflict(
                "This team already has 100 templates, including archived templates".into(),
            );
        }
        if d.constraint() == Some("issue_restore_expired") {
            return Error::Conflict("The 30-day restore period has ended".into());
        }
        if d.constraint() == Some("issue_deleted_graph") {
            return Error::Conflict(
                "Deleted issues cannot retain or acquire graph connections".into(),
            );
        }
        if d.constraint() == Some("issue_graph_capacity") {
            return Error::Conflict("This graph exceeds the 10,000-issue traversal limit".into());
        }
        if d.constraint() == Some("issue_graph_invalid") {
            return Error::Conflict("This relationship would create a cycle, exceed eight levels, or link a duplicate chain".into());
        }
        match d.code().as_deref() {
            Some("23505") => {
                return Error::Conflict("An item with these details already exists".into());
            }
            Some("23503" | "23514") => {
                return Error::Invalid(
                    "A referenced item is invalid or no longer available".into(),
                );
            }
            Some("55P03" | "57014" | "40001" | "40P01") => return Error::Unavailable,
            _ => {}
        }
    }
    // Never log SQL parameters, connection URLs, passwords, or arbitrary database detail.
    tracing::error!(kind=?std::mem::discriminant(&e),"Database operation failed");
    Error::Internal
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn random_secret() -> String {
    let mut b = [0_u8; 32];
    OsRng.fill_bytes(&mut b);
    b.iter().map(|v| format!("{v:02x}")).collect()
}
pub fn secret_matches(left: &str, right: &str) -> bool {
    use subtle::ConstantTimeEq;
    digest(left.as_bytes())
        .as_bytes()
        .ct_eq(digest(right.as_bytes()).as_bytes())
        .into()
}
