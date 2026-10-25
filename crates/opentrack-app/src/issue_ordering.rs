use crate::{
    App, db_error, digest, graph, lifecycle,
    worker::{BatchOutcome, JobClaim},
};
use opentrack_domain::{Actor, Error, IssueOrderMaintenance, IssueOrderPhase, Result};
use opentrack_store as store;
use sqlx::Row;
use uuid::Uuid;

pub(crate) enum RankAllocation {
    Ready(String),
    Maintenance(IssueOrderMaintenance),
}

async fn maintenance(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    id: Uuid,
) -> Result<IssueOrderMaintenance> {
    let team = store::team_detail(tx, w, t, a.user.id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    let row = sqlx::query("SELECT n.status_id,n.phase,n.staged_count::text,n.applied_count::text,n.total_count::text,j.state,j.version::text,j.error_code,g.epoch,(workspace.archived_at IS NULL AND team.archived_at IS NULL AND status.archived_at IS NULL AND status.retiring_at IS NULL AND EXISTS(SELECT 1 FROM issue_order_guards guard WHERE guard.workspace_id=n.workspace_id AND guard.team_id=n.team_id AND guard.status_id=n.status_id AND guard.job_id=n.id) AND NOT EXISTS(SELECT 1 FROM workflow_statuses retiring WHERE retiring.workspace_id=n.workspace_id AND retiring.team_id=n.team_id AND retiring.retiring_at IS NOT NULL AND (retiring.id=n.status_id OR retiring.replacement_status_id=n.status_id))) AS resumable_scope FROM issue_order_normalizations n JOIN jobs j USING(workspace_id,id) JOIN workspaces workspace ON workspace.id=n.workspace_id JOIN teams team ON team.workspace_id=n.workspace_id AND team.id=n.team_id JOIN workflow_statuses status ON status.workspace_id=n.workspace_id AND status.team_id=n.team_id AND status.id=n.status_id JOIN workspace_auth_guards g ON g.workspace_id=n.workspace_id WHERE n.workspace_id=$1 AND n.team_id=$2 AND n.id=$3 AND j.kind='issues.order_normalize'")
        .bind(w).bind(t).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    let state: String = row.try_get("state").map_err(db_error)?;
    let version: String = row.try_get("version").map_err(db_error)?;
    let phase = match row.try_get::<&str, _>("phase").map_err(db_error)? {
        "staging" => IssueOrderPhase::Staging,
        "applying" => IssueOrderPhase::Applying,
        "completed" => IssueOrderPhase::Completed,
        _ => return Err(Error::Internal),
    };
    let can_resume = team.can_manage
        && row
            .try_get::<bool, _>("resumable_scope")
            .map_err(db_error)?
        && ["blocked", "dead_letter", "retry_wait"].contains(&state.as_str())
        && !matches!(phase, IssueOrderPhase::Completed);
    let epoch: i64 = row.try_get("epoch").map_err(db_error)?;
    let binding = digest(&serde_json::to_vec(&(w, a.user.id, epoch)).map_err(|_| Error::Internal)?);
    Ok(IssueOrderMaintenance {
        id,
        team_id: t,
        status_id: row.try_get("status_id").map_err(db_error)?,
        state,
        phase,
        staged_count: row.try_get("staged_count").map_err(db_error)?,
        applied_count: row.try_get("applied_count").map_err(db_error)?,
        total_count: row.try_get("total_count").map_err(db_error)?,
        error_code: row.try_get("error_code").map_err(db_error)?,
        etag: IssueOrderMaintenance::etag(id, &version, can_resume, &binding),
        version,
        can_resume,
    })
}

pub(crate) async fn active(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    status: Option<Uuid>,
) -> Result<Option<IssueOrderMaintenance>> {
    let id: Option<Uuid> = sqlx::query_scalar("SELECT job_id FROM issue_order_guards WHERE workspace_id=$1 AND team_id=$2 AND ($3::uuid IS NULL OR status_id=$3) ORDER BY status_id LIMIT 1")
        .bind(w).bind(t).bind(status).fetch_optional(&mut **tx).await.map_err(db_error)?;
    match id {
        Some(id) => maintenance(tx, a, w, t, id).await.map(Some),
        None => Ok(None),
    }
}

pub(crate) async fn append(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    status: Uuid,
) -> Result<RankAllocation> {
    if let Some(progress) = active(tx, a, w, t, Some(status)).await? {
        return Ok(RankAllocation::Maintenance(progress));
    }
    if let Some(rank) = store::append_issue_rank(tx, w, t, status)
        .await
        .map_err(db_error)?
    {
        return Ok(RankAllocation::Ready(rank));
    }
    let inline: bool = sqlx::query_scalar("SELECT issue_order_rebalance_inline($1,$2,$3)")
        .bind(w)
        .bind(t)
        .bind(status)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    if inline {
        return Ok(RankAllocation::Ready(
            store::append_issue_rank(tx, w, t, status)
                .await
                .map_err(db_error)?
                .ok_or(Error::Internal)?,
        ));
    }
    enqueue(tx, a, w, t, status)
        .await
        .map(RankAllocation::Maintenance)
}

async fn enqueue(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    status: Uuid,
) -> Result<IssueOrderMaintenance> {
    let generation: i64 =
        sqlx::query_scalar("SELECT ordering_version FROM teams WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(t)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    let id = Uuid::new_v4();
    graph::one(sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'issues.order_normalize',$3,$4)").bind(w).bind(id).bind(a.user.id).bind(format!("{t}:{status}:{generation}")).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    graph::one(sqlx::query("INSERT INTO issue_order_normalizations(workspace_id,id,team_id,status_id,original_requested_by) VALUES($1,$2,$3,$4,$5)").bind(w).bind(id).bind(t).bind(status).bind(a.user.id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    graph::one(sqlx::query("INSERT INTO issue_order_guards(workspace_id,team_id,status_id,job_id) VALUES($1,$2,$3,$4)").bind(w).bind(t).bind(status).bind(id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    sqlx::query(
        "INSERT INTO worker_workspace_registry(workspace_id) VALUES($1) ON CONFLICT DO NOTHING",
    )
    .bind(w)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    let registered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM worker_workspace_registry WHERE workspace_id=$1)",
    )
    .bind(w)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if !registered {
        return Err(Error::Internal);
    }
    store::identity_audit(
        tx,
        w,
        a.user.id,
        id,
        "issue.order_maintenance_requested",
        id,
    )
    .await
    .map_err(db_error)?;
    maintenance(tx, a, w, t, id).await
}

impl App {
    pub async fn issue_order_maintenance(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<IssueOrderMaintenance> {
        let mut tx = self.scoped(a, w, false).await?;
        let result = maintenance(&mut tx, a, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }

    pub async fn resume_issue_order_maintenance(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
    ) -> Result<IssueOrderMaintenance> {
        let mut tx = self.scoped(a, w, true).await?;
        maintenance(&mut tx, a, w, t, id).await?;
        sqlx::query("SELECT id FROM jobs WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if !store::visible_team(&mut tx, w, t, a.user.id, true)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let team = store::team_detail(&mut tx, w, t, a.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !team.can_manage {
            return Err(Error::Forbidden);
        }
        let current = maintenance(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &current.etag, &current)?;
        if !current.can_resume {
            return Err(Error::Conflict(
                "This maintenance operation cannot currently be resumed".into(),
            ));
        }
        graph::one(sqlx::query("UPDATE jobs SET requested_by=$3,state='queued',retry_count=0,error_code=NULL,available_at=clock_timestamp(),finished_at=NULL,updated_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).bind(a.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        store::identity_audit(
            &mut tx,
            w,
            a.user.id,
            id,
            "issue.order_maintenance_resumed",
            Uuid::new_v4(),
        )
        .await
        .map_err(db_error)?;
        let result = maintenance(&mut tx, a, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}

pub(crate) async fn run_batch(tx: &mut store::Tx, claim: &JobClaim) -> Result<BatchOutcome> {
    let outcome: String = sqlx::query_scalar("SELECT worker_issue_order_batch($1,$2,$3,$4,$5)")
        .bind(claim.workspace_id)
        .bind(claim.id)
        .bind(claim.requested_by.ok_or(Error::Internal)?)
        .bind(claim.lease_owner)
        .bind(claim.lease_token)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(match outcome.as_str() {
        "completed" => BatchOutcome::Complete,
        "more" => BatchOutcome::More,
        "access_lost" => BatchOutcome::Blocked("access_lost"),
        "parent_archived" => BatchOutcome::Blocked("parent_archived"),
        "target_unavailable" => BatchOutcome::Blocked("target_unavailable"),
        _ => return Err(Error::Internal),
    })
}
