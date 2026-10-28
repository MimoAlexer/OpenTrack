use crate::{
    consumer, db_error,
    worker::{BatchOutcome, JobClaim, Worker},
};
use opentrack_domain::Result;
use opentrack_store as store;
use uuid::Uuid;

impl Worker {
    pub async fn apply_planning_events(&self, workspace: Uuid) -> Result<consumer::ConsumerTick> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer::bounds(&mut tx).await?;
        let row = store::apply_planning_events(&mut tx, workspace)
            .await
            .map_err(db_error)?;
        let result = consumer::consumer_tick(&row)?;
        tx.commit().await.map_err(db_error)?;
        if result.dead_lettered {
            tracing::error!(
                "Required planning consumer is blocked; inspect operator-consumer-status"
            );
        }
        Ok(result)
    }

    pub async fn apply_planning_events_next(
        &self,
        cursor: &mut Option<Uuid>,
    ) -> Result<Option<(Uuid, consumer::ConsumerTick)>> {
        let Some(workspace) = self.consumer_scope(cursor).await? else {
            return Ok(None);
        };
        Ok(Some((
            workspace,
            self.apply_planning_events(workspace).await?,
        )))
    }
}

pub(crate) async fn run_batch(tx: &mut store::Tx, claim: &JobClaim) -> Result<BatchOutcome> {
    let complete = store::planning_backfill_batch(
        tx,
        claim.workspace_id,
        claim.id,
        claim.lease_owner,
        claim.lease_token,
    )
    .await
    .map_err(db_error)?;
    Ok(if complete {
        BatchOutcome::Complete
    } else {
        BatchOutcome::More
    })
}

pub(crate) fn progress(row: &sqlx::postgres::PgRow) -> Result<opentrack_domain::ProjectProgress> {
    use opentrack_domain::PlanningCalculationSource;
    use sqlx::Row;
    let mut progress = crate::projects::progress_counts(row)?;
    if progress.total > 0 && row.try_get::<bool, _>("facts_ready").map_err(db_error)? {
        progress.calculation_source = PlanningCalculationSource::Projection;
        progress.facts_as_of = row.try_get("facts_as_of").map_err(db_error)?;
    }
    Ok(progress)
}
