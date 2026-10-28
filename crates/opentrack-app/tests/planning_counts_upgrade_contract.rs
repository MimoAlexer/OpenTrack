#[allow(dead_code)]
mod support;
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;
fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn source(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(a.user.id, w).await;
    let data=sqlx::query_scalar("SELECT jsonb_build_object('issues',(SELECT jsonb_agg(to_jsonb(i) ORDER BY id) FROM issues i WHERE workspace_id=$1),'projects',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM projects p WHERE workspace_id=$1),'project_links',(SELECT jsonb_agg(to_jsonb(p) ORDER BY issue_id) FROM project_issues p WHERE workspace_id=$1),'milestones',(SELECT jsonb_agg(to_jsonb(m) ORDER BY id) FROM milestones m WHERE workspace_id=$1),'cycles',(SELECT jsonb_agg(to_jsonb(c) ORDER BY id) FROM cycles c WHERE workspace_id=$1),'cycle_links',(SELECT jsonb_agg(to_jsonb(c) ORDER BY issue_id) FROM cycle_issues c WHERE workspace_id=$1),'snapshots',(SELECT jsonb_agg(to_jsonb(s) ORDER BY id) FROM cycle_snapshots s WHERE workspace_id=$1),'documents',(SELECT jsonb_agg(to_jsonb(d) ORDER BY id) FROM documents d WHERE workspace_id=$1),'events',(SELECT jsonb_agg(to_jsonb(e) ORDER BY seq) FROM workspace_events e WHERE workspace_id=$1),'head',(SELECT to_jsonb(h) FROM workspace_stream_heads h WHERE workspace_id=$1),'assignment_registration',(SELECT to_jsonb(r) FROM consumer_registrations r WHERE workspace_id=$1 AND consumer_name='assignment_cleanup'),'assignment_checkpoint',(SELECT to_jsonb(c) FROM consumer_checkpoints c WHERE workspace_id=$1 AND consumer_name='assignment_cleanup'))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    data
}
async fn ledger(h: &Harness) -> Value {
    sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('version',version,'checksum',encode(checksum,'hex'),'success',success) ORDER BY version) FROM _sqlx_migrations").fetch_one(&h.owner).await.unwrap()
}
async fn migrate(h: &Harness) -> std::result::Result<(), sqlx::migrate::MigrateError> {
    let mut connection = h.owner.acquire().await.unwrap();
    connection.close_on_drop();
    let result = sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 28)
            .cloned()
            .collect::<Vec<_>>()
            .into(),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run_direct(None, &mut *connection, false)
    .await;
    connection.close().await.unwrap();
    result
}
#[tokio::test]
async fn populated_27_to_28_enrollment_is_atomic_and_preserves_source_history() {
    let h = Harness::with_version(6, 27).await;
    tokio::time::timeout(std::time::Duration::from_secs(40), upgrade(&h))
        .await
        .expect("Bounded planning upgrade fixture");
    h.cleanup().await;
}

async fn upgrade(h: &Harness) {
    let(session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@example.test","password":"Planning upgrade contract password 947!","display_name":"Example Owner","workspace_name":"Planning upgrade","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let a = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let t = h.app.bootstrap(&a, w).await.unwrap().teams[0].id;
    let mut p = h
        .app
        .create_project(
            &a,
            w,
            t,
            "upgrade-project",
            req(json!({"name":"Preserved planning source"})),
        )
        .await
        .unwrap();
    let m = h
        .app
        .create_milestone(
            &a,
            w,
            t,
            p.id,
            "upgrade-milestone",
            &p.etag,
            req(json!({"title":"Preserved milestone"})),
        )
        .await
        .unwrap();
    p = h.app.get_project(&a, w, t, p.id).await.unwrap();
    let i = h
        .app
        .create_issue(
            &a,
            w,
            "upgrade-issue",
            req(json!({"team_id":t,"title":"Preserved issue","estimate":5})),
        )
        .await
        .unwrap();
    p = h
        .app
        .set_project_issue(
            &a,
            w,
            t,
            p.id,
            i.id,
            &p.etag,
            ProjectMembership { included: true },
        )
        .await
        .unwrap();
    h.app
        .assign_milestone(
            &a,
            w,
            t,
            p.id,
            i.id,
            &p.etag,
            MilestoneAssignment {
                milestone_id: Some(m.id),
            },
        )
        .await
        .unwrap();
    let mut c=h.app.create_cycle(&a,w,t,"upgrade-cycle",req(json!({"name":"Preserved interval","start_date":"2026-10-01","end_date_exclusive":"2026-10-15","timezone":"UTC"}))).await.unwrap();
    c = h
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
                issue_etag: h.app.get_issue(&a, w, i.id).await.unwrap().etag(),
            },
        )
        .await
        .unwrap();
    c = h.app.open_cycle(&a, w, t, c.id, &c.etag).await.unwrap();
    h.app
        .capture_cycle_snapshot(&a, w, t, c.id, &c.etag)
        .await
        .unwrap();
    let organization = h.app.organizations(&a).await.unwrap()[0].id;
    let second=h.app.create_workspace(&a,"upgrade-second-workspace",req(json!({"organization_id":organization,"name":"Second enrolled workspace","team_name":"Operations","team_key":"OPS"}))).await.unwrap().id;
    let before = source(h, &a, w).await;
    let second_before = source(h, &a, second).await;
    let before_ledger = ledger(h).await;
    let old_function: String = sqlx::query_scalar(
        "SELECT pg_get_functiondef('register_workspace_consumers()'::regprocedure)",
    )
    .fetch_one(&h.owner)
    .await
    .unwrap();
    sqlx::raw_sql("CREATE FUNCTION test_planning_enrollment_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.consumer_name='planning_counts' THEN RETURN NULL;END IF;RETURN NEW;END $$;CREATE TRIGGER test_planning_enrollment_suppress BEFORE INSERT ON consumer_registrations FOR EACH ROW EXECUTE FUNCTION test_planning_enrollment_suppress();").execute(&h.owner).await.unwrap();
    assert!(migrate(h).await.is_err());
    assert_eq!(ledger(h).await, before_ledger);
    assert_eq!(source(h, &a, w).await, before);
    assert_eq!(source(h, &a, second).await, second_before);
    assert!(sqlx::query_scalar::<_,bool>("SELECT to_regclass('planning_issue_facts') IS NULL AND to_regclass('planning_backfills') IS NULL").fetch_one(&h.owner).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT pg_get_functiondef('register_workspace_consumers()'::regprocedure)"
        )
        .fetch_one(&h.owner)
        .await
        .unwrap(),
        old_function
    );
    sqlx::raw_sql("DROP TRIGGER test_planning_enrollment_suppress ON consumer_registrations;DROP FUNCTION test_planning_enrollment_suppress();").execute(&h.owner).await.unwrap();
    migrate(h).await.unwrap();
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&h.owner)
            .await
            .unwrap();
    let expected: Vec<_> = store::MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 28)
        .collect();
    assert_eq!(applied.len(), expected.len());
    for ((version, checksum, success), migration) in applied.iter().zip(expected) {
        assert_eq!(*version, migration.version);
        assert_eq!(checksum.as_slice(), migration.checksum.as_ref());
        assert!(*success);
    }
    assert!(!store::schema_is_current(&h.owner).await.unwrap());
    assert_eq!(source(h, &a, w).await, before);
    assert_eq!(source(h, &a, second).await, second_before);
    for workspace in [w, second] {
        let mut tx = h.context(a.user.id, workspace).await;
        let correct:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM consumer_registrations r JOIN consumer_checkpoints c USING(workspace_id,consumer_name,consumer_version) WHERE r.workspace_id=$1 AND r.consumer_name='planning_counts' AND r.consumer_version=1 AND r.required AND r.enabled AND c.last_seq=0 AND c.backfill_job_id IS NULL) AND NOT EXISTS(SELECT 1 FROM planning_issue_facts WHERE workspace_id=$1)").bind(workspace).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert!(correct);
    }
    let worker = Worker::connect(
        &h.worker_url().await,
        WorkerOptions {
            max_connections: 1,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(worker.apply_planning_events(w).await.unwrap().backfilled);
    let mut tx = h.context(a.user.id, w).await;
    let checkpoint:i64=sqlx::query_scalar("SELECT last_seq FROM consumer_checkpoints WHERE workspace_id=$1 AND consumer_name='planning_counts'").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(checkpoint, 0);
    let claim = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(claim.kind, "planning.backfill");
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::Completed
    );
    let progress = h.app.project_progress(&a, w, t, p.id).await.unwrap();
    assert_eq!(
        progress.calculation_source,
        PlanningCalculationSource::Projection
    );
    assert_eq!(progress.eligible, 1);
    assert!(progress.facts_as_of.is_some());
    let milestone = h
        .app
        .milestone_progress(&a, w, t, p.id, m.id)
        .await
        .unwrap();
    assert_eq!(
        milestone.calculation_source,
        PlanningCalculationSource::Projection
    );
    assert_eq!(milestone.eligible, 1);
    let scope = h.app.cycle_scope(&a, w, t, c.id).await.unwrap();
    assert_eq!(
        scope.calculation_source,
        PlanningCalculationSource::Projection
    );
    assert_eq!(scope.estimate_total, 5);
    assert_eq!(source(h, &a, w).await, before);
    worker.pool.close().await;
}
