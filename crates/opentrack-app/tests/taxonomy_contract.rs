//! Labels and assignees use current authority, historical projections, and atomic sets.
#[allow(dead_code)]
mod support;
use opentrack_domain::*;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;
const PASSWORD: &str = "taxonomy contract password 306!";
fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn label(h: &Harness, a: &Actor, w: Uuid, t: Option<Uuid>, name: &str) -> Label {
    h.app
        .create_label(
            a,
            w,
            &Uuid::new_v4().to_string(),
            req(json!({"team_id":t,"name":name})),
        )
        .await
        .unwrap()
}
async fn issue(h: &Harness, a: &Actor, w: Uuid, t: Uuid, extra: Value) -> Issue {
    let mut body = json!({"team_id":t,"title":format!("Issue {}",Uuid::new_v4())});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    h.app
        .create_issue(a, w, &Uuid::new_v4().to_string(), req(body))
        .await
        .unwrap()
}
async fn person(h: &Harness, a: &Actor, w: Uuid, role: &str, name: &str) -> Actor {
    let id = Uuid::new_v4();
    let email = format!("{id}@taxonomy.invalid");
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,$3,password_hash FROM users WHERE id=$4").bind(id).bind(&email).bind(name).bind(a.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3)")
        .bind(w)
        .bind(id)
        .bind(role)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, token) = h
        .app
        .login(req(json!({"email":email,"password":PASSWORD})))
        .await
        .unwrap();
    h.app.authenticate(&token).await.unwrap()
}
async fn team(h: &Harness, a: &Actor, w: Uuid, private: bool) -> Uuid {
    let id = Uuid::new_v4().simple().to_string();
    h.app
        .create_team(
            a,
            w,
            &id,
            req(json!({"name":id,"key":format!("T{}",&id[..6]),"private":private})),
        )
        .await
        .unwrap()
        .id
}
async fn snapshot(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(a.user.id, w).await;
    let value=sqlx::query_scalar("SELECT jsonb_build_object('labels',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM labels x),'links',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY issue_id,label_id),'[]') FROM issue_labels x),'issues',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM issues x),'retry',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records x),'teams',(SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM teams x),'audit',(SELECT count(*) FROM audit_entries),'activity',(SELECT count(*) FROM activity_entries),'events',(SELECT count(*) FROM workspace_events),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}
async fn fault(h: &Harness, table: &str, event: &str) {
    sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION taxonomy_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER taxonomy_suppress BEFORE {event} ON {table} FOR EACH ROW EXECUTE FUNCTION taxonomy_suppress()"))).execute(&h.owner).await.unwrap();
}
async fn clear(h: &Harness, table: &str) {
    sqlx::raw_sql(AssertSqlSafe(format!(
        "DROP TRIGGER taxonomy_suppress ON {table}; DROP FUNCTION taxonomy_suppress()"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_taxonomy_contract() {
    let h = Harness::with_connections(6).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (session,token)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@taxonomy.invalid","password":PASSWORD,"display_name":"ZZ owner","workspace_name":"Taxonomy","team_name":"Public","team_key":"PUB"}))).await.unwrap();
    let a = h.app.authenticate(&token).await.unwrap();
    let w = session.workspaces[0].id;
    let t = h.app.bootstrap(&a, w).await.unwrap().teams[0].id;
    authority_and_sets(h, &a, w, t).await;
    directories_and_history(h, &a, w, t).await;
    filters_and_validators(h, &a, w, t).await;
    atomicity(h, &a, w, t).await;
}
async fn authority_and_sets(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let shared = label(h, a, w, None, "Same name").await;
    assert_eq!(shared.color, "gray");
    assert!(matches!(
        h.app
            .create_label(a, w, "same-label-again", req(json!({"name":"sAmE NaMe"})))
            .await,
        Err(Error::Conflict(_))
    ));
    let scoped = label(h, a, w, Some(t), "Same name").await;
    let other = team(h, a, w, false).await;
    let other_label = label(h, a, w, Some(other), "Same name").await;
    let private = team(h, a, w, true).await;
    let hidden = label(h, a, w, Some(private), "Hidden canary").await;
    let admin = person(h, a, w, "admin", "Admin outsider").await;
    let member = person(h, a, w, "member", "Contributor").await;
    let viewer = person(h, a, w, "viewer", "Viewer").await;
    for actor in [&admin, &member, &viewer] {
        assert!(matches!(
            h.app.get_label(actor, w, hidden.id).await,
            Err(Error::NotFound)
        ));
        let visible = h
            .app
            .list_labels(actor, w, req(json!({"limit":100})))
            .await
            .unwrap();
        assert!(!visible.data.iter().any(|l| l.id == hidden.id));
        assert!(matches!(
            h.app
                .update_label(
                    actor,
                    w,
                    hidden.id,
                    &hidden.etag(),
                    req(json!({"name":"leak"}))
                )
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            h.app
                .list_issues(actor, w, req(json!({"label_ids":[hidden.id]})))
                .await,
            Err(Error::Invalid(_))
        ));
    }
    for actor in [&member, &viewer] {
        assert!(matches!(
            h.app
                .create_label(
                    actor,
                    w,
                    "denied-label-create",
                    req(json!({"name":"Denied"}))
                )
                .await,
            Err(Error::Forbidden)
        ));
    }
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query(
        "INSERT INTO team_members(workspace_id,team_id,user_id,role) VALUES($1,$2,$3,'maintainer')",
    )
    .bind(w)
    .bind(t)
    .bind(member.user.id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    label(h, &member, w, Some(t), "Maintainer label").await;
    let item = issue(
        h,
        &member,
        w,
        t,
        json!({"label_ids":[scoped.id,shared.id,shared.id]}),
    )
    .await;
    assert_eq!(item.label_ids.len(), 2);
    assert_eq!(item.labels.len(), 2);
    assert!(item.label_ids.windows(2).all(|p| p[0] < p[1]));
    assert_eq!(item.etag, item.etag());
    assert!(matches!(
        h.app
            .update_issue(&viewer, w, item.id, &item.etag(), json!({"label_ids":[]}))
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .update_issue(
                a,
                w,
                item.id,
                &item.etag(),
                json!({"label_ids":[other_label.id]})
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let mut tx = h.context(a.user.id, w).await;
    assert!(
        sqlx::query("INSERT INTO issue_labels(workspace_id,issue_id,label_id) VALUES($1,$2,$3)")
            .bind(w)
            .bind(item.id)
            .bind(other_label.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    let mut tx = h.context(a.user.id, w).await;
    assert!(
        sqlx::query("UPDATE labels SET team_id=$2 WHERE id=$1")
            .bind(shared.id)
            .bind(t)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    // A second workspace cannot supply a link even when the actor owns both.
    let organization = h.app.organizations(a).await.unwrap()[0].id;
    let foreign = h.app.create_workspace(a, "foreign-label-workspace", req(json!({"organization_id":organization,"name":"Foreign labels","team_name":"Foreign","team_key":"FOR"}))).await.unwrap();
    let foreign_label = label(h, a, foreign.id, None, "Foreign reference").await;
    assert!(matches!(
        h.app
            .update_issue(
                a,
                w,
                item.id,
                &item.etag(),
                json!({"label_ids":[foreign_label.id]})
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let mut tx = h.context(a.user.id, w).await;
    assert!(
        sqlx::query("INSERT INTO issue_labels(workspace_id,issue_id,label_id) VALUES($1,$2,$3)")
            .bind(w)
            .bind(item.id)
            .bind(foreign_label.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    // A future team move cannot bypass retained-label compatibility.
    let mut tx = h.context(a.user.id, w).await;
    let status: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default",
    )
    .bind(w)
    .bind(other)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE issues SET team_id=$2,status_id=$3 WHERE id=$1")
        .bind(item.id)
        .bind(other)
        .bind(status)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(tx.commit().await.is_err());
    assert_eq!(h.app.get_issue(a, w, item.id).await.unwrap().team_id, t);
    assert!(matches!(
        h.app
            .create_issue(
                a,
                w,
                "viewer-not-assignable",
                req(json!({"team_id":t,"title":"Viewer assignment","assignee_id":viewer.user.id}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let archived = h
        .app
        .update_label(
            a,
            w,
            shared.id,
            &shared.etag(),
            req(json!({"archived":true})),
        )
        .await
        .unwrap();
    let live_again = label(h, a, w, None, "SAME NAME").await;
    assert!(matches!(
        h.app
            .update_label(
                a,
                w,
                shared.id,
                &archived.etag(),
                req(json!({"archived":false}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let historical = h.app.get_issue(a, w, item.id).await.unwrap();
    assert!(
        historical
            .labels
            .iter()
            .find(|l| l.id == shared.id)
            .unwrap()
            .archived_at
            .is_some()
    );
    let edited = h
        .app
        .update_issue(
            a,
            w,
            item.id,
            &historical.etag(),
            json!({"title":"Keep archived label"}),
        )
        .await
        .unwrap();
    let removed = h
        .app
        .update_issue(
            a,
            w,
            item.id,
            &edited.etag(),
            json!({"label_ids":[scoped.id]}),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_issue(
                a,
                w,
                item.id,
                &removed.etag(),
                json!({"label_ids":[shared.id]})
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let restored = h
        .app
        .update_label(
            a,
            w,
            shared.id,
            &archived.etag(),
            req(json!({"archived":false,"name":"Restored distinct"})),
        )
        .await
        .unwrap();
    assert!(restored.archived_at.is_none());
    assert_ne!(restored.id, live_again.id);
    // SQL, not only request validation, enforces the 50-link limit.
    let mut tx = h.context(a.user.id, w).await;
    let fifty_one:Vec<Uuid>=sqlx::query_scalar("INSERT INTO labels(id,workspace_id,name,color) SELECT gen_random_uuid(),$1,'Limit label '||n,'gray' FROM generate_series(1,51)n RETURNING id").bind(w).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let before = snapshot(h, a, w).await;
    assert!(matches!(
        h.app
            .update_issue(
                a,
                w,
                item.id,
                &removed.etag(),
                json!({"label_ids":fifty_one})
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(snapshot(h, a, w).await, before);
    let fresh = issue(h, a, w, t, json!({})).await;
    let mut tx = h.context(a.user.id, w).await;
    assert!(sqlx::query("INSERT INTO issue_labels(workspace_id,issue_id,label_id) SELECT $1,$2,unnest($3::uuid[])").bind(w).bind(fresh.id).bind(&fifty_one).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let team_state = h.app.get_team(a, w, other).await.unwrap();
    h.app
        .update_team(
            a,
            w,
            other,
            &team_state.etag(),
            req(json!({"archived":true})),
        )
        .await
        .unwrap();
    let historical_label = h.app.get_label(a, w, other_label.id).await.unwrap();
    assert!(!historical_label.can_manage);
    assert!(matches!(
        h.app
            .update_label(
                a,
                w,
                other_label.id,
                &historical_label.etag(),
                req(json!({"name":"No parent writes"}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
}
async fn directories_and_history(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let mut tx = h.context(a.user.id, w).await;
    let _ids:Vec<Uuid>=sqlx::query_scalar("WITH added AS(INSERT INTO users(id,email,display_name,password_hash) SELECT gen_random_uuid(),'member-'||n||'@directory.invalid','Member '||lpad(n::text,3,'0'),u.password_hash FROM users u CROSS JOIN generate_series(1,125)n WHERE u.id=$1 RETURNING id) INSERT INTO workspace_members(workspace_id,user_id,role) SELECT $2,id,'member' FROM added RETURNING user_id").bind(a.user.id).bind(w).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let alternate = person(h, a, w, "member", "Cursor alternate").await;
    let first = h
        .app
        .eligible_assignees(a, w, t, req(json!({"q":"Member","limit":20})))
        .await
        .unwrap();
    assert_eq!(first.data.len(), 20);
    assert!(first.self_assignable);
    assert!(first.has_more);
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains("@directory.invalid")
    );
    let cursor = first.next_cursor.clone().unwrap();
    let mut found = first.data.iter().map(|v| v.id).collect::<Vec<_>>();
    let mut next = Some(cursor.clone());
    while let Some(token) = next {
        let page = h
            .app
            .eligible_assignees(
                a,
                w,
                t,
                req(json!({"q":"Member","limit":20,"cursor":token})),
            )
            .await
            .unwrap();
        found.extend(page.data.iter().map(|v| v.id));
        next = page.next_cursor;
    }
    assert_eq!(found.len(), 125);
    found.sort();
    found.dedup();
    assert_eq!(found.len(), 125);
    assert!(
        h.app
            .eligible_assignees(
                a,
                w,
                t,
                req(json!({"q":"different","limit":20,"cursor":cursor}))
            )
            .await
            .is_err()
    );
    assert!(
        h.app
            .eligible_assignees(
                a,
                w,
                t,
                req(json!({"q":"Member","limit":20,"cursor":format!("{cursor}tamper")}))
            )
            .await
            .is_err()
    );
    assert!(matches!(
        h.app
            .eligible_assignees(
                &alternate,
                w,
                t,
                req(json!({"q":"Member","limit":20,"cursor":cursor}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let another_team = h
        .app
        .bootstrap(a, w)
        .await
        .unwrap()
        .teams
        .into_iter()
        .find(|team| team.id != t && team.archived_at.is_none())
        .unwrap()
        .id;
    assert!(matches!(
        h.app
            .eligible_assignees(
                a,
                w,
                another_team,
                req(json!({"q":"Member","limit":20,"cursor":cursor}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let selected: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE display_name='Member 125'")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    assert!(!first.data.iter().any(|u| u.id == selected));
    let bootstrap = h.app.bootstrap(a, w).await.unwrap();
    assert_eq!(bootstrap.members.len(), 100);
    assert!(!bootstrap.members.iter().any(|u| u.id == selected));
    let historical = issue(h, a, w, t, json!({"assignee_id":selected})).await;
    assert_eq!(historical.assignee.as_ref().unwrap().id, selected);
    assert!(historical.assignee.as_ref().unwrap().assignable);
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("UPDATE workspace_members SET active=false WHERE workspace_id=$1 AND user_id=$2")
        .bind(w)
        .bind(selected)
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
            .eligible_assignees(
                a,
                w,
                t,
                req(json!({"q":"Member","limit":20,"cursor":cursor}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let after = h.app.get_issue(a, w, historical.id).await.unwrap();
    assert_eq!(after.assignee_id, Some(selected));
    assert!(!after.assignee.as_ref().unwrap().assignable);
    assert_eq!(after.version, historical.version);
    assert_ne!(after.etag(), historical.etag());
    assert!(matches!(
        h.app
            .update_issue(
                a,
                w,
                after.id,
                &historical.etag(),
                json!({"title":"Stale projection"})
            )
            .await,
        Err(Error::Stale(_))
    ));
    let renamed = h
        .app
        .update_issue(
            a,
            w,
            after.id,
            &after.etag(),
            json!({"title":"Historical owner stays visible"}),
        )
        .await
        .unwrap();
    assert_eq!(renamed.assignee_id, Some(selected));
    assert!(matches!(
        h.app
            .create_issue(
                a,
                w,
                "ineligible-assignee",
                req(json!({"team_id":t,"title":"Denied assignment","assignee_id":selected}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let archived = h
        .app
        .update_issue(a, w, renamed.id, &renamed.etag(), json!({"archived":true}))
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_issue(
                a,
                w,
                archived.id,
                &archived.etag(),
                json!({"archived":false})
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let repaired = h
        .app
        .update_issue(
            a,
            w,
            archived.id,
            &archived.etag(),
            json!({"archived":false,"assignee_id":null}),
        )
        .await
        .unwrap();
    assert!(repaired.archived_at.is_none());
    assert!(repaired.assignee.is_none());
    // Literal wildcards never expand an autocomplete search.
    label(h, a, w, None, "Literal %_ label").await;
    let literal = h
        .app
        .list_labels(a, w, req(json!({"q":"%_"})))
        .await
        .unwrap();
    assert_eq!(literal.data.len(), 1);
    assert_eq!(literal.data[0].name, "Literal %_ label");
    let no_match = h
        .app
        .eligible_assignees(a, w, t, req(json!({"q":"%_"})))
        .await
        .unwrap();
    assert!(no_match.data.is_empty());
    assert!(no_match.self_assignable);
    let labels = h
        .app
        .list_labels(
            a,
            w,
            req(json!({"scope":"workspace","q":"Limit label","limit":20})),
        )
        .await
        .unwrap();
    assert!(labels.has_more);
    let token = labels.next_cursor.unwrap();
    assert!(
        h.app
            .list_labels(
                a,
                w,
                req(json!({"scope":"workspace","q":"Limit label","limit":21,"cursor":token}))
            )
            .await
            .is_err()
    );
    let private = team(h, a, w, true).await;
    let choices = h
        .app
        .eligible_assignees(a, w, private, req(json!({})))
        .await
        .unwrap();
    assert_eq!(choices.data.len(), 1);
    assert_eq!(choices.data[0].id, a.user.id);
    let state = h.app.get_team(a, w, private).await.unwrap();
    h.app
        .update_team(a, w, private, &state.etag(), req(json!({"archived":true})))
        .await
        .unwrap();
    let unavailable = h
        .app
        .eligible_assignees(a, w, private, req(json!({})))
        .await
        .unwrap();
    assert!(unavailable.data.is_empty());
    assert!(!unavailable.self_assignable);
}
async fn filters_and_validators(h: &Harness, a: &Actor, w: Uuid, _t: Uuid) {
    let t = team(h, a, w, false).await;
    let one = label(h, a, w, Some(t), "Filter one").await;
    let two = label(h, a, w, Some(t), "Filter two").await;
    let both = issue(
        h,
        a,
        w,
        t,
        json!({"priority":2,"label_ids":[one.id,two.id]}),
    )
    .await;
    issue(h, a, w, t, json!({"priority":1,"label_ids":[one.id]})).await;
    issue(h, a, w, t, json!({"priority":3,"label_ids":[two.id]})).await;
    issue(h, a, w, t, json!({"priority":4})).await;
    issue(h, a, w, t, json!({"priority":0})).await;
    let any = h
        .app
        .list_issues(
            a,
            w,
            req(json!({"team_id":t,"label_ids":[one.id,two.id],"label_mode":"any"})),
        )
        .await
        .unwrap();
    assert_eq!(any.data.len(), 3);
    let all = h
        .app
        .list_issues(
            a,
            w,
            req(json!({"team_id":t,"label_ids":[two.id,one.id,one.id],"label_mode":"all"})),
        )
        .await
        .unwrap();
    assert_eq!(all.data.len(), 1);
    assert_eq!(all.data[0].id, both.id);
    let first = h
        .app
        .list_issues(a, w, req(json!({"team_id":t,"sort":"priority","limit":2})))
        .await
        .unwrap();
    assert_eq!(
        first.data.iter().map(|i| i.priority).collect::<Vec<_>>(),
        [1, 2]
    );
    for changed in [
        json!({"team_id":t,"sort":"created","limit":2,"cursor":first.next_cursor}),
        json!({"team_id":t,"sort":"priority","limit":2,"label_ids":[one.id],"cursor":first.next_cursor}),
        json!({"team_id":t,"sort":"priority","limit":3,"cursor":first.next_cursor}),
    ] {
        assert!(matches!(
            h.app.list_issues(a, w, req(changed)).await,
            Err(Error::Invalid(_))
        ));
    }
    let mut seen = first.data;
    let mut next = first.next_cursor;
    while let Some(cursor) = next {
        let page = h
            .app
            .list_issues(
                a,
                w,
                req(json!({"team_id":t,"sort":"priority","limit":2,"cursor":cursor})),
            )
            .await
            .unwrap();
        seen.extend(page.data);
        next = page.next_cursor;
    }
    assert_eq!(
        seen.iter().map(|i| i.priority).collect::<Vec<_>>(),
        [1, 2, 3, 4, 0]
    );
    let old = h.app.get_issue(a, w, both.id).await.unwrap();
    let renamed = h
        .app
        .update_label(
            a,
            w,
            one.id,
            &one.etag(),
            req(json!({"name":"Renamed reference"})),
        )
        .await
        .unwrap();
    let current = h.app.get_issue(a, w, both.id).await.unwrap();
    assert_eq!(old.version, current.version);
    assert_ne!(old.etag(), current.etag());
    match h
        .app
        .update_issue(a, w, both.id, &old.etag(), json!({"priority":1}))
        .await
    {
        Err(Error::Stale(now)) => {
            assert_eq!(now.etag(), current.etag());
            assert!(now.labels.iter().any(|l| l.name == "Renamed reference"));
        }
        other => panic!("Expected stale metadata, got {other:?}"),
    };
    h.app
        .update_label(a, w, one.id, &renamed.etag(), req(json!({"archived":true})))
        .await
        .unwrap();
    let selected = h
        .app
        .list_issues(a, w, req(json!({"team_id":t,"label_ids":[one.id]})))
        .await
        .unwrap();
    assert_eq!(selected.data.len(), 2);
}
async fn atomicity(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    for (table, event) in [
        ("labels", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("idempotency_records", "UPDATE"),
    ] {
        let before = snapshot(h, a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .create_label(
                a,
                w,
                "fault-label-retry",
                req(json!({"name":"Atomic label"})),
            )
            .await;
        clear(h, table).await;
        assert!(result.is_err());
        assert_eq!(snapshot(h, a, w).await, before);
    }
    let l = h
        .app
        .create_label(
            a,
            w,
            "fault-label-retry",
            req(json!({"name":"Atomic label"})),
        )
        .await
        .unwrap();
    for table in ["audit_entries", "workspace_events"] {
        let before = snapshot(h, a, w).await;
        fault(h, table, "INSERT").await;
        let result = h
            .app
            .update_label(a, w, l.id, &l.etag(), req(json!({"name":"Must roll back"})))
            .await;
        clear(h, table).await;
        assert!(result.is_err());
        assert_eq!(snapshot(h, a, w).await, before);
    }
    for (table, event) in [
        ("issues", "INSERT"),
        ("issue_labels", "INSERT"),
        ("activity_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("idempotency_records", "UPDATE"),
    ] {
        let before = snapshot(h, a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .create_issue(
                a,
                w,
                "atomic-label-issue",
                req(json!({"team_id":t,"title":"Atomic issue","label_ids":[l.id]})),
            )
            .await;
        clear(h, table).await;
        assert!(result.is_err());
        assert_eq!(snapshot(h, a, w).await, before);
    }
    let created = h
        .app
        .create_issue(
            a,
            w,
            "atomic-label-issue",
            req(json!({"team_id":t,"title":"Atomic issue","label_ids":[l.id]})),
        )
        .await
        .unwrap();
    for (table, event) in [
        ("issue_labels", "DELETE"),
        ("issues", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        let before = snapshot(h, a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .update_issue(a, w, created.id, &created.etag(), json!({"label_ids":[]}))
            .await;
        clear(h, table).await;
        assert!(result.is_err(), "{table} {event} suppression must fail");
        assert_eq!(snapshot(h, a, w).await, before);
    }
    let removed = h
        .app
        .update_issue(a, w, created.id, &created.etag(), json!({"label_ids":[]}))
        .await
        .unwrap();
    assert!(removed.label_ids.is_empty());
}
