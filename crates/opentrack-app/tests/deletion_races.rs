//! Observed PostgreSQL waits establish graph/lifecycle order and expiry across locks.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use serde_json::json;
use std::time::Duration;
use support::Harness;
use uuid::Uuid;
fn repair(i: &Issue) -> RestoreIssue {
    RestoreIssue {
        status_id: i.status_id,
        assignee_id: i.assignee_id,
        label_ids: i.label_ids.clone(),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_deletion_lock_order_and_deadline() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (a, w, t) = setup(h).await;
    for delete_first in [true, false] {
        parent_race(h, &a, w, t, delete_first).await;
        lifecycle_race(h, &a, w, t, delete_first).await;
    }
    double_restore(h, &a, w, t).await;
    expiry_after_wait(h, &a, w, t).await;
}
async fn parent_race(h: &Harness, a: &Actor, w: Uuid, t: Uuid, delete_first: bool) {
    let target = issue(h, a, w, t, json!({})).await;
    let child = issue(h, a, w, t, json!({})).await;
    let (holder, pid) = gate(h, a, w, t, Some(target.id)).await;
    let delete = {
        let app = h.app.clone();
        let actor = a.clone();
        let target = target.clone();
        async move { app.delete_issue(&actor, w, target.id, &target.etag()).await }
    };
    let attach = {
        let app = h.app.clone();
        let actor = a.clone();
        let target = target.clone();
        let child = child.clone();
        async move {
            app.set_issue_parent(
                &actor,
                w,
                child.id,
                &child.etag(),
                req(json!({"parent_id":target.id,"parent_etag":target.etag()})),
            )
            .await
            .map(|_| ())
        }
    };
    let (first, second) = if delete_first {
        let first = tokio::spawn(delete);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(attach);
        blocked(h, first_pid).await;
        (first, second)
    } else {
        let first = tokio::spawn(attach);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(delete);
        blocked(h, first_pid).await;
        (first, second)
    };
    holder.commit().await.unwrap();
    first.await.unwrap().unwrap();
    assert!(matches!(
        second.await.unwrap(),
        Err(Error::NotFound | Error::Stale(_))
    ));
    let parent = h.app.issue_parent(a, w, child.id).await.unwrap().parent;
    if delete_first {
        assert!(parent.is_none());
        assert!(h.app.get_deleted_issue(a, w, target.id).await.is_ok());
    } else {
        assert_eq!(parent.unwrap().target.unwrap().id, target.id);
        assert!(h.app.get_issue(a, w, target.id).await.is_ok());
    }
}
async fn lifecycle_race(h: &Harness, a: &Actor, w: Uuid, t: Uuid, delete_first: bool) {
    let mut item = issue(h, a, w, t, json!({})).await;
    if !delete_first {
        h.app
            .delete_issue(a, w, item.id, &item.etag())
            .await
            .unwrap();
        item = h.app.get_deleted_issue(a, w, item.id).await.unwrap().issue;
    }
    let (holder, pid) = gate(h, a, w, t, Some(item.id)).await;
    let delete = {
        let app = h.app.clone();
        let a = a.clone();
        let i = item.clone();
        async move { app.delete_issue(&a, w, i.id, &i.etag()).await }
    };
    let restore = {
        let app = h.app.clone();
        let a = a.clone();
        let i = item.clone();
        async move {
            app.restore_issue(&a, w, i.id, &i.etag(), repair(&i))
                .await
                .map(|_| ())
        }
    };
    let (first, second) = if delete_first {
        let first = tokio::spawn(delete);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(restore);
        blocked(h, first_pid).await;
        (first, second)
    } else {
        let first = tokio::spawn(restore);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(delete);
        blocked(h, first_pid).await;
        (first, second)
    };
    holder.commit().await.unwrap();
    first.await.unwrap().unwrap();
    assert!(matches!(second.await.unwrap(), Err(Error::Stale(_))));
    if delete_first {
        assert!(h.app.get_deleted_issue(a, w, item.id).await.is_ok());
    } else {
        assert!(h.app.get_issue(a, w, item.id).await.is_ok());
    }
}
async fn double_restore(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let issue = issue(h, a, w, t, json!({})).await;
    h.app
        .delete_issue(a, w, issue.id, &issue.etag())
        .await
        .unwrap();
    let issue = h.app.get_deleted_issue(a, w, issue.id).await.unwrap().issue;
    let (holder, pid) = gate(h, a, w, t, Some(issue.id)).await;
    let run = || {
        let app = h.app.clone();
        let a = a.clone();
        let i = issue.clone();
        async move { app.restore_issue(&a, w, i.id, &i.etag(), repair(&i)).await }
    };
    let first = tokio::spawn(run());
    let first_pid = blocked(h, pid).await;
    let second = tokio::spawn(run());
    blocked(h, first_pid).await;
    holder.commit().await.unwrap();
    let restored = first.await.unwrap().unwrap();
    assert!(matches!(second.await.unwrap(), Err(Error::NotFound)));
    advanced(&issue, &restored);
    let mut tx = h.context(a.user.id, w).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_entries WHERE action='issue.restored' AND target_id=$1"
        )
        .bind(issue.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap(),
        1
    );
    tx.commit().await.unwrap();
}
async fn expiry_after_wait(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let item = issue(h, a, w, t, json!({})).await;
    h.app
        .delete_issue(a, w, item.id, &item.etag())
        .await
        .unwrap();
    let old = h.app.get_deleted_issue(a, w, item.id).await.unwrap();
    assert!(old.can_restore);
    let mut holder = h.context(a.user.id, w).await;
    // The owner fixture sets a precise future boundary without changing the
    // entity version. The waiter initially sees the prior unexpired committed row.
    let deadline:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("UPDATE issues SET deleted_at=clock_timestamp()-interval '30 days'+interval '250 milliseconds' WHERE id=$1 RETURNING deleted_at+interval '30 days'").bind(item.id).fetch_one(&mut *holder).await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let app = h.app.clone();
    let actor = a.clone();
    let restore = tokio::spawn(async move {
        app.restore_issue(
            &actor,
            w,
            old.issue.id,
            &old.issue.etag(),
            repair(&old.issue),
        )
        .await
    });
    blocked(h, pid).await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while !sqlx::query_scalar::<_, bool>("SELECT clock_timestamp()>=$1")
            .bind(deadline)
            .fetch_one(&mut *holder)
            .await
            .unwrap()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    holder.commit().await.unwrap();
    assert!(
        matches!(restore.await.unwrap(),Err(Error::Conflict(message)) if message.contains("30-day"))
    );
    let after = h.app.get_deleted_issue(a, w, item.id).await.unwrap();
    assert!(!after.can_restore);
    assert_eq!(
        after.issue.version,
        item.version
            .parse::<u64>()
            .unwrap()
            .checked_add(1)
            .unwrap()
            .to_string()
    );
    let mut tx = h.context(a.user.id, w).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_entries WHERE action='issue.restored' AND target_id=$1"
        )
        .bind(item.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap(),
        0
    );
    tx.commit().await.unwrap();
}
