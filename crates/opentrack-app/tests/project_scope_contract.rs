//! Attributable scope history, agreement, replay and current-access contracts.
mod graph_support;
#[allow(dead_code)]
mod support;

use graph_support::{grant, issue, person, req, setup, team};
use opentrack_domain::*;
use serde_json::{Value, json};
use std::future::Future;
use support::Harness;
use uuid::Uuid;

async fn checked<F, Fut>(version: i64, body: F)
where
    F: FnOnce(Harness) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let h = Harness::with_version(8, version).await;
    let run = tokio::spawn(body(h.clone())).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn project(h: &Harness, actor: &Actor, w: Uuid, t: Uuid) -> Project {
    h.app
        .create_project(
            actor,
            w,
            t,
            &Uuid::new_v4().to_string(),
            req(json!({"name":"Scope contract"})),
        )
        .await
        .unwrap()
}

async fn history(h: &Harness, a: &Actor, w: Uuid, t: Uuid, p: Uuid) -> ProjectScopeHistory {
    h.app
        .project_scope_history(a, w, t, p, ProjectScopeQuery::default())
        .await
        .unwrap()
}

async fn link(h: &Harness, a: &Actor, w: Uuid, t: Uuid, p: Uuid, i: Uuid, included: bool) {
    let current = h.app.get_project(a, w, t, p).await.unwrap();
    h.app
        .set_project_issue(a, w, t, p, i, &current.etag, ProjectMembership { included })
        .await
        .unwrap();
}

fn agreement() -> AgreeProjectScope {
    AgreeProjectScope {
        label: "Agreed delivery scope".into(),
        reason: "Team reviewed the deliverable together".into(),
    }
}

async fn snapshot(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(a.user.id, w).await;
    let out = sqlx::query_scalar("SELECT jsonb_build_object('projects',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY id),'[]') FROM projects v WHERE workspace_id=$1),'links',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY issue_id),'[]') FROM project_issues v WHERE workspace_id=$1),'heads',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY project_id),'[]') FROM project_scope_heads v WHERE workspace_id=$1),'history',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY ordinal,id),'[]') FROM project_scope_events v WHERE workspace_id=$1),'baselines',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY id),'[]') FROM project_scope_baselines v WHERE workspace_id=$1),'items',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY baseline_id,issue_id),'[]') FROM project_scope_baseline_items v WHERE workspace_id=$1),'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records v WHERE workspace_id=$1),'events',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY seq),'[]') FROM workspace_events v WHERE workspace_id=$1),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(v) ORDER BY id),'[]') FROM audit_entries v WHERE workspace_id=$1))")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recorded_scope_agreement_and_reasoned_changes_replay_once() {
    checked(i64::MAX, |h| async move {
        let (a, w, t) = setup(&h).await;
        let p = project(&h, &a, w, t).await;
        let first = h.app.create_issue(&a, w, "scope-create-linked", req(json!({"team_id":t,"title":"Original scope","project_id":p.id,"project_etag":p.etag}))).await.unwrap();
        let initial = history(&h, &a, w, t, p.id).await;
        assert!(!initial.legacy_history_unknown);
        assert_eq!(initial.current.len(), 1);
        assert_eq!(initial.events.data.len(), 1);
        assert_eq!(initial.events.data[0].action, ProjectScopeAction::Added);
        assert_eq!(initial.events.data[0].actor_id, Some(a.user.id));
        assert!(initial.baseline.is_none());
        let agreed = h.app.agree_project_scope(&a, w, t, p.id, &initial.scope_etag, "scope-agreement-key", agreement()).await.unwrap();
        let baseline = agreed.baseline.as_ref().unwrap().id;
        assert_eq!(agreed.comparison.as_ref().unwrap().unchanged, 1);
        assert!(agreed.events.data.is_empty());
        let agreed_snapshot = snapshot(&h, &a, w).await;
        let replay = h.app.agree_project_scope(&a, w, t, p.id, &initial.scope_etag, "scope-agreement-key", agreement()).await.unwrap();
        assert_eq!(replay.baseline.unwrap().id, baseline);
        assert_eq!(snapshot(&h, &a, w).await, agreed_snapshot);

        let second = issue(&h, &a, w, t, json!({"title":"Added after agreement"})).await;
        link(&h, &a, w, t, p.id, second.id, true).await;
        let once = snapshot(&h, &a, w).await;
        link(&h, &a, w, t, p.id, second.id, true).await;
        assert_eq!(snapshot(&h, &a, w).await, once, "no-op linking must not manufacture a fact");
        let changed = history(&h, &a, w, t, p.id).await;
        assert_eq!(changed.comparison.as_ref().unwrap().added[0].id, second.id);
        assert_eq!(changed.comparison.as_ref().unwrap().unchanged, 1);
        let change = ChangeProjectScope { issue_id: first.id, action: ProjectScopeAction::Postponed, reason: "Await customer dependency".into() };
        let postponed = h.app.change_project_scope(&a, w, t, p.id, &changed.scope_etag, "scope-postpone-key", change.clone()).await.unwrap();
        assert_eq!(postponed.comparison.as_ref().unwrap().postponed[0].id, first.id);
        assert_eq!(postponed.comparison.as_ref().unwrap().postponed[0].reason, change.reason);
        assert_eq!(postponed.events.data[0].action, ProjectScopeAction::Postponed);
        let after = snapshot(&h, &a, w).await;
        h.app.change_project_scope(&a, w, t, p.id, &changed.scope_etag, "scope-postpone-key", change).await.unwrap();
        assert_eq!(snapshot(&h, &a, w).await, after);
        link(&h, &a, w, t, p.id, second.id, false).await;
        let final_state = history(&h, &a, w, t, p.id).await;
        assert!(final_state.current.is_empty());
        assert_eq!(final_state.events.data.len(), 3);
        assert_eq!(final_state.events.data[0].action, ProjectScopeAction::Removed);
    }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn competing_agreements_require_review_of_the_changed_scope() {
    checked(i64::MAX, |h| async move {
        let (a, w, t) = setup(&h).await;
        let p = project(&h, &a, w, t).await;
        let before = history(&h, &a, w, t, p.id).await;
        let (left, right) = tokio::join!(
            h.app.agree_project_scope(
                &a,
                w,
                t,
                p.id,
                &before.scope_etag,
                "scope-concurrent-left",
                agreement()
            ),
            h.app.agree_project_scope(
                &a,
                w,
                t,
                p.id,
                &before.scope_etag,
                "scope-concurrent-right",
                agreement()
            )
        );
        assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
        assert!(matches!(
            (left, right),
            (Ok(_), Err(Error::VersionConflict(_))) | (Err(Error::VersionConflict(_)), Ok(_))
        ));
        assert_eq!(history(&h, &a, w, t, p.id).await.baselines.len(), 1);
        let viewer = person(&h, &a, w, "viewer").await;
        let visible = history(&h, &viewer, w, t, p.id).await;
        assert!(!visible.can_agree);
        assert!(matches!(
            h.app
                .agree_project_scope(
                    &viewer,
                    w,
                    t,
                    p.id,
                    &visible.scope_etag,
                    "scope-viewer-denied",
                    agreement()
                )
                .await,
            Err(Error::Forbidden)
        ));
        let member = person(&h, &a, w, "member").await;
        let visible = history(&h, &member, w, t, p.id).await;
        assert!(!visible.can_agree);
        assert!(matches!(
            h.app
                .agree_project_scope(
                    &member,
                    w,
                    t,
                    p.id,
                    &visible.scope_etag,
                    "scope-member-denied",
                    agreement()
                )
                .await,
            Err(Error::Forbidden)
        ));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_required_history_or_baseline_rolls_back_every_effect() {
    checked(i64::MAX, |h| async move {
        let (a,w,t)=setup(&h).await;
        let p=project(&h,&a,w,t).await;
        let i=issue(&h,&a,w,t,json!({})).await;
        sqlx::raw_sql("CREATE FUNCTION suppress_scope_fact() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_scope_fact BEFORE INSERT ON project_scope_events FOR EACH ROW EXECUTE FUNCTION suppress_scope_fact()").execute(&h.owner).await.unwrap();
        let before=snapshot(&h,&a,w).await;
        assert!(matches!(h.app.set_project_issue(&a,w,t,p.id,i.id,&p.etag,ProjectMembership{included:true}).await,Err(Error::Internal)));
        assert_eq!(snapshot(&h,&a,w).await,before);
        sqlx::raw_sql("DROP TRIGGER suppress_scope_fact ON project_scope_events; DROP FUNCTION suppress_scope_fact()").execute(&h.owner).await.unwrap();
        sqlx::raw_sql("CREATE FUNCTION suppress_scope_baseline() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_scope_baseline BEFORE INSERT ON project_scope_baselines FOR EACH ROW EXECUTE FUNCTION suppress_scope_baseline()").execute(&h.owner).await.unwrap();
        let view=history(&h,&a,w,t,p.id).await;
        let before=snapshot(&h,&a,w).await;
        assert!(matches!(h.app.agree_project_scope(&a,w,t,p.id,&view.scope_etag,"scope-baseline-failure",agreement()).await,Err(Error::Internal)));
        assert_eq!(snapshot(&h,&a,w).await,before);
        sqlx::raw_sql("DROP TRIGGER suppress_scope_baseline ON project_scope_baselines; DROP FUNCTION suppress_scope_baseline(); CREATE FUNCTION suppress_scope_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_scope_receipt BEFORE UPDATE ON idempotency_records FOR EACH ROW EXECUTE FUNCTION suppress_scope_receipt()").execute(&h.owner).await.unwrap();
        assert!(matches!(h.app.agree_project_scope(&a,w,t,p.id,&view.scope_etag,"scope-receipt-failure",agreement()).await,Err(Error::Internal)));
        assert_eq!(snapshot(&h,&a,w).await,before);
        sqlx::raw_sql("DROP TRIGGER suppress_scope_receipt ON idempotency_records; DROP FUNCTION suppress_scope_receipt(); CREATE FUNCTION suppress_scope_head() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_scope_head BEFORE INSERT ON project_scope_heads FOR EACH ROW EXECUTE FUNCTION suppress_scope_head()").execute(&h.owner).await.unwrap();
        assert!(matches!(h.app.create_project(&a,w,t,"scope-head-failure",req(json!({"name":"Must roll back"}))).await,Err(Error::Internal)));
        assert_eq!(snapshot(&h,&a,w).await,before);
        sqlx::raw_sql("DROP TRIGGER suppress_scope_head ON project_scope_heads; DROP FUNCTION suppress_scope_head()").execute(&h.owner).await.unwrap();
        link(&h,&a,w,t,p.id,i.id,true).await;
        sqlx::raw_sql("CREATE FUNCTION suppress_scope_invalidation() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_scope_invalidation BEFORE UPDATE ON project_scope_heads FOR EACH ROW EXECUTE FUNCTION suppress_scope_invalidation()").execute(&h.owner).await.unwrap();
        let before=snapshot(&h,&a,w).await;
        assert!(matches!(h.app.delete_issue(&a,w,i.id,&i.etag).await,Err(Error::Internal)));
        assert_eq!(snapshot(&h,&a,w).await,before);
        assert_eq!(h.app.get_issue(&a,w,i.id).await.unwrap().version,i.version);
    }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleted_and_moved_private_sources_are_omitted_without_false_membership_facts() {
    checked(i64::MAX, |h| async move {
        let (a,w,t)=setup(&h).await;
        let p=project(&h,&a,w,t).await;
        let member=person(&h,&a,w,"member").await;
        let moved=issue(&h,&a,w,t,json!({"title":"Private relocated source"})).await;
        let deleted=issue(&h,&a,w,t,json!({"title":"Deleted source"})).await;
        let retained=issue(&h,&a,w,t,json!({"title":"Visible retained source"})).await;
        for id in [moved.id,deleted.id,retained.id] {link(&h,&a,w,t,p.id,id,true).await;}
        let before=history(&h,&a,w,t,p.id).await;
        h.app.agree_project_scope(&a,w,t,p.id,&before.scope_etag,"scope-privacy-agreement",agreement()).await.unwrap();
        link(&h,&a,w,t,p.id,moved.id,false).await;
        let destination=team(&h,&a,w,true).await;
        let current=h.app.get_issue(&a,w,moved.id).await.unwrap();
        let preview=h.app.preview_issue_move(&a,w,moved.id,destination).await.unwrap();
        h.app.move_issue(&a,w,moved.id,"scope-private-move",&current.etag,MoveIssue{destination_team_id:destination,status_id:preview.destination_default_status_id,assignee_id:None,label_ids:vec![],preview_token:preview.preview_token}).await.unwrap();
        let current=h.app.get_issue(&a,w,deleted.id).await.unwrap();
        h.app.delete_issue(&a,w,deleted.id,&current.etag).await.unwrap();
        let current=h.app.get_issue(&a,w,retained.id).await.unwrap();
        h.app.update_issue(&a,w,retained.id,&current.etag,json!({"archived":true})).await.unwrap();
        let visible=history(&h,&member,w,t,p.id).await;
        let bytes=serde_json::to_string(&visible).unwrap();
        for secret in [moved.id.to_string(),deleted.id.to_string(),moved.title,deleted.title] {assert!(!bytes.contains(&secret));}
        assert_eq!(visible.current.len(),1);
        let comparison=visible.comparison.unwrap();
        assert_eq!(comparison.unchanged,1);
        assert!(comparison.added.is_empty() && comparison.removed.is_empty() && comparison.postponed.is_empty());
        assert!(visible.events.data.is_empty());
        assert!(!visible.events.has_more);
        // Soft deletion retains the original link and never invents a removal fact.
        let mut tx=h.context(a.user.id,w).await;
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM project_issues WHERE workspace_id=$1 AND project_id=$2 AND issue_id=$3").bind(w).bind(p.id).bind(deleted.id).fetch_one(&mut *tx).await.unwrap(),1);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM project_scope_events WHERE workspace_id=$1 AND project_id=$2 AND issue_id=$3 AND action<>'added'").bind(w).bind(p.id).bind(deleted.id).fetch_one(&mut *tx).await.unwrap(),0);
        tx.commit().await.unwrap();
        let deleted_state=h.app.get_deleted_issue(&a,w,deleted.id).await.unwrap();
        h.app.restore_issue(&a,w,deleted.id,&deleted_state.issue.etag,RestoreIssue{status_id:deleted.status_id,assignee_id:None,label_ids:vec![]}).await.unwrap();
        let restored=history(&h,&member,w,t,p.id).await;
        assert_eq!(restored.current.len(),2,"restoration re-exposes the retained membership");
        assert_eq!(restored.comparison.unwrap().unchanged,2);
        assert!(restored.events.data.is_empty(),"archive/delete/restore are not fabricated link changes");
        grant(&h,&a,w,destination,member.user.id).await;
        let private=h.app.create_project(&a,w,destination,"private-history-project",req(json!({"name":"Private history","lead_id":member.user.id}))).await.unwrap();
        let visible=history(&h,&member,w,destination,private.id).await;
        assert!(visible.can_agree);
        h.app.agree_project_scope(&member,w,destination,private.id,&visible.scope_etag,"private-history-agreement",agreement()).await.unwrap();
        let mut tx=h.context(a.user.id,w).await;
        sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3").bind(w).bind(destination).bind(member.user.id).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1").bind(w).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert!(matches!(h.app.project_scope_history(&member,w,destination,private.id,ProjectScopeQuery::default()).await,Err(Error::NotFound)));
        let before=snapshot(&h,&a,w).await;
        assert!(matches!(h.app.agree_project_scope(&member,w,destination,private.id,&visible.scope_etag,"private-history-agreement",agreement()).await,Err(Error::NotFound)));
        assert_eq!(snapshot(&h,&a,w).await,before,"a saved receipt must not bypass current private-team access");

    }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn populated_schema_upgrade_preserves_memberships_without_inventing_old_history() {
    checked(29, |h| async move {
        let (a, w, t) = setup(&h).await;
        let p = project(&h, &a, w, t).await;
        let i = issue(&h, &a, w, t, json!({"title":"Legacy project scope"})).await;
        link(&h, &a, w, t, p.id, i.id, true).await;
        opentrack_store::MIGRATOR.run(&h.owner).await.unwrap();
        let view = history(&h, &a, w, t, p.id).await;
        assert!(view.legacy_history_unknown);
        assert_eq!(view.current.len(), 1);
        assert_eq!(view.current[0].id, i.id);
        assert!(view.events.data.is_empty());
        assert!(view.baselines.is_empty() && view.comparison.is_none());
        let agreed = h
            .app
            .agree_project_scope(
                &a,
                w,
                t,
                p.id,
                &view.scope_etag,
                "scope-legacy-agreement",
                agreement(),
            )
            .await
            .unwrap();
        assert_eq!(agreed.comparison.unwrap().unchanged, 1);
        assert!(agreed.legacy_history_unknown);
        assert_eq!(
            h.app.get_project(&a, w, t, p.id).await.unwrap().name,
            p.name
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agreement_never_captures_only_the_first_page_of_an_oversized_scope() {
    checked(i64::MAX, |h| async move {
        let (a, w, t) = setup(&h).await;
        let p = project(&h, &a, w, t).await;
        let template = issue(&h, &a, w, t, json!({"title":"Bulk scope fixture"})).await;
        let mut tx = h.context(a.user.id, w).await;
        sqlx::query("SET LOCAL statement_timeout='20s'").execute(&mut *tx).await.unwrap();
        let ids: Vec<Uuid> = sqlx::query_scalar("WITH source AS (SELECT * FROM issues WHERE workspace_id=$1 AND id=$2), generated AS (SELECT gen_random_uuid() AS id,n FROM generate_series(10000,11000) n), cloned AS (INSERT INTO issues SELECT (jsonb_populate_record(NULL::issues,to_jsonb(source)||jsonb_build_object('id',generated.id,'number',generated.n))).* FROM source CROSS JOIN generated RETURNING id) SELECT array_agg(id) FROM cloned")
            .bind(w).bind(template.id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(ids.len(),1001);
        sqlx::query("INSERT INTO project_issues(workspace_id,team_id,project_id,issue_id) SELECT $1,$2,$3,i FROM unnest($4::uuid[]) i")
            .bind(w).bind(t).bind(p.id).bind(&ids).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let view = history(&h,&a,w,t,p.id).await;
        assert_eq!(view.capture_limit,1000);
        assert!(view.items_has_more);
        assert!(view.current.len()<1001);
        let before=snapshot(&h,&a,w).await;
        assert!(matches!(h.app.agree_project_scope(&a,w,t,p.id,&view.scope_etag,"scope-over-limit-agreement",agreement()).await,Err(Error::Invalid(_))));
        assert_eq!(snapshot(&h,&a,w).await,before,"a capture limit error must not save a partial baseline or receipt");
        let mut ids_seen = view.current.iter().map(|i|i.id).collect::<std::collections::BTreeSet<_>>();
        let mut cursor=view.items_next_cursor;
        while let Some(next)=cursor {
            let page=h.app.project_scope_history(&a,w,t,p.id,ProjectScopeQuery{items_cursor:Some(next),..Default::default()}).await.unwrap();
            for item in page.current {assert!(ids_seen.insert(item.id),"scope pages cannot repeat members");}
            cursor=page.items_next_cursor;
        }
        assert_eq!(ids_seen.len(),1001,"every accessible source remains inspectable through pages");
    }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scope_sources_baselines_and_unicode_reasoned_events_are_bounded_and_pageable() {
    checked(i64::MAX, |h| async move {
        let (a, w, t) = setup(&h).await;
        let p = project(&h, &a, w, t).await;
        let mut issue_ids = Vec::new();
        for _ in 0..26 {
            let i = issue(&h, &a, w, t, json!({"title":"😀".repeat(256)})).await;
            issue_ids.push(i.id);
            link(&h, &a, w, t, p.id, i.id, true).await;
        }
        let first = history(&h, &a, w, t, p.id).await;
        assert_eq!(first.current.len(), 25);
        assert!(first.items_has_more && first.events.has_more);
        let mut sources = first
            .current
            .iter()
            .map(|i| i.id)
            .collect::<std::collections::BTreeSet<_>>();
        let next = h
            .app
            .project_scope_history(
                &a,
                w,
                t,
                p.id,
                ProjectScopeQuery {
                    items_cursor: first.items_next_cursor,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        for i in next.current {
            assert!(sources.insert(i.id));
        }
        assert_eq!(sources.len(), 26);
        assert!(!next.items_has_more);
        let mut events = first
            .events
            .data
            .iter()
            .map(|e| e.id)
            .collect::<std::collections::BTreeSet<_>>();
        let next = h
            .app
            .project_scope_history(
                &a,
                w,
                t,
                p.id,
                ProjectScopeQuery {
                    cursor: first.events.next_cursor,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        for e in next.events.data {
            assert!(events.insert(e.id));
        }
        assert_eq!(events.len(), 26);
        assert!(!next.events.has_more);
        let mut view = history(&h, &a, w, t, p.id).await;
        for number in 0..26 {
            view = h
                .app
                .agree_project_scope(
                    &a,
                    w,
                    t,
                    p.id,
                    &view.scope_etag,
                    &format!("unicode-baseline-{number}"),
                    AgreeProjectScope {
                        label: "😀".repeat(160),
                        reason: "😀".repeat(1000),
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(view.baselines.len(), 25);
        assert!(view.baselines_has_more);
        let older = h
            .app
            .project_scope_history(
                &a,
                w,
                t,
                p.id,
                ProjectScopeQuery {
                    baseline_cursor: view.baselines_next_cursor.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(older.baselines.len(), 1);
        assert!(!older.baselines_has_more);
        assert!(!view.baselines.iter().any(|b| b.id == older.baselines[0].id));
        for (index, id) in issue_ids.into_iter().enumerate() {
            view = h
                .app
                .change_project_scope(
                    &a,
                    w,
                    t,
                    p.id,
                    &view.scope_etag,
                    &format!("unicode-postpone-{index}"),
                    ChangeProjectScope {
                        issue_id: id,
                        action: ProjectScopeAction::Postponed,
                        reason: "😀".repeat(1000),
                    },
                )
                .await
                .unwrap();
        }
        assert!(view.current.is_empty());
        assert_eq!(view.comparison.as_ref().unwrap().postponed.len(), 25);
        assert!(view.items_has_more && view.events.has_more);
        assert!(
            serde_json::to_vec(&view).unwrap().len() < 900 * 1024,
            "maximal Unicode labels/titles/reasons must remain below the native response budget"
        );
        let page = h
            .app
            .project_scope_history(
                &a,
                w,
                t,
                p.id,
                ProjectScopeQuery {
                    items_cursor: view.items_next_cursor.clone(),
                    cursor: view.events.next_cursor.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.comparison.unwrap().postponed.len(), 1);
        assert_eq!(page.events.data.len(), 1);
        assert!(!page.items_has_more && !page.events.has_more);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_cannot_forge_history_rewind_heads_or_reassign_membership() {
    checked(i64::MAX,|h|async move{
  let(a,w,t)=setup(&h).await;let p=project(&h,&a,w,t).await;let other=project(&h,&a,w,t).await;let i=issue(&h,&a,w,t,json!({})).await;
  link(&h,&a,w,t,p.id,i.id,true).await;
  for query in [
   "INSERT INTO project_scope_events(workspace_id,team_id,project_id,ordinal,issue_id,action) VALUES($1,$2,$3,10000,$4,'added')",
   "UPDATE project_scope_heads SET revision=0 WHERE workspace_id=$1 AND team_id=$2 AND project_id=$3 AND $4::uuid IS NOT NULL",
  ]{
   let mut tx=opentrack_store::begin(&h.app.pool).await.unwrap();opentrack_store::context(&mut tx,a.user.id,Some(w)).await.unwrap();
   assert!(sqlx::query(query).bind(w).bind(t).bind(p.id).bind(i.id).execute(&mut *tx).await.is_err());tx.rollback().await.unwrap();
  }
  let mut tx=h.context(a.user.id,w).await;
  assert!(sqlx::query("UPDATE project_issues SET project_id=$3 WHERE workspace_id=$1 AND issue_id=$2").bind(w).bind(i.id).bind(other.id).execute(&mut *tx).await.is_err());tx.rollback().await.unwrap();
  let before=history(&h,&a,w,t,p.id).await;
  let mut tx=h.context(a.user.id,w).await;sqlx::query("UPDATE project_issues SET milestone_id=NULL WHERE workspace_id=$1 AND issue_id=$2").bind(w).bind(i.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
  assert_eq!(history(&h,&a,w,t,p.id).await.scope_etag,before.scope_etag);
  let archived=issue(&h,&a,w,t,json!({})).await;let mut tx=h.context(a.user.id,w).await;sqlx::query("UPDATE issues SET archived_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(archived.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
  assert!(matches!(h.app.change_project_scope(&a,w,t,p.id,&before.scope_etag,"scope-archived-add",ChangeProjectScope{issue_id:archived.id,action:ProjectScopeAction::Added,reason:"Must remain unavailable for addition".into()}).await,Err(Error::NotFound)));
 }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cursors_disclose_visible_identities_instead_of_hidden_revision_gaps() {
    checked(i64::MAX,|h|async move{
  let(a,w,t)=setup(&h).await;let p=project(&h,&a,w,t).await;
  let visible=issue(&h,&a,w,t,json!({"title":"Visible source"})).await;let hidden=issue(&h,&a,w,t,json!({"title":"Deleted historical source"})).await;
  for index in 0..52 {
   let id=if index%2==0 {visible.id} else {hidden.id};link(&h,&a,w,t,p.id,id,(index/2)%2==0).await;
  }
  let deleted=h.app.get_issue(&a,w,hidden.id).await.unwrap();h.app.delete_issue(&a,w,hidden.id,&deleted.etag).await.unwrap();
  let full=history(&h,&a,w,t,p.id).await;assert_eq!(full.events.data.len(),25);assert!(full.events.has_more);
  let older=h.app.project_scope_history(&a,w,t,p.id,ProjectScopeQuery{cursor:full.events.next_cursor,..Default::default()}).await.unwrap();assert_eq!(older.events.data.len(),1);assert!(!older.events.has_more);
  let page=h.app.project_scope_history(&a,w,t,p.id,ProjectScopeQuery{limit:Some(1),..Default::default()}).await.unwrap();
  assert_eq!(page.events.data.len(),1);assert!(page.events.has_more);
  let cursor=page.events.next_cursor.as_ref().unwrap();let payload:Value=serde_json::from_str(cursor.split('|').next().unwrap()).unwrap();
  assert_eq!(payload[2],json!(page.events.data[0].id));assert!(payload[2].is_string());
  let body=serde_json::to_string(&page).unwrap();assert!(!body.contains(&hidden.id.to_string()));assert!(!body.contains("Deleted historical source"));
  let mut tx=h.context(a.user.id,w).await;sqlx::query("UPDATE issues SET title='Renamed visible source',version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(visible.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
  assert!(h.app.project_scope_history(&a,w,t,p.id,ProjectScopeQuery{limit:Some(1),cursor:Some(cursor.clone()),..Default::default()}).await.is_err());
 }).await;
}
