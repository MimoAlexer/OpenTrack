use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::{RecoveryComplete, RecoveryReceipt, validate_password};

/// A separately provisioned operator login can execute the explicit recovery and
/// account lifecycle commands. It never becomes an App principal or receives table access.
#[derive(Clone)]
pub struct Operator {
    pool: PgPool,
}

/// Receipt for a committed operator action. It contains no credential or reason.
#[derive(Debug, serde::Serialize)]
pub struct AccountLifecycleOutcome {
    pub user_id: Uuid,
    pub disabled: bool,
    pub changed: bool,
    pub disabled_at: Option<DateTime<Utc>>,
    pub correlation_id: Uuid,
    pub revoked_sessions: u64,
    pub revoked_recovery_codes: u64,
    pub revoked_invitations: u64,
}

#[derive(sqlx::FromRow)]
struct AccountLifecycleRow {
    user_id: Uuid,
    disabled: bool,
    changed: bool,
    disabled_at: Option<DateTime<Utc>>,
    correlation_id: Uuid,
    revoked_sessions: i64,
    revoked_recovery_codes: i64,
    revoked_invitations: i64,
}

impl Operator {
    /// This preflight runs before the CLI creates a file containing a new code.
    /// Effective privileges are checked, including PUBLIC and inherited grants.
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(url)
            .await
            .map_err(db_error)?;
        let prohibited: bool = sqlx::query_scalar(
            "SELECT current_user<>session_user OR EXISTS(
               SELECT 1 FROM pg_roles WHERE rolname<>current_user
               AND pg_has_role(current_user,oid,'MEMBER')) OR EXISTS(
               SELECT 1 FROM pg_roles WHERE pg_has_role(current_user,oid,'MEMBER')
               AND (rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb OR rolreplication))
             OR has_schema_privilege(current_schema(),'CREATE')
             OR EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
               WHERE n.nspname=current_schema() AND c.relkind IN ('r','p','v','m','f')
               AND (pg_has_role(current_user,c.relowner,'MEMBER')
                 OR has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
                 OR has_any_column_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,REFERENCES')))
             OR EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
               WHERE n.nspname=current_schema() AND c.relkind='S'
               AND has_sequence_privilege(current_user,c.oid,'USAGE,SELECT,UPDATE'))
             OR EXISTS(SELECT 1 FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace
               AND left(p.proname,7)='worker_' AND has_function_privilege(current_user,p.oid,'EXECUTE'))",
        )
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
        let allowed: bool = sqlx::query_scalar(
            "SELECT COALESCE(has_function_privilege(current_user,
               to_regprocedure(format('%I.operator_issue_recovery(uuid,uuid,text,integer,text)',current_schema())),'EXECUTE'),false)
             AND COALESCE(has_function_privilege(current_user,
               to_regprocedure(format('%I.operator_revoke_recovery(uuid,text)',current_schema())),'EXECUTE'),false)
             AND COALESCE(has_function_privilege(current_user,
               to_regprocedure(format('%I.operator_disable_account(uuid,text)',current_schema())),'EXECUTE'),false)
             AND COALESCE(has_function_privilege(current_user,
               to_regprocedure(format('%I.operator_enable_account(uuid,text)',current_schema())),'EXECUTE'),false)",
        )
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
        if prohibited || !allowed {
            tracing::error!(
                "Operator database role must have only the explicit operator capabilities and no role memberships, table, ownership, schema creation, or elevated privileges"
            );
            return Err(Error::Forbidden);
        }
        Ok(Self { pool })
    }

    pub async fn consumer_status(&self, workspace: Uuid) -> Result<serde_json::Value> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer_bounds(&mut tx).await?;
        let status = sqlx::query_scalar("SELECT operator_consumer_status($1) || jsonb_build_object('event_admission',operator_event_admission_status($1))")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .map_err(operator_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(status)
    }

    pub async fn set_event_admission_limits(
        &self,
        workspace: Uuid,
        max_pending_events: i64,
        max_event_bytes: i64,
        reason: &str,
    ) -> Result<Uuid> {
        if max_pending_events <= 0 || max_event_bytes <= 0 {
            return Err(Error::Invalid(
                "Event limits must be positive integers".into(),
            ));
        }
        let reason = operator_reason(reason)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer_bounds(&mut tx).await?;
        let correlation = sqlx::query_scalar("SELECT operator_event_admission_limits($1,$2,$3,$4)")
            .bind(workspace)
            .bind(max_pending_events)
            .bind(max_event_bytes)
            .bind(reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(operator_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(correlation)
    }

    pub async fn consumer_disposition(
        &self,
        workspace: Uuid,
        dead_letter: Uuid,
        disposition: &str,
        reason: &str,
    ) -> Result<Uuid> {
        if !matches!(disposition, "retry" | "reconcile") {
            return Err(Error::Invalid(
                "Consumer disposition must be retry or reconcile".into(),
            ));
        }
        let reason = operator_reason(reason)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer_bounds(&mut tx).await?;
        let correlation = sqlx::query_scalar("SELECT operator_consumer_disposition($1,$2,$3,$4)")
            .bind(workspace)
            .bind(dead_letter)
            .bind(disposition)
            .bind(reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(operator_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(correlation)
    }

    pub async fn set_consumer_enabled(
        &self,
        workspace: Uuid,
        enabled: bool,
        reason: &str,
    ) -> Result<Uuid> {
        let reason = operator_reason(reason)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer_bounds(&mut tx).await?;
        let correlation = sqlx::query_scalar("SELECT operator_consumer_set_enabled($1,$2,$3)")
            .bind(workspace)
            .bind(enabled)
            .bind(reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(operator_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(correlation)
    }

    pub async fn resume_assignment_cleanup(
        &self,
        workspace: Uuid,
        job: Uuid,
        reason: &str,
    ) -> Result<Uuid> {
        let reason = operator_reason(reason)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer_bounds(&mut tx).await?;
        let correlation = sqlx::query_scalar("SELECT operator_resume_assignment_cleanup($1,$2,$3)")
            .bind(workspace)
            .bind(job)
            .bind(reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(operator_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(correlation)
    }

    /// The caller has already written this code to its private create-new file.
    /// An uncertain commit must be reported as uncertain; a fresh receipt and code
    /// safely supersede all prior pending codes. A receipt is never resurrected.
    pub async fn issue_recovery(
        &self,
        user_id: Uuid,
        receipt_id: Uuid,
        code: &str,
        ttl_minutes: u32,
        reason: &str,
    ) -> Result<RecoveryReceipt> {
        if recovery_user(code).ok() != Some(user_id) {
            return Err(Error::Invalid("Invalid recovery code format".into()));
        }
        if !(5..=60).contains(&ttl_minutes) {
            return Err(Error::Invalid(
                "Recovery lifetime must be between 5 and 60 minutes".into(),
            ));
        }
        let reason = operator_reason(reason)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        let expires_at: DateTime<Utc> =
            sqlx::query_scalar("SELECT operator_issue_recovery($1,$2,$3,$4,$5)")
                .bind(user_id)
                .bind(receipt_id)
                .bind(digest(code.as_bytes()))
                .bind(ttl_minutes as i32)
                .bind(reason)
                .fetch_one(&mut *tx)
                .await
                .map_err(operator_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(RecoveryReceipt {
            id: receipt_id,
            user_id,
            expires_at,
        })
    }

    pub async fn revoke_recovery(&self, user_id: Uuid, reason: &str) -> Result<u64> {
        let reason = operator_reason(reason)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        let affected: i32 = sqlx::query_scalar("SELECT operator_revoke_recovery($1,$2)")
            .bind(user_id)
            .bind(reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(operator_error)?;
        tx.commit().await.map_err(db_error)?;
        u64::try_from(affected).map_err(|_| Error::Internal)
    }

    pub async fn disable_account(
        &self,
        user_id: Uuid,
        reason: &str,
    ) -> Result<AccountLifecycleOutcome> {
        self.set_account_disabled(user_id, reason, true).await
    }

    pub async fn enable_account(
        &self,
        user_id: Uuid,
        reason: &str,
    ) -> Result<AccountLifecycleOutcome> {
        self.set_account_disabled(user_id, reason, false).await
    }

    async fn set_account_disabled(
        &self,
        user_id: Uuid,
        reason: &str,
        disabled: bool,
    ) -> Result<AccountLifecycleOutcome> {
        let reason = operator_reason(reason)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        let query = if disabled {
            sqlx::query_as::<_, AccountLifecycleRow>(
                "SELECT * FROM operator_disable_account($1,$2)",
            )
        } else {
            sqlx::query_as::<_, AccountLifecycleRow>("SELECT * FROM operator_enable_account($1,$2)")
        };
        let row = query
            .bind(user_id)
            .bind(reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(operator_error)?;
        let receipt = AccountLifecycleOutcome {
            user_id: row.user_id,
            disabled: row.disabled,
            changed: row.changed,
            disabled_at: row.disabled_at,
            correlation_id: row.correlation_id,
            revoked_sessions: u64::try_from(row.revoked_sessions).map_err(|_| Error::Internal)?,
            revoked_recovery_codes: u64::try_from(row.revoked_recovery_codes)
                .map_err(|_| Error::Internal)?,
            revoked_invitations: u64::try_from(row.revoked_invitations)
                .map_err(|_| Error::Internal)?,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(receipt)
    }
}

/// Versioned bearer: a public account identifier plus 256 OS-random bits.
pub fn generate_recovery_code(user_id: Uuid) -> String {
    format!("otrec1.{user_id}.{}", random_secret())
}

fn recovery_user(code: &str) -> Result<Uuid> {
    // Exact size prevents unbounded splitting/allocation and rejects alternate
    // encodings of the same bearer. Only the full string's digest is persisted.
    if code.len() != 108 || !code.starts_with("otrec1.") {
        return Err(Error::InvalidRecovery);
    }
    let mut parts = code.split('.');
    let prefix = parts.next();
    let user = parts.next().ok_or(Error::InvalidRecovery)?;
    let secret = parts.next().ok_or(Error::InvalidRecovery)?;
    if prefix != Some("otrec1")
        || parts.next().is_some()
        || user.len() != 36
        || secret.len() != 64
        || !secret
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::InvalidRecovery);
    }
    let id = Uuid::parse_str(user).map_err(|_| Error::InvalidRecovery)?;
    if user != id.to_string() {
        return Err(Error::InvalidRecovery);
    }
    Ok(id)
}

fn operator_reason(reason: &str) -> Result<&str> {
    let trimmed = reason.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 500 || reason.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Operator reason must contain 1–500 characters without control characters".into(),
        ));
    }
    Ok(trimmed)
}

fn operator_error(error: sqlx::Error) -> Error {
    if let sqlx::Error::Database(ref database) = error {
        match database.code().as_deref() {
            Some("P0002") => return Error::NotFound,
            Some("22023") => return Error::Invalid("Invalid operator request".into()),
            Some("23514") if database.constraint() == Some("consumer_must_be_drained") => return Error::Conflict(
                "Drain the consumer and assignment cleanup jobs before disabling it".into(),
            ),
            Some("23514") if matches!(database.constraint(), Some("enabled_workspace_owner_required" | "enabled_organization_owner_required")) => return Error::Conflict(
                "Keep another enabled owner in every workspace and organization before disabling this account".into(),
            ),
            Some("42501") => return Error::Forbidden,
            _ => {}
        }
    }
    db_error(error)
}

impl App {
    pub async fn complete_recovery(&self, request: RecoveryComplete) -> Result<()> {
        validate_password(&request.password)?;
        let user_id = recovery_user(&request.token)?;
        let token_hash = digest(request.token.as_bytes());
        // Invalid/unknown/expired bearers do not consume expensive Argon2 work.
        // The indexed preliminary check is not authority to mutate anything.
        let mut check = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut check, user_id, None)
            .await
            .map_err(db_error)?;
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM recovery_tokens r JOIN users u ON u.id=r.user_id
             WHERE r.user_id=$1 AND r.token_hash=$2 AND r.purpose='password_reset'
             AND r.used_at IS NULL AND r.revoked_at IS NULL AND r.expires_at>clock_timestamp()
             AND u.disabled_at IS NULL)",
        )
        .bind(user_id)
        .bind(&token_hash)
        .fetch_one(&mut *check)
        .await
        .map_err(db_error)?;
        check.commit().await.map_err(db_error)?;
        if !valid {
            return Err(Error::InvalidRecovery);
        }
        let password_hash = self.password_work(request.password, None).await?;

        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        // Same exclusive guard as login and account-session revocation, acquired
        // before token/session locks. No database transaction spans Argon2 work.
        store::context_exclusive(&mut tx, user_id, None)
            .await
            .map_err(db_error)?;
        let receipt: Uuid = sqlx::query_scalar(
            "SELECT r.id FROM recovery_tokens r JOIN users u ON u.id=r.user_id
             WHERE r.user_id=$1 AND r.token_hash=$2 AND r.purpose='password_reset'
             AND r.used_at IS NULL AND r.revoked_at IS NULL AND r.expires_at>clock_timestamp()
             AND u.disabled_at IS NULL FOR UPDATE OF r",
        )
        .bind(user_id)
        .bind(&token_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::InvalidRecovery)?;
        let updated = sqlx::query("UPDATE users SET password_hash=$2 WHERE id=$1")
            .bind(user_id)
            .bind(password_hash)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        if updated.rows_affected() != 1 {
            return Err(Error::Internal);
        }
        let consumed = sqlx::query(
            "UPDATE recovery_tokens SET used_at=clock_timestamp() WHERE user_id=$1 AND id=$2",
        )
        .bind(user_id)
        .bind(receipt)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        if consumed.rows_affected() != 1 {
            return Err(Error::Internal);
        }
        sqlx::query("UPDATE recovery_tokens SET revoked_at=clock_timestamp() WHERE user_id=$1 AND id<>$2 AND used_at IS NULL AND revoked_at IS NULL")
            .bind(user_id).bind(receipt).execute(&mut *tx).await.map_err(db_error)?;
        let (expected, revoked, audited): (i64, i64, i64) = sqlx::query_as(
            "WITH eligible AS MATERIALIZED (
               SELECT id FROM sessions WHERE user_id=$1 AND revoked_at IS NULL
             ), revoked AS (
               UPDATE sessions s SET revoked_at=clock_timestamp() FROM eligible e
               WHERE s.id=e.id RETURNING s.id
             ), audited AS (
               INSERT INTO account_security_events(user_id,session_id,action)
               SELECT $1,id,'session.revoked' FROM revoked RETURNING id
             ) SELECT (SELECT count(*) FROM eligible),(SELECT count(*) FROM revoked),
               (SELECT count(*) FROM audited)",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if expected != revoked || revoked != audited {
            return Err(Error::Internal);
        }
        let appended = sqlx::query("INSERT INTO account_security_events(user_id,action,actor_kind,recovery_id) VALUES($1,'recovery.completed','recovery',$2)")
            .bind(user_id).bind(receipt).execute(&mut *tx).await.map_err(db_error)?;
        if appended.rows_affected() != 1 {
            return Err(Error::Internal);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }
}

async fn consumer_bounds(tx: &mut store::Tx) -> Result<()> {
    // A definer function cannot retroactively time-limit its outer SELECT.
    sqlx::query("SELECT set_config('statement_timeout','3000',true),set_config('lock_timeout','1000',true),set_config('idle_in_transaction_session_timeout','5000',true)")
        .execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_are_bounded_canonical_and_account_bound() {
        let user = Uuid::new_v4();
        let first = generate_recovery_code(user);
        assert_eq!(recovery_user(&first).unwrap(), user);
        assert_ne!(first, generate_recovery_code(user));
        for invalid in [
            "".to_owned(),
            first.to_uppercase(),
            format!("{first}.extra"),
            first.replacen("otrec1", "otrec2", 1),
            "x".repeat(10_000),
            format!("otrec1.{user}.{}", "g".repeat(64)),
        ] {
            assert!(matches!(
                recovery_user(&invalid),
                Err(Error::InvalidRecovery)
            ));
        }
    }

    #[test]
    fn operator_reasons_are_bounded_and_never_echoed_in_errors() {
        assert_eq!(
            operator_reason("  Lost password  ").unwrap(),
            "Lost password"
        );
        for reason in [
            " ".to_owned(),
            "x".repeat(501),
            "reason\nsecret".to_owned(),
            "\treason".to_owned(),
        ] {
            let error = operator_reason(&reason).unwrap_err().to_string();
            assert_eq!(
                error,
                "Operator reason must contain 1–500 characters without control characters"
            );
        }
    }
}
