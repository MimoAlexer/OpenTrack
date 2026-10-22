//! Cancelling BEGIN must not return an open server transaction to the pool.
#[allow(dead_code)]
mod support;

use opentrack_store as store;
use sqlx::{AssertSqlSafe, SqlSafeStr};
use std::{task::Poll, time::Duration};
use support::Harness;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_begin_does_not_leak_transaction_or_tenant_context() {
    let h = Harness::with_connections(1).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let tenant = Uuid::new_v4();
    let lock = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    let mut blocker = h.owner.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let mut conn = h.app.pool.acquire().await.unwrap();
    let runtime_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    // A blocked custom BEGIN deterministically holds the same driver await that
    // normal BEGIN can be cancelled in during network delivery. Only generated
    // UUID/numeric constants are interpolated; no application input enters SQL.
    let statement = AssertSqlSafe(format!(
        "BEGIN; SELECT set_config('opentrack.workspace_id','{tenant}',true); SELECT pg_advisory_xact_lock({lock})"
    ))
    .into_sql_str();
    let mut begin = Box::pin(store::Tx::begin_with(conn, statement));
    std::future::poll_fn(|cx| match begin.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("BEGIN cannot complete while the fixture holds its lock"),
    })
    .await;
    tokio::time::timeout(Duration::from_millis(800), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $1=ANY(pg_blocking_pids($2))")
                .bind(blocker_pid)
                .bind(runtime_pid)
                .fetch_one(&h.owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("PostgreSQL has begun the transaction and reached the controlled lock wait");

    drop(begin);
    blocker.rollback().await.unwrap();
    let mut recycled = h.app.pool.acquire().await.unwrap();
    let recycled_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *recycled)
        .await
        .unwrap();
    assert_ne!(
        runtime_pid, recycled_pid,
        "A cancelled transaction's connection is closed before replacement"
    );
    let context: String =
        sqlx::query_scalar("SELECT COALESCE(current_setting('opentrack.workspace_id',true),'')")
            .fetch_one(&mut *recycled)
            .await
            .unwrap();
    assert_eq!(
        context, "",
        "Cancelled BEGIN leaked transaction-local tenant context"
    );
    drop(recycled);
    // A fresh transaction still commits and the one-connection pool is usable.
    let mut healthy = store::begin(&h.app.pool).await.unwrap();
    let value: i32 = sqlx::query_scalar("SELECT 42")
        .fetch_one(&mut *healthy)
        .await
        .unwrap();
    assert_eq!(value, 42);
    healthy.commit().await.unwrap();
    let healthy_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(
        healthy_pid, recycled_pid,
        "Successful transactions keep reusing the pool connection"
    );

    let user = Uuid::new_v4();
    let mut abandoned = store::begin(&h.app.pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) VALUES($1,$2,'Aborted fixture','not-a-login-hash')")
        .bind(user).bind(format!("{user}@cancel.invalid")).execute(&mut *abandoned).await.unwrap();
    drop(abandoned);
    let persisted: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE id=$1")
        .bind(user)
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(
        persisted, 0,
        "Dropping an unfinished write rolls it back through disconnect"
    );
}
