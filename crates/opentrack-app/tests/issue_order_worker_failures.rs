#[allow(dead_code)]
mod support;

use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::{Actor, Error};
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use std::time::Duration;
use support::Harness;
use tokio::task::JoinSet;
use uuid::Uuid;

const PASSWORD: &str = "Synthetic ordering failure passphrase 2026!";

struct Fixture {
    manager: Actor,
    requester: Actor,
    workspace: Uuid,
    team: Uuid,
    job: Uuid,
    ordered: Vec<Uuid>,
}

fn request<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

async fn fixture(h: &Harness) -> Fixture {
    let (session, cookie) = h.app.setup(request(json!({
        "setup_token":h.setup_token,"email":"manager@ordering-failure.example.test",
        "display_name":"Example Manager","password":PASSWORD,"workspace_name":"Ordering failures",
        "team_name":"Initial team","team_key":"ORD"
    }))).await.unwrap();
    let manager = h.app.authenticate(&cookie).await.unwrap();
    let workspace = session
        .workspaces
        .iter()
        .find(|item| item.name == "Ordering failures")
        .unwrap()
        .id;
    let team = h
        .app
        .create_team(
            &manager,
            workspace,
            &Uuid::new_v4().to_string(),
            request(json!({
                "name":"Private ordering faults","key":"PVT","private":true
            })),
        )
        .await
        .unwrap();
    let invitation = h.app.create_invitation(&manager, workspace, &Uuid::new_v4().to_string(), request(json!({
        "email":"contributor@ordering-failure.example.test","role":"member","team_ids":[team.id]
    }))).await.unwrap();
    let (_, cookie) = h.app.accept_invitation(None, request(json!({
        "token":invitation.token.unwrap(),"display_name":"Example Contributor","password":PASSWORD
    }))).await.unwrap();
    let requester = h.app.authenticate(&cookie).await.unwrap();
    assert!(
        !h.app
            .get_team(&requester, workspace, team.id)
            .await
            .unwrap()
            .can_manage
    );
    assert!(
        h.app
            .get_team(&manager, workspace, team.id)
            .await
            .unwrap()
            .can_manage
    );
    let mut tx = h.context(manager.user.id, workspace).await;
    let status: Uuid = sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default AND archived_at IS NULL")
        .bind(workspace).bind(team.id).fetch_one(&mut *tx).await.unwrap();
    // This base predates normal create-rank allocation; only this fresh fixture inserts raw issues.
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,description,status_id,rank,version,created_at,archived_at,deleted_at) SELECT gen_random_uuid(),$1,$2,$3,n,'Ordering fault fixture','Preserved document',$4,900000000000+n,7,'2026-10-20'::timestamptz+n*interval '1 second',CASE WHEN n=502 THEN clock_timestamp() END,CASE WHEN n=503 THEN clock_timestamp() END FROM generate_series(1,503) n")
        .bind(workspace).bind(team.id).bind(requester.user.id).bind(status).execute(&mut *tx).await.unwrap();
    let ordered = sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 AND archived_at IS NULL AND deleted_at IS NULL ORDER BY rank,id")
        .bind(workspace).bind(team.id).bind(status).fetch_all(&mut *tx).await.unwrap();
    let job = Uuid::new_v4();
    sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'issues.order_normalize',$3,$2::text)")
        .bind(workspace).bind(job).bind(requester.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issue_order_normalizations(workspace_id,id,team_id,status_id,original_requested_by) VALUES($1,$2,$3,$4,$5)")
        .bind(workspace).bind(job).bind(team.id).bind(status).bind(requester.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO issue_order_guards(workspace_id,team_id,status_id,job_id) VALUES($1,$2,$3,$4)",
    )
    .bind(workspace)
    .bind(team.id)
    .bind(status)
    .bind(job)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    Fixture {
        manager,
        requester,
        workspace,
        team: team.id,
        job,
        ordered,
    }
}

async fn logical(h: &Harness, f: &Fixture) -> Vec<Value> {
    let mut tx = h.context(f.manager.user.id, f.workspace).await;
    let mut result = Vec::new();
    for table in [
        "issues",
        "teams",
        "documents",
        "document_revisions",
        "issue_subscriptions",
        "subscription_events",
        "activity_entries",
        "audit_entries",
        "workspace_events",
        "workspace_stream_heads",
        "project_issues",
        "issue_labels",
        "issue_relations",
        "workspace_auth_guards",
        "workspace_members",
        "team_members",
    ] {
        let projection = match table {
            "issues" => "to_jsonb(x)-'rank'",
            "teams" => "to_jsonb(x)-'ordering_version'",
            _ => "to_jsonb(x)",
        };
        result.push(sqlx::query_scalar(AssertSqlSafe(format!("SELECT coalesce(jsonb_agg(data ORDER BY data::text),'[]'::jsonb) FROM (SELECT {projection} AS data FROM {table} x WHERE workspace_id=$1) rows")))
            .bind(f.workspace).fetch_one(&mut *tx).await.unwrap());
    }
    tx.commit().await.unwrap();
    result
}

async fn maintenance(h: &Harness, f: &Fixture) -> Value {
    let mut tx = h.context(f.manager.user.id, f.workspace).await;
    let value = sqlx::query_scalar("SELECT jsonb_build_object('job',to_jsonb(j),'operation',to_jsonb(n),'attempts',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY attempt),'[]'::jsonb) FROM job_attempts a WHERE a.workspace_id=j.workspace_id AND a.job_id=j.id),'staging',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY ordinal),'[]'::jsonb) FROM issue_order_staging s WHERE s.workspace_id=j.workspace_id AND s.job_id=j.id),'guard',(SELECT to_jsonb(g) FROM issue_order_guards g WHERE g.workspace_id=j.workspace_id AND g.job_id=j.id),'team',(SELECT to_jsonb(t) FROM teams t WHERE t.workspace_id=j.workspace_id AND t.id=n.team_id),'issues',(SELECT jsonb_agg(to_jsonb(i) ORDER BY i.id) FROM issues i WHERE i.workspace_id=j.workspace_id AND i.team_id=n.team_id)) FROM jobs j JOIN issue_order_normalizations n USING(workspace_id,id) WHERE j.workspace_id=$1 AND j.id=$2")
        .bind(f.workspace).bind(f.job).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}

async fn batch(worker: &Worker, f: &Fixture) -> WorkerTick {
    let claim = worker.claim(f.workspace).await.unwrap().unwrap();
    assert_eq!(claim.id, f.job);
    worker.run_claim(&claim).await.unwrap()
}

async fn prepare_applying(worker: &Worker, h: &Harness, f: &Fixture, applied: i64) {
    for _ in 0..6 + applied / 100 {
        assert_eq!(batch(worker, f).await, WorkerTick::Progress);
    }
    let state = maintenance(h, f).await;
    assert_eq!(state["operation"]["phase"], "applying");
    assert_eq!(state["operation"]["staged_count"], 501);
    assert_eq!(state["operation"]["applied_count"], applied);
    assert_eq!(state["operation"]["total_count"], 501);
    assert!(!state["guard"].is_null());
}

async fn finish(worker: &Worker, h: &Harness, f: &Fixture) {
    let mut complete = false;
    for _ in 0..6 {
        let result = batch(worker, f).await;
        assert!(matches!(
            result,
            WorkerTick::Progress | WorkerTick::Completed
        ));
        if result == WorkerTick::Completed {
            complete = true;
            break;
        }
    }
    assert!(complete);
    let state = maintenance(h, f).await;
    assert_eq!(state["job"]["state"], "succeeded");
    assert_eq!(state["operation"]["applied_count"], 501);
    assert_eq!(state["operation"]["phase"], "completed");
    assert!(state["guard"].is_null());
    assert_eq!(
        state["operation"]["original_requested_by"],
        f.requester.user.id.to_string()
    );
    let mut tx = h.context(f.manager.user.id, f.workspace).await;
    let normalized: bool = sqlx::query_scalar("SELECT count(*)=501 AND bool_and(i.rank=s.ordinal::numeric*1024) FROM issue_order_staging s JOIN issues i ON i.workspace_id=s.workspace_id AND i.id=s.issue_id WHERE s.workspace_id=$1 AND s.job_id=$2")
        .bind(f.workspace).bind(f.job).fetch_one(&mut *tx).await.unwrap();
    assert!(normalized);
    let ordered: Vec<Uuid> = sqlx::query_scalar("SELECT i.id FROM issue_order_staging s JOIN issues i ON i.workspace_id=s.workspace_id AND i.id=s.issue_id WHERE s.workspace_id=$1 AND s.job_id=$2 ORDER BY i.rank,i.id")
        .bind(f.workspace).bind(f.job).fetch_all(&mut *tx).await.unwrap();
    assert_eq!(ordered, f.ordered);
    tx.commit().await.unwrap();
}

async fn partial_access_loss(h: &Harness, worker: &Worker, f: &Fixture) {
    let before = logical(h, f).await;
    prepare_applying(worker, h, f, 100).await;
    assert!(
        logical(h, f).await == before,
        "Partial applying changed logical data or history"
    );
    let partial = maintenance(h, f).await;
    let mut tx = h.context(f.manager.user.id, f.workspace).await;
    let mixed: (i64, i64) = sqlx::query_as("SELECT count(*) FILTER(WHERE s.ordinal<=100 AND i.rank=s.ordinal::numeric*1024),count(*) FILTER(WHERE s.ordinal>100 AND i.rank=s.original_rank) FROM issue_order_staging s JOIN issues i ON i.workspace_id=s.workspace_id AND i.id=s.issue_id WHERE s.workspace_id=$1 AND s.job_id=$2")
        .bind(f.workspace).bind(f.job).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(mixed, (100, 401));
    tx.commit().await.unwrap();
    let member = h
        .app
        .get_team_member(&f.manager, f.workspace, f.team, f.requester.user.id)
        .await
        .unwrap();
    h.app
        .remove_team_member(
            &f.manager,
            f.workspace,
            f.team,
            f.requester.user.id,
            &member.etag(f.team),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app.get_team(&f.requester, f.workspace, f.team).await,
        Err(Error::NotFound)
    ));
    let mut tx = h.context(f.manager.user.id, f.workspace).await;
    let active: bool = sqlx::query_scalar("SELECT m.active AND u.disabled_at IS NULL FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND m.user_id=$2")
        .bind(f.workspace).bind(f.requester.user.id).fetch_one(&mut *tx).await.unwrap();
    assert!(active);
    tx.commit().await.unwrap();
    let after_revocation = logical(h, f).await;
    let revoked = maintenance(h, f).await;
    assert_eq!(batch(worker, f).await, WorkerTick::Blocked);
    let blocked = maintenance(h, f).await;
    assert_eq!(blocked["job"]["state"], "blocked");
    assert_eq!(blocked["job"]["error_code"], "access_lost");
    for key in ["operation", "staging", "guard", "issues"] {
        assert!(
            revoked[key] == partial[key],
            "Revocation changed maintenance {key}"
        );
    }
    for key in ["operation", "staging", "guard", "issues", "team"] {
        assert!(blocked[key] == revoked[key], "Blocked batch changed {key}");
    }
    assert!(
        logical(h, f).await == after_revocation,
        "Blocked batch emitted effects"
    );
    assert!(
        h.app
            .get_team(&f.manager, f.workspace, f.team)
            .await
            .unwrap()
            .can_manage
    );
    assert_ne!(f.manager.user.id, f.requester.user.id);
    // This is controlled fixture takeover, not the later manager-resume API.
    let mut tx = h.context(f.manager.user.id, f.workspace).await;
    sqlx::query("UPDATE jobs SET requested_by=$3,state='queued',error_code=NULL,finished_at=NULL,available_at=clock_timestamp(),updated_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(f.workspace).bind(f.job).bind(f.manager.user.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let resumed = maintenance(h, f).await;
    assert_eq!(
        resumed["job"]["requested_by"],
        f.manager.user.id.to_string()
    );
    for key in ["operation", "staging", "guard", "issues", "attempts"] {
        assert!(resumed[key] == blocked[key], "Fixture takeover reset {key}");
    }
    let claim = worker.claim(f.workspace).await.unwrap().unwrap();
    assert_eq!(claim.id, f.job);
    assert_eq!(claim.requested_by, Some(f.manager.user.id));
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::Progress
    );
    finish(worker, h, f).await;
    assert!(
        logical(h, f).await == after_revocation,
        "Takeover changed issue metadata, timestamps or history"
    );
    assert!(matches!(
        h.app.get_team(&f.requester, f.workspace, f.team).await,
        Err(Error::NotFound)
    ));
    println!(
        "partial_access_loss: applied=100 original_suffix=401 blocked=true same_job=true different_current_requester=true original_requester_retained=true staging_retained=true final_exact=501 logical_effects_unchanged=true fixture_takeover_only=true"
    );
}

async fn observe_gate(h: &Harness, holder: i32, expiry: i64) -> Result<(i32, i64, i64), String> {
    let mut entered = None;
    loop {
        let row: Option<(i32, i64)> = sqlx::query_as("SELECT pid,(extract(epoch FROM clock_timestamp())*1000000)::bigint FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1")
            .bind(holder).fetch_optional(&h.owner).await.map_err(|error| error.to_string())?;
        let now: i64 =
            sqlx::query_scalar("SELECT (extract(epoch FROM clock_timestamp())*1000000)::bigint")
                .fetch_one(&h.owner)
                .await
                .map_err(|error| error.to_string())?;
        match (entered, row) {
            (None, Some((pid, time))) if time < expiry => entered = Some((pid, time)),
            (Some((pid, first)), Some((current, time)))
                if current == pid && time > expiry + 75_000 =>
            {
                return Ok((pid, first, time));
            }
            (Some(_), None) => {
                return Err("Worker stopped waiting before the controlled release".into());
            }
            _ => {}
        }
        if entered.is_none() && now >= expiry {
            return Err("No mid-effect waiter was observed before lease expiry".into());
        }
        if now > expiry + 200_000 {
            return Err("Fault gate did not satisfy the bounded release window".into());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn mid_effect_expiry(h: &Harness, worker: &Worker, f: &Fixture) {
    prepare_applying(worker, h, f, 500).await;
    let claim = worker.claim(f.workspace).await.unwrap().unwrap();
    let gate = (Uuid::new_v4().as_u128() as i64) & i64::MAX;
    let ddl = format!(
        r#"
        CREATE SEQUENCE test_gate_entered;
        CREATE SEQUENCE test_gate_expiry;
        CREATE SEQUENCE test_gate_released;
        CREATE FUNCTION test_issue_order_gate() RETURNS trigger LANGUAGE plpgsql AS $$
        DECLARE expires bigint;
        BEGIN
          IF NEW.id='{team}'::uuid AND EXISTS(SELECT 1 FROM issue_order_normalizations n WHERE n.workspace_id='{workspace}'::uuid AND n.id='{job}'::uuid AND n.phase='completed')
            AND NOT EXISTS(SELECT 1 FROM issue_order_guards WHERE workspace_id='{workspace}'::uuid AND job_id='{job}'::uuid) THEN
            IF current_setting('statement_timeout')<>'3s' OR current_setting('lock_timeout')<>'1s'
              OR NOT EXISTS(SELECT 1 FROM issue_order_normalizations WHERE workspace_id='{workspace}'::uuid AND id='{job}'::uuid AND staged_count=501 AND applied_count=501 AND total_count=501)
              OR EXISTS(SELECT 1 FROM issue_order_staging s JOIN issues i ON i.workspace_id=s.workspace_id AND i.id=s.issue_id WHERE s.workspace_id='{workspace}'::uuid AND s.job_id='{job}'::uuid AND i.rank<>s.ordinal::numeric*1024) THEN
              RAISE EXCEPTION 'Fault gate reached outside the required final effect';
            END IF;
            SELECT (extract(epoch FROM lease_expires_at)*1000000)::bigint INTO expires FROM jobs WHERE workspace_id='{workspace}'::uuid AND id='{job}'::uuid;
            IF expires<=(extract(epoch FROM clock_timestamp())*1000000)::bigint THEN RAISE EXCEPTION 'Lease expired before fault gate entry';END IF;
            PERFORM setval('test_gate_entered',(extract(epoch FROM clock_timestamp())*1000000)::bigint);
            PERFORM setval('test_gate_expiry',expires);
            PERFORM pg_advisory_xact_lock({gate});
            PERFORM setval('test_gate_released',(extract(epoch FROM clock_timestamp())*1000000)::bigint);
          END IF;
          RETURN NEW;
        END $$;
        REVOKE ALL ON FUNCTION test_issue_order_gate() FROM PUBLIC;
        CREATE TRIGGER test_issue_order_gate AFTER UPDATE OF ordering_version ON teams FOR EACH ROW EXECUTE FUNCTION test_issue_order_gate();
    "#,
        team = f.team,
        workspace = f.workspace,
        job = f.job
    );
    sqlx::raw_sql(AssertSqlSafe(ddl))
        .execute(&h.owner)
        .await
        .unwrap();
    let mut holder = h.context(f.manager.user.id, f.workspace).await;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(gate)
        .execute(&mut *holder)
        .await
        .unwrap();
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let business_before = logical(h, f).await;
    let mut before = maintenance(h, f).await;
    let mut shorten = h.context(f.manager.user.id, f.workspace).await;
    let remaining: f64 = sqlx::query_scalar("SELECT extract(epoch FROM lease_expires_at-clock_timestamp())::float8 FROM jobs WHERE workspace_id=$1 AND id=$2")
        .bind(f.workspace).bind(f.job).fetch_one(&mut *shorten).await.unwrap();
    assert!((55.0..=60.0).contains(&remaining));
    let (job, expiry): (Value, i64) = sqlx::query_as("UPDATE jobs SET lease_expires_at=clock_timestamp()+interval '600 milliseconds' WHERE workspace_id=$1 AND id=$2 RETURNING to_jsonb(jobs),(extract(epoch FROM lease_expires_at)*1000000)::bigint")
        .bind(f.workspace).bind(f.job).fetch_one(&mut *shorten).await.unwrap();
    shorten.commit().await.unwrap();
    before["job"] = job;
    let mut effects = JoinSet::new();
    let runner = worker.clone();
    effects.spawn(async move { runner.run_claim(&claim).await });
    let observed = tokio::time::timeout(
        Duration::from_millis(850),
        observe_gate(h, holder_pid, expiry),
    )
    .await;
    // Release/reap before inspecting any failed witness, so failures cannot strand the gate.
    let released = holder.rollback().await;
    let result = tokio::time::timeout(Duration::from_secs(5), effects.join_next()).await;
    let completed = match result {
        Ok(value) => value,
        Err(_) => {
            effects.shutdown().await;
            None
        }
    };
    released.unwrap();
    let (_, first, last) = observed
        .expect("Fault observer exceeded its strict deadline")
        .expect("A real mid-effect lease-expiry window was not established");
    assert_eq!(completed.unwrap().unwrap().unwrap(), WorkerTick::LostLease);
    let (entered, recorded_expiry, left, finished): (i64, i64, i64, i64) = sqlx::query_as("SELECT (SELECT last_value FROM test_gate_entered),(SELECT last_value FROM test_gate_expiry),(SELECT last_value FROM test_gate_released),(extract(epoch FROM clock_timestamp())*1000000)::bigint").fetch_one(&h.owner).await.unwrap();
    assert_eq!(recorded_expiry, expiry);
    assert!(entered < expiry && first < expiry && last > expiry && last - entered < 900_000);
    assert!(left >= last && left > expiry && left - entered < 1_000_000);
    assert!(finished >= left && finished - entered < 2_000_000);
    assert!(
        maintenance(h, f).await == before,
        "Expired final effect changed exact ranks, progress, marker, job, attempt or generation"
    );
    assert!(
        logical(h, f).await == business_before,
        "Expired final effect emitted logical effects"
    );
    sqlx::raw_sql("DROP TRIGGER test_issue_order_gate ON teams; DROP FUNCTION test_issue_order_gate(); DROP SEQUENCE test_gate_entered,test_gate_expiry,test_gate_released;").execute(&h.owner).await.unwrap();
    println!(
        "mid_effect_expiry: entered_before_expiry_us={} observed_before_expiry_us={} observed_after_expiry_us={} observed_gate_wait_us={} normal_gate_return_us={} gate_left_after_expiry_us={} effect_finished_since_entry_us={} outcome=LostLease exact_business_job_attempt_rollback=true defaults_lease_seconds=60 defaults_connections=5 statement_timeout_ms=3000 lock_timeout_ms=1000 fixture_lease_ms=600",
        expiry - entered,
        expiry - first,
        last - expiry,
        last - entered,
        left - entered,
        left - expiry,
        finished - entered
    );
}

async fn run_case(lease_fault: bool) {
    let h = Harness::new().await;
    let schema: String = sqlx::query_scalar("SELECT current_schema()::text")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let options = WorkerOptions::default();
    assert_eq!((options.max_connections, options.lease_seconds), (5, 60));
    let worker = Worker::connect(&h.worker_url().await, options)
        .await
        .unwrap();
    let copy = h.clone();
    let runner = worker.clone();
    let result = tokio::spawn(async move {
        let f = fixture(&copy).await;
        if lease_fault {
            mid_effect_expiry(&copy, &runner, &f).await;
        } else {
            partial_access_loss(&copy, &runner, &f).await;
        }
    })
    .await;
    worker.pool.close().await;
    if let Err(error) = result {
        h.app.pool.close().await;
        h.owner.close().await;
        eprintln!("Failed fresh fixture retained for diagnosis: {schema}");
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_applying_private_access_loss_and_different_requester_takeover() {
    run_case(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_mid_effect_wall_clock_lease_expiry_rolls_back_final_application() {
    run_case(true).await;
}
