//! PostgreSQL admission, rollback, accounting and restricted-role contracts.
#[allow(dead_code)]
mod support;

use opentrack_app::{
    Operator,
    worker::{Worker, WorkerOptions, WorkerTick},
};
use opentrack_domain::{Actor, Error};
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, PgPool};
use std::time::Duration;
use support::Harness;
use uuid::Uuid;

const MAX_BYTES: i64 = 268_435_456;

fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

async fn setup(h: &Harness) -> (Actor, Uuid, Uuid) {
    let (session, cookie) = h
        .app
        .setup(req(json!({
            "setup_token":h.setup_token,"email":"owner@example.test",
            "password":"Event admission contract password 724!","display_name":"Example Owner",
            "workspace_name":"Event admission","team_name":"Engineering","team_key":"ENG"
        })))
        .await
        .unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let team = h.app.bootstrap(&actor, w).await.unwrap().teams[0].id;
    h.app
        .create_issue(
            &actor,
            w,
            "admission-baseline",
            req(json!({"team_id":team,"title":"Initial committed event"})),
        )
        .await
        .unwrap();
    (actor, w, team)
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

async fn state(h: &Harness, actor: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(actor.user.id, w).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object(
      'workspace',(SELECT to_jsonb(w) FROM workspaces w WHERE id=$1),
      'teams',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM teams t WHERE workspace_id=$1),
      'issues',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY id),'[]') FROM issues i WHERE workspace_id=$1),
      'retries',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records r WHERE workspace_id=$1),
      'activity',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM activity_entries a WHERE workspace_id=$1),
      'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM audit_entries a WHERE workspace_id=$1),
      'operations',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY id),'[]') FROM consumer_operations o WHERE workspace_id=$1),
      'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY seq),'[]') FROM workspace_events e WHERE workspace_id=$1),
      'head',(SELECT to_jsonb(h) FROM workspace_stream_heads h WHERE workspace_id=$1),
      'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1))")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}

async fn accounting(h: &Harness, actor: &Actor, w: Uuid) -> i64 {
    let mut tx = h.context(actor.user.id, w).await;
    let (charged, retained, exact): (i64, i64, bool) = sqlx::query_as("SELECT h.retained_event_bytes,coalesce(sum(e.storage_bytes),0)::bigint,coalesce(bool_and(e.storage_bytes=192+octet_length(e.event_type)::bigint+octet_length(e.actor_kind)::bigint),true) FROM workspace_stream_heads h LEFT JOIN workspace_events e USING(workspace_id) WHERE h.workspace_id=$1 GROUP BY h.retained_event_bytes")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        charged, retained,
        "Stored charges must equal the workspace total"
    );
    assert!(exact, "Every retained row must use the logical v1 charge");
    tx.commit().await.unwrap();
    charged
}

async fn age(h: &Harness, actor: &Actor, w: Uuid) {
    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("UPDATE workspace_events SET occurred_at=clock_timestamp()-interval '9 days' WHERE workspace_id=$1")
        .bind(w).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

async fn drain(worker: &Worker, w: Uuid) {
    for _ in 0..20 {
        worker.capture(w).await.unwrap();
        let Some(claim) = worker.claim(w).await.unwrap() else {
            return;
        };
        assert!(matches!(
            worker.run_claim(&claim).await.unwrap(),
            WorkerTick::Completed | WorkerTick::Progress
        ));
    }
    panic!("Bounded admission fixture did not drain");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pending_and_byte_caps_roll_back_api_commands_and_preserve_retry_keys() {
    let h = Harness::with_connections(5).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { api_rollback(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn api_rollback(h: &Harness) {
    let (actor, w, team) = setup(h).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let worker = worker(h).await;
    operator
        .set_event_admission_limits(w, 1, MAX_BYTES, "Exercise pending admission")
        .await
        .unwrap();
    let original = h.app.workspace(&actor, w).await.unwrap();
    for (key, expected_reason) in [
        ("pending-retry", "pending_event_limit"),
        ("byte-retry", "retained_event_byte_limit"),
    ] {
        assert_eq!(
            operator.consumer_status(w).await.unwrap()["event_admission"]["admission_reason"],
            expected_reason
        );
        let before = state(h, &actor, w).await;
        assert!(matches!(
            h.app
                .create_issue(
                    &actor,
                    w,
                    key,
                    req(json!({"team_id":team,"title":"Admission retry"}))
                )
                .await,
            Err(Error::Unavailable)
        ));
        assert!(matches!(
            h.app
                .update_workspace(
                    &actor,
                    w,
                    &original.etag(),
                    req(json!({"name":"Rejected identity update"}))
                )
                .await,
            Err(Error::Unavailable)
        ));
        assert_eq!(
            state(h, &actor, w).await,
            before,
            "Admission must roll back domain data, history, retry keys and the stream together"
        );
        if key == "pending-retry" {
            assert!(worker.capture(w).await.unwrap().backfilled);
        } else {
            operator
                .set_event_admission_limits(w, 100_000, MAX_BYTES, "Restore byte capacity")
                .await
                .unwrap();
        }
        let issue = h
            .app
            .create_issue(
                &actor,
                w,
                key,
                req(json!({"team_id":team,"title":"Admission retry"})),
            )
            .await
            .unwrap();
        let committed = state(h, &actor, w).await;
        let replay = h
            .app
            .create_issue(
                &actor,
                w,
                key,
                req(json!({"team_id":team,"title":"Admission retry"})),
            )
            .await
            .unwrap();
        assert_eq!(issue.id, replay.id);
        assert_eq!(
            state(h, &actor, w).await,
            committed,
            "Committed retries must succeed even at the pending cap"
        );
        accounting(h, &actor, w).await;
        if key == "pending-retry" {
            let bytes = accounting(h, &actor, w).await;
            operator
                .set_event_admission_limits(w, 100_000, bytes, "Exercise retained-byte admission")
                .await
                .unwrap();
        }
    }
    assert_eq!(
        h.app
            .update_workspace(
                &actor,
                w,
                &original.etag(),
                req(json!({"name":"Accepted identity update"}))
            )
            .await
            .unwrap()
            .name,
        "Accepted identity update"
    );
    accounting(h, &actor, w).await;
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disabled_required_consumer_pins_admission_until_capture_and_safe_pruning() {
    let h = Harness::with_connections(5).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { retention(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn retention(h: &Harness) {
    let (actor, w, team) = setup(h).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let worker = worker(h).await;
    drain(&worker, w).await;
    age(h, &actor, w).await;
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 1);
    assert_eq!(accounting(h, &actor, w).await, 0);
    operator
        .set_consumer_enabled(w, false, "Pause a drained required consumer")
        .await
        .unwrap();
    operator
        .set_event_admission_limits(w, 1, MAX_BYTES, "Keep one pending event")
        .await
        .unwrap();
    h.app
        .create_issue(
            &actor,
            w,
            "paused-event",
            req(json!({"team_id":team,"title":"Required backlog"})),
        )
        .await
        .unwrap();
    age(h, &actor, w).await;
    let bytes = accounting(h, &actor, w).await;
    assert!(bytes > 0);
    assert_eq!(worker.capture(w).await.unwrap().processed, 0);
    assert_eq!(
        worker.retain(w, 1000).await.unwrap().deleted,
        0,
        "Nine-day-old uncaptured events must remain"
    );
    let before = state(h, &actor, w).await;
    assert!(matches!(
        h.app
            .create_issue(
                &actor,
                w,
                "paused-retry",
                req(json!({"team_id":team,"title":"Wait for capture"}))
            )
            .await,
        Err(Error::Unavailable)
    ));
    assert_eq!(state(h, &actor, w).await, before);
    operator
        .set_consumer_enabled(w, true, "Resume required capture")
        .await
        .unwrap();
    assert_eq!(worker.capture(w).await.unwrap().processed, 1);
    assert_eq!(
        accounting(h, &actor, w).await,
        bytes,
        "Capture alone cannot release replay storage"
    );
    operator
        .set_event_admission_limits(w, 1, bytes, "Require safe byte reclamation")
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .create_issue(
                &actor,
                w,
                "paused-retry",
                req(json!({"team_id":team,"title":"Wait for capture"}))
            )
            .await,
        Err(Error::Unavailable)
    ));
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 1);
    assert_eq!(accounting(h, &actor, w).await, 0);
    h.app
        .create_issue(
            &actor,
            w,
            "paused-retry",
            req(json!({"team_id":team,"title":"Wait for capture"})),
        )
        .await
        .unwrap();
    worker.capture(w).await.unwrap();
    assert_eq!(
        worker.retain(w, 1000).await.unwrap().deleted,
        0,
        "Recent captured events retain their browser replay window"
    );
    assert_eq!(accounting(h, &actor, w).await, bytes);
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multiple_consumer_watermarks_preserve_required_work_and_storage_accounting() {
    let h = Harness::with_connections(5).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { multiple_consumers(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn multiple_consumers(h: &Harness) {
    let (actor, w, team) = setup(h).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let worker = worker(h).await;
    drain(&worker, w).await;
    age(h, &actor, w).await;
    let bytes = accounting(h, &actor, w).await;
    assert!(bytes > 0);
    let mut tx = h.context(actor.user.id, w).await;
    let (captured_seq, backfill): (i64, Uuid) = sqlx::query_as("SELECT last_seq,backfill_job_id FROM consumer_checkpoints WHERE workspace_id=$1 AND consumer_name='assignment_cleanup' AND consumer_version=1")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(captured_seq, 1);
    sqlx::query("INSERT INTO consumer_registrations(workspace_id,consumer_name,consumer_version,required) VALUES($1,'fixture_required',1,true),($1,'fixture_optional',1,false)")
        .bind(w).execute(&mut *tx).await.unwrap();
    // Synthetic registration/checkpoint metadata exercises the shared watermark only.
    // This job reference satisfies its FK; no second production consumer ran.
    sqlx::query("INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version,last_seq,backfill_job_id) VALUES($1,'fixture_required',1,0,$2)")
        .bind(w).bind(backfill).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    operator
        .set_event_admission_limits(w, 1, MAX_BYTES, "Exercise the slowest required watermark")
        .await
        .unwrap();

    for enabled in [true, false] {
        let mut tx = h.context(actor.user.id, w).await;
        sqlx::query("UPDATE consumer_registrations SET enabled=$2 WHERE workspace_id=$1 AND consumer_name='fixture_required' AND consumer_version=1")
            .bind(w).bind(enabled).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let status = operator.consumer_status(w).await.unwrap();
        assert_eq!(status["head_seq"], captured_seq.to_string());
        assert_eq!(status["event_admission"]["pending_events"], "1");
        assert_eq!(
            status["event_admission"]["admission_reason"],
            "pending_event_limit"
        );
        let before = state(h, &actor, w).await;
        assert!(matches!(
            h.app
                .create_issue(
                    &actor,
                    w,
                    "multiple-consumer-retry",
                    req(json!({"team_id":team,"title":"Wait for every required consumer"}))
                )
                .await,
            Err(Error::Unavailable)
        ));
        assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 0);
        assert_eq!(state(h, &actor, w).await, before);
        assert_eq!(accounting(h, &actor, w).await, bytes);
    }

    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("DELETE FROM consumer_checkpoints WHERE workspace_id=$1 AND consumer_name='fixture_required' AND consumer_version=1")
        .bind(w).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        operator.consumer_status(w).await.unwrap()["event_admission"]["admission_reason"],
        "required_consumer_unavailable"
    );
    let before = state(h, &actor, w).await;
    assert!(matches!(
        h.app
            .create_issue(
                &actor,
                w,
                "multiple-consumer-retry",
                req(json!({"team_id":team,"title":"Wait for every required consumer"}))
            )
            .await,
        Err(Error::Unavailable)
    ));
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 0);
    assert_eq!(state(h, &actor, w).await, before);
    assert_eq!(accounting(h, &actor, w).await, bytes);

    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version,last_seq) VALUES($1,'fixture_required',1,$2)")
        .bind(w).bind(captured_seq).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let status = operator.consumer_status(w).await.unwrap();
    assert_eq!(status["event_admission"]["pending_events"], "0");
    assert_eq!(status["event_admission"]["admission_blocked"], false);
    assert_eq!(
        worker.retain(w, 1000).await.unwrap().deleted,
        0,
        "A required checkpoint without snapshot intent must still pin retention"
    );
    assert_eq!(accounting(h, &actor, w).await, bytes);

    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("UPDATE consumer_checkpoints SET backfill_job_id=$2 WHERE workspace_id=$1 AND consumer_name='fixture_required' AND consumer_version=1")
        .bind(w).bind(backfill).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        operator.consumer_status(w).await.unwrap()["event_admission"]["pending_events"],
        "0",
        "An optional missing checkpoint must not contribute to required admission"
    );
    let retained = worker.retain(w, 1).await.unwrap();
    assert_eq!(retained.deleted, 1);
    assert_eq!(retained.purged_through_seq, captured_seq);
    assert_eq!(accounting(h, &actor, w).await, 0);
    h.app
        .create_issue(
            &actor,
            w,
            "multiple-consumer-retry",
            req(json!({"team_id":team,"title":"Wait for every required consumer"})),
        )
        .await
        .unwrap();
    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version,last_seq) VALUES($1,'fixture_optional',1,0)")
        .bind(w).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(accounting(h, &actor, w).await, bytes);
    assert_eq!(worker.capture(w).await.unwrap().processed, 1);
    assert_eq!(
        accounting(h, &actor, w).await,
        bytes,
        "Real assignment capture cannot release replay storage"
    );
    age(h, &actor, w).await;
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 0);
    assert_eq!(
        operator.consumer_status(w).await.unwrap()["event_admission"]["pending_events"],
        "1",
        "Real assignment capture cannot advance the synthetic required checkpoint"
    );

    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("UPDATE consumer_checkpoints SET last_seq=$2 WHERE workspace_id=$1 AND consumer_name='fixture_required' AND consumer_version=1")
        .bind(w).bind(captured_seq + 1).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        operator.consumer_status(w).await.unwrap()["event_admission"]["pending_events"],
        "0"
    );
    assert_eq!(accounting(h, &actor, w).await, bytes);
    let retained = worker.retain(w, 1).await.unwrap();
    assert_eq!(retained.deleted, 1);
    assert_eq!(retained.purged_through_seq, captured_seq + 1);
    assert_eq!(accounting(h, &actor, w).await, 0);
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_required_registration_or_checkpoint_fails_closed() {
    let h = Harness::with_connections(5).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { missing_required(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn missing_required(h: &Harness) {
    let (actor, w, team) = setup(h).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let worker = worker(h).await;
    age(h, &actor, w).await;
    for remove_registration in [false, true] {
        let mut tx = h.context(actor.user.id, w).await;
        sqlx::query("DELETE FROM consumer_checkpoints WHERE workspace_id=$1")
            .bind(w)
            .execute(&mut *tx)
            .await
            .unwrap();
        if remove_registration {
            sqlx::query("DELETE FROM consumer_registrations WHERE workspace_id=$1")
                .bind(w)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        let status = operator.consumer_status(w).await.unwrap();
        assert_eq!(
            status["event_admission"]["admission_reason"],
            "required_consumer_unavailable"
        );
        assert_eq!(status["event_admission"]["admission_blocked"], true);
        let before = state(h, &actor, w).await;
        assert!(matches!(
            h.app
                .create_issue(
                    &actor,
                    w,
                    "missing-required",
                    req(json!({"team_id":team,"title":"Repair required state"}))
                )
                .await,
            Err(Error::Unavailable)
        ));
        assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 0);
        assert_eq!(state(h, &actor, w).await, before);
        let mut tx = h.context(actor.user.id, w).await;
        if remove_registration {
            sqlx::query("INSERT INTO consumer_registrations(workspace_id,consumer_name,consumer_version) VALUES($1,'assignment_cleanup',1)").bind(w).execute(&mut *tx).await.unwrap();
        }
        sqlx::query("INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version) VALUES($1,'assignment_cleanup',1)").bind(w).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let mut tx = h.context(actor.user.id, w).await;
    let (checkpoint, head): (i64, i64) = sqlx::query_as("SELECT c.last_seq,h.last_seq FROM consumer_checkpoints c JOIN workspace_stream_heads h USING(workspace_id) WHERE c.workspace_id=$1 AND c.consumer_name='assignment_cleanup' AND c.consumer_version=1")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("UPDATE consumer_checkpoints SET last_seq=$2 WHERE workspace_id=$1 AND consumer_name='assignment_cleanup' AND consumer_version=1")
        .bind(w).bind(head + 1).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        operator.consumer_status(w).await.unwrap()["event_admission"]["admission_reason"],
        "required_consumer_unavailable"
    );
    let before = state(h, &actor, w).await;
    assert!(matches!(
        h.app
            .create_issue(
                &actor,
                w,
                "missing-required",
                req(json!({"team_id":team,"title":"Repair required state"}))
            )
            .await,
        Err(Error::Unavailable)
    ));
    assert_eq!(
        state(h, &actor, w).await,
        before,
        "A future checkpoint cannot become valid by reserving the event it would skip"
    );
    let mut tx = h.context(actor.user.id, w).await;
    let retained_checkpoint: i64 = sqlx::query_scalar("SELECT last_seq FROM consumer_checkpoints WHERE workspace_id=$1 AND consumer_name='assignment_cleanup' AND consumer_version=1")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(retained_checkpoint, head + 1);
    sqlx::query("UPDATE consumer_checkpoints SET last_seq=$2 WHERE workspace_id=$1 AND consumer_name='assignment_cleanup' AND consumer_version=1")
        .bind(w).bind(checkpoint).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    h.app
        .create_issue(
            &actor,
            w,
            "missing-required",
            req(json!({"team_id":team,"title":"Repair required state"})),
        )
        .await
        .unwrap();
    accounting(h, &actor, w).await;
    worker.pool.close().await;
}

async fn denied(pool: &PgPool, w: Uuid) {
    for query in [
        "SELECT operator_event_admission_status($1)",
        "SELECT operator_event_admission_limits($1,1,1,'forged')",
        "UPDATE workspace_stream_heads SET max_pending_events=1 WHERE workspace_id=$1",
        "UPDATE workspace_stream_heads SET max_retained_event_bytes=1 WHERE workspace_id=$1",
        "UPDATE workspace_stream_heads SET retained_event_bytes=0 WHERE workspace_id=$1",
    ] {
        let mut tx = store::begin(pool).await.unwrap();
        sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true)")
            .bind(w.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(query)
            .bind(w)
            .execute(&mut *tx)
            .await
            .expect_err("Admission maintenance must remain operator-only");
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
        tx.rollback().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn operator_limits_are_validated_audited_atomic_and_restricted() {
    let h = Harness::with_connections(5).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { operator_contract(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn operator_contract(h: &Harness) {
    let (actor, w, _) = setup(h).await;
    let operator_url = h.operator_url().await;
    let operator = Operator::connect(&operator_url).await.unwrap();
    let worker = worker(h).await;
    let status = operator.consumer_status(w).await.unwrap()["event_admission"].clone();
    assert_eq!(status["max_pending_events"], "100000");
    assert_eq!(status["max_retained_event_bytes"], MAX_BYTES.to_string());
    assert_eq!(status["pending_events"], "1");
    assert_eq!(status["accounting"], "logical_v1");
    assert_eq!(status["admission_blocked"], false);
    assert!(status["admission_reason"].is_null());
    let before = state(h, &actor, w).await;
    for (pending, bytes, reason) in [
        (0, MAX_BYTES, "Invalid pending"),
        (1, 0, "Invalid bytes"),
        (1, MAX_BYTES, " "),
        (1, MAX_BYTES, "Invalid\nreason"),
    ] {
        assert!(matches!(
            operator
                .set_event_admission_limits(w, pending, bytes, reason)
                .await,
            Err(Error::Invalid(_))
        ));
    }
    assert!(matches!(
        operator
            .set_event_admission_limits(Uuid::new_v4(), 1, 1, "Unavailable workspace")
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(state(h, &actor, w).await, before);
    sqlx::raw_sql("CREATE FUNCTION test_suppress_admission() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$").execute(&h.owner).await.unwrap();
    for (table, operation) in [
        ("audit_entries", "INSERT"),
        ("consumer_operations", "INSERT"),
        ("workspace_stream_heads", "UPDATE"),
    ] {
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE TRIGGER test_suppress_admission BEFORE {operation} ON {table} FOR EACH ROW EXECUTE FUNCTION test_suppress_admission()"))).execute(&h.owner).await.unwrap();
        assert!(
            operator
                .set_event_admission_limits(w, 7, 4096, "Must persist every operation effect")
                .await
                .is_err()
        );
        assert_eq!(
            state(h, &actor, w).await,
            before,
            "Missing {table} must roll back the limit change"
        );
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP TRIGGER test_suppress_admission ON {table}"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
    }
    let correlation = operator
        .set_event_admission_limits(w, 7, 4096, " Reviewed capacity ")
        .await
        .unwrap();
    let after = state(h, &actor, w).await;
    let audit = after["audit"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == correlation.to_string())
        .unwrap();
    assert_eq!(audit["action"], "events.admission_limits_changed");
    assert_eq!(audit["actor_kind"], "local_operator");
    assert_eq!(audit["metadata"]["before"]["max_pending_events"], "100000");
    assert_eq!(audit["metadata"]["after"]["max_pending_events"], "7");
    assert_eq!(
        audit["metadata"]["after"]["max_retained_event_bytes"],
        "4096"
    );
    assert_eq!(audit["metadata"]["reason"], "Reviewed capacity");
    assert!(
        after["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == correlation.to_string()
                && entry["reason"] == "Reviewed capacity")
    );
    for field in ["events", "activity", "issues", "epoch"] {
        assert_eq!(after[field], before[field]);
    }
    denied(&h.app.pool, w).await;
    denied(&worker.pool, w).await;
    let operator_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&operator_url)
        .await
        .unwrap();
    assert!(
        sqlx::query("SELECT * FROM workspace_stream_heads")
            .execute(&operator_pool)
            .await
            .is_err()
    );
    operator_pool.close().await;
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_24_upgrade_backfills_charges_and_preserves_existing_grant_boundaries() {
    let h = Harness::with_version(5, 24).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn upgrade(h: &Harness) {
    assert!(!store::event_admission_ready(&h.owner).await.unwrap());
    let (actor, w, team) = setup(h).await;
    h.app
        .create_issue(
            &actor,
            w,
            "pre-admission",
            req(json!({"team_id":team,"title":"Existing issue"})),
        )
        .await
        .unwrap();
    let worker_url = h.worker_url().await;
    let before = state(h, &actor, w).await;
    sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 25)
            .cloned()
            .collect::<Vec<_>>()
            .into(),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(&h.owner)
    .await
    .unwrap();
    assert!(store::event_admission_ready(&h.owner).await.unwrap());
    for trigger in [
        "event_storage_charge",
        "event_storage_insert",
        "event_storage_update",
        "event_storage_delete",
    ] {
        sqlx::raw_sql(AssertSqlSafe(format!(
            "ALTER TABLE workspace_events DISABLE TRIGGER {trigger}"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
        assert!(!store::event_admission_ready(&h.owner).await.unwrap());
        sqlx::raw_sql(AssertSqlSafe(format!(
            "ALTER TABLE workspace_events ENABLE TRIGGER {trigger}"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
        assert!(store::event_admission_ready(&h.owner).await.unwrap());
    }
    let worker = Worker::connect(
        &worker_url,
        WorkerOptions {
            max_connections: 1,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        25
    );
    let after = state(h, &actor, w).await;
    let mut events = after["events"].clone();
    for event in events.as_array_mut().unwrap() {
        event.as_object_mut().unwrap().remove("storage_bytes");
    }
    assert_eq!(events, before["events"]);
    for field in [
        "workspace",
        "issues",
        "retries",
        "activity",
        "audit",
        "epoch",
    ] {
        assert_eq!(after[field], before[field]);
    }
    assert!(accounting(h, &actor, w).await > 0);
    assert_eq!(after["head"]["max_pending_events"], 100_000);
    assert_eq!(after["head"]["max_retained_event_bytes"], MAX_BYTES);
    denied(&h.app.pool, w).await;
    denied(&worker.pool, w).await;
    assert!(
        worker.capture(w).await.unwrap().backfilled,
        "An existing worker grant must retain its capture capability"
    );
    age(h, &actor, w).await;
    assert_eq!(worker.retain(w, 1000).await.unwrap().deleted, 2);
    assert_eq!(accounting(h, &actor, w).await, 0);
    h.app
        .create_issue(
            &actor,
            w,
            "post-admission",
            req(json!({"team_id":team,"title":"Issue after upgrade"})),
        )
        .await
        .unwrap();
    accounting(h, &actor, w).await;
    worker.pool.close().await;
}

async fn append(
    tx: &mut store::Tx,
    actor: Uuid,
    w: Uuid,
    team: Uuid,
    count: usize,
) -> Result<(), sqlx::Error> {
    let events: Vec<_> = (0..count)
        .map(|_| store::Event {
            workspace: w,
            team,
            entity: Uuid::new_v4(),
            version: 1,
            actor,
            kind: "issue.updated",
            mutation: Uuid::new_v4(),
            changes: json!({}),
        })
        .collect();
    store::append_stream_events(tx, &events).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_appends_cannot_overshoot_the_pending_cap_or_leak_batch_reservations() {
    let h = Harness::with_connections(5).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { concurrent_cap(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn concurrent_cap(h: &Harness) {
    let (actor, w, team) = setup(h).await;
    let worker = worker(h).await;
    worker.capture(w).await.unwrap();
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    operator
        .set_event_admission_limits(w, 1, MAX_BYTES, "One concurrent append may commit")
        .await
        .unwrap();
    let before = state(h, &actor, w).await;
    let mut first = h.context(actor.user.id, w).await;
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *first)
        .await
        .unwrap();
    append(&mut first, actor.user.id, w, team, 1).await.unwrap();
    let mut second = h.context(actor.user.id, w).await;
    let waiter: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *second)
        .await
        .unwrap();
    let user = actor.user.id;
    let writer = tokio::spawn(async move {
        let result = append(&mut second, user, w, team, 1).await;
        second.rollback().await.unwrap();
        result
    });
    tokio::time::timeout(Duration::from_millis(800), async {
        loop {
            if sqlx::query_scalar::<_, bool>("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(waiter)
                .bind(holder)
                .fetch_one(&h.app.pool)
                .await
                .unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("The competing writer must wait for the stream-head reservation");
    first.commit().await.unwrap();
    let error = writer.await.unwrap().unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("event_admission_blocked")
    );
    let after = state(h, &actor, w).await;
    assert_eq!(
        after["head"]["last_seq"].as_i64().unwrap(),
        before["head"]["last_seq"].as_i64().unwrap() + 1
    );
    assert_eq!(
        after["events"].as_array().unwrap().len(),
        before["events"].as_array().unwrap().len() + 1
    );
    assert_eq!(
        operator.consumer_status(w).await.unwrap()["event_admission"]["pending_events"],
        "1"
    );
    accounting(h, &actor, w).await;
    worker.capture(w).await.unwrap();
    let before_batch = state(h, &actor, w).await;
    let mut batch = h.context(actor.user.id, w).await;
    let error = append(&mut batch, actor.user.id, w, team, 2)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("event_admission_blocked")
    );
    batch.rollback().await.unwrap();
    assert_eq!(
        state(h, &actor, w).await,
        before_batch,
        "A rejected multi-event reservation must leave no sequence or storage charge"
    );
    worker.pool.close().await;
}
