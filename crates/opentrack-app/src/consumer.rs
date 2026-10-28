//! The first required event consumer. Capture commits durable intent before a cursor.
use crate::{db_error, worker::Worker};
use opentrack_domain::{Error, Result};
use opentrack_store as store;
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumerTick {
    pub processed: u64,
    pub captured: u64,
    pub backfilled: bool,
    pub dead_lettered: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionOutcome {
    pub deleted: u64,
    pub purged_through_seq: i64,
}
impl Worker {
    pub async fn claim_attachment_gc(
        &self,
        workspace: Uuid,
        limit: i32,
    ) -> Result<Vec<crate::AttachmentGcClaim>> {
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid(
                "Attachment cleanup batch must be between 1 and 100".into(),
            ));
        }
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        bounds(&mut tx).await?;
        let rows = sqlx::query("SELECT * FROM worker_claim_attachment_gc($1,$2)")
            .bind(workspace)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
        let mut claims = Vec::new();
        for row in rows {
            claims.push(crate::AttachmentGcClaim {
                workspace,
                id: row.try_get("id").map_err(db_error)?,
                storage_key: row
                    .try_get::<Uuid, _>("storage_key")
                    .map_err(db_error)?
                    .to_string(),
                generation: row.try_get("generation").map_err(db_error)?,
                lease_token: row.try_get("lease_token").map_err(db_error)?,
                lease_expires_at: row.try_get("lease_expires_at").map_err(db_error)?,
            });
        }
        tx.commit().await.map_err(db_error)?;
        Ok(claims)
    }
    pub async fn claim_attachment_gc_next(
        &self,
        cursor: &mut Option<Uuid>,
        limit: i32,
    ) -> Result<Option<(Uuid, Vec<crate::AttachmentGcClaim>)>> {
        let Some(w) = self.consumer_scope(cursor).await? else {
            return Ok(None);
        };
        Ok(Some((w, self.claim_attachment_gc(w, limit).await?)))
    }
    pub async fn acknowledge_attachment_gc(
        &self,
        claim: &crate::AttachmentGcClaim,
    ) -> Result<bool> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        bounds(&mut tx).await?;
        let removed = sqlx::query_scalar("SELECT worker_ack_attachment_gc($1,$2,$3,$4)")
            .bind(claim.workspace)
            .bind(claim.id)
            .bind(claim.generation)
            .bind(claim.lease_token)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(removed)
    }
    pub async fn prune_document_revisions(&self, workspace: Uuid, limit: i32) -> Result<i64> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::Invalid(
                "Revision retention batch must be between 1 and 1000".into(),
            ));
        }
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        bounds(&mut tx).await?;
        let deleted: i64 = sqlx::query_scalar("SELECT worker_prune_document_revisions($1,$2)")
            .bind(workspace)
            .bind(limit)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(deleted)
    }
    pub async fn prune_document_revisions_next(
        &self,
        cursor: &mut Option<Uuid>,
        limit: i32,
    ) -> Result<Option<(Uuid, i64)>> {
        let Some(w) = self.consumer_scope(cursor).await? else {
            return Ok(None);
        };
        Ok(Some((w, self.prune_document_revisions(w, limit).await?)))
    }

    pub async fn capture(&self, workspace: Uuid) -> Result<ConsumerTick> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        bounds(&mut tx).await?;
        let row = sqlx::query("SELECT * FROM worker_capture_events($1,100)")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let result = consumer_tick(&row)?;
        tx.commit().await.map_err(db_error)?;
        if result.dead_lettered {
            tracing::error!("Required event consumer is blocked; inspect operator-consumer-status");
        }
        Ok(result)
    }
    pub async fn capture_next(
        &self,
        cursor: &mut Option<Uuid>,
    ) -> Result<Option<(Uuid, ConsumerTick)>> {
        let Some(workspace) = self.consumer_scope(cursor).await? else {
            return Ok(None);
        };
        Ok(Some((workspace, self.capture(workspace).await?)))
    }
    pub async fn retain(&self, workspace: Uuid, limit: u32) -> Result<RetentionOutcome> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::Invalid(
                "Retention batch must be between 1 and 1000".into(),
            ));
        }
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        bounds(&mut tx).await?;
        let row = sqlx::query("SELECT * FROM worker_retain_events($1,$2)")
            .bind(workspace)
            .bind(limit as i32)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let result = RetentionOutcome {
            deleted: row
                .try_get::<i64, _>("deleted")
                .map_err(db_error)?
                .try_into()
                .map_err(|_| Error::Internal)?,
            purged_through_seq: row.try_get("purged_through_seq").map_err(db_error)?,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn retain_next(
        &self,
        cursor: &mut Option<Uuid>,
        limit: u32,
    ) -> Result<Option<(Uuid, RetentionOutcome)>> {
        let Some(workspace) = self.consumer_scope(cursor).await? else {
            return Ok(None);
        };
        Ok(Some((workspace, self.retain(workspace, limit).await?)))
    }
    pub(crate) async fn consumer_scope(&self, cursor: &mut Option<Uuid>) -> Result<Option<Uuid>> {
        let workspace: Option<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM worker_ready_workspaces($1,1)")
                .bind(*cursor)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        *cursor = workspace;
        Ok(workspace)
    }
}

pub(crate) async fn bounds(tx: &mut store::Tx) -> Result<()> {
    // Set this before invoking a definer function: changing statement_timeout
    // inside that function does not bound its already-started outer statement.
    sqlx::query("SELECT set_config('statement_timeout','3000',true),set_config('lock_timeout','1000',true),set_config('idle_in_transaction_session_timeout','5000',true)")
        .execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}

pub(crate) fn consumer_tick(row: &sqlx::postgres::PgRow) -> Result<ConsumerTick> {
    Ok(ConsumerTick {
        processed: row
            .try_get::<i64, _>("processed")
            .map_err(db_error)?
            .try_into()
            .map_err(|_| Error::Internal)?,
        captured: row
            .try_get::<i64, _>("captured")
            .map_err(db_error)?
            .try_into()
            .map_err(|_| Error::Internal)?,
        backfilled: row.try_get("backfilled").map_err(db_error)?,
        dead_lettered: row.try_get("dead_lettered").map_err(db_error)?,
    })
}
