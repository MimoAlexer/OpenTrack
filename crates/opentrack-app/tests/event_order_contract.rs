#[allow(dead_code)]
mod support;

use opentrack_domain::Actor;
use opentrack_store as store;
use serde_json::json;
use std::time::Duration;
use support::Harness;
use tokio::sync::oneshot;
use uuid::Uuid;

async fn setup(h: &Harness) -> (Actor, Uuid, Uuid) {
    let (session, cookie) = h
        .app
        .setup(
            serde_json::from_value(json!({
                "setup_token":h.setup_token,"email":"owner@example.test",
                "password":"Event contract password 724!","display_name":"Example Owner",
                "workspace_name":"Event contracts","team_name":"Engineering","team_key":"ENG"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let workspace = session.workspaces[0].id;
    let team = h.app.bootstrap(&actor, workspace).await.unwrap().teams[0].id;
    (actor, workspace, team)
}

async fn append(
    tx: &mut store::Tx,
    actor: Uuid,
    workspace: Uuid,
    team: Uuid,
    mutation: Uuid,
    count: usize,
) {
    let events: Vec<_> = (0..count)
        .map(|_| store::Event {
            workspace,
            team,
            entity: Uuid::new_v4(),
            version: 1,
            actor,
            kind: "issue.updated",
            mutation,
            changes: json!({}),
        })
        .collect();
    store::append_stream_events(tx, &events).await.unwrap();
}

async fn snapshot(
    h: &Harness,
    actor: Uuid,
    workspace: Uuid,
    after: i64,
) -> (i64, Vec<(i64, Uuid)>) {
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, actor, Some(workspace))
        .await
        .unwrap();
    let head =
        sqlx::query_scalar("SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let events = sqlx::query_as(
        "SELECT seq,mutation_id FROM workspace_events WHERE workspace_id=$1 AND seq>$2 ORDER BY seq",
    )
    .bind(workspace)
    .bind(after)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (head, events)
}

async fn pid(tx: &mut store::Tx) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

async fn blocked(h: &Harness, waiter: i32, holder: i32) {
    tokio::time::timeout(Duration::from_millis(800), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(waiter)
                .bind(holder)
                .fetch_one(&h.app.pool)
                .await
                .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("The competing append must reach the stream-head lock");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn competing_appends_cannot_commit_out_of_sequence() {
    let h = Harness::with_connections(4).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { commit_order(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn commit_order(h: &Harness) {
    let (actor, workspace, team) = setup(h).await;
    let user = actor.user.id;
    let (baseline, _) = snapshot(h, user, workspace, 0).await;
    let mut earlier = h.context(user, workspace).await;
    let earlier_pid = pid(&mut earlier).await;
    let mut later = h.context(user, workspace).await;
    let later_pid = pid(&mut later).await;
    let first_mutation = Uuid::new_v4();
    store::identity_event_correlated(
        &mut later,
        store::IdentityEvent {
            workspace,
            actor: user,
            entity: workspace,
            version: 1,
            team: None,
            action: "workspace.updated",
            access_changed: false,
        },
        first_mutation,
    )
    .await
    .unwrap();
    let second_mutation = Uuid::new_v4();
    let (appended, ready) = oneshot::channel();
    let (commit, permitted) = oneshot::channel();
    let writer = tokio::spawn(async move {
        append(&mut earlier, user, workspace, team, second_mutation, 1).await;
        appended.send(()).unwrap();
        permitted.await.unwrap();
        earlier.commit().await.unwrap();
    });
    blocked(h, earlier_pid, later_pid).await;
    assert_eq!(
        snapshot(h, user, workspace, baseline).await,
        (baseline, vec![]),
        "An uncommitted lower sequence must hide every later append"
    );
    assert!(!writer.is_finished());
    later.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot(h, user, workspace, baseline).await,
        (baseline + 1, vec![(baseline + 1, first_mutation)])
    );
    commit.send(()).unwrap();
    writer.await.unwrap();
    assert_eq!(
        snapshot(h, user, workspace, baseline + 1).await,
        (baseline + 2, vec![(baseline + 2, second_mutation)]),
        "Advancing the replay cursor after the first commit must retain the next event"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rollback_reuses_sequences_and_batches_remain_contiguous() {
    let h = Harness::with_connections(4).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { rollback_order(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn rollback_order(h: &Harness) {
    let (actor, workspace, team) = setup(h).await;
    let user = actor.user.id;
    let (baseline, _) = snapshot(h, user, workspace, 0).await;
    let mut rolled_back = h.context(user, workspace).await;
    let holder = pid(&mut rolled_back).await;
    append(&mut rolled_back, user, workspace, team, Uuid::new_v4(), 3).await;
    let mut next = h.context(user, workspace).await;
    let waiter = pid(&mut next).await;
    let first_mutation = Uuid::new_v4();
    let writer = tokio::spawn(async move {
        append(&mut next, user, workspace, team, first_mutation, 2).await;
        next.commit().await.unwrap();
    });
    blocked(h, waiter, holder).await;
    assert_eq!(
        snapshot(h, user, workspace, baseline).await,
        (baseline, vec![])
    );
    rolled_back.rollback().await.unwrap();
    writer.await.unwrap();
    assert_eq!(
        snapshot(h, user, workspace, baseline).await,
        (
            baseline + 2,
            vec![
                (baseline + 1, first_mutation),
                (baseline + 2, first_mutation)
            ]
        ),
        "A rolled-back reservation must leave no gap before the waiting batch"
    );
    let second_mutation = Uuid::new_v4();
    let mut last = h.context(user, workspace).await;
    append(&mut last, user, workspace, team, second_mutation, 3).await;
    last.commit().await.unwrap();
    assert_eq!(
        snapshot(h, user, workspace, baseline + 2).await,
        (
            baseline + 5,
            vec![
                (baseline + 3, second_mutation),
                (baseline + 4, second_mutation),
                (baseline + 5, second_mutation)
            ]
        )
    );
}
