//! Real PostgreSQL capture, checkpoint, retention and restricted-credential contracts.
#[allow(dead_code)]
mod support;

use opentrack_app::{
    App, Operator,
    worker::{Worker, WorkerOptions, WorkerTick},
};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
async fn setup(h: &Harness) -> (Actor, Uuid) {
    let (session, cookie) = h
        .app
        .setup(req(json!({
            "setup_token":h.setup_token,"email":"consumer@contract.invalid",
            "password":"Consumer contract password 724!","display_name":"Consumer owner",
            "workspace_name":"Consumer contracts","team_name":"Engineering","team_key":"ENG"
        })))
        .await
        .unwrap();
    (
        h.app.authenticate(&cookie).await.unwrap(),
        session.workspaces[0].id,
    )
}
async fn fixture(h: &Harness, workspace: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true)")
        .bind(workspace.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}
async fn worker(h: &Harness) -> Worker {
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
async fn snapshot(h: &Harness, workspace: Uuid) -> Value {
    let mut tx = fixture(h, workspace).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object(
      'registration',(SELECT to_jsonb(r) FROM consumer_registrations r WHERE workspace_id=$1 AND consumer_name='assignment_cleanup' AND consumer_version=1),
      'checkpoint',(SELECT to_jsonb(c) FROM consumer_checkpoints c WHERE workspace_id=$1 AND consumer_name='assignment_cleanup' AND consumer_version=1),
      'jobs',(SELECT coalesce(jsonb_agg(to_jsonb(j) ORDER BY j.id),'[]') FROM jobs j WHERE workspace_id=$1),
      'progress',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY p.id),'[]') FROM assignment_reconciliations p WHERE workspace_id=$1),
      'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY r.seq),'[]') FROM consumer_receipts r WHERE workspace_id=$1),
      'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY e.seq),'[]') FROM workspace_events e WHERE workspace_id=$1),
      'floor',(SELECT purged_through_seq FROM event_retention_state WHERE workspace_id=$1),
      'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1))")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}
async fn event(
    h: &Harness,
    actor: &Actor,
    workspace: Uuid,
    kind: &str,
    schema: i32,
    old: bool,
) -> (Uuid, i64) {
    let mut tx = fixture(h, workspace).await;
    let seq:i64 = sqlx::query_scalar("UPDATE workspace_stream_heads SET last_seq=last_seq+1 WHERE workspace_id=$1 RETURNING last_seq")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,schema_version,entity_id,entity_version,actor_id,mutation_id,occurred_at) VALUES($1,$2,$3,$4,$5,$1,1,$6,$7,clock_timestamp()-CASE WHEN $8 THEN interval '9 days' ELSE interval '0 days' END)")
        .bind(workspace).bind(seq).bind(id).bind(kind).bind(schema).bind(actor.user.id).bind(Uuid::new_v4()).bind(old)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    (id, seq)
}
async fn age_events(h: &Harness, workspace: Uuid) {
    let mut tx = fixture(h, workspace).await;
    sqlx::query("UPDATE workspace_events SET occurred_at=clock_timestamp()-interval '9 days' WHERE workspace_id=$1")
        .bind(workspace).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn due(h: &Harness, workspace: Uuid) {
    let mut tx = fixture(h, workspace).await;
    sqlx::query("UPDATE consumer_dead_letters SET available_at=clock_timestamp()-interval '1 second' WHERE workspace_id=$1 AND state='retry_wait'")
        .bind(workspace).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn letter(h: &Harness, workspace: Uuid, seq: i64) -> Value {
    let mut tx = fixture(h, workspace).await;
    let result = sqlx::query_scalar(
        "SELECT to_jsonb(d) FROM consumer_dead_letters d WHERE workspace_id=$1 AND seq=$2",
    )
    .bind(workspace)
    .bind(seq)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    result
}
async fn suppress(h: &Harness, table: &str, operation: &str) {
    assert!(matches!(
        table,
        "jobs"
            | "assignment_reconciliations"
            | "consumer_receipts"
            | "consumer_checkpoints"
            | "consumer_dead_letters"
            | "consumer_alerts"
            | "workspace_events"
            | "event_retention_state"
            | "consumer_operations"
    ));
    assert!(matches!(operation, "INSERT" | "UPDATE" | "DELETE"));
    sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION test_consumer_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER test_consumer_suppress BEFORE {operation} ON {table} FOR EACH ROW EXECUTE FUNCTION test_consumer_suppress();")))
        .execute(&h.owner).await.unwrap();
}
async fn unsuppress(h: &Harness, table: &str) {
    sqlx::raw_sql(AssertSqlSafe(format!(
        "DROP TRIGGER test_consumer_suppress ON {table}; DROP FUNCTION test_consumer_suppress();"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}
async fn drain(worker: &Worker, workspace: Uuid) {
    for _ in 0..200 {
        worker.capture(workspace).await.unwrap();
        let Some(claim) = worker.claim(workspace).await.unwrap() else {
            return;
        };
        assert!(matches!(
            worker.run_claim(&claim).await.unwrap(),
            WorkerTick::Completed | WorkerTick::Progress
        ));
    }
    panic!("Bounded fixture failed to drain");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registration_bounded_capture_deduplication_and_checkpoint_fencing() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { capture_contract(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn capture_contract(h: &Harness) {
    let (actor, w) = setup(h).await;
    let worker = worker(h).await;
    let initial = snapshot(h, w).await;
    assert_eq!(initial["registration"]["required"], true);
    assert_eq!(initial["registration"]["enabled"], true);
    assert_eq!(initial["checkpoint"]["last_seq"], 0);
    assert!(initial["checkpoint"]["backfill_job_id"].is_null());
    let backfill = worker.capture(w).await.unwrap();
    assert!(backfill.backfilled);
    assert_eq!(backfill.captured, 1);
    let captured = snapshot(h, w).await;
    assert_eq!(captured["checkpoint"]["last_seq"], captured["head"]);
    assert_eq!(captured["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(captured["jobs"][0]["requested_by"], Value::Null);
    assert_eq!(captured["jobs"][0]["kind"], "assignment.reconcile");
    assert_eq!(captured["progress"][0]["source_seq"], captured["head"]);
    assert_eq!(worker.capture(w).await.unwrap().captured, 0);

    let organization = h.app.organizations(&actor).await.unwrap()[0].id;
    let second=h.app.create_workspace(&actor,"consumer-second-workspace",req(json!({"organization_id":organization,"name":"Second","team_name":"Operations","team_key":"OPS"}))).await.unwrap();
    let second_state = snapshot(h, second.id).await;
    assert_eq!(second_state["registration"]["required"], true);
    assert!(
        second_state["head"].as_i64().unwrap() > 0,
        "Registration coexists with creation's first event"
    );
    assert_eq!(second_state["checkpoint"]["last_seq"], 0);
    assert!(worker.capture(second.id).await.unwrap().backfilled);
    let second_after = snapshot(h, second.id).await;
    assert_eq!(second_after["checkpoint"]["last_seq"], second_after["head"]);
    assert_eq!(
        snapshot(h, w).await,
        captured,
        "A second workspace cannot change the first consumer"
    );

    for n in 0..105 {
        event(
            h,
            &actor,
            w,
            if n % 2 == 0 {
                "access.changed"
            } else {
                "issue.updated"
            },
            1,
            false,
        )
        .await;
    }
    let first = worker.capture(w).await.unwrap();
    assert_eq!((first.processed, first.captured), (100, 50));
    let last = worker.capture(w).await.unwrap();
    assert_eq!((last.processed, last.captured), (5, 3));
    let complete = snapshot(h, w).await;
    assert_eq!(complete["receipts"].as_array().unwrap().len(), 105);
    assert_eq!(complete["jobs"].as_array().unwrap().len(), 54);
    assert_eq!(complete["checkpoint"]["last_seq"], complete["head"]);

    // Both consumers must demonstrably wait on the same checkpoint, not merely
    // happen to run sequentially. Releasing the row allows only one new intent.
    event(h, &actor, w, "access.changed", 1, false).await;
    let other = Worker::connect(
        &h.worker_url().await,
        WorkerOptions {
            max_connections: 1,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap();
    let pid1: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&worker.pool)
        .await
        .unwrap();
    let pid2: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&other.pool)
        .await
        .unwrap();
    let mut gate = fixture(h, w).await;
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT last_seq FROM consumer_checkpoints WHERE workspace_id=$1 FOR UPDATE")
        .bind(w)
        .execute(&mut *gate)
        .await
        .unwrap();
    let one = worker.clone();
    let two = other.clone();
    let first = tokio::spawn(async move { one.capture(w).await });
    let second = tokio::spawn(async move { two.capture(w).await });
    tokio::time::timeout(std::time::Duration::from_millis(800),async {
        loop {
            let blocked:bool=sqlx::query_scalar("SELECT $3=ANY(pg_blocking_pids($1)) AND ($3=ANY(pg_blocking_pids($2)) OR $1=ANY(pg_blocking_pids($2))) OR $3=ANY(pg_blocking_pids($2)) AND $2=ANY(pg_blocking_pids($1))")
                .bind(pid1).bind(pid2).bind(gate_pid).fetch_one(&h.owner).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }).await.expect("Both capture calls reach the checkpoint lock");
    gate.commit().await.unwrap();
    let a = first.await.unwrap().unwrap();
    let b = second.await.unwrap().unwrap();
    assert_eq!(a.captured + b.captured, 1);
    let before_replay = snapshot(h, w).await;
    let mut tx = fixture(h, w).await;
    sqlx::query("UPDATE consumer_checkpoints SET last_seq=last_seq-1 WHERE workspace_id=$1")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    worker.capture(w).await.unwrap();
    let replay = snapshot(h, w).await;
    for field in ["jobs", "progress", "receipts", "head"] {
        assert_eq!(
            replay[field], before_replay[field],
            "Replay changed {field}"
        );
    }
    assert_eq!(
        replay["checkpoint"]["last_seq"],
        before_replay["checkpoint"]["last_seq"]
    );
    other.pool.close().await;
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn poison_disposition_disabled_consumer_and_seven_day_retention() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { retention_contract(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn retention_contract(h: &Harness) {
    let (actor, w) = setup(h).await;
    let worker = worker(h).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    age_events(h, w).await;
    assert_eq!(
        worker.retain(w, 1000).await.unwrap().deleted,
        0,
        "No pruning before required snapshot intent"
    );
    worker.capture(w).await.unwrap();
    let initial = snapshot(h, w).await;
    let cutoff = initial["head"].as_i64().unwrap();
    let (_, poison_seq) = event(h, &actor, w, "access.changed", 2, true).await;
    event(h, &actor, w, "access.changed", 1, true).await;
    for attempt in 1..=5 {
        due(h, w).await;
        let result = worker.capture(w).await.unwrap();
        assert_eq!((result.processed, result.captured), (0, 0));
        assert_eq!(result.dead_lettered, attempt == 5);
        let failure = letter(h, w, poison_seq).await;
        assert_eq!(failure["attempts"], attempt);
        assert_eq!(failure["error_code"], "unsupported_event_schema");
        assert_eq!(snapshot(h, w).await["checkpoint"]["last_seq"], cutoff);
    }
    let failure = letter(h, w, poison_seq).await;
    let letter_id = Uuid::parse_str(failure["id"].as_str().unwrap()).unwrap();
    let retained = worker.retain(w, 1000).await.unwrap();
    assert_eq!(retained.purged_through_seq, cutoff);
    assert_eq!(
        snapshot(h, w).await["events"].as_array().unwrap().len(),
        2,
        "Poison and following event remain after nine days"
    );
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 0);
    let mut tx = fixture(h, w).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM consumer_alerts WHERE workspace_id=$1 AND resolved_at IS NULL"
        )
        .bind(w)
        .fetch_one(&mut *tx)
        .await
        .unwrap(),
        1
    );
    tx.commit().await.unwrap();
    assert!(
        operator
            .set_consumer_enabled(w, false, "Attempt unsafe skip")
            .await
            .is_err()
    );
    let before = snapshot(h, w).await;
    assert!(
        operator
            .consumer_disposition(w, letter_id, "discard", "No unsupported discard")
            .await
            .is_err()
    );
    assert_eq!(snapshot(h, w).await, before);
    suppress(h, "consumer_alerts", "UPDATE").await;
    assert!(
        operator
            .consumer_disposition(
                w,
                letter_id,
                "reconcile",
                "Missing alert resolution must roll back"
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(h, w).await, before);
    assert_eq!(letter(h, w, poison_seq).await["state"], "blocked");
    unsuppress(h, "consumer_alerts").await;
    let correlation = operator
        .consumer_disposition(
            w,
            letter_id,
            "reconcile",
            "Recheck current assignments after unsupported event",
        )
        .await
        .unwrap();
    let reconciled = snapshot(h, w).await;
    assert_eq!(reconciled["checkpoint"]["last_seq"], poison_seq);
    assert_eq!(reconciled["receipts"][0]["state"], "reconciled");
    assert_eq!(letter(h, w, poison_seq).await["state"], "resolved");
    let mut tx = fixture(h, w).await;
    let audit: Value = sqlx::query_scalar(
        "SELECT to_jsonb(a) FROM audit_entries a WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(correlation)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(audit["actor_kind"], "local_operator");
    assert_eq!(audit["action"], "consumer.reconcile");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM consumer_alerts WHERE workspace_id=$1 AND resolved_at IS NULL"
        )
        .bind(w)
        .fetch_one(&mut *tx)
        .await
        .unwrap(),
        0
    );
    tx.commit().await.unwrap();
    worker.capture(w).await.unwrap();
    assert_eq!(worker.retain(w, 1).await.unwrap().deleted, 1);
    assert_eq!(worker.retain(w, 1).await.unwrap().deleted, 1);
    drain(&worker, w).await;
    operator
        .set_consumer_enabled(w, false, "Maintenance pause after drain")
        .await
        .unwrap();
    let paused = snapshot(h, w).await;
    assert_eq!(paused["registration"]["enabled"], false);
    assert_eq!(paused["registration"]["required"], true);
    let (_, paused_seq) = event(h, &actor, w, "access.changed", 1, true).await;
    assert_eq!(worker.capture(w).await.unwrap().captured, 0);
    assert_eq!(
        worker.retain(w, 1000).await.unwrap().deleted,
        0,
        "Disabled required consumer still pins uncaptured work"
    );
    operator
        .set_consumer_enabled(w, true, "Resume the required consumer")
        .await
        .unwrap();
    assert_eq!(worker.capture(w).await.unwrap().captured, 1);
    assert_eq!(
        worker.retain(w, 1000).await.unwrap().purged_through_seq,
        paused_seq
    );
    event(h, &actor, w, "issue.updated", 1, false).await;
    worker.capture(w).await.unwrap();
    assert_eq!(
        worker.retain(w, 1000).await.unwrap().deleted,
        0,
        "Captured recent rows keep the browser replay guarantee"
    );
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_and_retention_missing_writes_roll_back_and_roles_stay_separate() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { failure_contract(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn failure_contract(h: &Harness) {
    let (actor, w) = setup(h).await;
    let worker = worker(h).await;
    worker.capture(w).await.unwrap();
    for (table, operation) in [
        ("jobs", "INSERT"),
        ("assignment_reconciliations", "INSERT"),
        ("consumer_receipts", "INSERT"),
        ("consumer_checkpoints", "UPDATE"),
    ] {
        let (_, seq) = event(h, &actor, w, "access.changed", 1, false).await;
        let before = snapshot(h, w).await;
        suppress(h, table, operation).await;
        let failed = worker.capture(w).await.unwrap();
        assert_eq!(
            (failed.processed, failed.captured),
            (0, 0),
            "Suppressed {table} reported progress"
        );
        assert_eq!(
            snapshot(h, w).await,
            before,
            "Suppressed {table} committed partial capture"
        );
        assert_eq!(letter(h, w, seq).await["state"], "retry_wait");
        if table == "jobs" {
            for _ in 0..3 {
                due(h, w).await;
                assert_eq!(worker.capture(w).await.unwrap().captured, 0);
            }
            // An alert is mandatory when the fifth failure becomes blocked.
            unsuppress(h, table).await;
            suppress(h, "consumer_alerts", "INSERT").await;
            // Keep the event poison while isolating the missing alert write.
            let mut tx = fixture(h, w).await;
            sqlx::query(
                "UPDATE workspace_events SET schema_version=2 WHERE workspace_id=$1 AND seq=$2",
            )
            .bind(w)
            .bind(seq)
            .execute(&mut *tx)
            .await
            .unwrap();
            tx.commit().await.unwrap();
            due(h, w).await;
            assert!(worker.capture(w).await.is_err());
            assert_eq!(letter(h, w, seq).await["attempts"], 4);
            unsuppress(h, "consumer_alerts").await;
            due(h, w).await;
            assert!(worker.capture(w).await.unwrap().dead_lettered);
            let mut tx = fixture(h, w).await;
            sqlx::query(
                "UPDATE workspace_events SET schema_version=1 WHERE workspace_id=$1 AND seq=$2",
            )
            .bind(w)
            .bind(seq)
            .execute(&mut *tx)
            .await
            .unwrap();
            tx.commit().await.unwrap();
            let operator = Operator::connect(&h.operator_url().await).await.unwrap();
            let id = Uuid::parse_str(letter(h, w, seq).await["id"].as_str().unwrap()).unwrap();
            operator
                .consumer_disposition(w, id, "retry", "Retry after repairing capture failure")
                .await
                .unwrap();
        } else {
            unsuppress(h, table).await;
        }
        due(h, w).await;
        if table == "jobs" {
            // A failure after receipt/checkpoint writes must roll back both SQL
            // work and the progress returned from the PL/pgSQL subtransaction.
            suppress(h, "consumer_dead_letters", "UPDATE").await;
            assert!(worker.capture(w).await.is_err());
            assert_eq!(snapshot(h, w).await, before);
            unsuppress(h, "consumer_dead_letters").await;
            due(h, w).await;
            suppress(h, "consumer_alerts", "UPDATE").await;
            let failed = worker.capture(w).await.unwrap();
            assert_eq!(
                (failed.processed, failed.captured),
                (0, 0),
                "Late alert failure must not report rolled-back progress"
            );
            assert_eq!(snapshot(h, w).await, before);
            unsuppress(h, "consumer_alerts").await;
            due(h, w).await;
        }
        assert_eq!(worker.capture(w).await.unwrap().captured, 1);
        assert_eq!(letter(h, w, seq).await["state"], "resolved");
    }
    age_events(h, w).await;
    for (table, operation) in [
        ("workspace_events", "DELETE"),
        ("event_retention_state", "UPDATE"),
        ("consumer_operations", "INSERT"),
    ] {
        let before = snapshot(h, w).await;
        suppress(h, table, operation).await;
        assert!(
            worker.retain(w, 1000).await.is_err(),
            "Missing {table} must reject retention"
        );
        assert_eq!(
            snapshot(h, w).await,
            before,
            "Missing {table} pruned without its durable receipt"
        );
        unsuppress(h, table).await;
    }
    assert!(worker.retain(w, 1000).await.unwrap().deleted > 0);

    for query in [
        "SELECT * FROM consumer_receipts",
        "UPDATE consumer_registrations SET required=false",
        "SELECT worker_capture_events($1,100)",
        "SELECT worker_retain_events($1,100)",
        "SELECT worker_assignee_eligible($1,$1,$1)",
    ] {
        let mut tx = store::begin(&h.app.pool).await.unwrap();
        sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true)")
            .bind(w.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let result = if query.contains("$1") {
            sqlx::query(query).bind(w).execute(&mut *tx).await
        } else {
            sqlx::query(query).execute(&mut *tx).await
        };
        assert!(
            result.is_err(),
            "Serving role unexpectedly received consumer capability: {query}"
        );
        tx.rollback().await.unwrap();
    }
    for query in [
        "SELECT password_hash FROM users",
        "SELECT token_hash FROM sessions",
        "SELECT title FROM issues",
        "SELECT * FROM consumer_receipts",
        "UPDATE consumer_registrations SET enabled=false",
        "SELECT operator_consumer_set_enabled($1,false,'forged')",
    ] {
        let result = if query.contains("$1") {
            sqlx::query(query).bind(w).execute(&worker.pool).await
        } else {
            sqlx::query(query).execute(&worker.pool).await
        };
        assert!(result.is_err(), "Worker received excess authority: {query}");
    }
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true)")
        .bind(w.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'assignment.reconcile',NULL,'forged')").bind(w).bind(Uuid::new_v4()).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();

    // Misprovisioning a single new function must fail API startup even when
    // the original worker functions remain inaccessible.
    let runtime_role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    let quoted = runtime_role.replace('"', "\"\"");
    sqlx::query(AssertSqlSafe(format!(
        "GRANT EXECUTE ON FUNCTION worker_retain_events(uuid,integer) TO \"{quoted}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    let mut url = url::Url::parse(&std::env::var("OPENTRACK_TEST_DATABASE_URL").unwrap()).unwrap();
    let worker_url = url::Url::parse(&h.worker_url().await).unwrap();
    url.set_query(worker_url.query());
    assert!(
        App::connect(url.as_str(), &h.setup_token, 1, 1)
            .await
            .is_err()
    );
    sqlx::query(AssertSqlSafe(format!(
        "REVOKE ALL ON FUNCTION worker_retain_events(uuid,integer) FROM \"{quoted}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    sqlx::query(AssertSqlSafe(format!(
        "GRANT SELECT ON consumer_receipts TO \"{quoted}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    assert!(
        App::connect(url.as_str(), &h.setup_token, 1, 1)
            .await
            .is_err(),
        "A serving role with direct consumer receipts must fail startup"
    );
    sqlx::query(AssertSqlSafe(format!(
        "REVOKE ALL ON consumer_receipts FROM \"{quoted}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_capture_has_a_real_statement_deadline_and_durable_retry() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let (actor, workspace) = setup(&runner).await;
        let worker = worker(&runner).await;
        worker.capture(workspace).await.unwrap();
        let (_, seq) = event(&runner, &actor, workspace, "access.changed", 1, false).await;
        let before = snapshot(&runner, workspace).await;
        sqlx::raw_sql("CREATE FUNCTION test_consumer_slow() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(10); RETURN NEW; END $$; CREATE TRIGGER test_consumer_slow BEFORE INSERT ON jobs FOR EACH ROW EXECUTE FUNCTION test_consumer_slow();")
            .execute(&runner.owner).await.unwrap();
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), worker.capture(workspace))
            .await.expect("The preceding statement_timeout must bound the already-running capture call").unwrap();
        assert!(started.elapsed() >= std::time::Duration::from_secs(2), "The injected statement reached its timeout rather than failing before the slow trigger");
        assert_eq!((result.processed, result.captured), (0, 0));
        assert!(!result.backfilled);
        assert_eq!(snapshot(&runner, workspace).await, before);
        let failure = letter(&runner, workspace, seq).await;
        assert_eq!(failure["state"], "retry_wait");
        assert_eq!(failure["error_code"], "effect_failed");
        assert_eq!(failure["attempts"], 1);
        sqlx::raw_sql("DROP TRIGGER test_consumer_slow ON jobs; DROP FUNCTION test_consumer_slow();")
            .execute(&runner.owner).await.unwrap();
        due(&runner, workspace).await;
        assert_eq!(worker.capture(workspace).await.unwrap().captured, 1);
        assert_eq!(letter(&runner, workspace, seq).await["state"], "resolved");
        worker.pool.close().await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
