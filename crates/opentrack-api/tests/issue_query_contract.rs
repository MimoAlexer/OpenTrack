//! Structured query contracts use the real PostgreSQL roles and an in-process
//! Axum router. Network listeners and rendered browser workflows are separate.
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use chrono::{DateTime, Utc};
use opentrack_api::{ApiState, router};
use opentrack_domain::*;
use serde_json::{Value, json};
use std::{collections::BTreeSet, future::Future, path::PathBuf, sync::Arc, time::Duration};
use support::Harness;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://query.contract.invalid";
const PASSWORD: &str = "Synthetic query contract password 2026!";

fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

#[derive(Clone)]
struct Client {
    id: Uuid,
    cookie: String,
    csrf: String,
}
struct Fixture {
    actor: Actor,
    client: Client,
    workspace: Uuid,
    team: Uuid,
    router: Router,
}
impl Fixture {
    async fn new(h: &Harness) -> Self {
        let (session, cookie) = h
            .app
            .setup(req(json!({"setup_token":h.setup_token,
            "email":"owner@example.test","password":PASSWORD,"display_name":"Example Owner",
            "workspace_name":"Issue query contracts","team_name":"Public","team_key":"PUB"})))
            .await
            .unwrap();
        let actor = h.app.authenticate(&cookie).await.unwrap();
        let workspace = session.workspaces[0].id;
        let team = h.app.bootstrap(&actor, workspace).await.unwrap().teams[0].id;
        let client = Client {
            id: session.user.id,
            cookie: format!("opentrack_session={cookie}"),
            csrf: session.csrf_token,
        };
        let router = router(ApiState {
            app: h.app.clone(),
            attachments: None,
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(PathBuf::from("unused-contract-static")),
        });
        Self {
            actor,
            client,
            workspace,
            team,
            router,
        }
    }
    fn query_path(&self) -> String {
        format!("/api/v1/workspaces/{}/issues/query", self.workspace)
    }
    async fn query(&self, body: &Value) -> Value {
        let reply = send(
            &self.router,
            Some(&self.client),
            "POST",
            &self.query_path(),
            Some(body),
            true,
        )
        .await;
        reply.expect(StatusCode::OK);
        reply.body
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}
impl Reply {
    #[track_caller]
    fn expect(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{}", self.body);
        assert_eq!(self.headers["cache-control"], "no-store");
    }
    #[track_caller]
    fn invalid(&self) {
        self.expect(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(self.body["code"], "validation_failed");
        assert_eq!(self.headers["content-type"], "application/problem+json");
        assert!(self.body.get("data").is_none());
    }
}

async fn send(
    router: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<&Value>,
    csrf: bool,
) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(client) = client {
        builder = builder.header("cookie", &client.cookie);
        if csrf {
            builder = builder.header("x-csrf-token", &client.csrf);
        }
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map(|value| serde_json::to_vec(value).unwrap())
                        .unwrap_or_default(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap(),
    }
}

fn query(filter: Value) -> Value {
    json!({"version":1,"filter":filter,"sort":"created","timezone":"UTC","limit":50,"archived":false})
}
fn predicate(field: &str, op: &str, value: Value) -> Value {
    if matches!(op, "in" | "not_in") {
        json!({"kind":"predicate","field":field,"op":op,"values":value})
    } else {
        json!({"kind":"predicate","field":field,"op":op,"value":value})
    }
}
fn ids(page: &Value) -> Vec<Uuid> {
    page["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| Uuid::parse_str(row["id"].as_str().unwrap()).unwrap())
        .collect()
}
#[track_caller]
fn contains_exactly(page: &Value, expected: &[Uuid]) {
    let actual = ids(page);
    assert_eq!(actual.len(), expected.len(), "{page}");
    assert_eq!(
        actual.into_iter().collect::<BTreeSet<_>>(),
        expected.iter().copied().collect()
    );
}
async fn issue(h: &Harness, f: &Fixture, team: Uuid, name: &str, extra: Value) -> Issue {
    let mut body = json!({"team_id":team,"title":name});
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    h.app
        .create_issue(
            &f.actor,
            f.workspace,
            &Uuid::new_v4().to_string(),
            req(body),
        )
        .await
        .unwrap()
}
async fn member(h: &Harness, f: &Fixture, name: &str) -> Client {
    let invitation = h
        .app
        .create_invitation(
            &f.actor,
            f.workspace,
            &format!("query-invite-{name}"),
            req(json!({"email":format!("{name}@example.test"),"role":"member"})),
        )
        .await
        .unwrap();
    let (session, cookie) = h
        .app
        .accept_invitation(
            None,
            req(json!({"token":invitation.token,
        "display_name":"Example Member","password":PASSWORD})),
        )
        .await
        .unwrap();
    Client {
        id: session.user.id,
        cookie: format!("opentrack_session={cookie}"),
        csrf: session.csrf_token,
    }
}
async fn other_workspace(h: &Harness, f: &Fixture) -> Uuid {
    let organization = h.app.organizations(&f.actor).await.unwrap()[0].id;
    h.app
        .create_workspace(
            &f.actor,
            "query-second-workspace",
            req(json!({"organization_id":organization,
        "name":"Other query workspace","team_name":"Other","team_key":"OTH"})),
        )
        .await
        .unwrap()
        .id
}
async fn effects(h: &Harness, f: &Fixture) -> Value {
    let mut tx = h.context(f.actor.user.id, f.workspace).await;
    let value = sqlx::query_scalar("SELECT jsonb_build_object('events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1),'jobs',(SELECT count(*) FROM jobs WHERE workspace_id=$1),'receipts',(SELECT count(*) FROM idempotency_records WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1))")
        .bind(f.workspace).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}
async fn contract<F, Fut>(case: F)
where
    F: FnOnce(Harness) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { case(copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

struct Links {
    a: Issue,
    b: Issue,
    c: Issue,
    label_a: Uuid,
    label_b: Uuid,
    project_a: Uuid,
    project_b: Uuid,
    cycle_a: Uuid,
    cycle_b: Uuid,
    member: Client,
}
async fn links(h: &Harness, f: &Fixture) -> Links {
    let member = member(h, f, "query-member").await;
    let label_a = h
        .app
        .create_label(
            &f.actor,
            f.workspace,
            "query-label-a",
            req(json!({"team_id":f.team,"name":"Alpha"})),
        )
        .await
        .unwrap()
        .id;
    let label_b = h
        .app
        .create_label(
            &f.actor,
            f.workspace,
            "query-label-b",
            req(json!({"team_id":f.team,"name":"Beta"})),
        )
        .await
        .unwrap()
        .id;
    let a = issue(h, f, f.team, "Alpha %_'; SELECT 1", json!({"priority":1,"assignee_id":f.client.id,"label_ids":[label_a],"due_date":"2026-03-29"})).await;
    let b = issue(
        h,
        f,
        f.team,
        "Beta",
        json!({"priority":2,"assignee_id":member.id,"label_ids":[label_b],"due_date":"2026-03-30"}),
    )
    .await;
    let c = issue(h, f, f.team, "Unassigned", json!({"priority":0})).await;
    let mut project_ids = Vec::new();
    let mut cycle_ids = Vec::new();
    for (index, source) in [&a, &b].iter().enumerate() {
        let project = h
            .app
            .create_project(
                &f.actor,
                f.workspace,
                f.team,
                &format!("query-project-{index}"),
                req(json!({"name":format!("Query project {index}")})),
            )
            .await
            .unwrap();
        h.app
            .set_project_issue(
                &f.actor,
                f.workspace,
                f.team,
                project.id,
                source.id,
                &project.etag,
                ProjectMembership { included: true },
            )
            .await
            .unwrap();
        project_ids.push(project.id);
        let cycle = h
            .app
            .create_cycle(
                &f.actor,
                f.workspace,
                f.team,
                &format!("query-cycle-{index}"),
                req(json!({"name":format!("Query cycle {index}"),
            "start_date":if index==0 {"2026-03-01"} else {"2026-04-01"},
            "end_date_exclusive":if index==0 {"2026-04-01"} else {"2026-05-01"},"timezone":"UTC"})),
            )
            .await
            .unwrap();
        let current = h
            .app
            .get_issue(&f.actor, f.workspace, source.id)
            .await
            .unwrap();
        h.app
            .set_cycle_issue(
                &f.actor,
                f.workspace,
                f.team,
                cycle.id,
                source.id,
                &cycle.etag,
                CycleMembership {
                    included: true,
                    issue_etag: current.etag(),
                },
            )
            .await
            .unwrap();
        cycle_ids.push(cycle.id);
    }
    Links {
        a,
        b,
        c,
        label_a,
        label_b,
        project_a: project_ids[0],
        project_b: project_ids[1],
        cycle_a: cycle_ids[0],
        cycle_b: cycle_ids[1],
        member,
    }
}

#[tokio::test]
async fn structured_fields_match_legacy_rows_without_creating_effects() {
    contract(field_parity).await;
}
async fn field_parity(h: Harness) {
    let f = Fixture::new(&h).await;
    let l = links(&h, &f).await;
    let before = effects(&h, &f).await;
    let canonical: Value = serde_json::from_str(include_str!(
        "../../opentrack-domain/tests/fixtures/issue-query-v1.json"
    ))
    .unwrap();
    let page = f.query(&canonical).await;
    assert_eq!(page["query_context"]["timezone"], "UTC");
    assert!(page["query_context"]["as_of"].is_string());
    let nested = query(json!({"kind":"all","children":[
        predicate("team","eq",json!(f.team)),
        json!({"kind":"any","children":[predicate("status","eq",json!(l.a.status_id)),predicate("priority","eq",json!(4))]}),
        predicate("priority","in",json!([1,2])),predicate("assignee","eq",json!(f.client.id)),
        predicate("project","eq",json!(l.project_a)),predicate("cycle","eq",json!(l.cycle_a)),
        predicate("label","eq",json!(l.label_a))]}));
    contains_exactly(&f.query(&nested).await, &[l.a.id]);
    let filtered = f.query(&query(json!({"kind":"all","children":[
        predicate("status","eq",json!(l.a.status_id)),predicate("priority","eq",json!(1)),
        predicate("assignee","eq",json!(f.client.id)),predicate("label","in",json!([l.label_a]))]}))).await;
    let legacy = send(&f.router,Some(&f.client),"GET",&format!("/api/v1/workspaces/{}/issues?team_id={}&status_id={}&priority=1&assignee_id={}&label_ids={}",f.workspace,f.team,l.a.status_id,f.client.id,l.label_a),None,false).await;
    legacy.expect(StatusCode::OK);
    assert_eq!(ids(&filtered), ids(&legacy.body));
    contains_exactly(
        &f.query(&json!({"version":1,"filter":null,"q":"%_'; SELECT 1"}))
            .await,
        &[l.a.id],
    );
    assert_eq!(effects(&h, &f).await, before);
}

#[tokio::test]
async fn nullable_singletons_and_label_sets_keep_distinct_negative_semantics() {
    contract(null_semantics).await;
}
async fn null_semantics(h: Harness) {
    let f = Fixture::new(&h).await;
    let l = links(&h, &f).await;
    for (field, excluded) in [
        ("assignee", json!(f.client.id)),
        ("project", json!(l.project_a)),
        ("cycle", json!(l.cycle_a)),
        ("due_date", json!("2026-03-29")),
    ] {
        contains_exactly(
            &f.query(&query(predicate(field, "not_in", json!([excluded]))))
                .await,
            &[l.b.id],
        );
        contains_exactly(
            &f.query(&query(predicate(field, "is_null", json!(true))))
                .await,
            &[l.c.id],
        );
        contains_exactly(
            &f.query(&query(predicate(field, "is_null", json!(false))))
                .await,
            &[l.a.id, l.b.id],
        );
    }
    contains_exactly(
        &f.query(&query(predicate("label", "not_in", json!([l.label_a]))))
            .await,
        &[l.b.id, l.c.id],
    );
    contains_exactly(
        &f.query(&query(predicate("label", "is_null", json!(true))))
            .await,
        &[l.c.id],
    );
    contains_exactly(
        &f.query(&query(predicate("label", "is_null", json!(false))))
            .await,
        &[l.a.id, l.b.id],
    );
    contains_exactly(
        &f.query(&query(predicate(
            "project",
            "in",
            json!([l.project_a, l.project_b]),
        )))
        .await,
        &[l.a.id, l.b.id],
    );
    contains_exactly(
        &f.query(&query(predicate(
            "cycle",
            "in",
            json!([l.cycle_a, l.cycle_b]),
        )))
        .await,
        &[l.a.id, l.b.id],
    );
    contains_exactly(
        &f.query(&query(predicate(
            "label",
            "in",
            json!([l.label_a, l.label_b]),
        )))
        .await,
        &[l.a.id, l.b.id],
    );
    contains_exactly(
        &f.query(&query(predicate("assignee", "in", json!([l.member.id]))))
            .await,
        &[l.b.id],
    );
}

#[tokio::test]
async fn structured_keysets_freeze_context_and_bind_identity_scope_and_manual_order() {
    contract(keysets).await;
}
async fn keysets(h: Harness) {
    let f = Fixture::new(&h).await;
    let peer = member(&h, &f, "query-peer").await;
    let elsewhere = other_workspace(&h, &f).await;
    let a = issue(&h, &f, f.team, "Keep Alpha", json!({"priority":1})).await;
    let b = issue(&h, &f, f.team, "Keep Beta", json!({"priority":2})).await;
    let _c = issue(&h, &f, f.team, "Other", json!({"priority":0})).await;
    let deleted = issue(
        &h,
        &f,
        f.team,
        "Deleted query source",
        json!({"priority":1}),
    )
    .await;
    h.app
        .delete_issue(&f.actor, f.workspace, deleted.id, &deleted.etag())
        .await
        .unwrap();
    let archived = issue(
        &h,
        &f,
        f.team,
        "Archived query source",
        json!({"priority":1}),
    )
    .await;
    h.app
        .update_issue(
            &f.actor,
            f.workspace,
            archived.id,
            &archived.etag(),
            json!({"archived":true}),
        )
        .await
        .unwrap();
    for sort in ["created", "priority"] {
        let legacy = send(
            &f.router,
            Some(&f.client),
            "GET",
            &format!(
                "/api/v1/workspaces/{}/issues?sort={sort}&limit=100",
                f.workspace
            ),
            None,
            false,
        )
        .await;
        legacy.expect(StatusCode::OK);
        let mut body = query(Value::Null);
        body["sort"] = json!(sort);
        body["limit"] = json!(1);
        let mut seen = Vec::new();
        let mut context = None;
        for page_number in 0..10 {
            let page = f.query(&body).await;
            if let Some(context) = &context {
                assert_eq!(&page["query_context"], context);
            } else {
                context = Some(page["query_context"].clone());
            }
            for id in ids(&page) {
                assert!(!seen.contains(&id), "repeated item on page {page_number}");
                seen.push(id);
            }
            if !page["has_more"].as_bool().unwrap() {
                break;
            }
            body["cursor"] = page["next_cursor"].clone();
        }
        assert_eq!(seen, ids(&legacy.body));
        assert!(!seen.contains(&deleted.id));
        assert!(!seen.contains(&archived.id));
    }
    let original =
        json!({"version":1,"filter":null,"sort":"created","timezone":"Europe/Zurich","limit":1});
    let first = f.query(&original).await;
    assert_eq!(first["has_more"], true);
    let mut continuation = original.clone();
    continuation["cursor"] = first["next_cursor"].clone();
    let next = f.query(&continuation).await;
    assert_eq!(first["query_context"], next["query_context"]);
    for (field, value) in [
        ("timezone", json!("UTC")),
        ("sort", json!("priority")),
        ("limit", json!(2)),
        ("q", json!("other")),
        ("filter", predicate("priority", "eq", json!(1))),
        ("team_id", json!(f.team)),
    ] {
        let mut changed = continuation.clone();
        changed[field] = value;
        send(
            &f.router,
            Some(&f.client),
            "POST",
            &f.query_path(),
            Some(&changed),
            true,
        )
        .await
        .invalid();
    }
    let mut tampered = continuation.clone();
    tampered["cursor"] = json!(format!("{}x", first["next_cursor"].as_str().unwrap()));
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &f.query_path(),
        Some(&tampered),
        true,
    )
    .await
    .invalid();
    send(
        &f.router,
        Some(&peer),
        "POST",
        &f.query_path(),
        Some(&continuation),
        true,
    )
    .await
    .invalid();
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &format!("/api/v1/workspaces/{elsewhere}/issues/query"),
        Some(&continuation),
        true,
    )
    .await
    .invalid();
    let mut with_clock = continuation.clone();
    with_clock["as_of"] = first["query_context"]["as_of"].clone();
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &f.query_path(),
        Some(&with_clock),
        true,
    )
    .await
    .invalid();
    contains_exactly(
        &f.query(&json!({"version":1,"filter":null,"archived":true}))
            .await,
        &[archived.id],
    );

    let manual = json!({"version":1,"filter":predicate("priority","in",json!([1,2])),"team_id":f.team,"sort":"manual","limit":1});
    let first_manual = f.query(&manual).await;
    contains_exactly(&first_manual, &[a.id]);
    let current = h.app.get_issue(&f.actor, f.workspace, b.id).await.unwrap();
    h.app
        .position_issue(
            &f.actor,
            f.workspace,
            b.id,
            "query-move-before",
            &current.etag(),
            req(json!({"order_version":first_manual["order_version"],"before_id":a.id})),
        )
        .await
        .unwrap();
    let mut stale_manual = manual.clone();
    stale_manual["cursor"] = first_manual["next_cursor"].clone();
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &f.query_path(),
        Some(&stale_manual),
        true,
    )
    .await
    .invalid();
    let mut complete_manual = manual;
    complete_manual["limit"] = json!(100);
    assert_eq!(ids(&f.query(&complete_manual).await), vec![b.id, a.id]);
    let mut tx = h.context(f.actor.user.id, f.workspace).await;
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(f.workspace)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &f.query_path(),
        Some(&continuation),
        true,
    )
    .await
    .invalid();
}

#[tokio::test]
async fn invalid_queries_fail_closed_and_the_largest_legal_body_stays_usable() {
    contract(invalid_and_maximum).await;
}
async fn invalid_and_maximum(h: Harness) {
    let f = Fixture::new(&h).await;
    let kept = issue(&h, &f, f.team, "Maximum query source", json!({})).await;
    let mut ids = vec![f.team];
    let mut tx = h.context(f.actor.user.id, f.workspace).await;
    let extra: Vec<Uuid>=sqlx::query_scalar("INSERT INTO teams(id,workspace_id,name,key,private) SELECT gen_random_uuid(),$1,'Filter fixture '||n::text,'Q'||lpad(n::text,3,'0'),false FROM generate_series(1,99) n RETURNING id")
        .bind(f.workspace).fetch_all(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_statuses(id,workspace_id,team_id,name,category,color,position,is_default) SELECT gen_random_uuid(),$1,id,'Ready','unstarted','gray',0,true FROM teams WHERE workspace_id=$1 AND id=ANY($2)")
        .bind(f.workspace).bind(&extra).execute(&mut *tx).await.unwrap();
    ids.extend(extra);
    tx.commit().await.unwrap();
    let maximum = query(
        json!({"kind":"all","children":(0..50).map(|_|json!({"kind":"any","children":[json!({"kind":"all","children":[predicate("team","in",json!(ids))]})]})).collect::<Vec<_>>()}),
    );
    assert!(serde_json::to_vec(&maximum).unwrap().len() < 1024 * 1024);
    let before = effects(&h, &f).await;
    contains_exactly(&f.query(&maximum).await, &[kept.id]);
    assert_eq!(effects(&h, &f).await, before);
    let flat_predicate = predicate("priority", "eq", json!(1));
    let mut too_deep = flat_predicate.clone();
    for _ in 0..4 {
        too_deep = json!({"kind":"all","children":[too_deep]});
    }
    let invalids = vec![
        json!({}),
        json!({"version":1}),
        json!({"filter":null}),
        json!({"version":2,"filter":null}),
        json!({"version":1,"filter":null,"sort":"created; DROP TABLE issues"}),
        json!({"version":1,"filter":null,"timezone":"Europe/Not_A_Zone"}),
        json!({"version":1,"filter":null,"timezone":"CET"}),
        json!({"version":1,"filter":null,"q":"bad\u{0000}query"}),
        json!({"version":1,"filter":null,"sql":"true"}),
        query(json!({"kind":"all","children":[]})),
        query(json!({"kind":"any","children":[]})),
        query(json!({"kind":"all","children":vec![flat_predicate.clone();51]})),
        query(too_deep),
        query(json!({"kind":"all","children":vec![json!({"kind":"all","children":[]});202]})),
        query(predicate("priority", "in", json!(vec![1; 101]))),
        query(predicate("priority", "in", json!([]))),
        query(predicate("priority", "eq", json!(5))),
        query(predicate("priority", "before", json!(1))),
        query(predicate("priority", "is_null", json!(true))),
        query(predicate("assignee", "eq", Value::Null)),
        query(predicate("assignee", "in", json!([null]))),
        query(predicate(
            "created_at",
            "eq",
            json!("2026-03-29T12:00:00.123456789Z"),
        )),
        query(predicate("due_date", "eq", json!("2026-03-29T12:00:00Z"))),
        query(predicate("due_date", "eq", json!("2026-02-30"))),
        query(predicate(
            "created_at",
            "before",
            json!({"relative_days":36_501}),
        )),
        query(json!({"kind":"predicate","field":"priority","op":"eq","value":1,"values":[]})),
        query(json!({"kind":"predicate","field":"priority","op":"in","values":[1],"value":1})),
        query(json!({"kind":"predicate","field":"title || true","op":"eq","value":"x"})),
        query(json!({"kind":"predicate","field":"priority","op":"contains","value":1})),
        json!({"version":1,"filter":null,"cursor":"x".repeat(2049)}),
    ];
    for body in invalids {
        send(
            &f.router,
            Some(&f.client),
            "POST",
            &f.query_path(),
            Some(&body),
            true,
        )
        .await
        .invalid();
    }
    let huge = json!({"version":1,"filter":null,"q":"x".repeat(1024*1024)});
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &f.query_path(),
        Some(&huge),
        true,
    )
    .await
    .expect(StatusCode::PAYLOAD_TOO_LARGE);
    send(
        &f.router,
        None,
        "POST",
        &f.query_path(),
        Some(&query(Value::Null)),
        true,
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &f.query_path(),
        Some(&query(Value::Null)),
        false,
    )
    .await
    .expect(StatusCode::FORBIDDEN);
    assert_eq!(effects(&h, &f).await, before);
    contains_exactly(&f.query(&query(Value::Null)).await, &[kept.id]);
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}
async fn dated(h: &Harness, f: &Fixture, name: &str, time: &str, due: &str) -> Issue {
    let item = issue(h, f, f.team, name, json!({"due_date":due})).await;
    let mut tx = h.context(f.actor.user.id, f.workspace).await;
    sqlx::query("UPDATE issues SET created_at=$3,updated_at=$3,version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(f.workspace).bind(item.id).bind(instant(time)).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    item
}

#[tokio::test]
async fn calendar_and_timestamp_predicates_observe_dst_and_frozen_relative_context() {
    contract(date_semantics).await;
}
async fn date_semantics(h: Harness) {
    let f = Fixture::new(&h).await;
    for (prefix, date, previous, next, times) in [
        (
            "Spring",
            "2026-03-29",
            "2026-03-28",
            "2026-03-30",
            [
                "2026-03-28T22:59:59.999999Z",
                "2026-03-28T23:00:00Z",
                "2026-03-29T21:59:59.999999Z",
                "2026-03-29T22:00:00Z",
            ],
        ),
        (
            "Autumn",
            "2026-10-25",
            "2026-10-24",
            "2026-10-26",
            [
                "2026-10-24T21:59:59.999999Z",
                "2026-10-24T22:00:00Z",
                "2026-10-25T22:59:59.999999Z",
                "2026-10-25T23:00:00Z",
            ],
        ),
    ] {
        let before = dated(&h, &f, &format!("{prefix} before"), times[0], previous).await;
        let first = dated(&h, &f, &format!("{prefix} first"), times[1], date).await;
        let last = dated(&h, &f, &format!("{prefix} last"), times[2], date).await;
        let after = dated(&h, &f, &format!("{prefix} after"), times[3], next).await;
        for field in ["created_at", "updated_at", "due_date"] {
            for (operator, expected) in [
                ("eq", vec![first.id, last.id]),
                ("before", vec![before.id]),
                ("after", vec![after.id]),
            ] {
                let mut body = query(predicate(field, operator, json!(date)));
                body["q"] = json!(prefix);
                body["timezone"] = json!("Europe/Zurich");
                contains_exactly(&f.query(&body).await, &expected);
            }
        }
        let mut mixed = query(predicate("created_at", "in", json!([date, times[0]])));
        mixed["q"] = json!(prefix);
        mixed["timezone"] = json!("Europe/Zurich");
        contains_exactly(&f.query(&mixed).await, &[before.id, first.id, last.id]);
        mixed["filter"] = predicate("created_at", "not_in", json!([date, times[0]]));
        contains_exactly(&f.query(&mixed).await, &[after.id]);
        let mut absolute = query(predicate("updated_at", "eq", json!(times[2])));
        absolute["q"] = json!(prefix);
        contains_exactly(&f.query(&absolute).await, &[last.id]);
    }
    let mut relative_ids = Vec::new();
    for index in 0..3 {
        relative_ids.push(
            issue(&h, &f, f.team, &format!("Relative {index}"), json!({}))
                .await
                .id,
        );
    }
    let mut relative = query(predicate(
        "created_at",
        "before",
        json!({"relative_days":1}),
    ));
    relative["q"] = json!("Relative");
    relative["limit"] = json!(1);
    relative["timezone"] = json!("Europe/Zurich");
    let first = f.query(&relative).await;
    let context = first["query_context"].clone();
    let mut seen = ids(&first);
    relative["cursor"] = first["next_cursor"].clone();
    for _ in 0..2 {
        let page = f.query(&relative).await;
        assert_eq!(page["query_context"], context);
        seen.extend(ids(&page));
        relative["cursor"] = page["next_cursor"].clone();
    }
    assert_eq!(seen.len(), 3);
    assert_eq!(
        seen.into_iter().collect::<BTreeSet<_>>(),
        relative_ids.into_iter().collect()
    );
}

async fn observe_blocker(h: &Harness, holder: i32) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(holder)
            .fetch_one(&h.owner)
            .await
            .unwrap();
            if blocked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe the query waiting on the actual workspace authorization guard");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_queries_reauthorize_private_targets_and_never_broaden_negative_filters() {
    contract(authority_race).await;
}
async fn authority_race(h: Harness) {
    let f = Fixture::new(&h).await;
    let public = issue(&h, &f, f.team, "Public query source", json!({})).await;
    let private = h
        .app
        .create_team(
            &f.actor,
            f.workspace,
            "query-private-team",
            req(json!({"name":"Private query team","key":"HID","private":true})),
        )
        .await
        .unwrap()
        .id;
    let private_issue = issue(&h, &f, private, "Concealed query source", json!({})).await;
    issue(&h, &f, private, "Second concealed source", json!({})).await;
    let label = h
        .app
        .create_label(
            &f.actor,
            f.workspace,
            "query-private-label",
            req(json!({"team_id":private,"name":"Concealed label"})),
        )
        .await
        .unwrap()
        .id;
    let project = h
        .app
        .create_project(
            &f.actor,
            f.workspace,
            private,
            "query-private-project",
            req(json!({"name":"Concealed project"})),
        )
        .await
        .unwrap();
    let cycle=h.app.create_cycle(&f.actor,f.workspace,private,"query-private-cycle",req(json!({"name":"Concealed interval","start_date":"2026-01-01","end_date_exclusive":"2026-02-01","timezone":"UTC"}))).await.unwrap();
    let unused=h.app.create_cycle(&f.actor,f.workspace,f.team,"query-deleted-cycle",req(json!({"name":"Removed interval","start_date":"2026-02-01","end_date_exclusive":"2026-03-01","timezone":"UTC"}))).await.unwrap();
    h.app
        .delete_cycle(&f.actor, f.workspace, f.team, unused.id, &unused.etag)
        .await
        .unwrap();
    let mut private_query = query(predicate("team", "eq", json!(private)));
    private_query["limit"] = json!(1);
    let first = f.query(&private_query).await;
    assert_eq!(first["has_more"], true);
    private_query["cursor"] = first["next_cursor"].clone();
    let mut holder = h.context(f.actor.user.id, f.workspace).await;
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE")
        .bind(f.workspace)
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let request_router = f.router.clone();
    let client = f.client.clone();
    let path = f.query_path();
    let queued = tokio::spawn(async move {
        send(
            &request_router,
            Some(&client),
            "POST",
            &path,
            Some(&private_query),
            true,
        )
        .await
    });
    observe_blocker(&h, holder_pid).await;
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(f.workspace)
        .bind(private)
        .bind(f.client.id)
        .execute(&mut *holder)
        .await
        .unwrap();
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(f.workspace)
        .execute(&mut *holder)
        .await
        .unwrap();
    holder.commit().await.unwrap();
    queued.await.unwrap().invalid();
    for (field, id) in [
        ("team", private),
        ("status", private_issue.status_id),
        ("label", label),
        ("project", project.id),
        ("cycle", cycle.id),
        ("cycle", unused.id),
    ] {
        for op in ["eq", "not_in"] {
            let selected = if op == "eq" { json!(id) } else { json!([id]) };
            let hidden = send(
                &f.router,
                Some(&f.client),
                "POST",
                &f.query_path(),
                Some(&query(predicate(field, op, selected))),
                true,
            )
            .await;
            hidden.invalid();
            let missing = if op == "eq" {
                json!(Uuid::new_v4())
            } else {
                json!([Uuid::new_v4()])
            };
            let missing = send(
                &f.router,
                Some(&f.client),
                "POST",
                &f.query_path(),
                Some(&query(predicate(field, op, missing))),
                true,
            )
            .await;
            missing.invalid();
            assert_eq!(hidden.body["detail"], missing.body["detail"]);
            assert!(!hidden.body.to_string().contains("Concealed"));
        }
    }
    let elsewhere = other_workspace(&h, &f).await;
    let foreign_bootstrap = h.app.bootstrap(&f.actor, elsewhere).await.unwrap();
    let foreign_team = foreign_bootstrap.teams[0].id;
    let foreign_status = foreign_bootstrap
        .statuses
        .iter()
        .find(|status| status.team_id == foreign_team)
        .unwrap()
        .id;
    let foreign_project = h
        .app
        .create_project(
            &f.actor,
            elsewhere,
            foreign_team,
            "foreign-query-project",
            req(json!({"name":"Other workspace project"})),
        )
        .await
        .unwrap()
        .id;
    let foreign_cycle = h.app.create_cycle(&f.actor, elsewhere, foreign_team, "foreign-query-cycle",
        req(json!({"name":"Other workspace interval","start_date":"2026-01-01","end_date_exclusive":"2026-02-01","timezone":"UTC"}))).await.unwrap().id;
    let foreign_label = h
        .app
        .create_label(
            &f.actor,
            elsewhere,
            "foreign-query-label",
            req(json!({"team_id":foreign_team,"name":"Other workspace label"})),
        )
        .await
        .unwrap()
        .id;
    for (field, id) in [
        ("team", foreign_team),
        ("status", foreign_status),
        ("project", foreign_project),
        ("cycle", foreign_cycle),
        ("label", foreign_label),
    ] {
        send(
            &f.router,
            Some(&f.client),
            "POST",
            &f.query_path(),
            Some(&query(predicate(field, "not_in", json!([id])))),
            true,
        )
        .await
        .invalid();
    }
    let foreign_user = Uuid::new_v4();
    let mut tx = h.context(f.actor.user.id, elsewhere).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Example Other Member',password_hash FROM users WHERE id=$3")
        .bind(foreign_user).bind(format!("{foreign_user}@example.test")).bind(f.client.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'member')")
        .bind(elsewhere)
        .bind(foreign_user)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    send(
        &f.router,
        Some(&f.client),
        "POST",
        &f.query_path(),
        Some(&query(predicate(
            "assignee",
            "not_in",
            json!([foreign_user]),
        ))),
        true,
    )
    .await
    .invalid();
    contains_exactly(&f.query(&query(Value::Null)).await, &[public.id]);
}
