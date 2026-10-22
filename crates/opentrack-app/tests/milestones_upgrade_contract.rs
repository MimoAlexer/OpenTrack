//! Upgrade populated project membership without losing scope or requiring runtime grants.
#[allow(dead_code)]
mod support;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;

fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn populated_schema_22_upgrades_with_existing_membership_and_runtime_grants() {
    let h = Harness::with_version(5, 22).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn upgrade(h: &Harness) {
    let (session,token)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@milestone-upgrade.example.test","display_name":"Example Owner","password":"Synthetic milestone upgrade password 2026!","workspace_name":"Milestone upgrade","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let actor = h.app.authenticate(&token).await.unwrap();
    let w = session.workspaces[0].id;
    let mut tx = h.context(actor.user.id, w).await;
    let team: uuid::Uuid = sqlx::query_scalar("SELECT id FROM teams WHERE workspace_id=$1")
        .bind(w)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let project = h
        .app
        .create_project(
            &actor,
            w,
            team,
            "upgrade-project",
            req(json!({"name":"Existing project"})),
        )
        .await
        .unwrap();
    let issue = h
        .app
        .create_issue(
            &actor,
            w,
            "upgrade-issue",
            req(json!({"team_id":team,"title":"Existing project issue"})),
        )
        .await
        .unwrap();
    let project = h
        .app
        .set_project_issue(
            &actor,
            w,
            team,
            project.id,
            issue.id,
            &project.etag,
            ProjectMembership { included: true },
        )
        .await
        .unwrap();
    store::MIGRATOR.run(&h.owner).await.unwrap();
    let rights:bool=sqlx::query_scalar("SELECT bool_and(has_table_privilege(current_user,format('%I.milestones',current_schema()),p)) FROM unnest(ARRAY['SELECT','INSERT','UPDATE','DELETE']) p").fetch_one(&h.app.pool).await.unwrap();
    assert!(rights);
    let page = h
        .app
        .project_issues(&actor, w, team, project.id, ProjectPageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].id, issue.id);
    assert_eq!(page.data[0].milestone_id, None);
    let milestone = h
        .app
        .create_milestone(
            &actor,
            w,
            team,
            project.id,
            "upgrade-milestone",
            &project.etag,
            req(json!({"title":"Upgraded outcome"})),
        )
        .await
        .unwrap();
    let current = h
        .app
        .get_project(&actor, w, team, project.id)
        .await
        .unwrap();
    h.app
        .assign_milestone(
            &actor,
            w,
            team,
            project.id,
            issue.id,
            &current.etag,
            MilestoneAssignment {
                milestone_id: Some(milestone.id),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        h.app
            .milestone_progress(&actor, w, team, project.id, milestone.id)
            .await
            .unwrap()
            .eligible,
        1
    );
    h.app
        .delete_milestone(&actor, w, team, project.id, milestone.id, &milestone.etag)
        .await
        .unwrap();
    assert_eq!(
        h.app.get_issue(&actor, w, issue.id).await.unwrap().title,
        issue.title
    );
    let page = h
        .app
        .project_issues(&actor, w, team, project.id, ProjectPageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].milestone_id, None);
}
