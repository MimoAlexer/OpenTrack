//! One durable batch per claim. No sessions, operator credential, or external effects.
use crate::{assignment_cleanup, db_error, issue_ordering, workflow};
use opentrack_domain::{Error, Result};
use opentrack_store as store;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct JobClaim {
    pub workspace_id: Uuid,
    pub id: Uuid,
    pub requested_by: Option<Uuid>,
    pub kind: String,
    pub lease_owner: Uuid,
    pub lease_token: i64,
    pub attempt_count: i32,
}
#[derive(Debug)]
pub enum BatchOutcome {
    Complete,
    More,
    Blocked(&'static str),
}
#[derive(Clone, Copy, Debug)]
pub struct WorkerOptions {
    pub max_connections: u32,
    pub lease_seconds: u32,
}
impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            max_connections: 5,
            lease_seconds: 60,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerTick {
    Idle,
    Scanning,
    Progress,
    Completed,
    Blocked,
    Retry,
    LostLease,
}
#[derive(Clone)]
pub struct Worker {
    pub pool: PgPool,
    id: Uuid,
    options: WorkerOptions,
}
impl Worker {
    pub async fn connect(url: &str, options: WorkerOptions) -> Result<Self> {
        if !(1..=50).contains(&options.max_connections)
            || !(10..=300).contains(&options.lease_seconds)
        {
            return Err(Error::Invalid("Invalid worker limits".into()));
        }
        let pool = PgPoolOptions::new()
            .max_connections(options.max_connections)
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(url)
            .await
            .map_err(db_error)?;
        let forbidden: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname=current_user AND (r.rolsuper OR r.rolbypassrls OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolinherit)) OR EXISTS(SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member OR r.oid=m.roleid WHERE r.rolname=current_user) OR has_schema_privilege(current_user,current_schema(),'CREATE') OR EXISTS(SELECT 1 FROM pg_class c WHERE c.relnamespace=current_schema()::regnamespace AND c.relowner=current_user::regrole)")
            .fetch_one(&pool).await.map_err(db_error)?;
        let unsafe_tables: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_class c WHERE c.relnamespace=current_schema()::regnamespace AND c.relkind='r' AND c.relname IN('users','sessions','login_attempts','recovery_tokens','account_security_events','organizations','organization_members') AND (has_any_column_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,REFERENCES') OR has_table_privilege(current_user,c.oid,'DELETE,TRUNCATE,TRIGGER')))")
            .fetch_one(&pool).await.map_err(db_error)?;
        let capabilities: bool=sqlx::query_scalar("SELECT coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_ready_workspaces(uuid,integer)',current_schema())),'EXECUTE'),false) AND coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_lock_context(uuid,uuid)',current_schema())),'EXECUTE'),false) AND (to_regclass('consumer_receipts') IS NULL OR (coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_capture_events(uuid,integer)',current_schema())),'EXECUTE'),false) AND coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_retain_events(uuid,integer)',current_schema())),'EXECUTE'),false) AND coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_lock_maintenance_context(uuid)',current_schema())),'EXECUTE'),false) AND coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_assignee_eligible(uuid,uuid,uuid)',current_schema())),'EXECUTE'),false) AND coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_clear_assignment(uuid,uuid,uuid,uuid,uuid,uuid,bigint)',current_schema())),'EXECUTE'),false))) AND NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proname LIKE 'operator\\_%' ESCAPE '\\' AND has_function_privilege(current_user,p.oid,'EXECUTE'))")
            .fetch_one(&pool).await.map_err(db_error)?;
        let extra_authority: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_class c JOIN pg_attribute a ON a.attrelid=c.oid
             WHERE c.relnamespace=current_schema()::regnamespace AND c.relkind IN('r','p') AND a.attnum>0 AND NOT a.attisdropped AND (
              (has_column_privilege(current_user,c.oid,a.attnum,'SELECT') AND NOT (
                c.relname IN('jobs','job_attempts','workflow_statuses','workflow_replacements','workspace_stream_heads','assignment_reconciliations','issue_order_guards')
                OR (c.relname='workspaces' AND a.attname IN('id','archived_at'))
                OR (c.relname='workspace_members' AND a.attname IN('workspace_id','user_id','role','active'))
                OR (c.relname='team_members' AND a.attname IN('workspace_id','team_id','user_id','role'))
                OR (c.relname='teams' AND a.attname IN('id','workspace_id','private','archived_at','workflow_version'))
                OR (c.relname='_sqlx_migrations' AND a.attname IN('version','success','checksum'))
                OR (c.relname='issues' AND a.attname IN('id','workspace_id','team_id','status_id','version','archived_at','deleted_at','assignee_id'))))
              OR (has_column_privilege(current_user,c.oid,a.attnum,'INSERT') AND c.relname NOT IN('job_attempts','activity_entries','audit_entries','workspace_events'))
              OR (has_column_privilege(current_user,c.oid,a.attnum,'UPDATE') AND NOT (
                (c.relname='jobs' AND a.attname IN('state','version','available_at','lease_owner','lease_token','lease_expires_at','attempt_count','retry_count','error_code','updated_at','finished_at'))
                OR (c.relname='job_attempts' AND a.attname IN('lease_token','ended_at','outcome'))
                OR (c.relname='teams' AND a.attname='workflow_version')
                OR (c.relname='workflow_statuses' AND a.attname IN('archived_at','retiring_at','replacement_status_id','replacement_job_id','is_default'))
                OR (c.relname='issues' AND a.attname IN('status_id','version','updated_at'))
                OR (c.relname='assignment_reconciliations' AND a.attname IN('after_team_id','after_issue_id','scanned_count','cleared_count'))
                OR (c.relname='workflow_replacements' AND a.attname IN('processed_count','remaining_count'))
                OR (c.relname='workspace_stream_heads' AND a.attname='last_seq')))
              OR has_table_privilege(current_user,c.oid,'DELETE,TRUNCATE,TRIGGER')
              OR has_column_privilege(current_user,c.oid,a.attnum,'REFERENCES')))
             OR EXISTS(SELECT 1 FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proowner=current_user::regrole)
             OR EXISTS(SELECT 1 FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.prorettype<>'trigger'::regtype AND has_function_privilege(current_user,p.oid,'EXECUTE') AND p.proname NOT IN('worker_ready_workspaces','worker_lock_context','worker_lock_maintenance_context','worker_assignee_eligible','worker_clear_assignment','worker_capture_events','worker_retain_events','worker_prune_document_revisions','worker_claim_attachment_gc','worker_ack_attachment_gc','worker_issue_order_batch'))
             OR has_database_privilege(current_user,current_database(),'CREATE,TEMP')"
        ).fetch_one(&pool).await.map_err(db_error)?;
        let revision_capability:bool=sqlx::query_scalar("SELECT to_regclass('documents') IS NULL OR coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_prune_document_revisions(uuid,integer)',current_schema())),'EXECUTE'),false)").fetch_one(&pool).await.map_err(db_error)?;
        let attachment_capability:bool=sqlx::query_scalar("SELECT to_regclass('attachments') IS NULL OR (coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_claim_attachment_gc(uuid,integer)',current_schema())),'EXECUTE'),false) AND coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_ack_attachment_gc(uuid,uuid,bigint,uuid)',current_schema())),'EXECUTE'),false))").fetch_one(&pool).await.map_err(db_error)?;
        let ordering_capability: bool = sqlx::query_scalar("SELECT to_regclass('issue_order_normalizations') IS NULL OR (to_regclass('issue_order_staging') IS NOT NULL AND to_regclass('issue_order_guards') IS NOT NULL AND coalesce(has_function_privilege(current_user,to_regprocedure(format('%I.worker_issue_order_batch(uuid,uuid,uuid,uuid,bigint)',current_schema())),'EXECUTE'),false))")
            .fetch_one(&pool).await.map_err(db_error)?;
        if forbidden
            || unsafe_tables
            || extra_authority
            || !capabilities
            || !revision_capability
            || !attachment_capability
            || !ordering_capability
        {
            tracing::error!(
                "Worker credential must be isolated, restricted, and explicitly provisioned"
            );
            pool.close().await;
            return Err(Error::Internal);
        }
        Ok(Self {
            pool,
            id: Uuid::new_v4(),
            options,
        })
    }
    /// Bounded discovery and exactly one committed effect batch at most.
    /// The caller retains the cursor between ticks so idle workspaces cannot starve later ones.
    pub async fn tick(&self, cursor: &mut Option<Uuid>) -> Result<WorkerTick> {
        let continuing = cursor.is_some();
        match self.next_claim(cursor).await? {
            Some(claim) => self.run_claim(&claim).await,
            None if continuing || cursor.is_some() => Ok(WorkerTick::Scanning),
            None => Ok(WorkerTick::Idle),
        }
    }
    pub async fn next_claim(&self, cursor: &mut Option<Uuid>) -> Result<Option<JobClaim>> {
        let scopes: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM worker_ready_workspaces($1,20)")
                .bind(*cursor)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?;
        if scopes.is_empty() {
            *cursor = None;
            return Ok(None);
        }
        for workspace in scopes {
            *cursor = Some(workspace);
            if let Some(claim) = self.claim(workspace).await? {
                return Ok(Some(claim));
            }
        }
        Ok(None)
    }
    pub async fn claim(&self, workspace: Uuid) -> Result<Option<JobClaim>> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true),set_config('statement_timeout','3000',true),set_config('lock_timeout','1000',true),set_config('idle_in_transaction_session_timeout','5000',true)").bind(workspace.to_string()).execute(&mut *tx).await.map_err(db_error)?;
        let row=sqlx::query("SELECT id,kind,requested_by,state,lease_token,attempt_count,retry_count,max_attempts FROM jobs WHERE workspace_id=$1 AND kind IN('workflow.status_replace','assignment.reconcile','issues.order_normalize') AND ((state IN('queued','retry_wait') AND available_at<=clock_timestamp()) OR (state='running' AND lease_expires_at<=clock_timestamp())) ORDER BY available_at,id FOR UPDATE SKIP LOCKED LIMIT 1").bind(workspace).fetch_optional(&mut *tx).await.map_err(db_error)?;
        let Some(row) = row else {
            tx.commit().await.map_err(db_error)?;
            return Ok(None);
        };
        let id: Uuid = row.try_get("id").map_err(db_error)?;
        let state: String = row.try_get("state").map_err(db_error)?;
        let mut attempt: i32 = row.try_get("attempt_count").map_err(db_error)?;
        let mut retries: i32 = row.try_get("retry_count").map_err(db_error)?;
        let maximum: i32 = row.try_get("max_attempts").map_err(db_error)?;
        if state == "running" {
            required(sqlx::query("UPDATE job_attempts SET ended_at=clock_timestamp(),outcome='abandoned' WHERE workspace_id=$1 AND job_id=$2 AND attempt=$3 AND ended_at IS NULL").bind(workspace).bind(id).bind(attempt).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            retries += 1;
            if retries >= maximum {
                required(sqlx::query("UPDATE jobs SET state='dead_letter',retry_count=$3,error_code='attempt_limit',lease_owner=NULL,lease_expires_at=NULL,finished_at=clock_timestamp(),updated_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(workspace).bind(id).bind(retries).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
                tx.commit().await.map_err(db_error)?;
                return Ok(None);
            }
        }
        let fence: i64 = row
            .try_get::<i64, _>("lease_token")
            .map_err(db_error)?
            .checked_add(1)
            .ok_or(Error::Internal)?;
        let open:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM job_attempts WHERE workspace_id=$1 AND job_id=$2 AND attempt=$3 AND ended_at IS NULL)").bind(workspace).bind(id).bind(attempt).fetch_one(&mut *tx).await.map_err(db_error)?;
        if open {
            required(sqlx::query("UPDATE job_attempts SET lease_token=$4 WHERE workspace_id=$1 AND job_id=$2 AND attempt=$3 AND ended_at IS NULL").bind(workspace).bind(id).bind(attempt).bind(fence).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        } else {
            attempt = attempt.checked_add(1).ok_or(Error::Internal)?;
            required(sqlx::query("INSERT INTO job_attempts(workspace_id,job_id,attempt,lease_token) VALUES($1,$2,$3,$4)").bind(workspace).bind(id).bind(attempt).bind(fence).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        required(sqlx::query("UPDATE jobs SET state='running',lease_owner=$3,lease_token=$4,lease_expires_at=clock_timestamp()+make_interval(secs=>$5),attempt_count=$6,retry_count=$7,error_code=NULL,finished_at=NULL,updated_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(workspace).bind(id).bind(self.id).bind(fence).bind(f64::from(self.options.lease_seconds)).bind(attempt).bind(retries).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let claim = JobClaim {
            workspace_id: workspace,
            id,
            requested_by: row.try_get("requested_by").map_err(db_error)?,
            kind: row.try_get("kind").map_err(db_error)?,
            lease_owner: self.id,
            lease_token: fence,
            attempt_count: attempt,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(Some(claim))
    }
    pub async fn run_claim(&self, claim: &JobClaim) -> Result<WorkerTick> {
        match self.effect(claim).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                // The effect transaction has dropped/rolled back before recording its failure.
                let transient = matches!(error, Error::Unavailable);
                tracing::warn!(job_id=%claim.id,transient,"Worker batch failed; durable attempt will be retried or blocked");
                self.fail(claim, transient).await
            }
        }
    }
    async fn effect(&self, claim: &JobClaim) -> Result<WorkerTick> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        let enabled: bool = match (claim.kind.as_str(), claim.requested_by) {
            ("workflow.status_replace" | "issues.order_normalize", Some(actor)) => {
                sqlx::query_scalar("SELECT worker_lock_context($1,$2)")
                    .bind(actor)
                    .bind(claim.workspace_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_error)?
            }
            ("assignment.reconcile", None) => {
                sqlx::query_scalar("SELECT worker_lock_maintenance_context($1)")
                    .bind(claim.workspace_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_error)?
            }
            _ => return Err(Error::Internal),
        };
        if !lock_claim(&mut tx, claim).await? {
            tx.rollback().await.map_err(db_error)?;
            return Ok(WorkerTick::LostLease);
        }
        // Lock the attempt before the handler can acquire the stream head.
        let found:Option<i32>=sqlx::query_scalar("SELECT attempt FROM job_attempts WHERE workspace_id=$1 AND job_id=$2 AND attempt=$3 AND ended_at IS NULL FOR UPDATE").bind(claim.workspace_id).bind(claim.id).bind(claim.attempt_count).fetch_optional(&mut *tx).await.map_err(db_error)?;
        if found.is_none() {
            return Err(Error::Internal);
        }
        let outcome = if enabled {
            if claim.kind == "assignment.reconcile" {
                assignment_cleanup::run_batch(&mut tx, claim).await?
            } else if claim.kind == "issues.order_normalize" {
                issue_ordering::run_batch(&mut tx, claim).await?
            } else {
                workflow::run_replacement_batch(&mut tx, claim).await?
            }
        } else {
            BatchOutcome::Blocked("access_lost")
        };
        let (state, error, result) = match outcome {
            BatchOutcome::Complete => ("succeeded", None, WorkerTick::Completed),
            BatchOutcome::More => ("queued", None, WorkerTick::Progress),
            BatchOutcome::Blocked(code) => ("blocked", Some(code), WorkerTick::Blocked),
        };
        if state != "queued" {
            required(sqlx::query("UPDATE job_attempts SET ended_at=clock_timestamp(),outcome=$4 WHERE workspace_id=$1 AND job_id=$2 AND attempt=$3 AND ended_at IS NULL").bind(claim.workspace_id).bind(claim.id).bind(claim.attempt_count).bind(state).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        let count=sqlx::query("UPDATE jobs SET state=$5,error_code=$6,lease_owner=NULL,lease_expires_at=NULL,available_at=clock_timestamp(),finished_at=CASE WHEN $5='queued' THEN NULL ELSE clock_timestamp() END,updated_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2 AND lease_owner=$3 AND lease_token=$4 AND state='running' AND lease_expires_at>clock_timestamp()")
            .bind(claim.workspace_id).bind(claim.id).bind(claim.lease_owner).bind(claim.lease_token).bind(state).bind(error).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
        if count != 1 {
            tx.rollback().await.map_err(db_error)?;
            return Ok(WorkerTick::LostLease);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    async fn fail(&self, claim: &JobClaim, transient: bool) -> Result<WorkerTick> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true),set_config('statement_timeout','3000',true),set_config('lock_timeout','1000',true),set_config('idle_in_transaction_session_timeout','5000',true)")
            .bind(claim.workspace_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        if !lock_claim(&mut tx, claim).await? {
            tx.rollback().await.map_err(db_error)?;
            return Ok(WorkerTick::LostLease);
        }
        let row = sqlx::query(
            "SELECT retry_count,max_attempts FROM jobs WHERE workspace_id=$1 AND id=$2",
        )
        .bind(claim.workspace_id)
        .bind(claim.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let retries = row.try_get::<i32, _>("retry_count").map_err(db_error)? + 1;
        let retry =
            transient && retries < row.try_get::<i32, _>("max_attempts").map_err(db_error)?;
        let state = if retry { "retry_wait" } else { "dead_letter" };
        let code = if transient {
            "dependency_unavailable"
        } else {
            "effect_failed"
        };
        let delay = (1_u32.checked_shl(retries.min(10) as u32).unwrap_or(900)).min(900);
        required(sqlx::query("UPDATE job_attempts SET ended_at=clock_timestamp(),outcome=$4 WHERE workspace_id=$1 AND job_id=$2 AND attempt=$3 AND ended_at IS NULL").bind(claim.workspace_id).bind(claim.id).bind(claim.attempt_count).bind(state).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        required(sqlx::query("UPDATE jobs SET state=$5,retry_count=$6,error_code=$7,lease_owner=NULL,lease_expires_at=NULL,available_at=clock_timestamp()+make_interval(secs=>$8),finished_at=CASE WHEN $5='retry_wait' THEN NULL ELSE clock_timestamp() END,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 AND lease_owner=$3 AND lease_token=$4 AND lease_expires_at>clock_timestamp()")
            .bind(claim.workspace_id).bind(claim.id).bind(claim.lease_owner).bind(claim.lease_token).bind(state).bind(retries).bind(code).bind(f64::from(delay)).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        tx.commit().await.map_err(db_error)?;
        Ok(WorkerTick::Retry)
    }
}
async fn lock_claim(tx: &mut store::Tx, claim: &JobClaim) -> Result<bool> {
    Ok(sqlx::query_scalar::<_,Uuid>("SELECT id FROM jobs WHERE workspace_id=$1 AND id=$2 AND requested_by IS NOT DISTINCT FROM $3 AND kind=$6 AND state='running' AND lease_owner=$4 AND lease_token=$5 AND lease_expires_at>clock_timestamp() FOR UPDATE")
        .bind(claim.workspace_id).bind(claim.id).bind(claim.requested_by).bind(claim.lease_owner).bind(claim.lease_token).bind(&claim.kind).fetch_optional(&mut **tx).await.map_err(db_error)?.is_some())
}
fn required(count: u64) -> Result<()> {
    if count == 1 {
        Ok(())
    } else {
        Err(Error::Internal)
    }
}
