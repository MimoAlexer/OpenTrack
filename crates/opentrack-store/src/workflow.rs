use super::*;
use opentrack_domain::{Workflow, WorkflowReplacement};

pub async fn workflow_status(
    tx: &mut Tx,
    w: Uuid,
    t: Uuid,
    s: Uuid,
) -> Result<Option<Status>, sqlx::Error> {
    let body:Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'team_id',team_id,'name',name,'category',category,'color',color,'position',position,'is_default',is_default,'retiring_at',retiring_at,'replacement_status_id',replacement_status_id,'replacement_job_id',replacement_job_id,'archived_at',archived_at) FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND id=$3")
        .bind(w).bind(t).bind(s).fetch_optional(&mut **tx).await?;
    body.map(decode).transpose()
}
pub async fn workflow(
    tx: &mut Tx,
    w: Uuid,
    t: Uuid,
    user: Uuid,
) -> Result<Option<Workflow>, sqlx::Error> {
    let Some(team) = team_detail(tx, w, t, user).await? else {
        return Ok(None);
    };
    let version: i64 =
        sqlx::query_scalar("SELECT workflow_version FROM teams WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(t)
            .fetch_one(&mut **tx)
            .await?;
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND archived_at IS NULL ORDER BY CASE category WHEN 'triage' THEN 0 WHEN 'backlog' THEN 1 WHEN 'unstarted' THEN 2 WHEN 'started' THEN 3 WHEN 'completed' THEN 4 ELSE 5 END,position,id").bind(w).bind(t).fetch_all(&mut **tx).await?;
    let mut statuses = Vec::with_capacity(ids.len());
    for id in ids {
        statuses.push(
            workflow_status(tx, w, t, id)
                .await?
                .ok_or(sqlx::Error::RowNotFound)?,
        );
    }
    Ok(Some(Workflow {
        team_id: t,
        version: version.to_string(),
        can_manage: team.can_manage,
        statuses,
    }))
}
pub async fn workflow_replacement(
    tx: &mut Tx,
    w: Uuid,
    t: Uuid,
    id: Uuid,
) -> Result<Option<WorkflowReplacement>, sqlx::Error> {
    let body:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'team_id',r.team_id,'source_status_id',r.source_status_id,'replacement_status_id',r.replacement_status_id,'state',j.state,'matched_count',r.matched_count::text,'processed_count',r.processed_count::text,'remaining_count',r.remaining_count::text,'version',j.version::text,'error_code',j.error_code) FROM workflow_replacements r JOIN jobs j ON j.workspace_id=r.workspace_id AND j.id=r.id WHERE r.workspace_id=$1 AND r.team_id=$2 AND r.id=$3").bind(w).bind(t).bind(id).fetch_optional(&mut **tx).await?;
    body.map(decode).transpose()
}
