//! System maintenance: current eligibility is rechecked under the workspace guard.
use crate::{
    db_error,
    worker::{BatchOutcome, JobClaim},
};
use opentrack_domain::{Error, Result};
use opentrack_store as store;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

pub(crate) async fn run_batch(tx: &mut store::Tx, claim: &JobClaim) -> Result<BatchOutcome> {
    let w = claim.workspace_id;
    let state=sqlx::query("SELECT after_team_id,after_issue_id FROM assignment_reconciliations WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
        .bind(w).bind(claim.id).fetch_one(&mut **tx).await.map_err(db_error)?;
    let after_team: Option<Uuid> = state.try_get("after_team_id").map_err(db_error)?;
    let after_issue: Option<Uuid> = state.try_get("after_issue_id").map_err(db_error)?;
    let active: bool = sqlx::query_scalar("SELECT archived_at IS NULL FROM workspaces WHERE id=$1")
        .bind(w)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    if !active {
        return Ok(BatchOutcome::Complete);
    }
    // One team per transaction: lock it before selecting/locking its issue rows.
    let team:Option<Uuid>=sqlx::query_scalar("SELECT t.id FROM teams t WHERE t.workspace_id=$1 AND t.archived_at IS NULL AND ($2::uuid IS NULL OR t.id>=$2) AND EXISTS(SELECT 1 FROM issues i WHERE i.workspace_id=$1 AND i.team_id=t.id AND i.assignee_id IS NOT NULL AND i.archived_at IS NULL AND i.deleted_at IS NULL AND ($2::uuid IS NULL OR t.id>$2 OR i.id>$3)) ORDER BY t.id LIMIT 1 FOR UPDATE OF t")
        .bind(w).bind(after_team).bind(after_issue).fetch_optional(&mut **tx).await.map_err(db_error)?;
    let Some(team) = team else {
        return Ok(BatchOutcome::Complete);
    };
    // Bound examined candidates too, not only changed rows. Completed/canceled
    // history advances the cursor but is not cleared.
    let rows=sqlx::query("SELECT i.id,i.assignee_id,s.category FROM issues i JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.assignee_id IS NOT NULL AND i.archived_at IS NULL AND i.deleted_at IS NULL AND ($3::uuid IS NULL OR $2<>$3 OR i.id>$4) ORDER BY i.id LIMIT 100 FOR UPDATE OF i")
        .bind(w).bind(team).bind(after_team).bind(after_issue).fetch_all(&mut **tx).await.map_err(db_error)?;
    let mut events = Vec::new();
    for row in &rows {
        let category: String = row.try_get("category").map_err(db_error)?;
        if matches!(category.as_str(), "completed" | "canceled") {
            continue;
        }
        let id: Uuid = row.try_get("id").map_err(db_error)?;
        let before: Uuid = row.try_get("assignee_id").map_err(db_error)?;
        let eligible: bool = sqlx::query_scalar("SELECT worker_assignee_eligible($1,$2,$3)")
            .bind(w)
            .bind(team)
            .bind(before)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
        if eligible {
            continue;
        }
        let version: i64 =
            sqlx::query_scalar("SELECT worker_clear_assignment($1,$2,$3,$4,$5,$6,$7)")
                .bind(w)
                .bind(team)
                .bind(id)
                .bind(before)
                .bind(claim.id)
                .bind(claim.lease_owner)
                .bind(claim.lease_token)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_error)?;
        let changes = json!({"fields":["assignee_id"],"assignee_id":{"before":before,"after":null},"source":"access_cleanup","job_id":claim.id});
        required(sqlx::query("INSERT INTO activity_entries(id,workspace_id,team_id,entity_id,actor_id,actor_kind,system_job_id,event_kind,changes) VALUES($1,$2,$3,$4,NULL,'system',$5,'issue.updated',$6)")
            .bind(Uuid::new_v4()).bind(w).bind(team).bind(id).bind(claim.id).bind(changes).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
        required(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_kind,action,target_id,correlation_id,system_job_id) VALUES($1,$2,'system','issue.assignment_cleared',$3,$4,$4)")
            .bind(Uuid::new_v4()).bind(w).bind(id).bind(claim.id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
        events.push((id, version));
    }
    let last = rows
        .last()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .transpose()
        .map_err(db_error)?
        .unwrap_or_else(|| Uuid::from_u128(u128::MAX));
    // A candidate can disappear while the team lock is awaited. Persist an
    // exhausted-team cursor rather than incorrectly completing other teams.
    required(sqlx::query("UPDATE assignment_reconciliations SET after_team_id=$3,after_issue_id=$4,scanned_count=scanned_count+$5,cleared_count=cleared_count+$6 WHERE workspace_id=$1 AND id=$2")
        .bind(w).bind(claim.id).bind(team).bind(last).bind(rows.len() as i64).bind(events.len() as i64).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    // Reserve contiguous stream positions only after every entity/activity/audit.
    if !events.is_empty() {
        let end:i64=sqlx::query_scalar("UPDATE workspace_stream_heads SET last_seq=last_seq+$2 WHERE workspace_id=$1 RETURNING last_seq")
            .bind(w).bind(events.len() as i64).fetch_one(&mut **tx).await.map_err(db_error)?;
        for (index, (id, version)) in events.iter().enumerate() {
            required(sqlx::query("INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,team_id,entity_id,entity_version,actor_id,actor_kind,mutation_id,system_job_id) VALUES($1,$2,$3,'issue.updated',$4,$5,$6,NULL,'system',$7,$7)")
                .bind(w).bind(end-events.len() as i64+index as i64+1).bind(Uuid::new_v4()).bind(team).bind(id).bind(version).bind(claim.id)
                .execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
        }
    }
    // The next bounded turn determines exhaustion without taking a later team lock.
    Ok(BatchOutcome::More)
}
fn required(count: u64) -> Result<()> {
    if count == 1 {
        Ok(())
    } else {
        Err(Error::Internal)
    }
}
