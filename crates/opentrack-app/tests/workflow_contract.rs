//! Real workflow invariants, durable bounded migration, and both observed write orders.
#[allow(dead_code)]
mod support;
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;
fn req<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn add(h: &Harness, a: &Actor, w: Uuid, t: Uuid, name: &str) -> (Uuid, Workflow) {
    let f = h.app.workflow(a, w, t).await.unwrap();
    h.app
        .create_status(
            a,
            w,
            t,
            &Uuid::new_v4().to_string(),
            &f.etag(),
            req(json!({"name":name,"category":"started","color":"blue"})),
        )
        .await
        .unwrap()
}
async fn blocked_by(h: &Harness, pid: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiter: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(pid)
            .fetch_optional(&h.owner)
            .await
            .unwrap();
            if let Some(p) = waiter {
                return p;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe actual PostgreSQL lock wait")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_workflow_contract() {
    let h = Harness::with_connections(6).await;
    let copy = h.clone();
    let result = tokio::spawn(async move { exercise(&copy).await }).await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic())
    }
    h.cleanup().await;
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"workflow@example.invalid","password":"workflow contract password 729!","display_name":"Workflow owner","workspace_name":"Workflow contract","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let a = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let boot = h.app.bootstrap(&a, w).await.unwrap();
    let t = boot.teams[0].id;
    let flow = h.app.workflow(&a, w, t).await.unwrap();
    assert_eq!(flow.statuses.iter().filter(|s| s.is_default).count(), 1);
    assert!(flow.etag().ends_with(":true\""));
    let (extra, flow) = add(h, &a, w, t, "Review").await;
    let flow = h
        .app
        .set_default_status(
            &a,
            w,
            t,
            &flow.etag(),
            SetDefaultStatus { status_id: extra },
        )
        .await
        .unwrap();
    let issue = h
        .app
        .create_issue(
            &a,
            w,
            "workflow-default-issue",
            req(json!({"team_id":t,"title":"Actual server default"})),
        )
        .await
        .unwrap();
    assert_eq!(issue.status_id, extra);
    assert!(matches!(
        h.app.archive_status(&a, w, t, extra, &flow.etag()).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        h.app
            .update_status(
                &a,
                w,
                t,
                extra,
                "\"workflow:stale\"",
                req(json!({"name":"Stale"}))
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    let renamed = h
        .app
        .update_status(
            &a,
            w,
            t,
            extra,
            &flow.etag(),
            req(json!({"name":"In review","color":"purple"})),
        )
        .await
        .unwrap();
    assert_eq!(
        renamed
            .statuses
            .iter()
            .find(|s| s.id == extra)
            .unwrap()
            .name,
        "In review"
    );
    assert_eq!(
        h.app.get_issue(&a, w, issue.id).await.unwrap().status_id,
        extra
    );
    let mut invalid = h.context(a.user.id, w).await;
    sqlx::query("UPDATE workflow_statuses SET is_default=false WHERE workspace_id=$1 AND team_id=$2 AND is_default").bind(w).bind(t).execute(&mut *invalid).await.unwrap();
    assert!(
        invalid.commit().await.is_err(),
        "Deferred exactly-one-default invariant"
    );
    let (unused, flow) = add(h, &a, w, t, "Unused").await;
    let flow = h
        .app
        .archive_status(&a, w, t, unused, &flow.etag())
        .await
        .unwrap();
    assert!(!flow.statuses.iter().any(|s| s.id == unused));
    let page = h
        .app
        .list_statuses(&a, w, t, req(json!({"state":"archived","limit":1})))
        .await
        .unwrap();
    assert_eq!(page.data[0].id, unused);
    let flow = h
        .app
        .restore_status(&a, w, t, unused, &flow.etag())
        .await
        .unwrap();
    assert!(flow.statuses.iter().any(|s| s.id == unused));
    let other = h
        .app
        .create_team(
            &a,
            w,
            "workflow-other-team",
            req(json!({"name":"Other","key":"OTH","private":false})),
        )
        .await
        .unwrap();
    let foreign = h.app.workflow(&a, w, other.id).await.unwrap().statuses[0].id;
    assert!(matches!(
        h.app
            .set_default_status(
                &a,
                w,
                t,
                &flow.etag(),
                SetDefaultStatus { status_id: foreign }
            )
            .await,
        Err(Error::NotFound)
    ));
    bounded_replacement(h, &a, w, t).await;
    retirement_race(h, &a, w, t, true).await;
    retirement_race(h, &a, w, t, false).await;
}
async fn bounded_replacement(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let (source, flow) = add(h, a, w, t, "Batch source").await;
    let target = flow
        .statuses
        .iter()
        .find(|s| s.category == "completed")
        .unwrap()
        .id;
    let mut tx = h.context(a.user.id, w).await;
    let number:i64=sqlx::query_scalar("UPDATE teams SET next_issue_number=next_issue_number+205 WHERE workspace_id=$1 AND id=$2 RETURNING next_issue_number-205").bind(w).bind(t).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank,archived_at,deleted_at) SELECT gen_random_uuid(),$1,$2,$3,$4+n,'Batch issue '||n,$5,($4+n)::numeric*1024,CASE WHEN n%3=0 THEN clock_timestamp() END,CASE WHEN n%7=0 THEN clock_timestamp() END FROM generate_series(0,204) n").bind(w).bind(t).bind(a.user.id).bind(number).bind(source).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let preview = h
        .app
        .preview_status_replacement(a, w, t, source, target)
        .await
        .unwrap();
    assert_eq!(preview.issue_count, "205");
    let key = Uuid::new_v4().to_string();
    let job = h
        .app
        .replace_status(
            a,
            w,
            t,
            source,
            &key,
            &flow.etag(),
            ReplaceStatus {
                replacement_status_id: target,
            },
        )
        .await
        .unwrap();
    assert_eq!(job.matched_count, "205");
    assert_eq!(job.state, "queued");
    assert_eq!(
        h.app
            .get_status(a, w, t, source)
            .await
            .unwrap()
            .replacement_job_id,
        Some(job.id)
    );
    assert_eq!(
        h.app
            .replace_status(
                a,
                w,
                t,
                source,
                &key,
                &flow.etag(),
                ReplaceStatus {
                    replacement_status_id: target
                }
            )
            .await
            .unwrap()
            .id,
        job.id
    );
    let live = h
        .app
        .list_issues(a, w, req(json!({"team_id":t,"status_id":source,"limit":1})))
        .await
        .unwrap()
        .data
        .remove(0);
    let edited = h
        .app
        .update_issue(
            a,
            w,
            live.id,
            &live.etag(),
            json!({"title":"Unrelated edit while retiring"}),
        )
        .await
        .unwrap();
    assert_eq!(edited.status_id, source);
    assert!(matches!(
        h.app
            .create_issue(
                a,
                w,
                "retiring-assignment-denied",
                req(json!({"team_id":t,"title":"Denied","status_id":source}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    for (index, expected) in [
        WorkerTick::Progress,
        WorkerTick::Progress,
        WorkerTick::Completed,
    ]
    .into_iter()
    .enumerate()
    {
        let claim = worker.claim(w).await.unwrap().unwrap();
        assert_eq!(claim.id, job.id);
        assert_eq!(worker.run_claim(&claim).await.unwrap(), expected);
        let receipt = h
            .app
            .get_workflow_replacement(a, w, t, job.id)
            .await
            .unwrap();
        assert_eq!(
            receipt.processed_count,
            ((index + 1) * 100).min(205).to_string()
        );
        assert_eq!(
            receipt.remaining_count,
            (205 - ((index + 1) * 100).min(205)).to_string()
        );
    }
    let done = h
        .app
        .get_workflow_replacement(a, w, t, job.id)
        .await
        .unwrap();
    assert_eq!(done.state, "succeeded");
    let old = h.app.get_status(a, w, t, source).await.unwrap();
    assert!(old.archived_at.is_some());
    assert_eq!(old.replacement_job_id, Some(job.id));
    let replacement = h.app.get_status(a, w, t, target).await.unwrap();
    let mut tx = h.context(a.user.id, w).await;
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3),(SELECT count(*) FROM activity_entries WHERE workspace_id=$1 AND changes->>'job_id'=$4),(SELECT attempt_count::bigint FROM jobs WHERE workspace_id=$1 AND id=$5)").bind(w).bind(t).bind(source).bind(job.id.to_string()).bind(job.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(counts, (0, 205, 1));
    let changes: Vec<Value> = sqlx::query_scalar(
        "SELECT changes FROM activity_entries WHERE workspace_id=$1 AND changes->>'job_id'=$2",
    )
    .bind(w)
    .bind(job.id.to_string())
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert!(changes.iter().all(|change| change == &json!({"fields":["status_id"],"status_id":{"before":source,"after":target},"status_names":{"before":old.name,"after":replacement.name},"source":"workflow_replacement","job_id":job.id})), "Background replacement retains its own source and captured status names");
    let versions:Vec<i64>=sqlx::query_scalar("SELECT version FROM issues WHERE workspace_id=$1 AND team_id=$2 AND title LIKE 'Batch issue %'").bind(w).bind(t).fetch_all(&mut *tx).await.unwrap();
    assert!(versions.iter().all(|v| *v == 2));
    tx.commit().await.unwrap();
    worker.pool.close().await;
}
async fn retirement_race(h: &Harness, a: &Actor, w: Uuid, t: Uuid, retire_first: bool) {
    let (source, flow) = add(
        h,
        a,
        w,
        t,
        if retire_first {
            "Retire first"
        } else {
            "Write first"
        },
    )
    .await;
    let target = flow
        .statuses
        .iter()
        .find(|s| s.category == "completed")
        .unwrap()
        .id;
    let gate = if retire_first { 871001_i64 } else { 871002_i64 };
    let function = if retire_first {
        "gate_retire"
    } else {
        "gate_issue"
    };
    let table = if retire_first {
        "workflow_statuses"
    } else {
        "issues"
    };
    let condition = if retire_first {
        format!(
            "NEW.id='{source}'::uuid AND NEW.retiring_at IS NOT NULL AND OLD.retiring_at IS NULL"
        )
    } else {
        format!("NEW.status_id='{source}'::uuid")
    };
    let op = if retire_first { "UPDATE" } else { "INSERT" };
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query(AssertSqlSafe(format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {condition} THEN PERFORM pg_advisory_xact_lock({gate}); END IF; RETURN NEW; END $$"))).execute(&mut *tx).await.unwrap();
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_workflow_gate BEFORE {op} ON {table} FOR EACH ROW EXECUTE FUNCTION {function}()"))).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut held = h.context(a.user.id, w).await;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(gate)
        .execute(&mut *held)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let retire = {
        let app = h.app.clone();
        let actor = a.clone();
        let tag = flow.etag();
        async move {
            app.replace_status(
                &actor,
                w,
                t,
                source,
                &Uuid::new_v4().to_string(),
                &tag,
                ReplaceStatus {
                    replacement_status_id: target,
                },
            )
            .await
        }
    };
    let create = {
        let app = h.app.clone();
        let actor = a.clone();
        async move {
            app.create_issue(
                &actor,
                w,
                &Uuid::new_v4().to_string(),
                req(json!({"team_id":t,"title":"Concurrent source assignment","status_id":source})),
            )
            .await
        }
    };
    let (retire, create) = if retire_first {
        let r = tokio::spawn(retire);
        let rpid = blocked_by(h, pid).await;
        let c = tokio::spawn(create);
        blocked_by(h, rpid).await;
        (r, c)
    } else {
        let c = tokio::spawn(create);
        let cpid = blocked_by(h, pid).await;
        let r = tokio::spawn(retire);
        blocked_by(h, cpid).await;
        (r, c)
    };
    held.commit().await.unwrap();
    let r = retire.await.unwrap().unwrap();
    let c = create.await.unwrap();
    if retire_first {
        assert!(matches!(c, Err(Error::Invalid(_))));
        assert_eq!(r.matched_count, "0")
    } else {
        assert!(c.is_ok());
        assert_eq!(r.matched_count, "1")
    }
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query(AssertSqlSafe(format!(
        "DROP TRIGGER test_workflow_gate ON {table}"
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(AssertSqlSafe(format!("DROP FUNCTION {function}()")))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
