//! Populated template upgrades and scoped project references in PostgreSQL.
mod graph_support;
#[allow(dead_code)]
mod support;

use graph_support::{req, setup, team};
use opentrack_domain::{CreateIssueTemplate, DocumentWrite};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;

#[derive(Serialize)]
struct LegacyDefaults {
    status_id: Option<Uuid>,
    priority: i16,
    label_ids: Vec<Uuid>,
}

#[derive(Serialize)]
struct LegacyTemplate {
    name: String,
    title: String,
    defaults: LegacyDefaults,
    document: DocumentWrite,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn populated_template_project_upgrade_preserves_legacy_receipts_and_scope() {
    for version in [27, 28] {
        let h = Harness::with_version(5, version).await;
        let copy = h.clone();
        let run = tokio::spawn(async move { upgrade(&copy, version).await }).await;
        h.cleanup().await;
        if let Err(error) = run {
            std::panic::resume_unwind(error.into_panic());
        }
    }
}

async fn upgrade(h: &Harness, version: i64) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        version
    );
    let (actor, workspace, team_id) = setup(h).await;
    let other_team = team(h, &actor, workspace, false).await;
    let mut tx = h.context(actor.user.id, workspace).await;
    let organization: Uuid =
        sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id=$1")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let other_workspace = h.app.create_workspace(&actor, "upgrade-workspace-key", req(json!({
        "organization_id":organization,"name":"Other workspace","team_name":"Other team","team_key":"OTH"
    }))).await.unwrap();
    let mut tx = h.context(actor.user.id, other_workspace.id).await;
    let other_workspace_team: Uuid =
        sqlx::query_scalar("SELECT id FROM teams WHERE workspace_id=$1")
            .bind(other_workspace.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();

    let same = h
        .app
        .create_project(
            &actor,
            workspace,
            team_id,
            "upgrade-project-same",
            req(json!({"name":"Same team"})),
        )
        .await
        .unwrap();
    let wrong_team = h
        .app
        .create_project(
            &actor,
            workspace,
            other_team,
            "upgrade-project-team",
            req(json!({"name":"Other team"})),
        )
        .await
        .unwrap();
    let wrong_workspace = h
        .app
        .create_project(
            &actor,
            other_workspace.id,
            other_workspace_team,
            "upgrade-project-workspace",
            req(json!({"name":"Other workspace"})),
        )
        .await
        .unwrap();

    let legacy = LegacyTemplate {
        name: "Legacy template".into(),
        title: "Review release".into(),
        defaults: LegacyDefaults {
            status_id: None,
            priority: 2,
            label_ids: vec![],
        },
        document: req(
            json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Legacy instructions"}]}]}}),
        ),
    };
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let mut current: CreateIssueTemplate = serde_json::from_slice(&bytes).unwrap();
    let plain = current.validate().unwrap();
    assert_eq!(serde_json::to_vec(&current).unwrap(), bytes);
    let definition = json!({"name":legacy.name,"title":legacy.title,"defaults":legacy.defaults,"schema_version":legacy.document.schema_version,"body":legacy.document.body});
    let definition_hash = hash(&serde_json::to_vec(&definition).unwrap());
    let request_hash = hash(&bytes);
    let template = Uuid::new_v4();
    let key = "legacy-template-project-upgrade";
    let response = json!({"id":template,"team_id":team_id,"name":legacy.name,"version":"1","etag":format!("\"template:{template}:1:1:1\""),"archived_at":null,"updated_at":"2026-01-01T00:00:00Z","can_manage":true,"can_use":true,"title":legacy.title,"defaults":legacy.defaults,"document":{"schema_version":1,"body":legacy.document.body,"plain_text":plain,"schema_supported":true}});
    let mut tx = h.context(actor.user.id, workspace).await;
    sqlx::query("INSERT INTO issue_templates(id,workspace_id,team_id,name,title,default_priority,document_schema_version,document_body,document_plain_text,definition_sha256) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(template).bind(workspace).bind(team_id).bind(&legacy.name).bind(&legacy.title).bind(legacy.defaults.priority).bind(legacy.document.schema_version).bind(&legacy.document.body).bind(&plain).bind(&definition_hash).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash,response_body) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(workspace).bind(actor.user.id).bind(format!("templates.create:{team_id}")).bind(hash(key.as_bytes())).bind(&request_hash).bind(&response).execute(&mut *tx).await.unwrap();
    let before: Value = sqlx::query_scalar("SELECT to_jsonb(t) FROM issue_templates t WHERE id=$1")
        .bind(template)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let receipt: Value = sqlx::query_scalar("SELECT to_jsonb(i) FROM idempotency_records i WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4")
        .bind(workspace).bind(actor.user.id).bind(format!("templates.create:{team_id}")).bind(hash(key.as_bytes())).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();

    opentrack_store::MIGRATOR.run(&h.owner).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        29
    );
    let mut tx = h.context(actor.user.id, workspace).await;
    let after: Value = sqlx::query_scalar(
        "SELECT to_jsonb(t)-'default_project_id' FROM issue_templates t WHERE id=$1",
    )
    .bind(template)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let after_receipt: Value = sqlx::query_scalar("SELECT to_jsonb(i) FROM idempotency_records i WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4")
        .bind(workspace).bind(actor.user.id).bind(format!("templates.create:{team_id}")).bind(hash(key.as_bytes())).fetch_one(&mut *tx).await.unwrap();
    let project: Option<Uuid> =
        sqlx::query_scalar("SELECT default_project_id FROM issue_templates WHERE id=$1")
            .bind(template)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(after, before);
    assert_eq!(after_receipt, receipt);
    assert_eq!(project, None);
    assert_eq!(
        h.app
            .create_template(&actor, workspace, team_id, key, current)
            .await
            .unwrap()
            .summary
            .id,
        template
    );
    let mut tx = h.context(actor.user.id, workspace).await;
    let replayed_receipt: Value = sqlx::query_scalar("SELECT to_jsonb(i) FROM idempotency_records i WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4")
        .bind(workspace).bind(actor.user.id).bind(format!("templates.create:{team_id}")).bind(hash(key.as_bytes())).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(replayed_receipt, receipt);

    for candidate in [wrong_team.id, wrong_workspace.id] {
        let mut tx = h.context(actor.user.id, workspace).await;
        let error = sqlx::query(
            "UPDATE issue_templates SET default_project_id=$2,version=version+1 WHERE id=$1",
        )
        .bind(template)
        .bind(candidate)
        .execute(&mut *tx)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23503")
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = h.context(actor.user.id, workspace).await;
    assert_eq!(
        sqlx::query(
            "UPDATE issue_templates SET default_project_id=$2,version=version+1 WHERE id=$1"
        )
        .bind(template)
        .bind(same.id)
        .execute(&mut *tx)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    tx.commit().await.unwrap();
}
