//! Runtime transactions own their checked-out connection until a confirmed end.
//!
//! SQLx 0.9 increments its PostgreSQL transaction depth after awaiting BEGIN's
//! response. Cancelling that await can leave a server transaction open while
//! its rollback-on-drop sees depth zero. Do not return that connection to the
//! pool: a later request could inherit transaction-local tenant context/locks.
use sqlx::{PgConnection, PgPool, Postgres, SqlSafeStr, pool::PoolConnection};
use std::ops::{Deref, DerefMut};

pub struct Tx {
    connection: PoolConnection<Postgres>,
    finished: bool,
}

pub async fn begin(pool: &PgPool) -> Result<Tx, sqlx::Error> {
    Tx::begin_with(pool.acquire().await?, "BEGIN").await
}

impl Tx {
    /// Use a checked-out connection so cancellation is covered before BEGIN is
    /// first sent. The custom statement also permits controlled protocol tests.
    #[doc(hidden)]
    pub async fn begin_with(
        connection: PoolConnection<Postgres>,
        statement: impl SqlSafeStr,
    ) -> Result<Self, sqlx::Error> {
        let mut transaction = Self {
            connection,
            finished: false,
        };
        sqlx::raw_sql(statement).execute(&mut *transaction).await?;
        Ok(transaction)
    }

    pub async fn commit(mut self) -> Result<(), sqlx::Error> {
        sqlx::raw_sql("COMMIT").execute(&mut *self).await?;
        self.finished = true;
        Ok(())
    }

    pub async fn rollback(mut self) -> Result<(), sqlx::Error> {
        sqlx::raw_sql("ROLLBACK").execute(&mut *self).await?;
        self.finished = true;
        Ok(())
    }
}

impl Deref for Tx {
    type Target = PgConnection;
    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl DerefMut for Tx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

impl Drop for Tx {
    fn drop(&mut self) {
        if !self.finished {
            // SQLx retains the pool permit while closing, bounding concurrent
            // cleanup by pool size. PostgreSQL rolls back on disconnect. This
            // covers cancellation/error during BEGIN, a query, COMMIT or ROLLBACK.
            self.connection.close_on_drop();
        }
    }
}
