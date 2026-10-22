mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::{req, setup};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::json;
use support::Harness;
use uuid::Uuid;
async fn blocked(h: &Harness, pid: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(&h.owner)
            .await
            .unwrap();
            if waiting {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe actual queued PostgreSQL lock");
}
async fn direct_insert(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    start: &str,
    end: &str,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO cycles(id,workspace_id,team_id,name,start_date,end_date_exclusive,timezone) VALUES($1,$2,$3,'Concurrent interval',$4::text::date,$5::text::date,'UTC')").bind(Uuid::new_v4()).bind(w).bind(t).bind(start).bind(end).execute(&mut **tx).await?;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_exclusion_serializes_direct_overlapping_inserts() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exclusion(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exclusion(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let mut holder = h.context(a.user.id, w).await;
    direct_insert(&mut holder, w, t, "2026-02-01", "2026-02-08")
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let pool = h.app.pool.clone();
    let actor = a.clone();
    let task = tokio::spawn(async move {
        let mut tx = store::begin(&pool).await.unwrap();
        store::context(&mut tx, actor.user.id, Some(w))
            .await
            .unwrap();
        let r = direct_insert(&mut tx, w, t, "2026-02-07", "2026-02-15").await;
        tx.rollback().await.unwrap();
        r
    });
    blocked(h, pid).await;
    holder.commit().await.unwrap();
    let failure = task.await.unwrap().unwrap_err();
    assert_eq!(
        failure.as_database_error().unwrap().code().as_deref(),
        Some("23P01")
    );
    let mut tx = h.context(a.user.id, w).await;
    direct_insert(&mut tx, w, t, "2026-02-08", "2026-02-15")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let list = h
        .app
        .list_cycles(&a, w, t, CycleQuery::default())
        .await
        .unwrap();
    assert_eq!(list.data.len(), 2);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn closing_waits_for_issue_write_and_rejects_the_stale_preview() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { close_race(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn close_race(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let i = graph_support::issue(h, &a, w, t, json!({})).await;
    let c=h.app.create_cycle(&a,w,t,"racing-cycle-create",req(json!({"name":"Review interval","start_date":"2026-06-01","end_date_exclusive":"2026-06-08","timezone":"UTC"}))).await.unwrap();
    let c = h
        .app
        .set_cycle_issue(
            &a,
            w,
            t,
            c.id,
            i.id,
            &c.etag,
            CycleMembership {
                included: true,
                issue_etag: i.etag(),
            },
        )
        .await
        .unwrap();
    let c = h.app.open_cycle(&a, w, t, c.id, &c.etag).await.unwrap();
    let preview = h.app.cycle_close_preview(&a, w, t, c.id).await.unwrap();
    let mut holder = h.context(a.user.id, w).await;
    sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
        .bind(w)
        .bind(t)
        .execute(&mut *holder)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let app = h.app.clone();
    let actor = a.clone();
    let cycle = c.clone();
    let task = tokio::spawn(async move {
        app.close_cycle(
            &actor,
            w,
            t,
            cycle.id,
            &cycle.etag,
            "concurrent-close-request",
            CloseCycle {
                review_token: preview.review_token,
            },
        )
        .await
    });
    blocked(h, pid).await;
    sqlx::query("UPDATE issues SET priority=2,version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(i.id)
        .execute(&mut *holder)
        .await
        .unwrap();
    holder.commit().await.unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::Conflict(_))));
    assert_eq!(
        h.app.get_cycle(&a, w, t, c.id).await.unwrap().state,
        CycleState::Active
    );
    let p = h.app.cycle_close_preview(&a, w, t, c.id).await.unwrap();
    let (app1, app2) = (h.app.clone(), h.app.clone());
    let (a1, a2) = (a.clone(), a.clone());
    let (c1, c2) = (c.clone(), c.clone());
    let (p1, p2) = (p.review_token.clone(), p.review_token);
    let (left, right) = tokio::join!(
        app1.close_cycle(
            &a1,
            w,
            t,
            c1.id,
            &c1.etag,
            "double-close-left",
            CloseCycle { review_token: p1 }
        ),
        app2.close_cycle(
            &a2,
            w,
            t,
            c2.id,
            &c2.etag,
            "double-close-right",
            CloseCycle { review_token: p2 }
        )
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert!(matches!(
        left.as_ref().err().or(right.as_ref().err()),
        Some(Error::VersionConflict(_))
    ));
    let mut tx = h.context(a.user.id, w).await;
    let reports: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cycle_close_reports WHERE workspace_id=$1 AND cycle_id=$2",
    )
    .bind(w)
    .bind(c.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(reports, 1);
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn cycle_upgrade_preserves_existing_issue_data() {
    let h = Harness::with_version(5, 22).await;
    let copy = h.clone();
    let run=tokio::spawn(async move{let(a,w,t)=setup(&copy).await;let i=graph_support::issue(&copy,&a,w,t,json!({})).await;store::MIGRATOR.run(&copy.owner).await.unwrap();assert_eq!(copy.app.get_issue(&a,w,i.id).await.unwrap().title,i.title);let c=copy.app.create_cycle(&a,w,t,"upgraded-cycle-create",req(json!({"name":"Upgraded interval","start_date":"2026-07-01","end_date_exclusive":"2026-07-08","timezone":"UTC"}))).await.unwrap();assert_eq!(c.state,CycleState::Planned);}).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cycle_migration_supports_concurrent_fresh_application_schemas() {
    let (left, right) = tokio::join!(Harness::new(), Harness::new());
    let check = async {
        for h in [&left, &right] {
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM pg_extension e JOIN pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='btree_gist' AND n.nspname='opentrack_extensions'").fetch_one(&h.owner).await.unwrap();
            assert_eq!(count, 1);
            let exists:bool=sqlx::query_scalar("SELECT to_regclass('cycles') IS NOT NULL AND to_regclass('cycle_close_reports') IS NOT NULL").fetch_one(&h.owner).await.unwrap();
            assert!(exists);
        }
    };
    check.await;
    left.cleanup().await;
    right.cleanup().await;
}

#[tokio::test]
async fn historical_cycle_facts_and_receipts_reauthorize_private_moved_sources() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { historical_privacy(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn historical_privacy(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let i = graph_support::issue(h, &a, w, t, json!({"estimate":5})).await;
    let create = |name: &str, start: &str, end: &str| {
        req(json!({"name":name,"start_date":start,"end_date_exclusive":end,"timezone":"UTC"}))
    };
    let c = h
        .app
        .create_cycle(
            &a,
            w,
            t,
            "private-history-cycle",
            create("Historical interval", "2026-08-01", "2026-08-08"),
        )
        .await
        .unwrap();
    let dest = h
        .app
        .create_cycle(
            &a,
            w,
            t,
            "private-next-cycle",
            create("Next interval", "2026-08-08", "2026-08-15"),
        )
        .await
        .unwrap();
    let c = h
        .app
        .set_cycle_issue(
            &a,
            w,
            t,
            c.id,
            i.id,
            &c.etag,
            CycleMembership {
                included: true,
                issue_etag: i.etag(),
            },
        )
        .await
        .unwrap();
    let c = h.app.open_cycle(&a, w, t, c.id, &c.etag).await.unwrap();
    let preview = h.app.cycle_close_preview(&a, w, t, c.id).await.unwrap();
    let close_req = CloseCycle {
        review_token: preview.review_token,
    };
    let r = h
        .app
        .close_cycle(
            &a,
            w,
            t,
            c.id,
            &c.etag,
            "private-close-receipt",
            close_req.clone(),
        )
        .await
        .unwrap();
    let current = h.app.get_issue(&a, w, i.id).await.unwrap();
    let carry_req = CarryForwardCycle {
        report_id: r.id,
        destination_cycle_id: dest.id,
        destination_etag: dest.etag.clone(),
        items: vec![CarryForwardItem {
            issue_id: i.id,
            issue_version: "1".into(),
        }],
    };
    let closed = h.app.get_cycle(&a, w, t, c.id).await.unwrap();
    let op = h
        .app
        .carry_forward_cycle(
            &a,
            w,
            t,
            c.id,
            &closed.etag,
            "private-carry-receipt",
            carry_req.clone(),
        )
        .await
        .unwrap();
    assert_eq!(op.state, "needs_review");
    let resume_req = ResumeCycleCarry {
        destination_etag: dest.etag.clone(),
        items: vec![CarryForwardItem {
            issue_id: i.id,
            issue_version: current.version.clone(),
        }],
    };
    let original_op_etag = op.etag.clone();
    let op = h
        .app
        .resume_cycle_carry(
            &a,
            w,
            t,
            c.id,
            op.id,
            &original_op_etag,
            "private-resume-receipt",
            resume_req.clone(),
        )
        .await
        .unwrap();
    assert_eq!(op.state, "complete");
    let hidden_team = graph_support::team(h, &a, w, true).await;
    assert!(matches!(
        h.app.preview_issue_move(&a, w, i.id, hidden_team).await,
        Err(Error::Conflict(_))
    ));
    let next = h.app.get_cycle(&a, w, t, dest.id).await.unwrap();
    let current = h.app.get_issue(&a, w, i.id).await.unwrap();
    h.app
        .set_cycle_issue(
            &a,
            w,
            t,
            dest.id,
            i.id,
            &next.etag,
            CycleMembership {
                included: false,
                issue_etag: current.etag(),
            },
        )
        .await
        .unwrap();
    let other = graph_support::person(h, &a, w, "member").await;
    graph_support::grant(h, &a, w, hidden_team, other.user.id).await;
    let movement = h
        .app
        .preview_issue_move(&a, w, i.id, hidden_team)
        .await
        .unwrap();
    h.app.move_issue(&a,w,i.id,"historical-private-move",&movement.issue_etag,req(json!({"destination_team_id":hidden_team,"preview_token":movement.preview_token,"status_id":movement.destination_default_status_id,"assignee_id":null,"label_ids":[]}))).await.unwrap();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE team_members SET role='maintainer' WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3").bind(w).bind(hidden_team).bind(other.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(w)
        .bind(hidden_team)
        .bind(a.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app.get_issue(&a, w, i.id).await,
        Err(Error::NotFound)
    ));
    let report = h.app.cycle_report(&a, w, t, c.id, r.id).await.unwrap();
    assert!(report.scope.redacted);
    assert_eq!(report.scope.scope_count, 0);
    assert_eq!(report.scope.estimate_total, 0);
    assert!(report.scope.items.is_empty());
    let history = h
        .app
        .cycle_snapshots(&a, w, t, c.id, CycleQuery::default())
        .await
        .unwrap();
    assert!(
        history
            .data
            .iter()
            .all(|s| s.scope_count == 0 && s.estimate_total == 0)
    );
    let receipt = h
        .app
        .close_cycle(&a, w, t, c.id, &c.etag, "private-close-receipt", close_req)
        .await
        .unwrap();
    assert!(receipt.scope.items.is_empty());
    assert_eq!(receipt.scope.scope_count, 0);
    let operation = h
        .app
        .cycle_carry_operation(&a, w, t, c.id, op.id)
        .await
        .unwrap();
    assert!(operation.redacted);
    assert_eq!(operation.state, "restricted");
    assert!(operation.results.is_empty());
    assert_ne!(operation.etag, op.etag);
    let receipt = h
        .app
        .carry_forward_cycle(
            &a,
            w,
            t,
            c.id,
            &closed.etag,
            "private-carry-receipt",
            carry_req,
        )
        .await
        .unwrap();
    assert!(receipt.results.is_empty());
    let receipt = h
        .app
        .resume_cycle_carry(
            &a,
            w,
            t,
            c.id,
            op.id,
            &original_op_etag,
            "private-resume-receipt",
            resume_req,
        )
        .await
        .unwrap();
    assert!(receipt.results.is_empty());

    let mut tx = h.context(a.user.id, w).await;
    let stored: i64 = sqlx::query_scalar(
        "SELECT scope_count FROM cycle_snapshots WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(r.scope.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(stored, 1);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn bounded_cycle_history_projects_fifty_ten_thousand_issue_snapshots() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { bounded_history(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn bounded_history(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let c=h.app.create_cycle(&a,w,t,"bounded-cycle-create",req(json!({"name":"Bounded interval","start_date":"2026-01-01","end_date_exclusive":"2026-03-01","timezone":"UTC"}))).await.unwrap();
    let status = h
        .app
        .bootstrap(&a, w)
        .await
        .unwrap()
        .statuses
        .iter()
        .find(|s| s.team_id == t && s.category == "unstarted")
        .unwrap()
        .id;
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("SELECT set_config('statement_timeout','30000',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,estimate) SELECT gen_random_uuid(),$1,$2,$3,n,'Bounded work',$4,1 FROM generate_series(1,10000) n").bind(w).bind(t).bind(a.user.id).bind(status).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO cycle_issues(workspace_id,team_id,cycle_id,issue_id) SELECT $1,$2,$3,id FROM issues WHERE workspace_id=$1 AND team_id=$2").bind(w).bind(t).bind(c.id).execute(&mut *tx).await.unwrap();
    sqlx::query("WITH facts AS(SELECT jsonb_agg(jsonb_build_object('issue_id',id,'issue_version',version::text,'category','unstarted','excluded',false,'estimate',estimate) ORDER BY id) AS items FROM issues WHERE workspace_id=$1 AND team_id=$2) INSERT INTO cycle_snapshots(id,workspace_id,team_id,cycle_id,local_date,captured_at,kind,definition_version,scope_count,completed_count,canceled_count,unfinished_count,excluded_count,estimate_total,items) SELECT gen_random_uuid(),$1,$2,$3,'2026-01-01'::date+n,clock_timestamp()+n*interval '1 second','daily',1,10000,0,0,10000,0,10000,items FROM facts CROSS JOIN generate_series(0,49) n").bind(w).bind(t).bind(c.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let history = h
        .app
        .cycle_snapshots(
            &a,
            w,
            t,
            c.id,
            CycleQuery {
                limit: Some(50),
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(history.data.len(), 50);
    assert!(
        history
            .data
            .iter()
            .all(|s| s.scope_count == 10000 && s.estimate_total == 10000 && !s.redacted)
    );
    let wire = serde_json::to_value(&history).unwrap();
    assert!(
        wire["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s.get("items").is_none())
    );
    assert!(serde_json::to_vec(&history).unwrap().len() < 64 * 1024);
    let scope = h.app.cycle_scope(&a, w, t, c.id).await.unwrap();
    assert_eq!(scope.items.len(), 10000);
    let size = serde_json::to_vec(&scope).unwrap().len();
    assert!(size > 1024 * 1024 && size < 4 * 1024 * 1024);
    let issues = h
        .app
        .cycle_issues(
            &a,
            w,
            t,
            c.id,
            CycleQuery {
                limit: Some(100),
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(issues.data.len(), 100);
    assert!(issues.has_more);
    assert!(serde_json::to_vec(&issues).unwrap().len() < 128 * 1024);
}

#[tokio::test]
async fn cycle_index_extension_does_not_expand_runtime_or_worker_authority() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run=tokio::spawn(async move{
        let(a,w,t)=setup(&copy).await;
        let privilege:bool=sqlx::query_scalar("SELECT has_schema_privilege(current_user,'opentrack_extensions','USAGE,CREATE')").fetch_one(&copy.app.pool).await.unwrap();assert!(!privilege);
        copy.app.create_cycle(&a,w,t,"isolated-extension-cycle",req(json!({"name":"Isolated index","start_date":"2026-12-01","end_date_exclusive":"2026-12-08","timezone":"UTC"}))).await.unwrap();
        let worker=opentrack_app::worker::Worker::connect(&copy.worker_url().await,opentrack_app::worker::WorkerOptions::default()).await.unwrap();
        let privilege:bool=sqlx::query_scalar("SELECT has_schema_privilege(current_user,'opentrack_extensions','USAGE,CREATE')").fetch_one(&worker.pool).await.unwrap();assert!(!privilege);
        worker.capture(w).await.unwrap();
        let mut tx=copy.context(a.user.id,w).await;let captured:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM consumer_checkpoints c JOIN workspace_stream_heads h USING(workspace_id) WHERE c.workspace_id=$1 AND c.last_seq=h.last_seq)").bind(w).fetch_one(&mut *tx).await.unwrap();assert!(captured);tx.rollback().await.unwrap();worker.pool.close().await;
    }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
