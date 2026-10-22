//! Actual 0011 -> 0012 upgrade, original receipts and unchanged worker authority.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::*;
use opentrack_store as store;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;

// This is the persisted pre-graph CreateIssue serialization order, including
// nonempty labels. New absent parent fields must not change its request hash.
#[derive(Serialize)]
struct LegacyCreate {
    team_id: Uuid,
    title: String,
    description: String,
    status_id: Option<Uuid>,
    priority: i16,
    assignee_id: Option<Uuid>,
    estimate: Option<i32>,
    due_date: Option<chrono::NaiveDate>,
    mutation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    label_ids: Vec<Uuid>,
}
fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_graph_upgrade_contract() {
    let h = Harness::with_version(5, 11).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@graph.invalid","password":PASSWORD,"display_name":"Graph owner","workspace_name":"Graph","team_name":"Initial","team_key":"GRA"}))).await.unwrap();
    // Pre-0013 fixtures use the original web-only session schema. Do not call
    // current authenticated App methods until every required migration is applied.
    let a = Actor {
        audience: SessionAudience::Web,
        user: session.user.clone(),
        token_hash: digest(cookie.as_bytes()),
        csrf_token: session.csrf_token.clone(),
    };
    let w = session.workspaces[0].id;
    let mut tx = h.context(a.user.id, w).await;
    let t: Uuid = sqlx::query_scalar("SELECT id FROM teams WHERE workspace_id=$1")
        .bind(w)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        11
    );
    let label = Uuid::new_v4();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO labels(id,workspace_id,team_id,name,color) VALUES($1,$2,$3,'Existing label','gray')").bind(label).bind(w).bind(t).execute(&mut *tx).await.unwrap();
    let status: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default",
    )
    .bind(w)
    .bind(t)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let id = Uuid::new_v4();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,assignee_id) VALUES($1,$2,$3,$4,1,'Before graph migration',$5,$4)")
        .bind(id).bind(w).bind(t).bind(a.user.id).bind(status).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issue_labels(workspace_id,issue_id,label_id) VALUES($1,$2,$3)")
        .bind(w)
        .bind(id)
        .bind(label)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE teams SET next_issue_number=2 WHERE id=$1")
        .bind(t)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let original = legacy_issue(h, &a, w, id).await;
    let original_body = serde_json::to_value(&original).unwrap();
    let old = LegacyCreate {
        team_id: t,
        title: original.title.clone(),
        description: String::new(),
        status_id: None,
        priority: 0,
        assignee_id: Some(a.user.id),
        estimate: None,
        due_date: None,
        mutation_id: None,
        label_ids: vec![label],
    };
    let bytes = serde_json::to_vec(&old).unwrap();
    let request_hash = digest(&bytes);
    let key = "issued-before-graph-schema";
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash,response_body) VALUES($1,$2,'issues.create',$3,$4,$5)")
        .bind(w).bind(a.user.id).bind(digest(key.as_bytes())).bind(&request_hash).bind(&original_body)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE issues SET title='Independent edit before upgrade',version=2 WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let live_before = serde_json::to_value(legacy_issue(h, &a, w, id).await).unwrap();
    // Establish the existing worker grant before upgrading, never rerun its grant
    // helper afterward. New tables and columns must remain inaccessible.
    let worker_url = h.worker_url().await;
    let worker = Worker::connect(&worker_url, WorkerOptions::default())
        .await
        .unwrap();
    // First prove the exact graph migration independently of later auth changes.
    sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|m| m.version <= 12)
            .cloned()
            .collect::<Vec<_>>()
            .into(),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(&h.owner)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        12
    );
    assert_eq!(
        serde_json::to_value(legacy_issue(h, &a, w, id).await).unwrap(),
        live_before
    );
    // Current App code then gets its matching full schema for behavior checks.
    store::MIGRATOR.run(&h.owner).await.unwrap();
    // Later consumer migration adds explicit worker capabilities; preserve the
    // original exact-version grant proof above, then provision the current worker.
    h.worker_url().await;
    let a = h.app.authenticate(&cookie).await.unwrap();
    for table in ["issue_relations", "issue_key_aliases"] {
        let allowed:bool = sqlx::query_scalar("SELECT bool_and(has_table_privilege(current_user,format('%I.%I',current_schema(),$1),p)) FROM unnest(ARRAY['SELECT','INSERT','UPDATE','DELETE']) p")
            .bind(table).fetch_one(&h.app.pool).await.unwrap();
        assert!(allowed, "Runtime migration defaults missing on {table}");
        let denied:bool = sqlx::query_scalar("SELECT NOT has_any_column_privilege(current_user,format('%I.%I',current_schema(),$1),'SELECT,INSERT,UPDATE,REFERENCES') AND NOT has_table_privilege(current_user,format('%I.%I',current_schema(),$1),'DELETE,TRUNCATE,TRIGGER')")
            .bind(table).fetch_one(&worker.pool).await.unwrap();
        assert!(
            denied,
            "Worker unexpectedly acquired authority over {table}"
        );
        let rls:bool = sqlx::query_scalar("SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid=to_regclass(format('%I.%I',current_schema(),$1))")
            .bind(table).fetch_one(&h.owner).await.unwrap();
        assert!(rls);
    }
    for (table, column) in [("issues", "parent_id"), ("teams", "graph_version")] {
        let denied:bool = sqlx::query_scalar("SELECT NOT has_column_privilege(current_user,format('%I.%I',current_schema(),$1),$2,'SELECT,INSERT,UPDATE,REFERENCES')")
            .bind(table).bind(column).fetch_one(&worker.pool).await.unwrap();
        assert!(denied, "New graph column privilege leaked to worker");
    }
    assert!(
        sqlx::query("SELECT parent_id FROM issues")
            .execute(&worker.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE issues SET title='Worker must not write titles'")
            .execute(&worker.pool)
            .await
            .is_err()
    );
    // The exact 0011 -> 0012 comparison above remains unchanged. Current 0016
    // additionally creates the initial body projection and binds it to the ETag.
    let mut current_live: Issue = serde_json::from_value(live_before.clone()).unwrap();
    assert_eq!(current_live.body_version, "0");
    current_live.body_version = "1".into();
    current_live.etag = current_live.etag();
    assert_eq!(
        serde_json::to_value(h.app.get_issue(&a, w, id).await.unwrap()).unwrap(),
        serde_json::to_value(current_live).unwrap()
    );
    assert!(
        h.app
            .issue_parent(&a, w, id)
            .await
            .unwrap()
            .parent
            .is_none()
    );
    let mut modern: CreateIssue = serde_json::from_slice(&bytes).unwrap();
    modern.validate().unwrap();
    assert_eq!(digest(&serde_json::to_vec(&modern).unwrap()), request_hash);
    let mut current_receipt = original.clone();
    current_receipt.body_version = "1".into();
    current_receipt.etag = current_receipt.etag();
    assert_eq!(
        serde_json::to_value(h.app.create_issue(&a, w, key, modern).await.unwrap()).unwrap(),
        serde_json::to_value(current_receipt).unwrap()
    );
    let mut tx = h.context(a.user.id, w).await;
    let saved: Value = sqlx::query_scalar(
        "SELECT response_body FROM idempotency_records WHERE workspace_id=$1 AND key_hash=$2",
    )
    .bind(w)
    .bind(digest(key.as_bytes()))
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(saved, original_body);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM issues")
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT next_issue_number FROM teams WHERE id=$1")
            .bind(t)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT graph_version FROM teams WHERE id=$1")
            .bind(t)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        1
    );
    tx.commit().await.unwrap();
    // A graph-bearing issue remains serviceable by the already-provisioned,
    // status-only worker without widening its new graph column/table access.
    let parent = h.app.get_issue(&a, w, id).await.unwrap();
    let child = issue(
        h,
        &a,
        w,
        t,
        json!({"parent_id":id,"parent_etag":parent.etag()}),
    )
    .await;
    let flow = h.app.workflow(&a, w, t).await.unwrap();
    let replacement = flow.statuses.iter().find(|s| s.id != status).unwrap().id;
    h.app
        .replace_status(
            &a,
            w,
            t,
            status,
            "upgraded-worker-replace",
            &flow.etag(),
            req(json!({"replacement_status_id":replacement})),
        )
        .await
        .unwrap();
    let claim = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::Completed
    );
    assert_eq!(
        h.app.get_issue(&a, w, id).await.unwrap().status_id,
        replacement
    );
    assert_eq!(
        h.app.get_issue(&a, w, child.id).await.unwrap().status_id,
        replacement
    );
    assert_eq!(
        h.app
            .issue_parent(&a, w, child.id)
            .await
            .unwrap()
            .parent
            .unwrap()
            .target
            .unwrap()
            .id,
        id
    );
    let still_restricted = Worker::connect(&worker_url, WorkerOptions::default())
        .await
        .unwrap();
    still_restricted.pool.close().await;
    worker.pool.close().await;
}

// Store projection has no session dependency and is compatible with0011.
// This helper only reads the deliberately seeded legacy fixture under its RLS context.
async fn legacy_issue(h: &Harness, a: &Actor, w: Uuid, id: Uuid) -> Issue {
    let mut tx = h.context(a.user.id, w).await;
    let value = store::issue(&mut tx, w, a.user.id, id, false)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    value
}
