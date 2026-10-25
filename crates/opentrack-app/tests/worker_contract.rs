//! Real fourth-credential worker contracts: leases, fencing, atomic effects, authority.
#[allow(dead_code)]
mod support;
use opentrack_app::{
    App,
    worker::{Worker, WorkerOptions, WorkerTick},
};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;
fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn fixture(h: &Harness, u: Uuid, w: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)").bind(u.to_string()).bind(w.to_string()).execute(&mut *tx).await.unwrap();
    tx
}
async fn job(h: &Harness, a: &Actor, w: Uuid, count: i32) -> (Uuid, Uuid, Uuid, Uuid) {
    let id = Uuid::new_v4().simple().to_string();
    let team=h.app.create_team(a,w,&id,req(json!({"name":format!("Worker {id}"),"key":format!("W{}",&id[..6]),"private":false}))).await.unwrap();
    let flow = h.app.workflow(a, w, team.id).await.unwrap();
    let source = flow.statuses.iter().find(|s| s.is_default).unwrap().id;
    let target = flow.statuses.iter().find(|s| !s.is_default).unwrap().id;
    let mut tx = fixture(h, a.user.id, w).await;
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank) SELECT gen_random_uuid(),$1,$2,$3,n,'Worker fixture',$4,n::numeric*1024 FROM generate_series(1,$5) n").bind(w).bind(team.id).bind(a.user.id).bind(source).bind(count).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let result = h
        .app
        .replace_status(
            a,
            w,
            team.id,
            source,
            &id,
            &flow.etag(),
            req(json!({"replacement_status_id":target})),
        )
        .await
        .unwrap();
    (result.id, team.id, source, target)
}
async fn snapshot(h: &Harness, a: &Actor, w: Uuid, t: Uuid) -> Value {
    let mut tx = fixture(h, a.user.id, w).await;
    let data=sqlx::query_scalar("SELECT jsonb_build_object('issues',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY id),'[]') FROM issues i WHERE workspace_id=$1 AND team_id=$2),'statuses',(SELECT jsonb_agg(to_jsonb(s) ORDER BY id) FROM workflow_statuses s WHERE workspace_id=$1 AND team_id=$2),'progress',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM workflow_replacements p WHERE workspace_id=$1 AND team_id=$2),'workflow',(SELECT workflow_version FROM teams WHERE workspace_id=$1 AND id=$2),'activity',(SELECT count(*) FROM activity_entries WHERE workspace_id=$1),'audit',(SELECT count(*) FROM audit_entries WHERE workspace_id=$1),'events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1))").bind(w).bind(t).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    data
}
async fn expire(h: &Harness, a: &Actor, w: Uuid, id: Uuid) {
    let mut tx = fixture(h, a.user.id, w).await;
    sqlx::query("UPDATE jobs SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn failure(h: &Harness, table: &str, event: &str, body: &str) {
    // Table/event/body are fixed test fragments, never untrusted inputs.
    sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION test_worker_drop() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN {body}; RETURN NEW; END $$; CREATE TRIGGER test_worker_drop BEFORE {event} ON {table} FOR EACH ROW EXECUTE FUNCTION test_worker_drop();"))).execute(&h.owner).await.unwrap();
}
async fn clear_failure(h: &Harness, table: &str) {
    sqlx::raw_sql(AssertSqlSafe(format!(
        "DROP TRIGGER test_worker_drop ON {table}; DROP FUNCTION test_worker_drop();"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_worker_contracts() {
    let h = Harness::with_connections(6).await;
    let r = h.clone();
    let result = tokio::spawn(async move { exercise(&r).await }).await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
    h.cleanup().await;
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@worker.invalid","password":"worker contract password 840!","display_name":"Worker owner","workspace_name":"Worker contracts","team_name":"Start","team_key":"START"}))).await.unwrap();
    let a = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let url = h.worker_url().await;
    let worker = Worker::connect(&url, WorkerOptions::default())
        .await
        .unwrap();
    let other = Worker::connect(&url, WorkerOptions::default())
        .await
        .unwrap();
    assert!(App::connect(&url, &h.setup_token, 2, 1).await.is_err());
    for variable in [
        "OPENTRACK_TEST_DATABASE_URL",
        "OPENTRACK_TEST_MIGRATION_DATABASE_URL",
        "OPENTRACK_TEST_OPERATOR_DATABASE_URL",
    ] {
        let mut wrong = url::Url::parse(&std::env::var(variable).unwrap()).unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        wrong.set_query(parsed.query());
        assert!(
            Worker::connect(wrong.as_str(), WorkerOptions::default())
                .await
                .is_err()
        );
    }
    for query in [
        "SELECT password_hash FROM users",
        "SELECT token_hash FROM sessions",
        "SELECT token_hash FROM recovery_tokens",
        "SELECT * FROM worker_workspace_registry",
        "UPDATE workspace_members SET active=false",
    ] {
        assert!(
            sqlx::query(query).execute(&worker.pool).await.is_err(),
            "Worker received forbidden credential/content authority"
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs")
            .fetch_one(&worker.pool)
            .await
            .unwrap(),
        0,
        "Missing workspace context fails closed"
    );
    // Reprovisioning must remove stale column grants, not only table grants.
    let role = url::Url::parse(&url)
        .unwrap()
        .username()
        .replace('"', "\"\"");
    sqlx::query(AssertSqlSafe(format!(
        "GRANT SELECT(description) ON issues TO \"{role}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    assert!(
        Worker::connect(&url, WorkerOptions::default())
            .await
            .is_err()
    );
    h.worker_url().await;
    assert!(
        sqlx::query("SELECT description FROM issues")
            .execute(&worker.pool)
            .await
            .is_err()
    );
    // Clean 100-row yield does not consume attempts. A replayed old claim has no effects.
    let (id, t, _, _) = job(h, &a, w, 101).await;
    let first = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(first.id, id);
    assert!(other.claim(w).await.unwrap().is_none());
    assert_eq!(
        worker.run_claim(&first).await.unwrap(),
        WorkerTick::Progress
    );
    let after = snapshot(h, &a, w, t).await;
    assert_eq!(
        worker.run_claim(&first).await.unwrap(),
        WorkerTick::LostLease
    );
    assert_eq!(snapshot(h, &a, w, t).await, after);
    let second = other.claim(w).await.unwrap().unwrap();
    assert_eq!(second.attempt_count, first.attempt_count);
    assert!(second.lease_token > first.lease_token);
    assert_eq!(
        other.run_claim(&second).await.unwrap(),
        WorkerTick::Completed
    );
    let view = h.app.get_workflow_replacement(&a, w, t, id).await.unwrap();
    assert_eq!(view.processed_count, "101");
    assert_eq!(view.remaining_count, "0");
    assert_eq!(view.state, "succeeded");
    // Crash/takeover changes the fence and attempt. The old process cannot commit.
    let (id, t, _, _) = job(h, &a, w, 1).await;
    let abandoned = worker.claim(w).await.unwrap().unwrap();
    expire(h, &a, w, id).await;
    let claimed = other.claim(w).await.unwrap().unwrap();
    assert!(claimed.lease_token > abandoned.lease_token);
    assert_eq!(claimed.attempt_count, abandoned.attempt_count + 1);
    let before = snapshot(h, &a, w, t).await;
    assert_eq!(
        worker.run_claim(&abandoned).await.unwrap(),
        WorkerTick::LostLease
    );
    assert_eq!(snapshot(h, &a, w, t).await, before);
    assert_eq!(
        other.run_claim(&claimed).await.unwrap(),
        WorkerTick::Completed
    );
    // Poisoned effects are durably visible; every aggregate and stream change rolls back.
    for (table, event, body) in [
        ("activity_entries", "INSERT", "RETURN NULL"),
        ("audit_entries", "INSERT", "RETURN NULL"),
        ("workspace_events", "INSERT", "RETURN NULL"),
        ("workspace_stream_heads", "UPDATE", "RETURN NULL"),
        ("workflow_replacements", "UPDATE", "RETURN NULL"),
        (
            "jobs",
            "UPDATE",
            "IF NEW.state='succeeded' THEN RETURN NULL; END IF",
        ),
        ("job_attempts", "UPDATE", "RETURN NULL"),
    ] {
        let (id, t, _, _) = job(h, &a, w, 1).await;
        let claim = worker.claim(w).await.unwrap().unwrap();
        let before = snapshot(h, &a, w, t).await;
        failure(h, table, event, body).await;
        let result = worker.run_claim(&claim).await;
        clear_failure(h, table).await;
        assert!(
            result.is_err() || matches!(result.unwrap(), WorkerTick::Retry | WorkerTick::LostLease)
        );
        assert_eq!(
            snapshot(h, &a, w, t).await,
            before,
            "{table} failure leaked effects"
        );
        let view = h.app.get_workflow_replacement(&a, w, t, id).await.unwrap();
        let fresh = if view.state == "dead_letter" {
            h.app
                .resume_workflow_replacement(&a, w, t, id, &view.etag())
                .await
                .unwrap();
            worker.claim(w).await.unwrap().unwrap()
        } else {
            claim
        };
        assert_eq!(
            worker.run_claim(&fresh).await.unwrap(),
            WorkerTick::Completed
        );
    }
    // Claim effects themselves are mandatory; a missing attempt or lease update cannot consume work.
    for (table, event) in [("job_attempts", "INSERT"), ("jobs", "UPDATE")] {
        let (id, t, _, _) = job(h, &a, w, 1).await;
        failure(h, table, event, "RETURN NULL").await;
        assert!(worker.claim(w).await.is_err());
        clear_failure(h, table).await;
        let view = h.app.get_workflow_replacement(&a, w, t, id).await.unwrap();
        assert_eq!(view.state, "queued");
        let claim = worker.claim(w).await.unwrap().unwrap();
        assert_eq!(claim.attempt_count, 1);
        assert_eq!(
            worker.run_claim(&claim).await.unwrap(),
            WorkerTick::Completed
        );
    }
    // Durable dead-letter after the configured attempt budget; explicit manager resume keeps history.
    let (id, t, _, _) = job(h, &a, w, 1).await;
    let mut tx = fixture(h, a.user.id, w).await;
    sqlx::query("UPDATE jobs SET max_attempts=2 WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let first = worker.claim(w).await.unwrap().unwrap();
    expire(h, &a, w, id).await;
    let second = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(second.attempt_count, 2);
    expire(h, &a, w, id).await;
    assert!(worker.claim(w).await.unwrap().is_none());
    let dead = h.app.get_workflow_replacement(&a, w, t, id).await.unwrap();
    assert_eq!(dead.state, "dead_letter");
    assert_eq!(dead.error_code.as_deref(), Some("attempt_limit"));
    h.app
        .resume_workflow_replacement(&a, w, t, id, &dead.etag())
        .await
        .unwrap();
    let resumed = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(resumed.attempt_count, 3);
    assert_eq!(
        worker.run_claim(&first).await.unwrap(),
        WorkerTick::LostLease
    );
    assert_eq!(
        worker.run_claim(&resumed).await.unwrap(),
        WorkerTick::Completed
    );
    expires_mid_effect(h, &a, w, &worker).await;
    bounded_failure_bookkeeping(h, &a, w, &worker).await;
    // A current authority change blocks the next batch, while a new manager can resume.
    let manager_id = Uuid::new_v4();
    let email = format!("{manager_id}@worker.invalid");
    let mut tx = fixture(h, a.user.id, w).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Worker manager',password_hash FROM users WHERE id=$3").bind(manager_id).bind(&email).bind(a.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'admin')")
        .bind(w)
        .bind(manager_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, token) = h
        .app
        .login(req(
            json!({"email":email,"password":"worker contract password 840!"}),
        ))
        .await
        .unwrap();
    let manager = h.app.authenticate(&token).await.unwrap();
    let (id, t, _, _) = job(h, &manager, w, 1).await;
    let claim = worker.claim(w).await.unwrap().unwrap();
    let before = snapshot(h, &a, w, t).await;
    let mut tx = fixture(h, a.user.id, w).await;
    sqlx::query("UPDATE workspace_members SET active=false WHERE workspace_id=$1 AND user_id=$2")
        .bind(w)
        .bind(manager_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(worker.run_claim(&claim).await.unwrap(), WorkerTick::Blocked);
    assert_eq!(snapshot(h, &a, w, t).await, before);
    let blocked = h.app.get_workflow_replacement(&a, w, t, id).await.unwrap();
    assert_eq!(blocked.error_code.as_deref(), Some("access_lost"));
    h.app
        .resume_workflow_replacement(&a, w, t, id, &blocked.etag())
        .await
        .unwrap();
    let resumed = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(resumed.requested_by, Some(a.user.id));
    assert_eq!(
        worker.run_claim(&resumed).await.unwrap(),
        WorkerTick::Completed
    );
    // Transient SQL failures back off durably and cannot be claimed before available_at.
    let (id, t, _, _) = job(h, &a, w, 1).await;
    let claim = worker.claim(w).await.unwrap().unwrap();
    let before = snapshot(h, &a, w, t).await;
    failure(
        h,
        "activity_entries",
        "INSERT",
        "RAISE EXCEPTION 'test transient' USING ERRCODE='40001'",
    )
    .await;
    assert_eq!(worker.run_claim(&claim).await.unwrap(), WorkerTick::Retry);
    clear_failure(h, "activity_entries").await;
    assert_eq!(snapshot(h, &a, w, t).await, before);
    assert!(worker.claim(w).await.unwrap().is_none());
    let retry = h.app.get_workflow_replacement(&a, w, t, id).await.unwrap();
    assert_eq!(retry.state, "retry_wait");
    assert_eq!(retry.error_code.as_deref(), Some("dependency_unavailable"));
    let mut tx = fixture(h, a.user.id, w).await;
    sqlx::query("UPDATE jobs SET available_at=clock_timestamp()-interval '1 second' WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let retry_claim = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(retry_claim.attempt_count, 2);
    assert_eq!(
        worker.run_claim(&retry_claim).await.unwrap(),
        WorkerTick::Completed
    );
    // Logout does not erase an authorized durable intent. No browser session is read by worker.
    let (_, _, _, _) = job(h, &a, w, 1).await;
    h.app.logout(&a).await.unwrap();
    let claim = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::Completed
    );
    worker.pool.close().await;
    other.pool.close().await;
}
async fn expires_mid_effect(h: &Harness, a: &Actor, w: Uuid, worker: &Worker) {
    let (id, t, _, _) = job(h, a, w, 1).await;
    let claim = worker.claim(w).await.unwrap().unwrap();
    let before = snapshot(h, a, w, t).await;
    let gate = Uuid::new_v4().as_u128() as i64;
    failure(
        h,
        "activity_entries",
        "INSERT",
        &format!("PERFORM pg_advisory_xact_lock({gate})"),
    )
    .await;
    let mut holder = fixture(h, a.user.id, w).await;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(gate)
        .execute(&mut *holder)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let mut tx = fixture(h, a.user.id, w).await;
    sqlx::query("UPDATE jobs SET lease_expires_at=clock_timestamp()+interval '400 milliseconds' WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.unwrap();
    let expiry: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT lease_expires_at FROM jobs WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let runner = worker.clone();
    let attempt = tokio::spawn(async move { runner.run_claim(&claim).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(&h.owner)
            .await
            .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    loop {
        let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>$1")
            .bind(expiry)
            .fetch_one(&h.owner)
            .await
            .unwrap();
        if expired {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    holder.commit().await.unwrap();
    assert_eq!(attempt.await.unwrap().unwrap(), WorkerTick::LostLease);
    clear_failure(h, "activity_entries").await;
    assert_eq!(snapshot(h, a, w, t).await, before);
    let next = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(
        worker.run_claim(&next).await.unwrap(),
        WorkerTick::Completed
    );
}

async fn wait_blocker(pool: &sqlx::PgPool, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let blocked: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(pid) = blocked {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe the actual PostgreSQL waiter before releasing its gate")
}
async fn bounded_failure_bookkeeping(h: &Harness, a: &Actor, w: Uuid, worker: &Worker) {
    let (id, t, _, _) = job(h, a, w, 1).await;
    let claim = worker.claim(w).await.unwrap().unwrap();
    let before = snapshot(h, a, w, t).await;
    let gate = Uuid::new_v4().as_u128() as i64;
    failure(h,"activity_entries","INSERT",&format!("PERFORM pg_advisory_xact_lock({gate}); RAISE EXCEPTION 'forced transient' USING ERRCODE='40001'")).await;
    let mut holder = fixture(h, a.user.id, w).await;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(gate)
        .execute(&mut *holder)
        .await
        .unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let runner = worker.clone();
    let saved = claim.clone();
    let effect = tokio::spawn(async move { runner.run_claim(&saved).await });
    let effect_pid = wait_blocker(&h.app.pool, gate_pid).await;
    let mut queued = fixture(h, a.user.id, w).await;
    let queued_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *queued)
        .await
        .unwrap();
    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let lock = tokio::spawn(async move {
        sqlx::query("SELECT id FROM jobs WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .execute(&mut *queued)
            .await
            .unwrap();
        locked_tx.send(()).unwrap();
        release_rx.await.unwrap();
        queued.rollback().await.unwrap();
    });
    assert_eq!(wait_blocker(&h.app.pool, effect_pid).await, queued_pid);
    // The owner waiter is queued ahead of failure bookkeeping, so it receives
    // the job lock when the injected effect failure rolls back.
    holder.commit().await.unwrap();
    locked_rx.await.unwrap();
    wait_blocker(&h.app.pool, queued_pid).await;
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), effect)
        .await
        .expect("Failure bookkeeping must obey its database lock timeout")
        .unwrap();
    assert!(matches!(result, Err(Error::Unavailable)));
    release_tx.send(()).unwrap();
    lock.await.unwrap();
    clear_failure(h, "activity_entries").await;
    assert_eq!(snapshot(h, a, w, t).await, before);
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::Completed
    );
}
