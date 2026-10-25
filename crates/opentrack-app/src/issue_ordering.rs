use crate::{
    db_error,
    worker::{BatchOutcome, JobClaim},
};
use opentrack_domain::{Error, Result};
use opentrack_store as store;

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
