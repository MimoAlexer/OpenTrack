//! Issue creation and its project scope must commit as one operation.
mod graph_support;
#[allow(dead_code)]
mod support;

use graph_support::{grant, person, req, setup, team};
use opentrack_domain::*;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

async fn project(h: &Harness, actor: &Actor, w: Uuid, t: Uuid) -> Project {
    h.app
        .create_project(
            actor,
            w,
            t,
            &Uuid::new_v4().to_string(),
            req(json!({"name":format!("Project {}", Uuid::new_v4())})),
        )
        .await
        .unwrap()
}

fn command(t: Uuid, p: &Project) -> CreateIssue {
    req(json!({"team_id":t,"title":"Project issue","project_id":p.id,"project_etag":p.etag}))
}

async fn snapshot(h: &Harness, actor: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(actor.user.id, w).await;
    let value = sqlx::query_scalar("SELECT jsonb_build_object('issues',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM issues x WHERE workspace_id=$1),'projects',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM projects x WHERE workspace_id=$1),'links',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY issue_id),'[]') FROM project_issues x WHERE workspace_id=$1),'documents',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM documents x WHERE workspace_id=$1),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY document_id,revision),'[]') FROM document_revisions x WHERE workspace_id=$1),'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records x WHERE workspace_id=$1),'activity',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM activity_entries x WHERE workspace_id=$1),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM audit_entries x WHERE workspace_id=$1),'events',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY seq),'[]') FROM workspace_events x WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'numbers',(SELECT jsonb_agg(jsonb_build_array(id,next_issue_number) ORDER BY id) FROM teams WHERE workspace_id=$1))")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_project_scope_and_failures_are_atomic() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (actor, w, t) = setup(h).await;
    let p = project(h, &actor, w, t).await;
    let body = command(t, &p);
    let key = Uuid::new_v4().to_string();
    let issue = h
        .app
        .create_issue(&actor, w, &key, body.clone())
        .await
        .unwrap();
    let after = snapshot(h, &actor, w).await;
    let updated = h.app.get_project(&actor, w, t, p.id).await.unwrap();
    assert_eq!(
        updated.version.parse::<i64>().unwrap(),
        p.version.parse::<i64>().unwrap() + 1
    );
    let page = h
        .app
        .project_issues(&actor, w, t, p.id, ProjectPageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].id, issue.id);
    let mut tx = h.context(actor.user.id, w).await;
    let events: Vec<(String, Uuid)> = sqlx::query_as("SELECT event_type,mutation_id FROM workspace_events WHERE workspace_id=$1 AND entity_id=ANY($2) AND event_type IN ('issue.created','project.scope_changed') ORDER BY seq")
        .bind(w).bind(vec![issue.id,p.id]).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, "issue.created");
    assert_eq!(events[1].0, "project.scope_changed");
    assert_eq!(events[0].1, events[1].1);
    let replay = h
        .app
        .create_issue(&actor, w, &key, body.clone())
        .await
        .unwrap();
    assert_eq!(replay.id, issue.id);
    assert_eq!(snapshot(h, &actor, w).await, after);
    assert!(matches!(
        h.app
            .create_issue(&actor, w, &Uuid::new_v4().to_string(), body)
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert_eq!(snapshot(h, &actor, w).await, after);

    let other = team(h, &actor, w, true).await;
    let wrong_team = project(h, &actor, w, other).await;
    let before = snapshot(h, &actor, w).await;
    assert!(matches!(
        h.app
            .create_issue(
                &actor,
                w,
                &Uuid::new_v4().to_string(),
                command(t, &wrong_team)
            )
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(snapshot(h, &actor, w).await, before);

    let race_project = project(h, &actor, w, t).await;
    let mut a = command(t, &race_project);
    a.project_etag = Some(race_project.metadata_etag.clone());
    let b = a.clone();
    let (first, second) = tokio::join!(
        h.app.create_issue(&actor, w, "project-race-first", a),
        h.app.create_issue(&actor, w, "project-race-second", b),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(matches!(
        (&first, &second),
        (Ok(_), Err(Error::VersionConflict(_))) | (Err(Error::VersionConflict(_)), Ok(_))
    ));

    sqlx::raw_sql("CREATE FUNCTION suppress_project_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='project.scope_changed' THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER suppress_project_audit BEFORE INSERT ON audit_entries FOR EACH ROW EXECUTE FUNCTION suppress_project_audit()").execute(&h.owner).await.unwrap();
    let current = h
        .app
        .get_project(&actor, w, t, race_project.id)
        .await
        .unwrap();
    let before = snapshot(h, &actor, w).await;
    assert!(matches!(
        h.app
            .create_issue(&actor, w, "project-audit-failure", command(t, &current))
            .await,
        Err(Error::Internal)
    ));
    assert_eq!(snapshot(h, &actor, w).await, before);
    sqlx::raw_sql("DROP TRIGGER suppress_project_audit ON audit_entries; DROP FUNCTION suppress_project_audit()").execute(&h.owner).await.unwrap();

    sqlx::raw_sql("CREATE FUNCTION suppress_project_event() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type='project.scope_changed' THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER suppress_project_event BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION suppress_project_event()").execute(&h.owner).await.unwrap();
    assert!(matches!(
        h.app
            .create_issue(&actor, w, "project-event-failure", command(t, &current))
            .await,
        Err(Error::Internal)
    ));
    assert_eq!(snapshot(h, &actor, w).await, before);
    sqlx::raw_sql("DROP TRIGGER suppress_project_event ON workspace_events; DROP FUNCTION suppress_project_event()").execute(&h.owner).await.unwrap();

    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("UPDATE projects SET archived_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND team_id=$2 AND id=$3")
        .bind(w).bind(t).bind(race_project.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let archived = h
        .app
        .get_project(&actor, w, t, race_project.id)
        .await
        .unwrap();
    let before = snapshot(h, &actor, w).await;
    assert!(matches!(
        h.app
            .create_issue(&actor, w, "project-archived-failure", command(t, &archived))
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(snapshot(h, &actor, w).await, before);

    let member = person(h, &actor, w, "member").await;
    grant(h, &actor, w, other, member.user.id).await;
    let visible = h
        .app
        .get_project(&member, w, other, wrong_team.id)
        .await
        .unwrap();
    let input = command(other, &visible);
    h.app
        .create_issue(&member, w, "project-revocation", input.clone())
        .await
        .unwrap();
    let mut tx = h.context(actor.user.id, w).await;
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(w)
        .bind(other)
        .bind(member.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app
            .create_issue(&member, w, "project-revocation", input)
            .await,
        Err(Error::NotFound)
    ));
}
