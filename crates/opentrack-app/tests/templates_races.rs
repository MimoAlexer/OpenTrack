//! Observe actual PostgreSQL blockers before releasing competing commands.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

fn definition(name: &str) -> CreateIssueTemplate {
    req(
        json!({"name":name,"title":"Template race issue","defaults":{"status_id":null,"priority":2,"label_ids":[]},"document":{"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Independent instructions"}]}]}}}),
    )
}
async fn head_gate(h: &Harness, a: &Actor, w: Uuid) -> (store::Tx, i32) {
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("SELECT workspace_id FROM workspace_stream_heads WHERE workspace_id=$1 FOR UPDATE")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    (tx, pid)
}
async fn payload(h: &Harness, a: &Actor, w: Uuid, t: Uuid, item: &IssueTemplate) -> CreateIssue {
    let p = h
        .app
        .preview_template(a, w, t, item.summary.id)
        .await
        .unwrap();
    req(
        json!({"team_id":t,"title":item.title,"status_id":p.resolved_status.unwrap().id,"document":{"schema_version":item.document.schema_version,"body":item.document.body},"template":{"id":item.summary.id,"version":item.summary.version}}),
    )
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_template_ordering_and_capacity() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (a, w, _) = setup(h).await;
    for kind in ["edit", "archive", "retire", "default"] {
        for mutation_first in [false, true] {
            mutation_order(h, &a, w, kind, mutation_first).await;
        }
    }
    for revoke_first in [false, true] {
        revoke_order(h, &a, w, revoke_first).await;
    }
    bounded_uniqueness(h, &a, w, false).await;
    bounded_uniqueness(h, &a, w, true).await;
}
async fn mutation_order(h: &Harness, a: &Actor, w: Uuid, kind: &'static str, mutation_first: bool) {
    let t = team(h, a, w, false).await;
    let item = h
        .app
        .create_template(a, w, t, &Uuid::new_v4().to_string(), definition(kind))
        .await
        .unwrap();
    let body = payload(h, a, w, t, &item).await;
    let chosen = body.status_id.unwrap();
    let flow = h.app.workflow(a, w, t).await.unwrap();
    let other = flow.statuses.iter().find(|s| s.id != chosen).unwrap().id;
    let (holder, pid) = head_gate(h, a, w).await;
    let app = h.app.clone();
    let actor = a.clone();
    let copy = item.clone();
    let mutation = async move {
        match kind {
            "edit" | "archive" => app
                .update_template(
                    &actor,
                    w,
                    t,
                    copy.summary.id,
                    &copy.summary.etag,
                    if kind == "archive" {
                        req(json!({"archived":true}))
                    } else {
                        req(json!({"title":"Changed before creation"}))
                    },
                )
                .await
                .map(|_| ()),
            "retire" => app
                .replace_status(
                    &actor,
                    w,
                    t,
                    chosen,
                    &Uuid::new_v4().to_string(),
                    &flow.etag(),
                    req(json!({"replacement_status_id":other})),
                )
                .await
                .map(|_| ()),
            "default" => app
                .set_default_status(&actor, w, t, &flow.etag(), req(json!({"status_id":other})))
                .await
                .map(|_| ()),
            _ => unreachable!(),
        }
    };
    let app = h.app.clone();
    let actor = a.clone();
    let creation = async move {
        app.create_issue(&actor, w, &Uuid::new_v4().to_string(), body)
            .await
    };
    let result = if mutation_first {
        let first = tokio::spawn(mutation);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(creation);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap()
    } else {
        let first = tokio::spawn(creation);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(mutation);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        let result = first.await.unwrap();
        second.await.unwrap().unwrap();
        result
    };
    if mutation_first && kind != "default" {
        match kind {
            "edit" | "archive" => assert!(matches!(result, Err(Error::VersionConflict(_)))),
            "retire" => assert!(matches!(result, Err(Error::Invalid(_)))),
            _ => unreachable!(),
        }
    } else {
        let created = result.unwrap();
        assert_eq!(created.status_id, chosen);
        assert_eq!(created.title, item.title);
        let mut tx = h.context(a.user.id, w).await;
        let source:(i64,String)=sqlx::query_as("SELECT template_version,definition_sha256 FROM issue_template_sources WHERE issue_id=$1").bind(created.id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(source.0, 1);
        assert_eq!(source.1.len(), 64);
        tx.commit().await.unwrap();
    }
    let mut tx = h.context(a.user.id, w).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM issues WHERE team_id=$1")
        .bind(t)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, i64::from(!mutation_first || kind == "default"));
    tx.commit().await.unwrap();
}
async fn revoke_order(h: &Harness, a: &Actor, w: Uuid, revoke_first: bool) {
    let t = team(h, a, w, true).await;
    let user = person(h, a, w, "member").await;
    grant(h, a, w, t, user.user.id).await;
    let membership = h.app.get_team_member(a, w, t, user.user.id).await.unwrap();
    let item = h
        .app
        .create_template(
            a,
            w,
            t,
            &Uuid::new_v4().to_string(),
            definition("Private instructions"),
        )
        .await
        .unwrap();
    let body = payload(h, &user, w, t, &item).await;
    let (holder, pid) = head_gate(h, a, w).await;
    let app = h.app.clone();
    let actor = a.clone();
    let id = user.user.id;
    let removal = async move {
        app.remove_team_member(&actor, w, t, id, &membership.etag(t))
            .await
    };
    let app = h.app.clone();
    let actor = user.clone();
    let creation = async move {
        app.create_issue(&actor, w, &Uuid::new_v4().to_string(), body)
            .await
    };
    let result = if revoke_first {
        let first = tokio::spawn(removal);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(creation);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap()
    } else {
        let first = tokio::spawn(creation);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(removal);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        let result = first.await.unwrap();
        second.await.unwrap().unwrap();
        result
    };
    if revoke_first {
        assert!(matches!(result, Err(Error::NotFound | Error::Forbidden)));
    } else {
        let created = result.unwrap();
        assert!(matches!(
            h.app.get_issue(&user, w, created.id).await,
            Err(Error::NotFound)
        ));
    }
    assert!(matches!(
        h.app.preview_template(&user, w, t, item.summary.id).await,
        Err(Error::NotFound)
    ));
}
async fn bounded_uniqueness(h: &Harness, a: &Actor, w: Uuid, capacity: bool) {
    let t = team(h, a, w, false).await;
    if capacity {
        let seed = h
            .app
            .create_template(a, w, t, &Uuid::new_v4().to_string(), definition("Seed"))
            .await
            .unwrap();
        let mut tx = h.context(a.user.id, w).await;
        sqlx::query("INSERT INTO issue_templates(id,workspace_id,team_id,name,title,default_priority,document_schema_version,document_body,document_plain_text,definition_sha256) SELECT gen_random_uuid(),workspace_id,team_id,'Seed '||n,title,default_priority,document_schema_version,document_body,document_plain_text,definition_sha256 FROM issue_templates CROSS JOIN generate_series(1,98) n WHERE id=$1")
            .bind(seed.summary.id).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let (holder, pid) = head_gate(h, a, w).await;
    let app = h.app.clone();
    let actor = a.clone();
    let first = tokio::spawn(async move {
        app.create_template(
            &actor,
            w,
            t,
            &Uuid::new_v4().to_string(),
            definition("Release   review"),
        )
        .await
    });
    let first_pid = blocked(h, pid).await;
    let app = h.app.clone();
    let actor = a.clone();
    let second = tokio::spawn(async move {
        app.create_template(
            &actor,
            w,
            t,
            &Uuid::new_v4().to_string(),
            definition(if capacity {
                "Final slot"
            } else {
                "release review"
            }),
        )
        .await
    });
    blocked(h, first_pid).await;
    holder.commit().await.unwrap();
    first.await.unwrap().unwrap();
    assert!(matches!(second.await.unwrap(), Err(Error::Conflict(_))));
    let mut tx = h.context(a.user.id, w).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM issue_templates WHERE team_id=$1")
        .bind(t)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, if capacity { 100 } else { 1 });
    tx.commit().await.unwrap();
}

#[derive(serde::Serialize)]
struct PreTemplateCreate {
    team_id: Uuid,
    title: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    document: Option<DocumentWrite>,
    status_id: Option<Uuid>,
    priority: i16,
    assignee_id: Option<Uuid>,
    estimate: Option<i32>,
    due_date: Option<chrono::NaiveDate>,
    mutation_id: Option<Uuid>,
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_templates_0017_upgrade_receipts_and_authority() {
    let h = Harness::with_version(5, 17).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn upgrade(h: &Harness) {
    use opentrack_app::worker::{Worker, WorkerOptions};
    use sha2::{Digest, Sha256};
    let (a, w, t) = setup(h).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        17
    );
    let legacy = PreTemplateCreate {
        team_id: t,
        title: "Before templates".into(),
        description: String::new(),
        document: Some(definition("Legacy").document),
        status_id: None,
        priority: 2,
        assignee_id: None,
        estimate: None,
        due_date: None,
        mutation_id: None,
    };
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let create: CreateIssue = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        serde_json::to_vec(&create).unwrap(),
        bytes,
        "Absent template attribution must preserve exact old request bytes"
    );
    let issue = h
        .app
        .create_issue(&a, w, "pre-template-create-retry", create.clone())
        .await
        .unwrap();
    let mut tx = h.context(a.user.id, w).await;
    // Current document readers require 0026; retain the exact 0017 representation.
    let stored: Value = sqlx::query_scalar(
        "SELECT to_jsonb(d) FROM documents d WHERE workspace_id=$1 AND issue_id=$2",
    )
    .bind(w)
    .bind(issue.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let mut document: Document = req(json!({
        "id":stored["id"],"owner_type":"issue","owner_id":issue.id,
        "schema_version":stored["schema_version"],"schema_supported":true,
        "body":stored["body"],"plain_text":stored["plain_text"],
        "version":stored["version"].as_i64().unwrap().to_string(),"etag":"",
        "created_at":stored["created_at"],"updated_at":stored["updated_at"],
        "can_edit":true,"mentions":[],"issue_references":[]
    }));
    document.refresh_etag();
    let recorded:String=sqlx::query_scalar("SELECT request_hash FROM idempotency_records WHERE route='issues.create' AND response_body->>'id'=$1").bind(issue.id.to_string()).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(recorded, format!("{:x}", Sha256::digest(bytes)));
    tx.commit().await.unwrap();
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    // Qualify this historical boundary exactly; later migrations have their own
    // upgrade contract and must not silently broaden this fixture's assertion.
    sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|m| m.version <= 18)
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
        18
    );
    assert_eq!(
        serde_json::to_value(
            h.app
                .create_issue(&a, w, "pre-template-create-retry", create)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&issue).unwrap()
    );
    let mut tx = h.context(a.user.id, w).await;
    let intermediate: Value = sqlx::query_scalar(
        "SELECT to_jsonb(d) FROM documents d WHERE workspace_id=$1 AND issue_id=$2",
    )
    .bind(w)
    .bind(issue.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(intermediate, stored);
    tx.commit().await.unwrap();
    for table in ["issue_templates", "issue_template_sources"] {
        assert!(sqlx::query_scalar::<_,bool>("SELECT has_table_privilege(current_user,$1,'SELECT') AND has_table_privilege(current_user,$1,'INSERT') AND NOT has_table_privilege(current_user,$1,'DELETE,TRUNCATE')").bind(table).fetch_one(&h.app.pool).await.unwrap());
        assert!(!sqlx::query_scalar::<_,bool>("SELECT has_any_column_privilege(current_user,$1,'SELECT,INSERT,UPDATE,REFERENCES') OR has_table_privilege(current_user,$1,'DELETE,TRUNCATE,TRIGGER')").bind(table).fetch_one(&worker.pool).await.unwrap());
        assert!(sqlx::query_scalar::<_,bool>("SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid=to_regclass($1)").bind(table).fetch_one(&h.owner).await.unwrap());
    }
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT has_table_privilege(current_user,'issue_template_sources','UPDATE')"
        )
        .fetch_one(&h.app.pool)
        .await
        .unwrap()
    );
    // Keep the historical 0018 assertions above exact. New writes through the
    // current API require its current schema, including explicit title mode.
    store::MIGRATOR.run(&h.owner).await.unwrap();
    let mut tx = h.context(a.user.id, w).await;
    let upgraded: Value = sqlx::query_scalar("SELECT to_jsonb(d)-'project_id'-'migrated_initial' FROM documents d WHERE workspace_id=$1 AND issue_id=$2")
        .bind(w).bind(issue.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(upgraded, stored);
    tx.commit().await.unwrap();
    assert_eq!(
        serde_json::to_value(h.app.issue_document(&a, w, issue.id).await.unwrap()).unwrap(),
        serde_json::to_value(document).unwrap()
    );
    h.app
        .create_template(
            &a,
            w,
            t,
            &Uuid::new_v4().to_string(),
            definition("After upgrade"),
        )
        .await
        .unwrap();
    worker.pool.close().await;
}
