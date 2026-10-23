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
