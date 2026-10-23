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
      'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY seq),'[]') FROM consumer_receipts r WHERE workspace_id=$1),
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
            let claim = worker.claim(workspace).await.unwrap().unwrap();
            assert_eq!(worker.run_claim(&claim).await.unwrap(), WorkerTick::Completed);
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
                && current["jobs"].as_array().unwrap().len() == 2
                && current["jobs"].as_array().unwrap().iter().all(|job| job["state"] == "succeeded") {
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
        let stages = ["Capture", "Job", "Retention", "DocumentRetention", "AttachmentGc"];
        let mut seen_stages = [false; 5];
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
        "Job",
        "Retention",
        "DocumentRetention",
        "AttachmentGc",
    ];
    let mut completions = [0_u64; 5];
    let mut worked = [0_u64; 5];
    let mut failed = [0_u64; 5];
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
    let mut queries = [0_u64; 5];
    let mut maintenance = 0;
    for call in calls {
        match call.3 {
            20 => queries[1] += 1,
            1 => {
                queries[[0, 2, 3, 4][maintenance % 4]] += 1;
                maintenance += 1;
            }
            _ => panic!("Unexpected selector page limit"),
        }
    }
    let mut stages = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        assert!(completions[index] > 0, "Every stage must execute");
        if index != 1 {
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
        "clock_tolerance_us":100000,"query_stage_mapping":"One owned child's fixed rotation; page20 is Job, page1 follows Capture/Retention/DocumentRetention/AttachmentGc. Final in-flight completion may be absent.",
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
        let claim = worker.claim(*scope).await.unwrap().unwrap();
        assert_eq!(
            worker.run_claim(&claim).await.unwrap(),
            WorkerTick::Completed
        );
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
        event(h, actor.user.id, scopes[1], 2).await;
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
        assert_discovery_gap(&calls[5], &calls[6]);
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
                "Capture failure must preserve its complete selected durable state");
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
        assert_discovery_gap(&calls[13], &calls[14]);
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
async fn short_and_full_fourth_discovery_pages_both_keep_the_empty_reset_cooldown() {
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
            assert_discovery_gap(&calls[4], &calls[5]);
            stop_discovery_worker(&mut daemon).await;
            let output = discovery_output(&path, &directory, &url);
            println!("{}", json!({"fourth_page_boundary":{"registered_scopes":count,
                "measurements":discovery_receipt(&output, &discovery_calls(h).await),
                "boundary":"A short or full fourth page consumes the burst; the later empty lookup starts its own cooldown."}}));
        }
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_runs_exactly_one_capture_job_retention_document_and_gc_poll() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let directory = PrivateDirectory::new();
        let url = runner.worker_url().await;
        let original = measure_discovery(&runner, &url).await;
        let (mut daemon, path) = start_discovery_lifecycle_worker(&directory, &url, true);
        await_discovery_exit(&mut daemon, Instant::now() + Duration::from_secs(35)).await;
        let calls = discovery_calls(&runner).await;
        assert_eq!(calls.iter().map(|call| call.3).collect::<Vec<_>>(), vec![1, 20, 1, 1, 1]);
        assert!(calls.iter().all(|call| call.2.is_none()));
        let output = discovery_output(&path, &directory, &url);
        assert!(output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|entry| entry["target"] == "opentrack_server::worker"
                && entry["fields"]["message"] == "Worker completed one bounded event, job and retention poll"),
            "The real once command must report successful bounded completion");
        println!("{}", json!({"once_discovery":{"selector_limits":calls.iter().map(|call| call.3).collect::<Vec<_>>(),
            "job_selector_calls":1,"maintenance_selector_calls":4,"clean_exit":true,
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
        assert_eq!(before_signal.iter().map(|call| call.3).collect::<Vec<_>>(), vec![1]);
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
        assert_eq!(calls.iter().map(|call| call.3).collect::<Vec<_>>(), vec![1, 20]);
        assert!(calls.iter().all(|call| call.2.is_none()));
        let output = discovery_output(&path, &directory, tagged.as_str());
        assert!(output.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|entry| entry["target"] == "opentrack_server::worker"
                && entry["fields"]["message"] == "Worker stopped; uncaptured events and unfinished leases remain recoverable"));
        println!("{}", json!({"inflight_discovery_shutdown":{"owned_lock_wait_observed":true,
            "shutdown_request_observed_before_release":true,"selector_limits":[1,20],"clean_exit":true,
            "boundary":"Owner-only measurement trigger held the actual Job selector. SIGTERM was acknowledged before releasing the parent transaction; no later stage selector began."}}));
        restore_discovery(h, &original).await;
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
