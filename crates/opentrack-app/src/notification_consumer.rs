//! Durable inbox effects and a separately admitted recipient invalidation outbox.
use crate::{consumer, db_error, worker::Worker};
use opentrack_domain::Result;
use opentrack_store as store;
use uuid::Uuid;

impl Worker {
    pub async fn apply_notification_events(
        &self,
        workspace: Uuid,
    ) -> Result<consumer::ConsumerTick> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer::bounds(&mut tx).await?;
        let row = sqlx::query("SELECT * FROM worker_apply_notification_events($1,100)")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let result = consumer::consumer_tick(&row)?;
        tx.commit().await.map_err(db_error)?;
        if result.dead_lettered {
            tracing::error!(
                "Required notification consumer is blocked; inspect operator-consumer-status"
            );
        }
        Ok(result)
    }

    pub async fn apply_notification_events_next(
        &self,
        cursor: &mut Option<Uuid>,
    ) -> Result<Option<(Uuid, consumer::ConsumerTick)>> {
        let Some(workspace) = self.consumer_scope(cursor).await? else {
            return Ok(None);
        };
        Ok(Some((
            workspace,
            self.apply_notification_events(workspace).await?,
        )))
    }

    pub async fn flush_notification_events(&self, workspace: Uuid) -> Result<i64> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        consumer::bounds(&mut tx).await?;
        let published = sqlx::query_scalar("SELECT worker_flush_notification_events($1,100)")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(published)
    }

    pub async fn flush_notification_events_next(
        &self,
        cursor: &mut Option<Uuid>,
    ) -> Result<Option<(Uuid, i64)>> {
        let Some(workspace) = self.consumer_scope(cursor).await? else {
            return Ok(None);
        };
        Ok(Some((
            workspace,
            self.flush_notification_events(workspace).await?,
        )))
    }
}
