//! Real 0010 -> 0011 upgrade with deployment default grants and legacy retry bytes.
#[allow(dead_code)]
mod support;
use opentrack_app::worker::{Worker, WorkerOptions};
use opentrack_domain::*;
use opentrack_store as store;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;
fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
// Exact pre-0011 request field order/shape: hash compatibility must not be
// inferred from a JSON map whose key order differs from the former Rust struct.
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
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_taxonomy_upgrade_contract() {
    let h = Harness::with_version(5, 10).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"upgrade@taxonomy.invalid","password":"upgrade contract password 698!","display_name":"Original assignee","workspace_name":"Upgrade","team_name":"Legacy","team_key":"LEG"}))).await.unwrap();
    // Test-only legacy cookie fixture: the current authenticator requires0013.
    // Before that migration, sessions had the web-only schema and no audience.
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
    let status: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default",
    )
    .bind(w)
    .bind(t)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,assignee_id) VALUES($1,$2,$3,$4,1,'Legacy retained result',$5,$4)").bind(id).bind(w).bind(t).bind(a.user.id).bind(status).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE teams SET next_issue_number=2 WHERE id=$1")
        .bind(t)
        .execute(&mut *tx)
        .await
        .unwrap();
    let original:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',i.id,'workspace_id',i.workspace_id,'team_id',i.team_id,'key','LEG-1','title',i.title,'description',i.description,'status_id',i.status_id,'priority',i.priority,'assignee_id',i.assignee_id,'estimate',i.estimate,'due_date',i.due_date,'version',i.version::text,'created_at',i.created_at,'updated_at',i.updated_at,'archived_at',i.archived_at) FROM issues i WHERE id=$1").bind(id).fetch_one(&mut *tx).await.unwrap();
    let old = LegacyCreate {
        team_id: t,
        title: "Legacy retained result".into(),
        description: String::new(),
        status_id: None,
        priority: 0,
        assignee_id: Some(a.user.id),
        estimate: None,
        due_date: None,
        mutation_id: None,
    };
    let request_hash = digest(&serde_json::to_vec(&old).unwrap());
    let key = "legacy-created-before-upgrade";
    sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash,response_body) VALUES($1,$2,'issues.create',$3,$4,$5)").bind(w).bind(a.user.id).bind(digest(key.as_bytes())).bind(&request_hash).bind(&original).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let worker_url = h.worker_url().await;
    let worker = Worker::connect(&worker_url, WorkerOptions::default())
        .await
        .unwrap();
    let previous: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    assert_eq!(previous, 10);
    sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|m| m.version <= 11)
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
        11
    );
    // Current App behavior is tested only after applying its matching schema.
    store::MIGRATOR.run(&h.owner).await.unwrap();
    // Later consumer migration adds explicit worker capabilities; preserve the
    // original exact-version grant proof above, then provision the current worker.
    h.worker_url().await;
    let a = h.app.authenticate(&cookie).await.unwrap();
    let now: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    assert!(now >= 11);
    // No blanket post-upgrade grant: these privileges came from deployment defaults.
    for table in ["labels", "issue_labels"] {
        let rights:bool=sqlx::query_scalar("SELECT bool_and(has_table_privilege(current_user,format('%I.%I',current_schema(),$1),privilege)) FROM unnest(ARRAY['SELECT','INSERT','UPDATE','DELETE']) privilege").bind(table).fetch_one(&h.app.pool).await.unwrap();
        assert!(
            rights,
            "New runtime table privileges were not inherited from defaults"
        );
        let denied:bool=sqlx::query_scalar("SELECT NOT has_any_column_privilege(current_user,format('%I.%I',current_schema(),$1),'SELECT,INSERT,UPDATE,REFERENCES') AND NOT has_table_privilege(current_user,format('%I.%I',current_schema(),$1),'DELETE,TRUNCATE,TRIGGER')").bind(table).fetch_one(&worker.pool).await.unwrap();
        assert!(denied, "Worker received unintended taxonomy authority");
    }
    let body = json!({"team_id":t,"title":"Legacy retained result","assignee_id":a.user.id,"label_ids":[]});
    let mut modern: CreateIssue = req(body);
    modern.validate().unwrap();
    assert_eq!(digest(&serde_json::to_vec(&modern).unwrap()), request_hash);
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("UPDATE users SET display_name='Current display name' WHERE id=$1")
        .bind(a.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE issues SET title='Later independent edit',version=2 WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let replay = h.app.create_issue(&a, w, key, modern).await.unwrap();
    assert_eq!(replay.id, id);
    assert_eq!(replay.title, "Legacy retained result");
    assert_eq!(replay.version, "1");
    assert_eq!(replay.assignee_id, Some(a.user.id));
    assert_eq!(
        replay.assignee.as_ref().unwrap().display_name,
        "Current display name"
    );
    assert!(replay.labels.is_empty());
    assert!(replay.label_ids.is_empty());
    assert_eq!(replay.etag, replay.etag());
    assert!(!replay.etag.is_empty());
    let live = h.app.get_issue(&a, w, id).await.unwrap();
    assert_eq!(live.version, "2");
    assert_eq!(live.title, "Later independent edit");
    let mut tx = h.context(a.user.id, w).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM issues WHERE workspace_id=$1")
        .bind(w)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let saved: Value = sqlx::query_scalar(
        "SELECT response_body FROM idempotency_records WHERE workspace_id=$1 AND key_hash=$2",
    )
    .bind(w)
    .bind(digest(key.as_bytes()))
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        saved, original,
        "Hydration must not overwrite the durable original outcome"
    );
    tx.commit().await.unwrap();
    let label = h
        .app
        .create_label(
            &a,
            w,
            "upgrade-label-works",
            req(json!({"name":"After upgrade"})),
        )
        .await
        .unwrap();
    h.app
        .update_issue(&a, w, id, &live.etag(), json!({"label_ids":[label.id]}))
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM labels")
            .fetch_one(&h.app.pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM issue_labels")
            .fetch_one(&h.app.pool)
            .await
            .unwrap(),
        0
    );
    let still_restricted = Worker::connect(&worker_url, WorkerOptions::default())
        .await
        .unwrap();
    still_restricted.pool.close().await;
    worker.pool.close().await;
}
