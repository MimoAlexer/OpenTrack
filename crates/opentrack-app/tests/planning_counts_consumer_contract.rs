#[allow(dead_code)]
mod support;
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn setup(h: &Harness) -> (Actor, Uuid, Uuid) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@example.test","password":"Planning contract password 947!","display_name":"Example Owner","workspace_name":"Planning contracts","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let t = h.app.bootstrap(&actor, w).await.unwrap().teams[0].id;
    (actor, w, t)
}
async fn fixture(h: &Harness, a: &Actor, w: Uuid) -> store::Tx {
    h.context(a.user.id, w).await
}
async fn state(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = fixture(h, a, w).await;
    let value=sqlx::query_scalar("SELECT jsonb_build_object('checkpoint',(SELECT to_jsonb(c) FROM consumer_checkpoints c WHERE workspace_id=$1 AND consumer_name='planning_counts' AND consumer_version=1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'facts',(SELECT coalesce(jsonb_agg(to_jsonb(f) ORDER BY issue_id),'[]') FROM planning_issue_facts f WHERE workspace_id=$1),'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY seq),'[]') FROM consumer_receipts r WHERE workspace_id=$1 AND consumer_name='planning_counts'),'letters',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY seq),'[]') FROM consumer_dead_letters d WHERE workspace_id=$1 AND consumer_name='planning_counts'))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}
async fn runner(h: &Harness) -> Worker {
    Worker::connect(
        &h.worker_url().await,
        WorkerOptions {
            max_connections: 1,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap()
}
async fn drain(worker: &Worker, w: Uuid) {
    for _ in 0..50 {
        let tick = worker.apply_planning_events(w).await.unwrap();
        if let Some(claim) = worker.claim(w).await.unwrap() {
            assert!(matches!(
                worker.run_claim(&claim).await.unwrap(),
                WorkerTick::Completed | WorkerTick::Progress
            ));
        } else if tick.processed == 0 && !tick.backfilled {
            return;
        }
    }
    panic!("Planning fixture did not drain within its bound");
}
#[tokio::test]
async fn planning_registration_backfill_and_atomic_current_issue_replay() {
    let h = Harness::new().await;
    let r = h.clone();
    let result = tokio::spawn(async move { initial(&r).await }).await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}
async fn initial(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let issue = h
        .app
        .create_issue(
            &a,
            w,
            "planning-source",
            req(json!({"team_id":t,"title":"Planning source","estimate":5})),
        )
        .await
        .unwrap();
    let worker = runner(h).await;
    let mut denied = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut denied, a.user.id, Some(w))
        .await
        .unwrap();
    let error=sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'planning.backfill',NULL,'forbidden-runtime-planning')").bind(w).bind(Uuid::new_v4()).execute(&mut *denied).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    denied.rollback().await.unwrap();
    assert!(!sqlx::query_scalar::<_,bool>("SELECT has_any_column_privilege(current_user,'planning_issue_facts','INSERT,UPDATE,REFERENCES') OR has_table_privilege(current_user,'planning_issue_facts','DELETE,TRUNCATE,TRIGGER') OR has_function_privilege(current_user,'worker_apply_planning_events(uuid,integer)','EXECUTE')").fetch_one(&h.app.pool).await.unwrap());
    assert!(!sqlx::query_scalar::<_,bool>("SELECT has_any_column_privilege(current_user,'planning_issue_facts','SELECT,INSERT,UPDATE,REFERENCES') OR has_function_privilege(current_user,'operator_planning_rebuild(uuid,text)','EXECUTE')").fetch_one(&worker.pool).await.unwrap());
    assert_eq!(state(h, &a, w).await["checkpoint"]["last_seq"], 0);
    assert!(worker.apply_planning_events(w).await.unwrap().backfilled);
    assert_eq!(state(h, &a, w).await["checkpoint"]["last_seq"], 0);
    drain(&worker, w).await;
    let first = state(h, &a, w).await;
    assert_eq!(first["checkpoint"]["last_seq"], first["head"]);
    assert_eq!(first["facts"].as_array().unwrap().len(), 1);
    assert_eq!(first["facts"][0]["issue_id"], json!(issue.id));
    assert_eq!(first["facts"][0]["estimate"], 5);
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("UPDATE issues SET title='Nonmetric change',version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(issue.id).execute(&mut *tx).await.unwrap();
    store::identity_event(
        &mut tx,
        store::IdentityEvent {
            workspace: w,
            actor: a.user.id,
            entity: issue.id,
            version: 2,
            team: Some(t),
            action: "issue.updated",
            access_changed: false,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    worker.apply_planning_events(w).await.unwrap();
    let applied = state(h, &a, w).await;
    assert_eq!(applied["facts"][0]["issue_version"], 2);
    assert_eq!(applied["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(applied["receipts"][0]["state"], "applied");
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("UPDATE consumer_checkpoints SET last_seq=last_seq-1 WHERE workspace_id=$1 AND consumer_name='planning_counts'").bind(w).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    worker.apply_planning_events(w).await.unwrap();
    let replay = state(h, &a, w).await;
    assert_eq!(replay["facts"], applied["facts"]);
    assert_eq!(replay["receipts"], applied["receipts"]);
    assert_eq!(
        replay["checkpoint"]["last_seq"],
        applied["checkpoint"]["last_seq"]
    );
    worker.pool.close().await;
}
use opentrack_app::Operator;
use sqlx::AssertSqlSafe;

async fn emit(h: &Harness, a: &Actor, w: Uuid, t: Uuid, id: Uuid, version: i64, action: &str) {
    let mut tx = fixture(h, a, w).await;
    store::identity_event(
        &mut tx,
        store::IdentityEvent {
            workspace: w,
            actor: a.user.id,
            entity: id,
            version,
            team: Some(t),
            action,
            access_changed: false,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn multi_batch_backfill_holds_retention_and_reconciles_cursor_races_and_stale_leases() {
    let h = Harness::new().await;
    let r = h.clone();
    let result = tokio::spawn(async move { backfill(&r).await }).await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}
async fn backfill(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let worker = runner(h).await;
    let s = h
        .app
        .workflow(&a, w, t)
        .await
        .unwrap()
        .statuses
        .into_iter()
        .find(|s| s.category == "unstarted")
        .unwrap()
        .id;
    let ids = (0..205)
        .map(|n| Uuid::from_u128(0x1000 + n))
        .collect::<Vec<_>>();
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank) SELECT id,$1,$2,$3,n,'Backfill source',$4,n::numeric*1024 FROM unnest($5::uuid[]) WITH ORDINALITY AS source(id,n)").bind(w).bind(t).bind(a.user.id).bind(s).bind(&ids).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    worker.capture(w).await.unwrap();
    let assignment = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(assignment.kind, "assignment.reconcile");
    assert_eq!(
        worker.run_claim(&assignment).await.unwrap(),
        WorkerTick::Completed
    );
    let target = state(h, &a, w).await["head"].as_i64().unwrap();
    worker.apply_planning_events(w).await.unwrap();
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("UPDATE workspace_events SET occurred_at=clock_timestamp()-interval '9 days' WHERE workspace_id=$1").bind(w).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 0);
    let claim = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(claim.kind, "planning.backfill");
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::Progress
    );
    let partial = state(h, &a, w).await;
    assert_eq!(partial["checkpoint"]["last_seq"], 0);
    assert_eq!(partial["facts"].as_array().unwrap().len(), 100);
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 0);
    let other = h
        .app
        .create_team(
            &a,
            w,
            "backfill-move-team",
            req(json!({"name":"Destination","key":"DEST","private":false})),
        )
        .await
        .unwrap()
        .id;
    let other_status = h
        .app
        .workflow(&a, w, other)
        .await
        .unwrap()
        .statuses
        .into_iter()
        .find(|s| s.category == "unstarted")
        .unwrap()
        .id;
    let behind = Uuid::from_u128(1);
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank) VALUES($1,$2,$3,$4,1000,'Behind cursor',$5,1024000)").bind(behind).bind(w).bind(t).bind(a.user.id).bind(s).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE issues SET estimate=9,version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(ids[0])
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE issues SET archived_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(ids[150]).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE issues SET deleted_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(ids[160]).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE issues SET team_id=$3,status_id=$4,number=1,version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(ids[50]).bind(other).bind(other_status).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for (id, team, version, action) in [
        (behind, t, 1, "issue.created"),
        (ids[0], t, 2, "issue.updated"),
        (ids[150], t, 2, "issue.updated"),
        (ids[160], t, 2, "issue.deleted"),
        (ids[50], other, 2, "issue.moved"),
    ] {
        emit(h, &a, w, team, id, version, action).await;
    }
    let stale = worker.claim(w).await.unwrap().unwrap();
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("UPDATE jobs SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE workspace_id=$1 AND id=$2").bind(w).bind(stale.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let fresh = worker.claim(w).await.unwrap().unwrap();
    assert!(fresh.lease_token > stale.lease_token);
    assert_eq!(
        worker.run_claim(&stale).await.unwrap(),
        WorkerTick::LostLease
    );
    assert_eq!(
        worker.run_claim(&fresh).await.unwrap(),
        WorkerTick::Progress
    );
    assert_eq!(state(h, &a, w).await["checkpoint"]["last_seq"], 0);
    let last = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(
        worker.run_claim(&last).await.unwrap(),
        WorkerTick::Completed
    );
    let before_tail = state(h, &a, w).await;
    assert_eq!(before_tail["checkpoint"]["last_seq"], target);
    assert_eq!(before_tail["facts"].as_array().unwrap().len(), 205);
    drain(&worker, w).await;
    let after = state(h, &a, w).await;
    assert_eq!(after["checkpoint"]["last_seq"], after["head"]);
    assert_eq!(after["facts"].as_array().unwrap().len(), 206);
    let mut tx = fixture(h, &a, w).await;
    let mismatch:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM issues i JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id LEFT JOIN planning_issue_facts f ON f.workspace_id=i.workspace_id AND f.consumer_version=1 AND f.issue_id=i.id WHERE i.workspace_id=$1 AND (f.issue_version IS DISTINCT FROM i.version OR f.category IS DISTINCT FROM s.category OR f.estimate IS DISTINCT FROM i.estimate OR f.excluded IS DISTINCT FROM (i.archived_at IS NOT NULL OR i.deleted_at IS NOT NULL)))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(!mismatch);
    worker.capture(w).await.unwrap();
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("UPDATE workspace_events SET occurred_at=clock_timestamp()-interval '9 days' WHERE workspace_id=$1").bind(w).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(worker.retain(w, 1000).await.unwrap().deleted > 0);
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let saved = after["facts"].clone();
    operator
        .rebuild_planning_counts(w, "Verify idempotent current-source repair")
        .await
        .unwrap();
    assert_eq!(
        state(h, &a, w).await["checkpoint"]["last_seq"],
        after["checkpoint"]["last_seq"]
    );
    drain(&worker, w).await;
    assert_eq!(
        state(h, &a, w).await["facts"],
        saved,
        "Unchanged backfill facts retain calculation times"
    );
    worker.pool.close().await;
}
#[tokio::test]
async fn suppressed_fact_receipt_checkpoint_and_poison_schema_preserve_the_gap() {
    let h = Harness::new().await;
    let r = h.clone();
    let result = tokio::spawn(async move { failures(&r).await }).await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}
async fn failures(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let issue = h
        .app
        .create_issue(
            &a,
            w,
            "failure-source",
            req(json!({"team_id":t,"title":"Atomic source"})),
        )
        .await
        .unwrap();
    let worker = runner(h).await;
    drain(&worker, w).await;
    for (index, (table, operation)) in [
        ("planning_issue_facts", "UPDATE"),
        ("consumer_receipts", "INSERT"),
        ("consumer_checkpoints", "UPDATE"),
    ]
    .into_iter()
    .enumerate()
    {
        let before = state(h, &a, w).await;
        let version = 2 + index as i64;
        let mut tx = fixture(h, &a, w).await;
        sqlx::query(
            "UPDATE issues SET version=version+1,estimate=$3 WHERE workspace_id=$1 AND id=$2",
        )
        .bind(w)
        .bind(issue.id)
        .bind(version as i32)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        emit(h, &a, w, t, issue.id, version, "issue.updated").await;
        let filter = if table == "planning_issue_facts" {
            "true"
        } else {
            "NEW.consumer_name='planning_counts'"
        };
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION test_planning_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {filter} THEN RETURN NULL;END IF;RETURN NEW;END $$;CREATE TRIGGER test_planning_suppress BEFORE {operation} ON {table} FOR EACH ROW EXECUTE FUNCTION test_planning_suppress();"))).execute(&h.owner).await.unwrap();
        let tick = worker.apply_planning_events(w).await.unwrap();
        assert_eq!((tick.processed, tick.captured), (0, 0));
        let failed = state(h, &a, w).await;
        assert_eq!(failed["facts"], before["facts"]);
        assert_eq!(failed["receipts"], before["receipts"]);
        assert_eq!(
            failed["checkpoint"]["last_seq"],
            before["checkpoint"]["last_seq"]
        );
        assert_eq!(
            failed["letters"].as_array().unwrap().last().unwrap()["error_code"],
            "effect_failed"
        );
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP TRIGGER test_planning_suppress ON {table};DROP FUNCTION test_planning_suppress();"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
        let mut tx = fixture(h, &a, w).await;
        sqlx::query("UPDATE consumer_dead_letters SET available_at=clock_timestamp() WHERE workspace_id=$1 AND consumer_name='planning_counts' AND state='retry_wait'").bind(w).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(worker.apply_planning_events(w).await.unwrap().captured, 1);
        assert_eq!(state(h, &a, w).await["facts"][0]["issue_version"], version);
    }
    emit(h, &a, w, t, issue.id, 4, "issue.updated").await;
    let mut tx = fixture(h, &a, w).await;
    sqlx::query("UPDATE workspace_events SET schema_version=2 WHERE workspace_id=$1 AND seq=(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1)").bind(w).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let before = state(h, &a, w).await;
    for attempt in 1..=5 {
        let mut tx = fixture(h, &a, w).await;
        sqlx::query("UPDATE consumer_dead_letters SET available_at=clock_timestamp() WHERE workspace_id=$1 AND consumer_name='planning_counts' AND state='retry_wait'").bind(w).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let tick = worker.apply_planning_events(w).await.unwrap();
        assert_eq!(tick.processed, 0);
        assert_eq!(tick.dead_lettered, attempt == 5);
    }
    let blocked = state(h, &a, w).await;
    assert_eq!(blocked["facts"], before["facts"]);
    assert_eq!(
        blocked["checkpoint"]["last_seq"],
        before["checkpoint"]["last_seq"]
    );
    assert_eq!(
        blocked["letters"].as_array().unwrap().last().unwrap()["error_code"],
        "unsupported_event_schema"
    );
    worker.pool.close().await;
    let restarted = runner(h).await;
    assert_eq!(
        restarted.apply_planning_events(w).await.unwrap().processed,
        0
    );
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    operator
        .rebuild_planning_counts(w, "Reconcile current facts after unsupported event")
        .await
        .unwrap();
    drain(&restarted, w).await;
    let repaired = state(h, &a, w).await;
    assert_eq!(repaired["checkpoint"]["last_seq"], repaired["head"]);
    assert!(
        repaired["letters"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["state"] == "resolved")
    );
    restarted.pool.close().await;
}

#[tokio::test]
async fn suppressed_baseline_checkpoint_does_not_leave_orphaned_backfill_intent() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move{
  let(a,w,_)=setup(&r).await;let worker=runner(&r).await;
  sqlx::raw_sql("CREATE FUNCTION test_planning_baseline_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.consumer_name='planning_counts' THEN RETURN NULL;END IF;RETURN NEW;END $$;CREATE TRIGGER test_planning_baseline_suppress BEFORE UPDATE ON consumer_checkpoints FOR EACH ROW EXECUTE FUNCTION test_planning_baseline_suppress();").execute(&r.owner).await.unwrap();
  let tick=worker.apply_planning_events(w).await.unwrap();assert!(!tick.backfilled);assert_eq!(tick.processed,0);
  let failed=state(&r,&a,w).await;assert_eq!(failed["checkpoint"]["last_seq"],0);assert!(failed["checkpoint"]["backfill_job_id"].is_null());assert_eq!(failed["letters"][0]["seq"],0);
  let mut tx=fixture(&r,&a,w).await;let jobs:i64=sqlx::query_scalar("SELECT count(*) FROM jobs WHERE workspace_id=$1 AND kind='planning.backfill'").bind(w).fetch_one(&mut *tx).await.unwrap();assert_eq!(jobs,0);
  sqlx::query("UPDATE consumer_dead_letters SET available_at=clock_timestamp() WHERE workspace_id=$1 AND consumer_name='planning_counts'").bind(w).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
  sqlx::raw_sql("DROP TRIGGER test_planning_baseline_suppress ON consumer_checkpoints;DROP FUNCTION test_planning_baseline_suppress();").execute(&r.owner).await.unwrap();
  drain(&worker,w).await;let after=state(&r,&a,w).await;assert!(!after["checkpoint"]["backfill_job_id"].is_null());assert_eq!(after["letters"][0]["state"],"resolved");worker.pool.close().await;
 }).await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}
