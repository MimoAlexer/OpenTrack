//! Populated 0026 -> 0027 upgrade, numeric allocation and maintenance exclusion.
#[allow(dead_code)]
mod support;

use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::Actor;
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

async fn runtime(h: &Harness, a: &Actor, w: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, a.user.id, Some(w)).await.unwrap();
    tx
}

async fn snapshot(h: &Harness, a: &Actor, w: Uuid) -> Vec<Value> {
    let mut tx = h.context(a.user.id, w).await;
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
    ] {
        let projection = match table {
            "issues" => "to_jsonb(x)-'rank'",
            "teams" => "to_jsonb(x)-'ordering_version'",
            _ => "to_jsonb(x)",
        };
        let rows = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT coalesce(jsonb_agg(data ORDER BY data::text),'[]'::jsonb) FROM (SELECT {projection} AS data FROM {table} x WHERE workspace_id=$1) rows"
        )))
        .bind(w).fetch_one(&mut *tx).await.unwrap();
        result.push(rows);
    }
    tx.commit().await.unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn populated_issue_order_upgrade_and_inline_bounds() {
    let h = Harness::with_version(5, 26).await;
    let copy = h.clone();
    let result = tokio::spawn(async move { exercise(&copy).await }).await;
    if let Err(error) = result {
        // Failed fixture schemas remain available for diagnosis in the isolated DB.
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}

async fn exercise(h: &Harness) {
    let (session, token) = h
        .app
        .setup(req(json!({
            "setup_token":h.setup_token,"email":"owner@order-upgrade.example.test",
            "display_name":"Example Owner","password":"Synthetic order upgrade password 2026!",
            "workspace_name":"Order upgrade","team_name":"Initial team","team_key":"ORD"
        })))
        .await
        .unwrap();
    let actor = h.app.authenticate(&token).await.unwrap();
    let w = session.workspaces[0].id;
    let mut tx = h.context(actor.user.id, w).await;
    let (team, organization): (Uuid, Uuid) = sqlx::query_as("SELECT t.id,w.organization_id FROM teams t JOIN workspaces w ON w.id=t.workspace_id WHERE w.id=$1")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let private = h
        .app
        .create_team(
            &actor,
            w,
            "order-private-team",
            req(json!({"name":"Private order","key":"PRV","private":true})),
        )
        .await
        .unwrap();
    let other = h.app.create_workspace(&actor, "order-other-workspace", req(json!({"organization_id":organization,"name":"Other order","team_name":"Other team","team_key":"OTH"}))).await.unwrap();
    let mut tx = h.context(actor.user.id, other.id).await;
    let other_team: Uuid = sqlx::query_scalar("SELECT id FROM teams WHERE workspace_id=$1")
        .bind(other.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut groups = Vec::new();
    for (workspace, team_id) in [(w, team), (w, private.id), (other.id, other_team)] {
        let mut tx = h.context(actor.user.id, workspace).await;
        let statuses: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 ORDER BY position,id LIMIT 2")
            .bind(workspace).bind(team_id).fetch_all(&mut *tx).await.unwrap();
        assert_eq!(statuses.len(), 2);
        for (index, status) in statuses.into_iter().enumerate() {
            let count = if team_id == team {
                500 + index as i32
            } else {
                3
            };
            sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,description,status_id,version,created_at,archived_at,deleted_at) SELECT gen_random_uuid(),$1,$2,$3,$4+n,'Existing order issue','Preserved body', $5,7,CASE WHEN n%3=0 THEN '2026-09-01'::timestamptz ELSE '2026-10-01'::timestamptz END,CASE WHEN n=$6+1 THEN clock_timestamp() END,CASE WHEN n=$6+2 THEN clock_timestamp() END FROM generate_series(1,$6+2) n")
                .bind(workspace).bind(team_id).bind(actor.user.id).bind(index as i64 * 10000).bind(status).bind(count).execute(&mut *tx).await.unwrap();
            let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 ORDER BY created_at DESC,id DESC")
                .bind(workspace).bind(team_id).bind(status).fetch_all(&mut *tx).await.unwrap();
            groups.push((workspace, team_id, status, count, ids));
        }
        tx.commit().await.unwrap();
    }
    let before = snapshot(h, &actor, w).await;
    let other_before = snapshot(h, &actor, other.id).await;
    let mut connection = h.owner.acquire().await.unwrap();
    connection.close_on_drop();
    store::MIGRATOR
        .run_direct(None, &mut *connection, false)
        .await
        .unwrap();
    connection.close().await.unwrap();
    assert!(
        snapshot(h, &actor, w).await == before,
        "Logical data and effects changed during backfill"
    );
    assert!(
        snapshot(h, &actor, other.id).await == other_before,
        "Other workspace data changed during backfill"
    );
    let forced: bool = sqlx::query_scalar("SELECT bool_and(relforcerowsecurity) FROM pg_class WHERE oid IN('workspaces'::regclass,'teams'::regclass,'issues'::regclass,'issue_order_staging'::regclass)")
        .fetch_one(&h.owner).await.unwrap();
    assert!(forced);
    for (workspace, team_id, status, _, expected) in &groups {
        let mut tx = runtime(h, &actor, *workspace).await;
        let rows: Vec<(Uuid, String)> = sqlx::query_as("SELECT i.id,i.rank::text FROM issues i WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.status_id=$3 ORDER BY i.rank,i.id")
            .bind(workspace).bind(team_id).bind(status).fetch_all(&mut *tx).await.unwrap();
        assert!(rows.iter().map(|r| r.0).eq(expected.iter().copied()));
        for (index, (_, rank)) in rows.iter().enumerate() {
            assert_eq!(rank, &format!("{}.000000000000000000", (index + 1) * 1024));
        }
        tx.commit().await.unwrap();
    }
    for (workspace, team_id, status, count, _) in groups.iter().take(2) {
        let mut tx = h.context(actor.user.id, *workspace).await;
        sqlx::query("UPDATE issues SET rank=99999999999999999999 WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 AND archived_at IS NULL AND deleted_at IS NULL")
            .bind(workspace).bind(team_id).bind(status).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = runtime(h, &actor, *workspace).await;
        assert!(
            store::append_issue_rank(&mut tx, *workspace, *team_id, *status)
                .await
                .unwrap()
                .is_none()
        );
        let normalized: bool = sqlx::query_scalar("SELECT issue_order_rebalance_inline($1,$2,$3)")
            .bind(workspace)
            .bind(team_id)
            .bind(status)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(normalized, *count == 500);
        if !normalized {
            let retained: Vec<String> = sqlx::query_scalar("SELECT rank::text FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 AND archived_at IS NULL AND deleted_at IS NULL")
                .bind(workspace).bind(team_id).bind(status).fetch_all(&mut *tx).await.unwrap();
            assert_eq!(retained.len(), 501);
            assert!(
                retained
                    .iter()
                    .all(|rank| rank == "99999999999999999999.000000000000000000")
            );
        }
        let next = store::append_issue_rank(&mut tx, *workspace, *team_id, *status)
            .await
            .unwrap();
        assert_eq!(
            next,
            (*count == 500).then(|| "513024.000000000000000000".into())
        );
        tx.commit().await.unwrap();
    }
    assert!(
        snapshot(h, &actor, w).await == before,
        "Physical normalization changed logical data or effects"
    );
    for (sql, constraint) in [
        (
            "UPDATE issues SET rank='NaN',version=version+1 WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 AND archived_at IS NULL AND deleted_at IS NULL",
            "issue_rank_finite",
        ),
        (
            "UPDATE issues SET rank=-1 WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 AND archived_at IS NULL AND deleted_at IS NULL",
            "issue_rank_version",
        ),
    ] {
        let mut tx = runtime(h, &actor, w).await;
        let error = sqlx::query(sql)
            .bind(w)
            .bind(team)
            .bind(groups[0].2)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some(constraint)
        );
        tx.rollback().await.unwrap();
    }
    let (_, _, status, _, _) = &groups[1];
    let mut tx = runtime(h, &actor, w).await;
    let live: Uuid = sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 AND archived_at IS NULL AND deleted_at IS NULL ORDER BY id LIMIT 1")
        .bind(w).bind(team).bind(status).fetch_one(&mut *tx).await.unwrap();
    let requester = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,'member@order-upgrade.example.test','Example Member',password_hash FROM users WHERE id=$2")
        .bind(requester).bind(actor.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'member')")
        .bind(w)
        .bind(requester)
        .execute(&mut *tx)
        .await
        .unwrap();
    let job = Uuid::new_v4();
    sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'issues.order_normalize',$3,$2::text)")
        .bind(w).bind(job).bind(requester).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issue_order_normalizations(workspace_id,id,team_id,status_id,original_requested_by) VALUES($1,$2,$3,$4,$5)")
        .bind(w).bind(job).bind(team).bind(status).bind(requester).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO issue_order_guards(workspace_id,team_id,status_id,job_id) VALUES($1,$2,$3,$4)",
    )
    .bind(w)
    .bind(team)
    .bind(status)
    .bind(job)
    .execute(&mut *tx)
    .await
    .unwrap();
    let generation: i64 =
        sqlx::query_scalar("SELECT ordering_version FROM teams WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(team)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("UPDATE issues SET title='Edited during maintenance',version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(w).bind(live).execute(&mut *tx).await.unwrap();
    let advanced: i64 =
        sqlx::query_scalar("SELECT ordering_version FROM teams WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(team)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(advanced, generation + 1);
    tx.commit().await.unwrap();
    for sql in [
        "UPDATE issues SET archived_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2",
        "UPDATE issues SET rank=0,version=version+1 WHERE workspace_id=$1 AND id=$2",
    ] {
        let mut tx = runtime(h, &actor, w).await;
        let error = sqlx::query(sql)
            .bind(w)
            .bind(live)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("issue_order_maintenance")
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = runtime(h, &actor, w).await;
    let isolated: bool = sqlx::query_scalar("SELECT NOT has_function_privilege(current_user,'worker_issue_order_batch(uuid,uuid,uuid,uuid,bigint)','EXECUTE')")
        .fetch_one(&mut *tx).await.unwrap();
    assert!(isolated);
    let error = sqlx::query("SELECT * FROM issue_order_staging")
        .fetch_all(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
    let mut tx = runtime(h, &actor, other.id).await;
    let hidden: i64 =
        sqlx::query_scalar("SELECT count(*) FROM issue_order_normalizations WHERE workspace_id=$1")
            .bind(w)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(hidden, 0);
    tx.commit().await.unwrap();
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    let limited: bool = sqlx::query_scalar("SELECT NOT has_column_privilege(current_user,'issues','rank','UPDATE') AND NOT has_column_privilege(current_user,'teams','ordering_version','UPDATE') AND NOT has_table_privilege(current_user,'issue_order_staging','SELECT,INSERT,UPDATE,DELETE') AND NOT has_function_privilege(current_user,'issue_order_rebalance_inline(uuid,uuid,uuid)','EXECUTE')")
        .fetch_one(&worker.pool).await.unwrap();
    assert!(limited);
    let before_worker = snapshot(h, &actor, w).await;
    let first = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(
        worker.run_claim(&first).await.unwrap(),
        WorkerTick::Progress
    );
    sqlx::query("UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1")
        .bind(requester)
        .execute(&h.owner)
        .await
        .unwrap();
    let denied = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(
        worker.run_claim(&denied).await.unwrap(),
        WorkerTick::Blocked
    );
    let mut tx = h.context(actor.user.id, w).await;
    let blocked: (String, i64, i64, Uuid) = sqlx::query_as("SELECT j.state,n.staged_count,n.applied_count,n.original_requested_by FROM jobs j JOIN issue_order_normalizations n USING(workspace_id,id) JOIN issue_order_guards g ON g.workspace_id=n.workspace_id AND g.job_id=n.id WHERE n.workspace_id=$1 AND n.id=$2")
        .bind(w).bind(job).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(blocked, ("blocked".into(), 100, 0, requester));
    tx.commit().await.unwrap();
    // Fixture requeue tests preserved storage; the manager-only resume API is separate.
    sqlx::query("UPDATE users SET disabled_at=NULL WHERE id=$1")
        .bind(requester)
        .execute(&h.owner)
        .await
        .unwrap();
    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("UPDATE jobs SET state='queued',error_code=NULL,finished_at=NULL,available_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(w).bind(job).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut staged = 100;
    let mut applied = 0;
    let mut finished = false;
    let mut previous_claim = None;
    for _ in 0..14 {
        let claim = worker.claim(w).await.unwrap().unwrap();
        assert_eq!(claim.id, job);
        if let Some(previous) = previous_claim.take() {
            assert_eq!(
                worker.run_claim(&previous).await.unwrap(),
                WorkerTick::LostLease
            );
        }
        let result = worker.run_claim(&claim).await.unwrap();
        assert!(matches!(
            result,
            WorkerTick::Progress | WorkerTick::Completed
        ));
        let mut tx = h.context(actor.user.id, w).await;
        let (phase, staged_now, applied_now, total): (String, i64, i64, Option<i64>) = sqlx::query_as("SELECT phase,staged_count,applied_count,total_count FROM issue_order_normalizations WHERE workspace_id=$1 AND id=$2")
            .bind(w).bind(job).fetch_one(&mut *tx).await.unwrap();
        assert!((0..=100).contains(&(staged_now - staged)));
        assert!((0..=100).contains(&(applied_now - applied)));
        assert!(staged_now == staged || applied_now == applied);
        assert_eq!(total, (phase != "staging").then_some(501));
        staged = staged_now;
        applied = applied_now;
        tx.commit().await.unwrap();
        if result == WorkerTick::Completed {
            finished = true;
            break;
        }
        previous_claim = Some(claim);
    }
    assert!(finished);
    assert_eq!((staged, applied), (501, 501));
    assert!(
        snapshot(h, &actor, w).await == before_worker,
        "Worker normalization changed logical issue data or effects"
    );
    let mut tx = runtime(h, &actor, w).await;
    let guards: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM issue_order_guards WHERE workspace_id=$1 AND job_id=$2",
    )
    .bind(w)
    .bind(job)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(guards, 0);
    assert_eq!(
        store::append_issue_rank(&mut tx, w, team, *status)
            .await
            .unwrap(),
        Some("514048.000000000000000000".into())
    );
    tx.commit().await.unwrap();
    worker.pool.close().await;
    let fresh = opentrack_app::App::connect(&h.runtime_url(), &h.setup_token, 2, 1)
        .await
        .unwrap();
    fresh.pool.close().await;
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    let role = role.replace('"', "\"\"");
    sqlx::query(AssertSqlSafe(format!(
        "GRANT SELECT ON issue_order_staging TO \"{role}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    let unsafe_runtime = opentrack_app::App::connect(&h.runtime_url(), &h.setup_token, 2, 1).await;
    sqlx::query(AssertSqlSafe(format!(
        "REVOKE SELECT ON issue_order_staging FROM \"{role}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    match unsafe_runtime {
        Ok(unexpected) => {
            unexpected.pool.close().await;
            panic!("Serving role must reject staging authority");
        }
        Err(error) => assert!(matches!(error, opentrack_domain::Error::Internal)),
    }
}
