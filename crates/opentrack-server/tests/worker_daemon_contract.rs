#![cfg(unix)]
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use support::Harness;
use uuid::Uuid;

struct PrivateDirectory(PathBuf);
impl PrivateDirectory {
    fn new() -> Self {
        let path = Path::new("/tmp").join(format!("opentrack-worker-contract-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct OwnedWorker(Child);
impl Drop for OwnedWorker {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

async fn state(h: &Harness, user: Uuid, workspace: Uuid) -> Value {
    let mut tx = h.context(user, workspace).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object(
      'head',h.last_seq,'bytes',h.retained_event_bytes,
      'sum',(SELECT coalesce(sum(storage_bytes),0)::bigint FROM workspace_events WHERE workspace_id=$1),
      'exact',(SELECT coalesce(bool_and(storage_bytes=192+octet_length(event_type)::bigint+octet_length(actor_kind)::bigint),true) FROM workspace_events WHERE workspace_id=$1),
      'checkpoint',(SELECT to_jsonb(c) FROM consumer_checkpoints c WHERE workspace_id=$1 AND consumer_name='assignment_cleanup' AND consumer_version=1),
      'floor',(SELECT purged_through_seq FROM event_retention_state WHERE workspace_id=$1),
      'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY seq),'[]') FROM workspace_events e WHERE workspace_id=$1),
      'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY seq),'[]') FROM consumer_receipts r WHERE workspace_id=$1 AND consumer_name='assignment_cleanup'),
      'jobs',(SELECT coalesce(jsonb_agg(to_jsonb(j) ORDER BY id),'[]') FROM jobs j WHERE workspace_id=$1),
      'letters',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY id),'[]') FROM consumer_dead_letters d WHERE workspace_id=$1),
      'operations',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY id),'[]') FROM consumer_operations o WHERE workspace_id=$1))
      FROM workspace_stream_heads h WHERE h.workspace_id=$1")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}

async fn event(h: &Harness, user: Uuid, workspace: Uuid, schema: i32) -> (Uuid, i64) {
    let mut tx = h.context(user, workspace).await;
    let seq: i64 = sqlx::query_scalar(
        "UPDATE workspace_stream_heads SET last_seq=last_seq+1 WHERE workspace_id=$1 RETURNING last_seq",
    ).bind(workspace).fetch_one(&mut *tx).await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,schema_version,entity_id,entity_version,actor_id,mutation_id,occurred_at) VALUES($1,$2,$3,'access.changed',$4,$1,1,$5,$6,clock_timestamp()-interval '9 days')")
        .bind(workspace).bind(seq).bind(id).bind(schema).bind(user).bind(Uuid::new_v4())
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    (id, seq)
}

async fn planning_observed(
    h: &Harness,
    user: Uuid,
    workspace: Uuid,
    event: Uuid,
) -> (i64, Option<String>) {
    let mut tx = h.context(user, workspace).await;
    let result = sqlx::query_as("SELECT c.last_seq,(SELECT r.state FROM consumer_receipts r WHERE r.workspace_id=$1 AND r.consumer_name='planning_counts' AND r.consumer_version=1 AND r.event_id=$2) FROM consumer_checkpoints c WHERE c.workspace_id=$1 AND c.consumer_name='planning_counts' AND c.consumer_version=1")
        .bind(workspace).bind(event).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daemon_continues_healthy_workspace_work_after_capture_error() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner;
        let directory = PrivateDirectory::new();
        let (session, cookie) = h.app.setup(serde_json::from_value(json!({
            "setup_token":h.setup_token,"email":"worker-daemon@example.test",
            "password":"Worker daemon contract password 624!","display_name":"Example Owner",
            "workspace_name":"Worker daemon contract","team_name":"Engineering","team_key":"ENG"
        })).unwrap()).await.unwrap();
        let actor = h.app.authenticate(&cookie).await.unwrap();
        let user = actor.user.id;
        let organization = h.app.organizations(&actor).await.unwrap()[0].id;
        let second = h.app.create_workspace(&actor, "daemon-second-workspace", serde_json::from_value(json!({
            "organization_id":organization,"name":"Second worker workspace","team_name":"Operations","team_key":"OPS"
        })).unwrap()).await.unwrap();
        let mut workspaces = [session.workspaces[0].id, second.id];
        workspaces.sort();
        let [poisoned, healthy] = workspaces;
        let worker_url = h.worker_url().await;
        let worker = Worker::connect(&worker_url, WorkerOptions {
            max_connections: 1, ..WorkerOptions::default()
        }).await.unwrap();
        for workspace in workspaces {
            assert!(worker.capture(workspace).await.unwrap().backfilled);
            assert!(worker.apply_planning_events(workspace).await.unwrap().backfilled);
            let mut kinds = Vec::new();
            for _ in 0..2 {
                let claim = worker.claim(workspace).await.unwrap().unwrap();
                kinds.push(claim.kind.clone());
                assert_eq!(worker.run_claim(&claim).await.unwrap(), WorkerTick::Completed);
            }
            kinds.sort();
            assert_eq!(kinds, ["assignment.reconcile", "planning.backfill"]);
            assert!(worker.claim(workspace).await.unwrap().is_none());
            let mut tx = h.context(user, workspace).await;
            sqlx::query("UPDATE workspace_events SET occurred_at=clock_timestamp()-interval '9 days' WHERE workspace_id=$1")
                .bind(workspace).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            worker.retain(workspace, 1000).await.unwrap();
            let baseline = state(h, user, workspace).await;
            assert_eq!(baseline["bytes"], 0);
            assert_eq!(baseline["sum"], 0);
            assert_eq!(baseline["exact"], true);
            assert!(baseline["events"].as_array().unwrap().is_empty());
            assert!(baseline["receipts"].as_array().unwrap().is_empty());
        }
        worker.pool.close().await;
        let poison = event(h, user, poisoned, 2).await;
        let healthy_event = event(h, user, healthy, 1).await;
        let event_bytes = 192 + "access.changed".len() as i64 + "account".len() as i64;
        let poisoned_before = state(h, user, poisoned).await;
        assert_eq!(poisoned_before["bytes"], event_bytes);
        assert_eq!(poisoned_before["sum"], event_bytes);
        assert_eq!(poisoned_before["exact"], true);
        assert_eq!(poisoned_before["head"], poison.1);
        assert_eq!(poisoned_before["checkpoint"]["last_seq"], poison.1 - 1);
        assert_eq!(poisoned_before["floor"], poison.1 - 1);
        assert_eq!(poisoned_before["events"][0]["event_id"], poison.0.to_string());
        let healthy_before = state(h, user, healthy).await;
        assert_eq!(healthy_before["bytes"], event_bytes);
        assert_eq!(healthy_before["sum"], event_bytes);
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE FUNCTION test_worker_dead_letter_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$;
             CREATE TRIGGER test_worker_dead_letter_failure BEFORE INSERT ON consumer_dead_letters FOR EACH ROW
             WHEN (NEW.workspace_id='{poisoned}'::uuid) EXECUTE FUNCTION test_worker_dead_letter_failure();"
        ))).execute(&h.owner).await.unwrap();

        let blobs = directory.0.join("blobs");
        fs::create_dir(&blobs).unwrap();
        fs::set_permissions(&blobs, fs::Permissions::from_mode(0o700)).unwrap();
        let log_path = directory.0.join("worker.log");
        let log = fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
            .open(&log_path).unwrap();
        let mut daemon = OwnedWorker(Command::new(env!("CARGO_BIN_EXE_opentrack-server"))
            .env_clear().env("OPENTRACK_ENVIRONMENT", "test")
            .env("OPENTRACK_WORKER_DATABASE_URL", &worker_url)
            .env("OPENTRACK__WORKER__MAX_CONNECTIONS", "1")
            .env("OPENTRACK__STORAGE__LOCAL_ROOT", &blobs).env("RUST_LOG", "warn,opentrack_server::worker=debug")
            .current_dir(&directory.0).args(["worker", "--worker-poll-interval-ms", "100"])
            .stdin(Stdio::null()).stdout(log.try_clone().unwrap()).stderr(log)
            .spawn().expect("Start the owned restricted worker"));
        let deadline = Instant::now() + Duration::from_secs(20);
        let healthy_after = loop {
            assert!(daemon.0.try_wait().unwrap().is_none(), "Worker exited before proving progress");
            assert!(fs::metadata(&log_path).unwrap().len() <= 65536, "Worker output exceeded its bound");
            let current = state(h, user, healthy).await;
            let output = fs::read_to_string(&log_path).unwrap();
            let mut stage_failed = false;
            let mut worked_job = false;
            let mut completed_idle = false;
            for entry in output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
                if entry["target"] != "opentrack_server::worker" { continue; }
                let fields = &entry["fields"];
                stage_failed |= entry["level"] == "WARN"
                    && fields["message"] == "Worker stage failed; durable state remains recoverable";
                worked_job |= entry["level"] == "DEBUG" && fields["message"] == "Worker stage completed"
                    && fields["stage"] == "Job" && fields["worked"] == true;
                completed_idle |= entry["level"] == "DEBUG" && fields["message"] == "Worker idle wait completed";
            }
            if stage_failed && worked_job && completed_idle && current["checkpoint"]["last_seq"] == healthy_event.1
                && current["floor"] == healthy_event.1 && current["bytes"] == 0
                && current["jobs"].as_array().unwrap().len() == 3
                && current["jobs"].as_array().unwrap().iter().all(|job| job["state"] == "succeeded")
                && planning_observed(h, user, poisoned, poison.0).await == (poison.1, Some("ignored".into()))
                && planning_observed(h, user, healthy, healthy_event.0).await == (healthy_event.1, Some("ignored".into())) {
                break current;
            }
            assert!(Instant::now() < deadline, "Daemon did not isolate capture failure and finish healthy work");
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        assert_eq!(healthy_after["head"], healthy_event.1);
        assert_eq!(healthy_after["sum"], 0);
        assert_eq!(healthy_after["exact"], true);
        assert!(healthy_after["events"].as_array().unwrap().is_empty());
        assert!(healthy_after["letters"].as_array().unwrap().is_empty());
        let receipts = healthy_after["receipts"].as_array().unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["event_id"], healthy_event.0.to_string());
        assert_eq!(receipts[0]["seq"], healthy_event.1);
        assert_eq!(receipts[0]["state"], "captured");
        let captured = healthy_after["jobs"].as_array().unwrap().iter()
            .find(|job| job["id"] == receipts[0]["job_id"]).unwrap();
        assert_eq!(captured["kind"], "assignment.reconcile");
        assert!(captured["requested_by"].is_null());
        let operations = healthy_after["operations"].as_array().unwrap();
        assert_eq!(operations.len(), healthy_before["operations"].as_array().unwrap().len() + 1);
        let retained = operations.iter().find(|operation| operation["through_seq"] == healthy_event.1).unwrap();
        assert_eq!(retained["action"], "events.retained");
        assert_eq!(retained["affected_count"], 1);
        assert_eq!(state(h, user, poisoned).await, poisoned_before);
        assert!(daemon.0.try_wait().unwrap().is_none(), "Worker must remain alive after the failed stage");
        let output = fs::read_to_string(&log_path).unwrap();
        assert!(!output.contains(&worker_url), "Worker output exposed its connection URL");
        if let Some(password) = url::Url::parse(&worker_url).unwrap().password() {
            assert!(!output.contains(password), "Worker output exposed its credential");
        }
        assert!(!output.contains(directory.0.to_str().unwrap()), "Worker output exposed its private directory");
        assert!(!output.contains(&poisoned.to_string()), "Stage error exposed its workspace");
        let stages = ["Capture", "Planning", "Job", "Retention", "DocumentRetention", "AttachmentGc"];
        let mut seen_stages = [false; 6];
        let mut startup = false;
        let mut failed_capture = false;
        let mut worked_job = false;
        let mut idle_pending = false;
        let mut completed_idle = false;
        for entry in output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
            if entry["target"] != "opentrack_server::worker" {
                continue;
            }
            let Some(fields) = entry["fields"].as_object() else { continue };
            let Some(message) = fields.get("message").and_then(Value::as_str) else { continue };
            let (allowed, level): (&[&str], &str) = match message {
                "Worker started with isolated database authority" =>
                    (&["message", "poll_interval_ms", "max_connections", "lease_seconds"], "INFO"),
                "Worker stage completed" =>
                    (&["message", "stage", "elapsed_ms", "worked", "failed"], "DEBUG"),
                "Worker idle wait started" => (&["message", "poll_interval_ms"], "DEBUG"),
                "Worker idle wait completed" => (&["message", "elapsed_ms"], "DEBUG"),
                _ => continue,
            };
            assert!(fields.len() == allowed.len() && fields.keys().all(|key| allowed.contains(&key.as_str())),
                    "Worker diagnostic payload contains missing or unexpected fields");
            assert!(entry["level"] == level, "Worker diagnostic used an unexpected log level");
            if let Some(duration) = fields.get("elapsed_ms") {
                assert!(duration.as_f64().is_some_and(|value| value.is_finite() && value >= 0.0),
                        "Worker diagnostic duration must be finite and nonnegative");
            }
            match message {
                "Worker started with isolated database authority" => {
                    assert!(fields["poll_interval_ms"] == 100 && fields["max_connections"] == 1
                            && fields["lease_seconds"] == 60, "Startup diagnostics must match the actual worker profile");
                    startup = true;
                }
                "Worker stage completed" => {
                    let stage = fields["stage"].as_str().expect("Worker stage must be a string");
                    let index = stages.iter().position(|expected| *expected == stage).expect("Unknown worker stage");
                    seen_stages[index] = true;
                    let worked = fields["worked"].as_bool().expect("Worked diagnostic must be a boolean");
                    let failed = fields["failed"].as_bool().expect("Failed diagnostic must be a boolean");
                    failed_capture |= stage == "Capture" && failed;
                    worked_job |= stage == "Job" && worked;
                }
                "Worker idle wait started" => {
                    assert!(fields["poll_interval_ms"] == 100, "Idle diagnostic must match the actual interval");
                    assert!(!idle_pending, "Idle diagnostics must alternate start and completion");
                    idle_pending = true;
                }
                "Worker idle wait completed" => {
                    assert!(idle_pending, "Completed idle diagnostic requires an actual preceding wait");
                    idle_pending = false;
                    completed_idle = true;
                }
                _ => unreachable!(),
            }
        }
        assert!(startup && seen_stages.into_iter().all(|seen| seen) && failed_capture && worked_job && completed_idle,
                "Actual worker diagnostics must cover startup, all stages, capture failure, job progress and completed idle");

        // The PID belongs to this unreaped child and cannot have been reused.
        assert_eq!(unsafe { libc::kill(daemon.0.id() as libc::pid_t, libc::SIGTERM) }, 0);
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            if let Some(status) = daemon.0.try_wait().unwrap() {
                assert!(status.success(), "Owned worker did not shut down cleanly");
                break;
            }
            assert!(Instant::now() < deadline, "Owned worker exceeded its graceful shutdown bound");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

type DiscoveryCall = (i64, i64, Option<Uuid>, i32);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_daemon_dispatches_issue_order_normalization() {
    let h = Harness::with_connections(6).await;
    let directory = std::sync::Arc::new(PrivateDirectory::new());
    let runner = h.clone();
    let location = directory.0.clone();
    let temporary = directory.clone();
    let result=tokio::spawn(async move {
        let h=&runner;
        let (session,cookie)=h.app.setup(serde_json::from_value(json!({
            "setup_token":h.setup_token,"email":"order-daemon@example.test",
            "password":"Synthetic ordering daemon password 2026!","display_name":"Example Owner",
            "workspace_name":"Ordering daemon","team_name":"Engineering","team_key":"ORD"
        })).unwrap()).await.unwrap();
        let actor=h.app.authenticate(&cookie).await.unwrap();
        let w=session.workspaces[0].id;
        let bootstrap=h.app.bootstrap(&actor,w).await.unwrap();
        let t=bootstrap.teams[0].id;
        let status=bootstrap.statuses.iter().find(|s|s.team_id==t&&s.is_default).unwrap().id;
        let mut tx=h.context(actor.user.id,w).await;
        sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank) SELECT gen_random_uuid(),$1,$2,$3,n,'Default daemon ordering',$4,CASE WHEN n=501 THEN 99999999999999999999::numeric ELSE n::numeric*1024 END FROM generate_series(1,501)n")
            .bind(w).bind(t).bind(actor.user.id).bind(status).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE teams SET next_issue_number=502 WHERE workspace_id=$1 AND id=$2")
            .bind(w).bind(t).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let request=serde_json::from_value(json!({"team_id":t,"status_id":status,"title":"Explicit retry after maintenance"})).unwrap();
        let operation=match h.app.create_issue(&actor,w,"default-order-create",request).await.unwrap_err() {
            opentrack_domain::Error::IssueOrderMaintenance(mut items) => { assert_eq!(items.len(),1);items.remove(0) },
            _ => panic!("Exhaustion must enqueue typed maintenance"),
        };
        let worker_url=h.worker_url().await;
        let (mut daemon,log)=start_discovery_worker(&temporary,&worker_url);
        let started=Instant::now();
        loop {
            let progress=h.app.issue_order_maintenance(&actor,w,t,operation.id).await.unwrap();
            if progress.state=="succeeded" {
                assert_eq!(progress.staged_count,"501");
                assert_eq!(progress.applied_count,"501");
                assert_eq!(progress.total_count.as_deref(),Some("501"));
                break;
            }
            assert!(matches!(progress.state.as_str(),"queued"|"running"),"Default daemon blocked normalization");
            assert!(started.elapsed()<Duration::from_secs(30),"Default daemon exceeded its dispatch completion bound");
            assert!(daemon.0.try_wait().unwrap().is_none(),"Owned daemon exited before completing normalization");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        stop_discovery_worker(&mut daemon).await;
        let output=fs::read_to_string(log).unwrap();
        assert!(output.len()<=1_048_576);
        assert!(!output.contains(&worker_url)&&!output.contains(worker_url.split('@').next().unwrap()));
        assert!(!output.contains(location.to_str().unwrap()));
        let startup=output.lines().filter_map(|line|serde_json::from_str::<Value>(line).ok())
            .find(|entry|entry["fields"]["message"]=="Worker started with isolated database authority").expect("Actual daemon profile diagnostic");
        assert_eq!(startup["fields"]["poll_interval_ms"],2000);
        assert_eq!(startup["fields"]["max_connections"],5);
        assert_eq!(startup["fields"]["lease_seconds"],60);
        let issue=h.app.create_issue(&actor,w,"default-order-create",serde_json::from_value(json!({"team_id":t,"status_id":status,"title":"Explicit retry after maintenance"})).unwrap()).await.unwrap();
        assert_eq!(issue.key,"ORD-502");
        let mut tx=h.context(actor.user.id,w).await;
        let result:(i64,i64,bool)=sqlx::query_as("SELECT count(*),count(*) FILTER(WHERE version=1),bool_and(rank=number::numeric*1024) FROM issues WHERE workspace_id=$1 AND team_id=$2")
            .bind(w).bind(t).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(result,(502,502,true));
        tx.commit().await.unwrap();
    }).await;
    if let Err(error) = result {
        std::mem::forget(directory);
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}

async fn measure_discovery(h: &Harness, worker_url: &str) -> (String, Value) {
    sqlx::raw_sql(
        "CREATE TABLE test_worker_selector_calls(
      id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      observed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
      after_workspace uuid,page_limit integer NOT NULL);
      REVOKE ALL ON test_worker_selector_calls FROM PUBLIC;
      REVOKE ALL ON SEQUENCE test_worker_selector_calls_id_seq FROM PUBLIC;",
    )
    .execute(&h.owner)
    .await
    .unwrap();
    let runtime: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    let worker = url::Url::parse(worker_url).unwrap();
    for role in [runtime.as_str(), worker.username()] {
        let role = role.replace('"', "\"\"");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "REVOKE ALL ON test_worker_selector_calls FROM \"{role}\";
             REVOKE ALL ON SEQUENCE test_worker_selector_calls_id_seq FROM \"{role}\";"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
    }
    let private: bool = sqlx::query_scalar("SELECT NOT EXISTS(
      SELECT 1 FROM pg_class c CROSS JOIN LATERAL aclexplode(c.relacl) a
      WHERE c.oid IN('test_worker_selector_calls'::regclass,'test_worker_selector_calls_id_seq'::regclass)
      AND a.grantee<>c.relowner)")
        .fetch_one(&h.owner).await.unwrap();
    assert!(private, "Selector measurements must remain owner-only");
    let (definition, body, attributes): (String, String, Value) = sqlx::query_as(
        "SELECT pg_get_functiondef(p.oid),prosrc,to_jsonb(p)-'prosrc'
         FROM pg_proc p WHERE oid='worker_ready_workspaces(uuid,integer)'::regprocedure",
    )
    .fetch_one(&h.owner)
    .await
    .unwrap();
    assert_eq!(definition.matches(&body).count(), 1);
    let measured = definition.replacen(
        &body,
        &format!(
        "WITH measurement AS (INSERT INTO test_worker_selector_calls(after_workspace,page_limit)
         VALUES($1,$2) RETURNING id)\n{body}"
    ),
        1,
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(measured))
        .execute(&h.owner)
        .await
        .unwrap();
    let current: Value = sqlx::query_scalar(
        "SELECT to_jsonb(p)-'prosrc' FROM pg_proc p WHERE oid='worker_ready_workspaces(uuid,integer)'::regprocedure",
    ).fetch_one(&h.owner).await.unwrap();
    assert!(
        attributes == current,
        "Only the isolated selector body may change"
    );
    (definition, attributes)
}

async fn restore_discovery(h: &Harness, original: &(String, Value)) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(original.0.clone()))
        .execute(&h.owner)
        .await
        .unwrap();
    let (definition, attributes): (String, Value) = sqlx::query_as(
        "SELECT pg_get_functiondef(p.oid),to_jsonb(p)-'prosrc'
         FROM pg_proc p WHERE oid='worker_ready_workspaces(uuid,integer)'::regprocedure",
    )
    .fetch_one(&h.owner)
    .await
    .unwrap();
    assert!(
        definition == original.0 && attributes == original.1,
        "Selector definition, authority, search path and ACL must be restored exactly"
    );
}

async fn discovery_calls(h: &Harness) -> Vec<DiscoveryCall> {
    sqlx::query_as(
        "SELECT id,(extract(epoch FROM observed_at)*1000000)::bigint,
      after_workspace,page_limit FROM test_worker_selector_calls ORDER BY id",
    )
    .fetch_all(&h.owner)
    .await
    .unwrap()
}

async fn wait_discovery_calls(
    h: &Harness,
    daemon: &mut OwnedWorker,
    expected: usize,
) -> Vec<DiscoveryCall> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "Owned worker exited during discovery measurement"
        );
        let calls: Vec<_> = discovery_calls(h)
            .await
            .into_iter()
            .filter(|call| call.3 == 20)
            .collect();
        if calls.len() >= expected {
            return calls;
        }
        assert!(
            Instant::now() < deadline,
            "Worker did not reach the bounded discovery observation"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn start_discovery_worker(
    directory: &PrivateDirectory,
    worker_url: &str,
) -> (OwnedWorker, PathBuf) {
    let blobs = directory.0.join("blobs");
    fs::create_dir(&blobs).unwrap();
    fs::set_permissions(&blobs, fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.0.join("worker.log");
    let log = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_opentrack-server"))
        .env_clear()
        .env("OPENTRACK_ENVIRONMENT", "test")
        .env("OPENTRACK_WORKER_DATABASE_URL", worker_url)
        .env("OPENTRACK__STORAGE__LOCAL_ROOT", blobs)
        .env("RUST_LOG", "warn,opentrack_server::worker=debug")
        .current_dir(&directory.0)
        .arg("worker")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("Start owned discovery worker");
    (OwnedWorker(child), path)
}

async fn stop_discovery_worker(daemon: &mut OwnedWorker) {
    assert!(
        daemon.0.try_wait().unwrap().is_none(),
        "Owned worker must remain alive"
    );
    // This PID belongs to the unreaped owned child.
    assert_eq!(
        unsafe { libc::kill(daemon.0.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        if let Some(status) = daemon.0.try_wait().unwrap() {
            assert!(
                status.success(),
                "Owned discovery worker did not shut down cleanly"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Owned discovery worker exceeded its shutdown bound"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn discovery_receipt(output: &str, calls: &[DiscoveryCall]) -> Value {
    let names = [
        "Capture",
        "Planning",
        "Job",
        "Retention",
        "DocumentRetention",
        "AttachmentGc",
    ];
    let mut completions = [0_u64; 6];
    let mut worked = [0_u64; 6];
    let mut failed = [0_u64; 6];
    let mut profile = None;
    for entry in output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
    {
        if entry["target"] != "opentrack_server::worker" {
            continue;
        }
        if entry["fields"]["message"] == "Worker started with isolated database authority" {
            let fields = entry["fields"]
                .as_object()
                .expect("Startup payload must be an object");
            let allowed = [
                "message",
                "poll_interval_ms",
                "max_connections",
                "lease_seconds",
            ];
            assert!(
                profile.is_none()
                    && entry["level"] == "INFO"
                    && fields.len() == allowed.len()
                    && fields.keys().all(|key| allowed.contains(&key.as_str()))
            );
            assert!(
                fields["poll_interval_ms"] == 2000
                    && fields["max_connections"] == 5
                    && fields["lease_seconds"] == 60,
                "Actual startup must retain the default worker profile"
            );
            profile = Some(json!({"poll_interval_ms":fields["poll_interval_ms"],
                "max_connections":fields["max_connections"],"lease_seconds":fields["lease_seconds"]}));
            continue;
        }
        if entry["fields"]["message"] != "Worker stage completed" {
            continue;
        }
        let stage = entry["fields"]["stage"]
            .as_str()
            .expect("Stage label must be a string");
        let index = names
            .iter()
            .position(|name| *name == stage)
            .expect("Unknown worker stage");
        completions[index] += 1;
        worked[index] += u64::from(entry["fields"]["worked"] == true);
        failed[index] += u64::from(entry["fields"]["failed"] == true);
    }
    let mut queries = [0_u64; 6];
    let mut maintenance = 0;
    for call in calls {
        match call.3 {
            20 => queries[2] += 1,
            1 => {
                queries[[0, 1, 3, 4, 5][maintenance % 5]] += 1;
                maintenance += 1;
            }
            _ => panic!("Unexpected selector page limit"),
        }
    }
    let mut stages = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        assert!(completions[index] > 0, "Every stage must execute");
        if index != 2 {
            assert!(
                queries[index] >= completions[index] && queries[index] <= completions[index] + 1,
                "Maintenance query/completion counts must agree, allowing shutdown's in-flight stage"
            );
        }
        stages.insert(
            (*name).into(),
            json!({"completed_calls":completions[index],
            "selector_calls":queries[index],"worked":worked[index],"failed":failed[index]}),
        );
    }
    json!({"stages":stages,"selector_calls":calls.iter().map(|call|
        json!({"ordinal":call.0,"db_epoch_us":call.1,"cursor_present":call.2.is_some(),"page_limit":call.3})
    ).collect::<Vec<_>>(),"profile":profile.expect("Actual owned startup diagnostics must be present"),
        "clock_tolerance_us":100000,"query_stage_mapping":"One owned child's fixed rotation; page20 is Job, page1 follows Capture/Planning/Retention/DocumentRetention/AttachmentGc. Final in-flight completion may be absent.",
        "limits":"Instrumented selector calls are not CPU measurements. Database wall-clock gaps allow 100ms clock/observation tolerance; no CPU percentage, throughput or p95 is inferred."})
}

fn discovery_output(path: &Path, directory: &PrivateDirectory, worker_url: &str) -> String {
    assert!(
        fs::metadata(path).unwrap().len() <= 1048576,
        "Private discovery log exceeded its bound"
    );
    let output = fs::read_to_string(path).unwrap();
    let parsed = url::Url::parse(worker_url).unwrap();
    for private in [Some(worker_url), parsed.password(), directory.0.to_str()]
        .into_iter()
        .flatten()
    {
        assert!(
            !output.contains(private),
            "Private credentials and directory must not appear in logs"
        );
    }
    output
}

async fn discovery_fixture(
    h: &Harness,
    count: usize,
) -> (opentrack_domain::Actor, Vec<Uuid>, String) {
    let (session, cookie) = h
        .app
        .setup(
            serde_json::from_value(json!({
                "setup_token":h.setup_token,"email":"discovery-daemon@example.test",
                "password":"Discovery daemon contract password 421!","display_name":"Example Owner",
                "workspace_name":"Discovery daemon","team_name":"Engineering","team_key":"ENG"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let organization = h.app.organizations(&actor).await.unwrap()[0].id;
    let mut scopes = vec![session.workspaces[0].id];
    while scopes.len() < count {
        let index = scopes.len();
        scopes.push(h.app.create_workspace(&actor, &format!("discovery-scope-{index}"),
            serde_json::from_value(json!({"organization_id":organization,
                "name":format!("Discovery scope {index}"),"team_name":"Engineering","team_key":"ENG"})).unwrap()
        ).await.unwrap().id);
    }
    scopes.sort();
    let worker_url = h.worker_url().await;
    let worker = Worker::connect(&worker_url, WorkerOptions::default())
        .await
        .unwrap();
    for scope in &scopes {
        assert!(worker.capture(*scope).await.unwrap().backfilled);
        assert!(
            worker
                .apply_planning_events(*scope)
                .await
                .unwrap()
                .backfilled
        );
        let mut kinds = Vec::new();
        for _ in 0..2 {
            let claim = worker.claim(*scope).await.unwrap().unwrap();
            kinds.push(claim.kind.clone());
            assert_eq!(
                worker.run_claim(&claim).await.unwrap(),
                WorkerTick::Completed
            );
        }
        kinds.sort();
        assert_eq!(kinds, ["assignment.reconcile", "planning.backfill"]);
        assert!(worker.claim(*scope).await.unwrap().is_none());
        assert_eq!(worker.capture(*scope).await.unwrap().processed, 0);
    }
    worker.pool.close().await;
    (actor, scopes, worker_url)
}

async fn discovery_issue(
    h: &Harness,
    actor: &opentrack_domain::Actor,
    scope: Uuid,
) -> opentrack_domain::Issue {
    let team = h.app.bootstrap(actor, scope).await.unwrap().teams[0].id;
    h.app
        .create_issue(
            actor,
            scope,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(json!({"team_id":team,"title":"Discovery scheduling contract"}))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn discovery_enqueue(
    h: &Harness,
    actor: &opentrack_domain::Actor,
    scope: Uuid,
    issue: &opentrack_domain::Issue,
) -> opentrack_domain::WorkflowReplacement {
    let flow = h.app.workflow(actor, scope, issue.team_id).await.unwrap();
    let target = flow
        .statuses
        .iter()
        .find(|status| status.id != issue.status_id)
        .unwrap()
        .id;
    h.app
        .replace_status(
            actor,
            scope,
            issue.team_id,
            issue.status_id,
            &Uuid::new_v4().to_string(),
            &flow.etag(),
            opentrack_domain::ReplaceStatus {
                replacement_status_id: target,
            },
        )
        .await
        .unwrap()
}

async fn discovery_job(
    h: &Harness,
    actor: &opentrack_domain::Actor,
    scope: Uuid,
    job: &opentrack_domain::WorkflowReplacement,
) -> (String, i32) {
    let mut tx = h.context(actor.user.id, scope).await;
    let result =
        sqlx::query_as("SELECT state,attempt_count FROM jobs WHERE workspace_id=$1 AND id=$2")
            .bind(scope)
            .bind(job.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    result
}

async fn discovery_completed(
    h: &Harness,
    actor: &opentrack_domain::Actor,
    scope: Uuid,
    issue: &opentrack_domain::Issue,
    job: &opentrack_domain::WorkflowReplacement,
) {
    assert_eq!(
        discovery_job(h, actor, scope, job).await,
        ("succeeded".into(), 1)
    );
    let saved = h.app.get_issue(actor, scope, issue.id).await.unwrap();
    assert_eq!(saved.status_id, job.replacement_status_id);
    assert_eq!(
        saved.version.parse::<u64>().unwrap(),
        issue.version.parse::<u64>().unwrap() + 1
    );
    let current = h
        .app
        .get_workflow_replacement(actor, scope, issue.team_id, job.id)
        .await
        .unwrap();
    assert_eq!(
        (
            current.processed_count.as_str(),
            current.remaining_count.as_str()
        ),
        ("1", "0")
    );
}

fn assert_discovery_gap(first: &DiscoveryCall, next: &DiscoveryCall) {
    assert!(
        next.1 - first.1 >= 1_900_000,
        "Actual Job selector calls must respect the default cooldown within the declared wall-clock tolerance"
    );
}

fn assert_discovery_continues(first: &DiscoveryCall, next: &DiscoveryCall) {
    assert!(
        next.1 - first.1 < 1_900_000,
        "Cursor wrap must continue within the remaining four-call discovery budget"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exhausted_empty_discovery_waits_before_calling_the_selector_again() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let directory = PrivateDirectory::new();
        let url = runner.worker_url().await;
        let original = measure_discovery(&runner, &url).await;
        let (mut daemon, path) = start_discovery_worker(&directory, &url);
        let calls = wait_discovery_calls(&runner, &mut daemon, 3).await;
        assert!(calls[..3].iter().all(|call| call.2.is_none()));
        assert_discovery_gap(&calls[0], &calls[1]);
        assert_discovery_gap(&calls[1], &calls[2]);
        stop_discovery_worker(&mut daemon).await;
        let output = discovery_output(&path, &directory, &url);
        println!(
            "{}",
            json!({"empty_discovery":discovery_receipt(&output, &discovery_calls(&runner).await)})
        );
        restore_discovery(&runner, &original).await;
    })
    .await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn discovery_cooldown_preserves_cursor_despite_capture_progress_and_errors() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner;
        let directory = PrivateDirectory::new();
        let (actor, scopes, url) = discovery_fixture(h, 81).await;
        let mut issues = Vec::new();
        for scope in &scopes { issues.push(discovery_issue(h, &actor, *scope).await); }
        let poison_event = event(h, actor.user.id, scopes[1], 2).await;
        let poison = state(h, actor.user.id, scopes[1]).await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE FUNCTION test_discovery_capture_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$;
             CREATE TRIGGER test_discovery_capture_failure BEFORE INSERT ON consumer_dead_letters FOR EACH ROW
             WHEN(NEW.workspace_id='{}'::uuid) EXECUTE FUNCTION test_discovery_capture_failure();", scopes[1]
        ))).execute(&h.owner).await.unwrap();
        let original = measure_discovery(h, &url).await;
        let (mut daemon, path) = start_discovery_worker(&directory, &url);
        let first = wait_discovery_calls(h, &mut daemon, 4).await;
        assert_eq!(first.len(), 4, "Observe the first cooldown before submitting normal work");
        assert_eq!(first.iter().map(|call| call.2).collect::<Vec<_>>(),
                   vec![None, Some(scopes[19]), Some(scopes[39]), Some(scopes[59])]);
        let completion_deadline = Instant::now() + Duration::from_secs(30);
        let ahead = discovery_enqueue(h, &actor, scopes[80], &issues[80]).await;
        let behind = discovery_enqueue(h, &actor, scopes[0], &issues[0]).await;
        assert_eq!(discovery_calls(h).await.iter().filter(|call| call.3 == 20).count(), 4,
                   "Both jobs must commit during the observed cooldown");
        let calls = wait_discovery_calls(h, &mut daemon, 7).await;
        assert_eq!(calls[4].2, Some(scopes[79]));
        assert_eq!(calls[5].2, Some(scopes[80]));
        assert_eq!(calls[6].2, None);
        assert_discovery_gap(&calls[3], &calls[4]);
        assert_discovery_continues(&calls[5], &calls[6]);
        loop {
            if discovery_job(h, &actor, scopes[80], &ahead).await.0 == "succeeded"
                && discovery_job(h, &actor, scopes[0], &behind).await.0 == "succeeded" { break; }
            assert!(Instant::now() < completion_deadline, "Normal workflow work exceeded its unchanged completion gate");
            assert!(daemon.0.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(Instant::now() < completion_deadline, "Both normal jobs must meet the unchanged 30-second gate");
        discovery_completed(h, &actor, scopes[80], &issues[80], &ahead).await;
        discovery_completed(h, &actor, scopes[0], &issues[0], &behind).await;
        assert!(state(h, actor.user.id, scopes[1]).await == poison,
                "Capture failure must preserve assignment and shared event state");
        assert_eq!(planning_observed(h, actor.user.id, scopes[1], poison_event.0).await,
                   (poison_event.1, Some("ignored".into())),
                   "The independent planning consumer must still advance on this unrelated event");
        stop_discovery_worker(&mut daemon).await;
        let output = discovery_output(&path, &directory, &url);
        let receipt = discovery_receipt(&output, &discovery_calls(h).await);
        assert!(receipt["stages"]["Capture"]["worked"].as_u64().unwrap() > 0);
        assert!(receipt["stages"]["Capture"]["failed"].as_u64().unwrap() > 0);
        println!("{}", json!({"busy_error_discovery":receipt,
            "budget_boundary":"The first four calls have no handled claims. Later normal workflow/maintenance claims may legitimately reset credits."}));
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn discovery_registry(h: &Harness, user: Uuid, scopes: &[Uuid], arrive: bool) {
    let identifiers = scopes
        .iter()
        .map(|scope| format!("'{scope}'::uuid"))
        .collect::<Vec<_>>()
        .join(",");
    let operation = if arrive {
        "INSERT INTO worker_workspace_registry(workspace_id) VALUES(w)"
    } else {
        "DELETE FROM worker_workspace_registry WHERE workspace_id=w"
    };
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DO $$ DECLARE w uuid; BEGIN
         PERFORM set_config('opentrack.user_id','{user}',true);
         FOREACH w IN ARRAY ARRAY[{identifiers}] LOOP
           PERFORM set_config('opentrack.workspace_id',w::text,true);
           {operation};
         END LOOP; END $$;"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finite_ahead_registry_arrivals_remain_budgeted_then_allow_behind_work() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner;
        let directory = PrivateDirectory::new();
        let (actor, scopes, url) = discovery_fixture(h, 241).await;
        let issue = discovery_issue(h, &actor, scopes[0]).await;
        let fixture_worker = Worker::connect(&url, WorkerOptions::default()).await.unwrap();
        assert!(fixture_worker.capture(scopes[0]).await.unwrap().processed > 0);
        assert!(fixture_worker.claim(scopes[0]).await.unwrap().is_none());
        fixture_worker.pool.close().await;
        discovery_registry(h, actor.user.id, &scopes[81..], false).await;
        let original = measure_discovery(h, &url).await;
        let (mut daemon, path) = start_discovery_worker(&directory, &url);
        let first = wait_discovery_calls(h, &mut daemon, 4).await;
        assert_eq!(first.len(), 4);
        let initial_output = discovery_output(&path, &directory, &url);
        let initial_idles = initial_output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|entry| entry["target"] == "opentrack_server::worker"
                && entry["fields"]["message"] == "Worker idle wait started").count();
        assert!(initial_idles <= 1, "Nonempty pages before burst exhaustion must not each wait a full interval");
        let completion_deadline = Instant::now() + Duration::from_secs(30);
        let behind = discovery_enqueue(h, &actor, scopes[0], &issue).await;
        for (completed, arrivals) in [(4, &scopes[81..161]), (8, &scopes[161..241])] {
            let calls = wait_discovery_calls(h, &mut daemon, completed).await;
            assert_eq!(calls.len(), completed, "Introduce reserved scopes during the measured cooldown");
            assert_eq!(calls[completed - 1].2, Some(scopes[completed * 20 - 21]));
            discovery_registry(h, actor.user.id, arrivals, true).await;
            assert_eq!(discovery_calls(h).await.iter().filter(|call| call.3 == 20).count(), completed,
                       "Finite registry arrivals must commit before the next discovery burst");
        }
        let calls = wait_discovery_calls(h, &mut daemon, 15).await;
        for burst in [4, 8, 12] { assert_discovery_gap(&calls[burst - 1], &calls[burst]); }
        assert_discovery_continues(&calls[13], &calls[14]);
        let expected = [None, Some(19), Some(39), Some(59), Some(79), Some(99), Some(119),
            Some(139), Some(159), Some(179), Some(199), Some(219), Some(239), Some(240), None];
        for (call, expected) in calls.iter().zip(expected) {
            assert_eq!(call.2, expected.map(|index| scopes[index]));
        }
        loop {
            if discovery_job(h, &actor, scopes[0], &behind).await.0 == "succeeded" { break; }
            assert!(Instant::now() < completion_deadline, "Behind-cursor work must finish after finite arrivals stop");
            assert!(daemon.0.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(Instant::now() < completion_deadline, "Behind-cursor work must meet its 30-second gate");
        discovery_completed(h, &actor, scopes[0], &issue, &behind).await;
        stop_discovery_worker(&mut daemon).await;
        let output = discovery_output(&path, &directory, &url);
        println!("{}", json!({"finite_registry_arrivals":discovery_receipt(&output, &discovery_calls(h).await),
            "fixture_boundary":"241 normal App workspaces were drained first. Higher-ID registry rows alone were withheld and introduced in two fixed 80-scope arrivals. Jobs, checkpoints, leases and authority were not rewritten.",
            "liveness_boundary":"This finite synthetic registry-arrival contract does not establish behind-cursor service under indefinite ahead growth or normal workspace-creation performance."}));
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn await_discovery_exit(daemon: &mut OwnedWorker, deadline: Instant) {
    loop {
        if let Some(status) = daemon.0.try_wait().unwrap() {
            assert!(
                status.success(),
                "Owned lifecycle worker did not exit cleanly"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Owned lifecycle worker exceeded its bounded exit allowance"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn start_discovery_lifecycle_worker(
    directory: &PrivateDirectory,
    worker_url: &str,
    once: bool,
) -> (OwnedWorker, PathBuf) {
    let blobs = directory.0.join("blobs");
    fs::create_dir(&blobs).unwrap();
    fs::set_permissions(&blobs, fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.0.join("worker.log");
    let log = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_opentrack-server"));
    command
        .env_clear()
        .env("OPENTRACK_ENVIRONMENT", "test")
        .env("OPENTRACK_WORKER_DATABASE_URL", worker_url)
        .env("OPENTRACK__STORAGE__LOCAL_ROOT", blobs)
        .env(
            "RUST_LOG",
            "warn,opentrack_server=info,opentrack_server::worker=debug",
        )
        .current_dir(&directory.0)
        .arg("worker")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    if once {
        command.arg("--once");
    }
    (
        OwnedWorker(command.spawn().expect("Start owned lifecycle worker")),
        path,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn short_and_full_fourth_discovery_pages_keep_the_cooldown_then_resume_through_wrap() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner;
        let (actor, scopes, url) = discovery_fixture(h, 80).await;
        discovery_registry(h, actor.user.id, &scopes[79..], false).await;
        let original = measure_discovery(h, &url).await;
        for count in [79, 80] {
            if count == 80 { discovery_registry(h, actor.user.id, &scopes[79..], true).await; }
            sqlx::query("TRUNCATE test_worker_selector_calls RESTART IDENTITY").execute(&h.owner).await.unwrap();
            let directory = PrivateDirectory::new();
            let (mut daemon, path) = start_discovery_worker(&directory, &url);
            let calls = wait_discovery_calls(h, &mut daemon, 6).await;
            let expected = [None, Some(scopes[19]), Some(scopes[39]), Some(scopes[59]),
                Some(scopes[count - 1]), None];
            for (call, expected) in calls.iter().zip(expected) { assert_eq!(call.2, expected); }
            assert_discovery_gap(&calls[3], &calls[4]);
            assert_discovery_continues(&calls[4], &calls[5]);
            stop_discovery_worker(&mut daemon).await;
            let output = discovery_output(&path, &directory, &url);
            println!("{}", json!({"fourth_page_boundary":{"registered_scopes":count,
                "measurements":discovery_receipt(&output, &discovery_calls(h).await),
                "boundary":"A short or full fourth page consumes the burst; the later empty wrap consumes credit within the next burst."}}));
        }
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_runs_exactly_one_capture_planning_job_retention_document_and_gc_poll() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let directory = PrivateDirectory::new();
        let url = runner.worker_url().await;
        let original = measure_discovery(&runner, &url).await;
        let (mut daemon, path) = start_discovery_lifecycle_worker(&directory, &url, true);
        await_discovery_exit(&mut daemon, Instant::now() + Duration::from_secs(35)).await;
        let calls = discovery_calls(&runner).await;
        assert_eq!(calls.iter().map(|call| call.3).collect::<Vec<_>>(), vec![1, 1, 20, 1, 1, 1]);
        assert!(calls.iter().all(|call| call.2.is_none()));
        let output = discovery_output(&path, &directory, &url);
        assert!(output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|entry| entry["target"] == "opentrack_server::worker"
                && entry["fields"]["message"] == "Worker completed one bounded event, job and retention poll"),
            "The real once command must report successful bounded completion");
        println!("{}", json!({"once_discovery":{"selector_limits":calls.iter().map(|call| call.3).collect::<Vec<_>>(),
            "job_selector_calls":1,"maintenance_selector_calls":5,"clean_exit":true,
            "boundary":"One real --once pass on an empty registry; no worker configuration override or second traversal."}}));
        restore_discovery(&runner, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_drains_the_owned_inflight_job_selector_without_starting_later_stages() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner;
        let directory = PrivateDirectory::new();
        let url = h.worker_url().await;
        let original = measure_discovery(h, &url).await;
        let key = Uuid::new_v4().as_u128() as i64;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE FUNCTION test_discovery_selector_gate() RETURNS trigger LANGUAGE plpgsql AS $$
               BEGIN PERFORM pg_catalog.pg_advisory_xact_lock('{key}'::bigint); RETURN NEW; END $$;
             REVOKE ALL ON FUNCTION test_discovery_selector_gate() FROM PUBLIC;
             CREATE TRIGGER test_discovery_selector_gate BEFORE INSERT ON test_worker_selector_calls
             FOR EACH ROW WHEN(NEW.page_limit=20) EXECUTE FUNCTION test_discovery_selector_gate();"
        ))).execute(&h.owner).await.unwrap();
        let mut gate = opentrack_store::begin(&h.owner).await.unwrap();
        let parent_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *gate).await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(key).execute(&mut *gate).await.unwrap();
        let marker = format!("discovery-shutdown-{}", Uuid::new_v4().simple());
        let mut tagged = url::Url::parse(&url).unwrap();
        tagged.query_pairs_mut().append_pair("application_name", &marker);
        let (mut daemon, path) = start_discovery_lifecycle_worker(&directory, tagged.as_str(), false);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(daemon.0.try_wait().unwrap().is_none(), "Owned worker exited before its selector gate");
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.datname=current_database()
                   AND a.application_name=$1 AND $2=ANY(pg_blocking_pids(a.pid))
                   AND EXISTS(SELECT 1 FROM pg_locks l WHERE l.pid=a.pid AND l.locktype='advisory' AND NOT l.granted))",
            ).bind(&marker).bind(parent_pid).fetch_one(&h.owner).await.unwrap();
            if waiting { break; }
            assert!(Instant::now() < deadline, "The owned worker did not reach the matching advisory wait");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let before_signal = discovery_calls(h).await;
        assert_eq!(before_signal.iter().map(|call| call.3).collect::<Vec<_>>(), vec![1, 1]);
        assert!(daemon.0.try_wait().unwrap().is_none(), "Signal only the unreaped owned child");
        // This PID belongs to the unreaped owned child.
        assert_eq!(unsafe { libc::kill(daemon.0.id() as libc::pid_t, libc::SIGTERM) }, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(daemon.0.try_wait().unwrap().is_none(), "In-flight work must remain pending until its gate is released");
            let output = discovery_output(&path, &directory, tagged.as_str());
            if output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|entry| entry["target"] == "opentrack_server"
                    && entry["fields"]["message"] == "Graceful shutdown requested") { break; }
            assert!(Instant::now() < deadline, "Owned worker did not acknowledge SIGTERM");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        gate.commit().await.unwrap();
        await_discovery_exit(&mut daemon, Instant::now() + Duration::from_secs(35)).await;
        let calls = discovery_calls(h).await;
        assert_eq!(calls.iter().map(|call| call.3).collect::<Vec<_>>(), vec![1, 1, 20]);
        assert!(calls.iter().all(|call| call.2.is_none()));
        let output = discovery_output(&path, &directory, tagged.as_str());
        assert!(output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|entry| entry["target"] == "opentrack_server::worker"
                && entry["fields"]["message"] == "Worker stopped; uncaptured events and unfinished leases remain recoverable"));
        println!("{}", json!({"inflight_discovery_shutdown":{"owned_lock_wait_observed":true,
            "shutdown_request_observed_before_release":true,"selector_limits":[1,1,20],"clean_exit":true,
            "boundary":"Owner-only measurement trigger held the actual Job selector. SIGTERM was acknowledged before releasing the parent transaction; no later stage selector began."}}));
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn dense_assignment_backlog(workspace_rank: usize) {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner;
        let directory = PrivateDirectory::new();
        let (actor, scopes, url) = discovery_fixture(h, 94).await;
        let workspace = scopes[workspace_rank];
        let user = actor.user.id;
        let team = h.app.bootstrap(&actor, workspace).await.unwrap().teams[0].id;
        let mut teams = vec![team];
        for (key, name) in [("BA", "Backlog Team A"), ("BB", "Backlog Team B")] {
            teams.push(h.app.create_team(&actor, workspace, &Uuid::new_v4().to_string(),
                serde_json::from_value(json!({"name":name,"key":key,"private":false})).unwrap()
            ).await.unwrap().id);
        }
        let mut assigned = Vec::new();
        for team_id in &teams {
            assigned.push(h.app.create_issue(&actor, workspace, &Uuid::new_v4().to_string(),
                serde_json::from_value::<opentrack_domain::CreateIssue>(json!({
                    "team_id":team_id,"title":"Owner-assigned scheduling fixture","assignee_id":user
                })).unwrap()
            ).await.unwrap());
        }
        let issue = assigned[0].clone();
        let worker = Worker::connect(&url, WorkerOptions::default()).await.unwrap();
        worker.capture(workspace).await.unwrap();
        let mut drained = false;
        for _ in 0..16 {
            let Some(claim) = worker.claim(workspace).await.unwrap() else {
                drained = true;
                break;
            };
            assert!(matches!(worker.run_claim(&claim).await.unwrap(),
                WorkerTick::Completed | WorkerTick::Progress));
        }
        assert!(drained, "Only initial fixture work may be drained directly");
        assert_eq!(worker.capture(workspace).await.unwrap().processed, 0);
        let mut tx = h.context(user, workspace).await;
        let baseline_seq: i64 = sqlx::query_scalar(
            "SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1"
        ).bind(workspace).fetch_one(&mut *tx).await.unwrap();
        let due: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM jobs WHERE workspace_id=$1 AND state<>'succeeded'"
        ).bind(workspace).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(due, 0);

        for index in 0..7 {
            let team_id = teams[1 + index % 2];
            let current = h.app.list_teams(&actor, workspace,
                serde_json::from_value(json!({"limit":100})).unwrap()
            ).await.unwrap().data.into_iter().find(|value| value.id == team_id).unwrap();
            let updated = h.app.update_team(&actor, workspace, team_id, &current.etag(),
                serde_json::from_value(json!({"private":!current.private})).unwrap()
            ).await.unwrap();
            assert_ne!(updated.private, current.private);
        }
        let epoch = h.app.bootstrap(&actor, workspace).await.unwrap().auth_epoch;
        let captured = worker.capture(workspace).await.unwrap();
        assert_eq!((captured.processed, captured.captured), (7, 7));
        assert!(!captured.backfilled && !captured.dead_lettered);
        let mut tx = h.context(user, workspace).await;
        let assignments: Vec<Uuid> = sqlx::query_scalar(
            "SELECT c.job_id FROM consumer_receipts c
             JOIN workspace_events e ON e.workspace_id=c.workspace_id AND e.event_id=c.event_id
             JOIN jobs j ON j.workspace_id=c.workspace_id AND j.id=c.job_id
             WHERE c.workspace_id=$1 AND e.seq>$2 AND e.event_type='access.changed'
             AND c.state='captured' AND c.consumer_name='assignment_cleanup' AND c.consumer_version=1
             AND j.kind='assignment.reconcile' AND j.state='queued' AND j.attempt_count=0
             AND j.available_at<=clock_timestamp() ORDER BY e.seq"
        ).bind(workspace).bind(baseline_seq).fetch_all(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(assignments.len(), 7);
        worker.pool.close().await;

        let queued = discovery_enqueue(h, &actor, workspace, &issue).await;
        let returned = Instant::now();
        assert_eq!(discovery_job(h, &actor, workspace, &queued).await, ("queued".into(), 0));
        let original = measure_discovery(h, &url).await;
        let (mut daemon, path) = start_discovery_worker(&directory, &url);
        let mut boundary = None;
        let deadline = returned + Duration::from_secs(75);
        let (completed, observed_ms) = loop {
            assert!(daemon.0.try_wait().unwrap().is_none(), "Owned worker exited during observation");
            assert!(fs::metadata(&path).unwrap().len() <= 1048576, "Private worker log exceeded its bound");
            let current = discovery_job(h, &actor, workspace, &queued).await;
            let elapsed = returned.elapsed();
            if boundary.is_none() && elapsed >= Duration::from_secs(30) {
                boundary = Some(json!({"observed_elapsed_ms":elapsed.as_secs_f64()*1000.0,
                    "state":current.0,"attempt_count":current.1}));
            }
            if current.0 == "succeeded" || current.0 == "blocked" || current.0 == "dead_letter"
                || Instant::now() >= deadline {
                break (current.0 == "succeeded", elapsed.as_secs_f64() * 1000.0);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        stop_discovery_worker(&mut daemon).await;
        let output = discovery_output(&path, &directory, &url);
        let calls = discovery_calls(h).await;
        let receipt = discovery_receipt(&output, &calls);
        let passed = completed && observed_ms <= 30_000.0;
        println!("{}", json!({"dense_assignment_backlog":{
            "registered_workspaces":94,"workspace_rank":workspace_rank,
            "live_assigned_teams":3,"captured_access_intents":7,
            "post_return_observed_ms":observed_ms,"completed_within_30_seconds":passed,
            "first_observation_at_or_after_30_seconds":boundary,
            "owned_worker_clean_exit":true,"diagnostic":receipt,
            "job_selector_cursors":calls.iter().filter(|call|call.3==20).map(|call|
                call.2.map(|cursor|scopes.binary_search(&cursor).unwrap())).collect::<Vec<_>>(),
            "timing_boundary":"Queue precedes daemon startup; post-return time includes fixture instrumentation, startup and observation. The 75-second collection bound does not extend the 30-second acceptance criterion.",
            "limits":"Fresh isolated PostgreSQL schema and real worker; no browser, cloned retained-state, statistical performance or whole-product acceptance is inferred."
        }}));
        restore_discovery(h, &original).await;

        assert!(completed, "Default worker did not eventually complete the normal workflow command");
        discovery_completed(h, &actor, workspace, &issue, &queued).await;
        assert_eq!(h.app.bootstrap(&actor, workspace).await.unwrap().auth_epoch, epoch);
        let mut tx = h.context(user, workspace).await;
        let exact: bool = sqlx::query_scalar(
            "SELECT j.requested_by=$3 AND j.attempt_count=1 AND j.retry_count=0 AND j.error_code IS NULL
             AND j.lease_owner IS NULL AND j.lease_expires_at IS NULL AND j.lease_token>0
             AND (SELECT count(*)=1 AND bool_and(lease_token=j.lease_token AND outcome='succeeded'
                 AND ended_at IS NOT NULL) FROM job_attempts WHERE workspace_id=$1 AND job_id=$2)
             AND (SELECT count(*)=1 AND bool_and(actor_id=$3 AND actor_kind='account')
                 FROM workspace_events WHERE workspace_id=$1 AND entity_id=$4 AND mutation_id=$2
                 AND event_type='issue.updated')
             AND (SELECT count(*)=1 FROM activity_entries WHERE workspace_id=$1 AND entity_id=$4
                 AND changes->>'job_id'=$2::text AND changes->>'source'='workflow_replacement')
             AND source.archived_at IS NOT NULL AND source.replacement_job_id=j.id AND target.is_default
             FROM jobs j JOIN workflow_replacements r USING(workspace_id,id)
             JOIN workflow_statuses source ON source.workspace_id=r.workspace_id
                 AND source.team_id=r.team_id AND source.id=r.source_status_id
             JOIN workflow_statuses target ON target.workspace_id=r.workspace_id
                 AND target.team_id=r.team_id AND target.id=r.replacement_status_id
             WHERE j.workspace_id=$1 AND j.id=$2"
        ).bind(workspace).bind(queued.id).bind(user).bind(issue.id)
            .fetch_one(&mut *tx).await.unwrap();
        let retained: (i64, bool) = sqlx::query_as(
            "SELECT count(*),coalesce(bool_and(j.kind='assignment.reconcile' AND j.attempt_count=1
                 AND j.retry_count=0 AND j.error_code IS NULL AND j.lease_token>0
                 AND j.lease_owner IS NULL AND j.lease_expires_at IS NULL
                 AND r.scanned_count>0 AND r.cleared_count=0
                 AND (SELECT count(*)=1 AND bool_and(a.lease_token=j.lease_token)
                     FROM job_attempts a WHERE a.workspace_id=j.workspace_id AND a.job_id=j.id)),false)
             FROM jobs j JOIN assignment_reconciliations r USING(workspace_id,id)
             JOIN consumer_receipts c ON c.workspace_id=j.workspace_id AND c.job_id=j.id
             WHERE j.workspace_id=$1 AND j.id=ANY($2) AND c.state='captured'
                 AND c.consumer_name='assignment_cleanup' AND c.consumer_version=1"
        ).bind(workspace).bind(&assignments).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert!(exact, "Workflow must retain its exact authorized one-attempt/fence/history effects");
        assert_eq!(retained, (7, true), "Maintenance must preserve healthy ownership and bounded cursor progress");
        for initial in &assigned {
            let saved = h.app.get_issue(&actor, workspace, initial.id).await.unwrap();
            assert_eq!(saved.assignee_id, Some(user));
            assert_eq!(saved.version.parse::<u64>().unwrap(),
                initial.version.parse::<u64>().unwrap() + u64::from(initial.id == issue.id));
        }
        assert!(passed, "Dense default-profile backlog exceeded the unchanged 30-second completion gate");
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dense_backlog_reaches_minimum_uuid_workspace_within_thirty_seconds() {
    dense_assignment_backlog(0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dense_backlog_reaches_maximum_uuid_workspace_within_thirty_seconds() {
    dense_assignment_backlog(93).await;
}

type QuantumClaim = (Uuid, Uuid, i64, i64);

async fn quantum_trace(h: &Harness, url: &str) {
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let schema = schema.replace('"', "\"\"");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE test_quantum_claims(
           ordinal bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
           workspace_id uuid NOT NULL,job_id uuid NOT NULL,fence bigint NOT NULL,
           selector_ordinal bigint NOT NULL);
         REVOKE ALL ON test_quantum_claims FROM PUBLIC;
         REVOKE ALL ON SEQUENCE test_quantum_claims_ordinal_seq FROM PUBLIC;
         CREATE FUNCTION test_quantum_trace() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
         SET search_path=pg_catalog,\"{schema}\",pg_temp AS $$ BEGIN
           INSERT INTO test_quantum_claims(workspace_id,job_id,fence,selector_ordinal)
           VALUES(NEW.workspace_id,NEW.id,NEW.lease_token,
             (SELECT coalesce(max(id),0) FROM test_worker_selector_calls));
           RETURN NEW;
         END $$;
         REVOKE ALL ON FUNCTION test_quantum_trace() FROM PUBLIC;
         CREATE TRIGGER test_quantum_trace AFTER UPDATE OF state,lease_token ON jobs
         FOR EACH ROW WHEN(NEW.state='running' AND
           (OLD.state<>'running' OR NEW.lease_token>OLD.lease_token))
         EXECUTE FUNCTION test_quantum_trace();"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    let runtime: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    let parsed = url::Url::parse(url).unwrap();
    for role in [runtime.as_str(), parsed.username()] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "REVOKE ALL ON test_quantum_claims FROM \"{role}\";
             REVOKE ALL ON SEQUENCE test_quantum_claims_ordinal_seq FROM \"{role}\";
             REVOKE ALL ON FUNCTION test_quantum_trace() FROM \"{role}\";",
            role = role.replace('"', "\"\"")
        )))
        .execute(&h.owner)
        .await
        .unwrap();
        let private: bool = sqlx::query_scalar(
            "SELECT NOT has_table_privilege($1,'test_quantum_claims','SELECT,INSERT,UPDATE,DELETE')
             AND NOT has_sequence_privilege($1,'test_quantum_claims_ordinal_seq','USAGE,SELECT,UPDATE')
             AND NOT has_function_privilege($1,'test_quantum_trace()','EXECUTE')"
        ).bind(role).fetch_one(&h.owner).await.unwrap();
        assert!(
            private,
            "Only the isolated owner may read/write claim observations"
        );
    }
}

async fn quantum_claims(h: &Harness) -> Vec<QuantumClaim> {
    sqlx::query_as("SELECT workspace_id,job_id,fence,selector_ordinal FROM test_quantum_claims ORDER BY ordinal")
        .fetch_all(&h.owner).await.unwrap()
}

async fn quantum_prepare(
    h: &Harness,
    actor: &opentrack_domain::Actor,
    w: Uuid,
    url: &str,
) -> Vec<opentrack_domain::Issue> {
    let mut teams = vec![h.app.bootstrap(actor, w).await.unwrap().teams[0].id];
    for (key, name) in [("QA", "Quantum team A"), ("QB", "Quantum team B")] {
        teams.push(
            h.app
                .create_team(
                    actor,
                    w,
                    &Uuid::new_v4().to_string(),
                    serde_json::from_value(json!({"name":name,"key":key,"private":false})).unwrap(),
                )
                .await
                .unwrap()
                .id,
        );
    }
    let mut issues = Vec::new();
    for team in teams {
        issues.push(
            h.app
                .create_issue(
                    actor,
                    w,
                    &Uuid::new_v4().to_string(),
                    serde_json::from_value(
                        json!({"team_id":team,"title":"Quantum owner-assigned control",
                "assignee_id":actor.user.id}),
                    )
                    .unwrap(),
                )
                .await
                .unwrap(),
        );
    }
    let worker = Worker::connect(url, WorkerOptions::default())
        .await
        .unwrap();
    worker.capture(w).await.unwrap();
    let mut drained = false;
    for _ in 0..64 {
        let Some(claim) = worker.claim(w).await.unwrap() else {
            drained = true;
            break;
        };
        assert!(matches!(
            worker.run_claim(&claim).await.unwrap(),
            WorkerTick::Progress | WorkerTick::Completed
        ));
    }
    assert!(
        drained,
        "Bounded fixture setup must finish before measured work is queued"
    );
    assert_eq!(worker.capture(w).await.unwrap().processed, 0);
    worker.pool.close().await;
    issues
}

async fn quantum_access(
    h: &Harness,
    actor: &opentrack_domain::Actor,
    w: Uuid,
    issues: &[opentrack_domain::Issue],
    url: &str,
) -> (Vec<Uuid>, String) {
    for index in 0..8 {
        let team = issues[1 + index % 2].team_id;
        let detail = h.app.get_team(actor, w, team).await.unwrap();
        h.app
            .update_team(
                actor,
                w,
                team,
                &detail.etag(),
                serde_json::from_value(json!({"private":!detail.private})).unwrap(),
            )
            .await
            .unwrap();
    }
    let epoch = h.app.bootstrap(actor, w).await.unwrap().auth_epoch;
    let worker = Worker::connect(url, WorkerOptions::default())
        .await
        .unwrap();
    let capture = worker.capture(w).await.unwrap();
    assert_eq!(
        (
            capture.processed,
            capture.captured,
            capture.backfilled,
            capture.dead_lettered
        ),
        (8, 8, false, false)
    );
    let mut tx = h.context(actor.user.id, w).await;
    let older: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE workspace_id=$1 AND kind='assignment.reconcile'
         AND state='queued' AND attempt_count=0 AND available_at<=clock_timestamp() ORDER BY available_at,id"
    ).bind(w).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(older.len(), 8);
    worker.pool.close().await;
    (older, epoch)
}

async fn quantum_fences(
    h: &Harness,
    actor: &opentrack_domain::Actor,
    w: Uuid,
    ids: &[Uuid],
    issues: &[opentrack_domain::Issue],
    replacement: Option<&opentrack_domain::WorkflowReplacement>,
    epoch: &str,
) {
    let mut tx = h.context(actor.user.id, w).await;
    let exact: (i64, bool) = sqlx::query_as(
        "SELECT count(*),coalesce(bool_and(j.attempt_count=1 AND j.retry_count=0 AND j.error_code IS NULL
         AND j.lease_token>0 AND j.lease_owner IS NULL AND j.lease_expires_at IS NULL
         AND j.state IN('queued','succeeded')
         AND (SELECT count(*)=1 AND bool_and(a.lease_token=j.lease_token AND
           ((j.state='queued' AND a.ended_at IS NULL AND a.outcome IS NULL)
            OR (j.state='succeeded' AND a.ended_at IS NOT NULL AND a.outcome='succeeded')))
           FROM job_attempts a WHERE a.workspace_id=j.workspace_id AND a.job_id=j.id)
         AND (j.kind<>'assignment.reconcile' OR EXISTS(SELECT 1 FROM assignment_reconciliations r
           WHERE r.workspace_id=j.workspace_id AND r.id=j.id AND r.scanned_count>0 AND r.cleared_count=0))),false)
         FROM jobs j WHERE j.workspace_id=$1 AND j.id=ANY($2)"
    ).bind(w).bind(ids).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(exact, (ids.len() as i64, true));
    assert_eq!(h.app.bootstrap(actor, w).await.unwrap().auth_epoch, epoch);
    for (index, old) in issues.iter().enumerate() {
        let saved = h.app.get_issue(actor, w, old.id).await.unwrap();
        assert_eq!(saved.assignee_id, Some(actor.user.id));
        assert_eq!(
            saved.version.parse::<u64>().unwrap(),
            old.version.parse::<u64>().unwrap() + u64::from(index == 0 && replacement.is_some())
        );
    }
    if let Some(job) = replacement {
        discovery_completed(h, actor, w, &issues[0], job).await;
        let mut tx = h.context(actor.user.id, w).await;
        let history: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM workspace_events WHERE workspace_id=$1 AND entity_id=$2
              AND mutation_id=$3 AND event_type='issue.updated' AND actor_id=$4 AND actor_kind='account'),
             (SELECT count(*) FROM activity_entries WHERE workspace_id=$1 AND entity_id=$2
              AND changes->>'job_id'=$3::text AND changes->>'source'='workflow_replacement')"
        ).bind(w).bind(issues[0].id).bind(job.id).bind(actor.user.id).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(history, (1, 1));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_claim_quantum_preserves_two_workspace_fifo_fairness_and_stage_rotation() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner;
        let directory = PrivateDirectory::new();
        let (actor, scopes, url) = discovery_fixture(h, 94).await;
        let a = scopes[0]; let b = scopes[93];
        let issues_a = quantum_prepare(h, &actor, a, &url).await;
        let issues_b = quantum_prepare(h, &actor, b, &url).await;
        let original = measure_discovery(h, &url).await;
        quantum_trace(h, &url).await;
        let (older_a, epoch_a) = quantum_access(h, &actor, a, &issues_a, &url).await;
        let (older_b, epoch_b) = quantum_access(h, &actor, b, &issues_b, &url).await;
        let job_a = discovery_enqueue(h, &actor, a, &issues_a[0]).await;
        let accepted = Instant::now();
        let job_b = discovery_enqueue(h, &actor, b, &issues_b[0]).await;
        let (mut daemon, path) = start_discovery_worker(&directory, &url);
        loop {
            assert!(daemon.0.try_wait().unwrap().is_none());
            if discovery_job(h, &actor, a, &job_a).await.0 == "succeeded"
                && discovery_job(h, &actor, b, &job_b).await.0 == "succeeded" { break; }
            assert!(accepted.elapsed() < Duration::from_secs(30), "Both FIFO commands retain the unchanged 30-second gate");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(accepted.elapsed() < Duration::from_secs(30));
        stop_discovery_worker(&mut daemon).await;
        let claims = quantum_claims(h).await;
        assert!(claims.len() >= 18);
        assert!(claims[..4].iter().all(|c| c.0 == a));
        assert!(claims[4..8].iter().all(|c| c.0 == b), "Four claims must advance to the other busy workspace");
        for pair in claims[..4].windows(2) {
            assert_eq!(pair[1].3 - pair[0].3, 5, "Each cached effect follows all five maintenance selectors");
        }
        let mut prior = None; let mut consecutive = 0;
        for claim in &claims {
            assert!(claim.0 == a || claim.0 == b); assert!(claim.2 > 0);
            consecutive = if prior == Some(claim.0) { consecutive + 1 } else { 1 };
            assert!(consecutive <= 4, "A still-busy peer cannot be bypassed by an unbounded local drain");
            prior = Some(claim.0);
        }
        for (w, older, job) in [(a,&older_a,&job_a),(b,&older_b,&job_b)] {
            let expected: Vec<_> = older.iter().copied().chain([job.id]).collect();
            let actual: Vec<_> = claims.iter().filter(|c| c.0 == w).take(9).map(|c| c.1).collect();
            assert_eq!(actual, expected, "Current available_at/id FIFO must survive the local quantum");
            let mut tx = h.context(actor.user.id, w).await;
            let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE workspace_id=$1 AND id=ANY($2) AND state='queued'")
                .bind(w).bind(older).fetch_one(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            assert!(pending > 0, "Both workspaces must still be busy throughout the observed trace");
        }
        let output = discovery_output(&path, &directory, &url);
        let expected = ["Capture","Planning","Job","Retention","DocumentRetention","AttachmentGc"];
        let stages: Vec<_> = output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|v| v["target"] == "opentrack_server::worker" && v["fields"]["message"] == "Worker stage completed")
            .map(|v| v["fields"]["stage"].as_str().unwrap().to_owned()).collect();
        for (index, stage) in stages.iter().enumerate() { assert_eq!(stage, expected[index % 6]); }
        let receipt = discovery_receipt(&output, &discovery_calls(h).await);
        quantum_fences(h, &actor, a, &older_a.iter().copied().chain([job_a.id]).collect::<Vec<_>>(), &issues_a, Some(&job_a), &epoch_a).await;
        quantum_fences(h, &actor, b, &older_b.iter().copied().chain([job_b.id]).collect::<Vec<_>>(), &issues_b, Some(&job_b), &epoch_b).await;
        restore_discovery(h, &original).await;
        println!("{}", json!({"four_claim_quantum":{"registered_scopes":94,"busy_scopes":2,
            "older_intents_per_scope":8,"claimed_batches":claims.len(),"maximum_consecutive_claims":4,
            "both_commands_completed_within_30_seconds":true,"default_profile":receipt["profile"],
            "boundary":"Owner-only committed claim trace; one effect per six-stage round; retained state and general p95 are separate."}}));
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn populated_once_leaves_the_remaining_claim_quantum_unused() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner; let directory = PrivateDirectory::new();
        let (actor, scopes, url) = discovery_fixture(h, 1).await; let w = scopes[0];
        let issues = quantum_prepare(h, &actor, w, &url).await;
        let original = measure_discovery(h, &url).await; quantum_trace(h, &url).await;
        let (older, epoch) = quantum_access(h, &actor, w, &issues, &url).await;
        let job = discovery_enqueue(h, &actor, w, &issues[0]).await;
        let (mut daemon, path) = start_discovery_lifecycle_worker(&directory, &url, true);
        await_discovery_exit(&mut daemon, Instant::now() + Duration::from_secs(35)).await;
        let claims = quantum_claims(h).await;
        assert_eq!(claims.len(), 1); assert_eq!(claims[0].1, older[0]);
        assert_eq!(discovery_job(h, &actor, w, &job).await, ("queued".into(),0));
        let mut tx = h.context(actor.user.id, w).await;
        let untouched: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE workspace_id=$1 AND id=ANY($2) AND state='queued' AND attempt_count=0")
            .bind(w).bind(&older[1..]).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap(); assert_eq!(untouched,7);
        quantum_fences(h, &actor, w, &older[..1], &issues, None, &epoch).await;
        let calls = discovery_calls(h).await;
        assert_eq!(calls.iter().map(|c| c.3).collect::<Vec<_>>(), vec![1,1,20,1,1,1]);
        discovery_output(&path, &directory, &url);
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_drains_only_the_second_cached_effect_and_starts_no_later_stage() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let h = &runner; let directory = PrivateDirectory::new();
        let (actor, scopes, url) = discovery_fixture(h, 94).await; let w = scopes[0];
        let issues = quantum_prepare(h, &actor, w, &url).await;
        let original = measure_discovery(h, &url).await; quantum_trace(h, &url).await;
        let (older, epoch) = quantum_access(h, &actor, w, &issues, &url).await;
        let job = discovery_enqueue(h, &actor, w, &issues[0]).await;
        let second = older[1]; let key = Uuid::new_v4().as_u128() as i64;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE FUNCTION test_quantum_gate() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN PERFORM pg_catalog.pg_advisory_xact_lock('{key}'::bigint); RETURN NEW; END $$;
             REVOKE ALL ON FUNCTION test_quantum_gate() FROM PUBLIC;
             CREATE TRIGGER test_quantum_gate BEFORE UPDATE ON assignment_reconciliations
             FOR EACH ROW WHEN(NEW.id='{second}'::uuid) EXECUTE FUNCTION test_quantum_gate();"
        ))).execute(&h.owner).await.unwrap();
        let mut gate = opentrack_store::begin(&h.owner).await.unwrap();
        let parent_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *gate).await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(key).execute(&mut *gate).await.unwrap();
        let marker = format!("quantum-shutdown-{}", Uuid::new_v4().simple());
        let mut tagged = url::Url::parse(&url).unwrap(); tagged.query_pairs_mut().append_pair("application_name", &marker);
        let (mut daemon, path) = start_discovery_lifecycle_worker(&directory, tagged.as_str(), false);
        let waiting_sql = "SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.datname=current_database()
            AND a.application_name=$1 AND $2=ANY(pg_blocking_pids(a.pid)) AND EXISTS(SELECT 1 FROM pg_locks l
            WHERE l.pid=a.pid AND l.locktype='advisory' AND NOT l.granted))";
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(daemon.0.try_wait().unwrap().is_none());
            let waiting: bool = sqlx::query_scalar(waiting_sql).bind(&marker).bind(parent_pid).fetch_one(&h.owner).await.unwrap();
            if waiting { break; }
            assert!(Instant::now() < deadline, "Witness the exact owned worker effect wait before signalling");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let claims = quantum_claims(h).await;
        assert_eq!(claims.iter().map(|c| c.1).collect::<Vec<_>>(), older[..2]);
        let before = discovery_calls(h).await;
        assert_eq!(before.iter().map(|c| c.3).collect::<Vec<_>>(), vec![1,1,20,1,1,1,1,1],
            "The second claim must be cached and follow the preceding maintenance stages");
        // This PID belongs to the unreaped owned child whose exact database wait was witnessed.
        assert_eq!(unsafe { libc::kill(daemon.0.id() as libc::pid_t, libc::SIGTERM) }, 0);
        let acknowledged = Instant::now() + Duration::from_millis(500);
        loop {
            assert!(daemon.0.try_wait().unwrap().is_none());
            let output = discovery_output(&path, &directory, tagged.as_str());
            if output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|v| v["target"] == "opentrack_server" && v["fields"]["message"] == "Graceful shutdown requested") { break; }
            assert!(Instant::now() < acknowledged, "Acknowledge shutdown while the unmodified effect deadline remains live");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let still_waiting: bool = sqlx::query_scalar(waiting_sql).bind(&marker).bind(parent_pid).fetch_one(&h.owner).await.unwrap();
        assert!(still_waiting, "The gate must remain live until after shutdown acknowledgement");
        assert_eq!(quantum_claims(h).await.len(), 2);
        gate.commit().await.unwrap();
        await_discovery_exit(&mut daemon, Instant::now() + Duration::from_secs(35)).await;
        assert_eq!(quantum_claims(h).await.len(), 2, "No third claim may start after shutdown");
        assert_eq!(discovery_calls(h).await, before, "No later maintenance or discovery stage may start");
        assert_eq!(discovery_job(h, &actor, w, &job).await, ("queued".into(),0));
        quantum_fences(h, &actor, w, &older[..2], &issues, None, &epoch).await;
        discovery_output(&path, &directory, tagged.as_str());
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
