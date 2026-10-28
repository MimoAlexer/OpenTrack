//! Current issue creation and retry across pre-project-default schema upgrades.
mod graph_support;
#[allow(dead_code)]
mod support;

use graph_support::setup;
use opentrack_domain::{Actor, CreateIssue, Error};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;

// This is the pre-project-field wire shape, not a historical executable.
#[derive(Serialize)]
struct LegacyIssue {
    team_id: Uuid,
    title: String,
    description: String,
    status_id: Option<Uuid>,
    priority: i16,
    assignee_id: Option<Uuid>,
    estimate: Option<i32>,
    due_date: Option<String>,
    mutation_id: Option<Uuid>,
}

async fn snapshot(h: &Harness, actor: &Actor, workspace: Uuid) -> Value {
    let mut tx = h.context(actor.user.id, workspace).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object('issues',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY id),'[]') FROM issues i WHERE workspace_id=$1),'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records r WHERE workspace_id=$1),'activity',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM activity_entries a WHERE workspace_id=$1),'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY seq),'[]') FROM workspace_events e WHERE workspace_id=$1),'links',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY issue_id),'[]') FROM project_issues p WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1))")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_issue_receipt_replays_after_project_default_upgrade() {
    for version in [27, 28] {
        let h = Harness::with_version(5, version).await;
        let copy = h.clone();
        let run = tokio::spawn(async move { exercise(&copy, version).await }).await;
        h.cleanup().await;
        if let Err(error) = run {
            std::panic::resume_unwind(error.into_panic());
        }
    }
}

async fn exercise(h: &Harness, version: i64) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        version
    );
    let (actor, workspace, team) = setup(h).await;
    let legacy = LegacyIssue {
        team_id: team,
        title: "Upgrade retry".into(),
        description: "Keep this issue once".into(),
        status_id: None,
        priority: 2,
        assignee_id: None,
        estimate: None,
        due_date: None,
        mutation_id: None,
    };
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let mut request: CreateIssue = serde_json::from_slice(&bytes).unwrap();
    request.validate().unwrap();
    assert_eq!(serde_json::to_vec(&request).unwrap(), bytes);
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let key = "legacy-issue-project-upgrade";

    // The current binary runs against the old schema; this is not a run of an old binary.
    let issue = h
        .app
        .create_issue(&actor, workspace, key, request.clone())
        .await
        .unwrap();
    let before = snapshot(h, &actor, workspace).await;
    assert_eq!(before["issues"].as_array().unwrap().len(), 1);
    assert_eq!(before["links"], json!([]));
    let mut tx = h.context(actor.user.id, workspace).await;
    let stored: String = sqlx::query_scalar("SELECT request_hash FROM idempotency_records WHERE workspace_id=$1 AND principal_id=$2 AND route='issues.create' AND response_body->>'id'=$3")
        .bind(workspace).bind(actor.user.id).bind(issue.id.to_string()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(stored, hash);

    opentrack_store::MIGRATOR.run(&h.owner).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        29
    );
    assert_eq!(snapshot(h, &actor, workspace).await, before);
    let replay = h
        .app
        .create_issue(&actor, workspace, key, request.clone())
        .await
        .unwrap();
    assert_eq!(replay.id, issue.id);
    assert_eq!(snapshot(h, &actor, workspace).await, before);

    request.title = "Different request".into();
    assert!(matches!(
        h.app.create_issue(&actor, workspace, key, request).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(snapshot(h, &actor, workspace).await, before);
}
