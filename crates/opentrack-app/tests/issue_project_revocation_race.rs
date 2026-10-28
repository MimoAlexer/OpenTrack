//! Project-scoped issue creation must serialize with private-team revocation.
mod graph_support;
#[allow(dead_code)]
mod support;

use graph_support::{blocked, grant, person, req, setup, team};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

async fn head_gate(h: &Harness, actor: &Actor, workspace: Uuid) -> (store::Tx, i32) {
    let mut tx = h.context(actor.user.id, workspace).await;
    sqlx::query("SELECT workspace_id FROM workspace_stream_heads WHERE workspace_id=$1 FOR UPDATE")
        .bind(workspace)
        .execute(&mut *tx)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    (tx, pid)
}

async fn snapshot(h: &Harness, actor: &Actor, w: Uuid, t: Uuid, p: Uuid, user: Uuid) -> Value {
    let mut tx = h.context(actor.user.id, w).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object('issues',(SELECT count(*) FROM issues WHERE workspace_id=$1 AND team_id=$2),'documents',(SELECT count(*) FROM documents d JOIN issues i ON i.workspace_id=d.workspace_id AND i.id=d.issue_id WHERE i.workspace_id=$1 AND i.team_id=$2),'links',(SELECT count(*) FROM project_issues WHERE workspace_id=$1 AND team_id=$2 AND project_id=$3),'project_version',(SELECT version FROM projects WHERE workspace_id=$1 AND team_id=$2 AND id=$3),'next_number',(SELECT next_issue_number FROM teams WHERE workspace_id=$1 AND id=$2),'receipts',(SELECT count(*) FROM idempotency_records WHERE workspace_id=$1 AND principal_id=$4 AND route='issues.create'),'issue_events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1 AND team_id=$2 AND event_type='issue.created'),'project_events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1 AND team_id=$2 AND entity_id=$3 AND event_type='project.scope_changed'))")
        .bind(w).bind(t).bind(p).bind(user).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn private_project_issue_create_serializes_with_revocation() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (owner, workspace, _) = setup(h).await;
    for revoke_first in [true, false] {
        let team = team(h, &owner, workspace, true).await;
        let member = person(h, &owner, workspace, "member").await;
        grant(h, &owner, workspace, team, member.user.id).await;
        let membership = h
            .app
            .get_team_member(&owner, workspace, team, member.user.id)
            .await
            .unwrap();
        let project = h
            .app
            .create_project(
                &owner,
                workspace,
                team,
                &Uuid::new_v4().to_string(),
                req(json!({"name":"Revocation race project"})),
            )
            .await
            .unwrap();
        let visible = h
            .app
            .get_project(&member, workspace, team, project.id)
            .await
            .unwrap();
        let body: CreateIssue = req(
            json!({"team_id":team,"title":"Revocation race issue","project_id":project.id,"project_etag":visible.etag}),
        );
        let key = Uuid::new_v4().to_string();
        let before = snapshot(h, &owner, workspace, team, project.id, member.user.id).await;

        // The first command reaches the held stream head while retaining its
        // authorization locks; observe the second command blocked behind it.
        let (holder, pid) = head_gate(h, &owner, workspace).await;
        let app = h.app.clone();
        let actor = owner.clone();
        let member_id = member.user.id;
        let removal = async move {
            app.remove_team_member(&actor, workspace, team, member_id, &membership.etag(team))
                .await
        };
        let app = h.app.clone();
        let actor = member.clone();
        let input = body.clone();
        let request_key = key.clone();
        let creation = async move {
            app.create_issue(&actor, workspace, &request_key, input)
                .await
        };
        let created = if revoke_first {
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
        let after = snapshot(h, &owner, workspace, team, project.id, member.user.id).await;
        if revoke_first {
            assert!(matches!(created, Err(Error::NotFound | Error::Forbidden)));
            assert_eq!(after, before);
        } else {
            let issue = created.unwrap();
            assert_eq!(after["issues"], 1);
            assert_eq!(after["documents"], 1);
            assert_eq!(after["links"], 1);
            assert_eq!(after["receipts"], 1);
            assert_eq!(after["issue_events"], 1);
            assert_eq!(after["project_events"], 1);
            assert_eq!(
                after["project_version"].as_i64().unwrap(),
                before["project_version"].as_i64().unwrap() + 1
            );
            assert_eq!(
                after["next_number"].as_i64().unwrap(),
                before["next_number"].as_i64().unwrap() + 1
            );
            let linked = h
                .app
                .project_issues(
                    &owner,
                    workspace,
                    team,
                    project.id,
                    ProjectPageQuery::default(),
                )
                .await
                .unwrap();
            assert_eq!(linked.data.len(), 1);
            assert_eq!(linked.data[0].id, issue.id);
            assert_eq!(
                h.app
                    .get_issue(&owner, workspace, issue.id)
                    .await
                    .unwrap()
                    .id,
                issue.id
            );
            let members_issue = h.app.get_issue(&member, workspace, issue.id).await;
            assert!(matches!(
                members_issue,
                Err(Error::NotFound | Error::Forbidden)
            ));
        }
        assert!(matches!(
            h.app.create_issue(&member, workspace, &key, body).await,
            Err(Error::NotFound | Error::Forbidden)
        ));
        assert_eq!(
            snapshot(h, &owner, workspace, team, project.id, member.user.id).await,
            after
        );
    }
}
