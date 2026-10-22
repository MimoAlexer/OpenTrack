//! Router and PostgreSQL admission failures preserve the complete mutation.
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use support::{Harness, ORIGIN};
use tower::ServiceExt;
use uuid::Uuid;

struct Client {
    actor: Uuid,
    cookie: String,
    csrf: String,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    fn expect(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{}", self.body);
    }

    fn unavailable(&self) {
        self.expect(StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(self.headers["content-type"], "application/problem+json");
        assert_eq!(self.headers["cache-control"], "no-store");
        let request_id = Uuid::parse_str(self.body["request_id"].as_str().unwrap()).unwrap();
        assert_eq!(
            self.body,
            json!({
                "type":"/api/v1/problems/dependency_unavailable",
                "title":"Service Unavailable","status":503,
                "detail":"Service temporarily unavailable",
                "instance":format!("urn:uuid:{request_id}"),
                "code":"dependency_unavailable","request_id":request_id,
                "retryable":true,"field_errors":[]
            }),
            "Responses must contain only generic problem fields, without internal limits, sequences, recipient identities or SQL"
        );
        assert_eq!(
            self.body["request_id"],
            self.headers["x-request-id"].to_str().unwrap()
        );
        for header in [
            "etag",
            "location",
            "set-cookie",
            "x-opentrack-session-invalidated",
        ] {
            assert!(!self.headers.contains_key(header));
        }
    }
}

async fn request(
    router: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(client) = client {
        builder = builder
            .header("cookie", &client.cookie)
            .header("x-csrf-token", &client.csrf);
    }
    for (key, value) in extra {
        builder = builder.header(*key, *value);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map(|value| serde_json::to_vec(&value).unwrap())
                        .unwrap_or_default(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    Reply {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap(),
    }
}

async fn state(h: &Harness, client: &Client, w: Uuid, team: Uuid) -> Value {
    let mut tx = h.lock_team(w, team, client.actor).await;
    let snapshot = sqlx::query_scalar("SELECT jsonb_build_object(
      'workspace',(SELECT to_jsonb(w) FROM workspaces w WHERE id=$1),
      'number',(SELECT next_issue_number FROM teams WHERE workspace_id=$1 AND id=$2),
      'issues',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY id),'[]') FROM issues i WHERE workspace_id=$1),
      'retries',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records r WHERE workspace_id=$1),
      'activity',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM activity_entries a WHERE workspace_id=$1),
      'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM audit_entries a WHERE workspace_id=$1),
      'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY seq),'[]') FROM workspace_events e WHERE workspace_id=$1),
      'head',(SELECT to_jsonb(h) FROM workspace_stream_heads h WHERE workspace_id=$1),
      'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1))")
        .bind(w).bind(team).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    snapshot
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_caps_return_retryable_503_without_partial_writes_and_retry_after_capacity_recovery()
 {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let setup = request(
        &h.router,
        None,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "setup_token":h.setup_token,"email":"owner@example.test","display_name":"Example Owner",
            "password":"HTTP event admission password 724!","workspace_name":"HTTP event admission",
            "team_name":"Engineering","team_key":"ENG"
        })),
        &[],
    )
    .await;
    setup.expect(StatusCode::CREATED);
    let client = Client {
        actor: Uuid::parse_str(setup.body["user"]["id"].as_str().unwrap()).unwrap(),
        cookie: setup.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned(),
        csrf: setup.body["csrf_token"].as_str().unwrap().to_owned(),
    };
    let w = Uuid::parse_str(setup.body["workspaces"][0]["id"].as_str().unwrap()).unwrap();
    let base = format!("/api/v1/workspaces/{w}");
    let bootstrap = request(
        &h.router,
        Some(&client),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    bootstrap.expect(StatusCode::OK);
    let team = Uuid::parse_str(bootstrap.body["teams"][0]["id"].as_str().unwrap()).unwrap();
    let collection = format!("{base}/issues");
    request(
        &h.router,
        Some(&client),
        "POST",
        &collection,
        Some(json!({"team_id":team,"title":"Initial committed event"})),
        &[("idempotency-key", "admission-baseline")],
    )
    .await
    .expect(StatusCode::CREATED);
    let workspace = request(&h.router, Some(&client), "GET", &base, None, &[]).await;
    workspace.expect(StatusCode::OK);
    let etag = workspace.headers["etag"].to_str().unwrap();
    for (key, cap) in [
        (
            "pending-cap-retry",
            "UPDATE workspace_stream_heads SET max_pending_events=last_seq WHERE workspace_id=$1",
        ),
        (
            "byte-cap-retry",
            "UPDATE workspace_stream_heads SET max_retained_event_bytes=retained_event_bytes WHERE workspace_id=$1",
        ),
    ] {
        let mut tx = h.lock_team(w, team, client.actor).await;
        sqlx::query(cap).bind(w).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let before = state(h, &client, w, team).await;
        let body = json!({"team_id":team,"title":"Recoverable admitted issue","mutation_id":Uuid::new_v4()});
        request(
            &h.router,
            Some(&client),
            "POST",
            &collection,
            Some(body.clone()),
            &[("idempotency-key", key)],
        )
        .await
        .unavailable();
        request(
            &h.router,
            Some(&client),
            "PATCH",
            &base,
            Some(json!({"name":"Rejected identity update"})),
            &[("if-match", etag)],
        )
        .await
        .unavailable();
        assert_eq!(
            state(h, &client, w, team).await,
            before,
            "A 503 must leave the entity, number, retry key, audit, activity, sequence and logical bytes unchanged"
        );
        let mut tx = h.lock_team(w, team, client.actor).await;
        sqlx::query("UPDATE workspace_stream_heads SET max_pending_events=100000,max_retained_event_bytes=268435456 WHERE workspace_id=$1").bind(w).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let created = request(
            &h.router,
            Some(&client),
            "POST",
            &collection,
            Some(body.clone()),
            &[("idempotency-key", key)],
        )
        .await;
        created.expect(StatusCode::CREATED);
        let committed = state(h, &client, w, team).await;
        let replay = request(
            &h.router,
            Some(&client),
            "POST",
            &collection,
            Some(body),
            &[("idempotency-key", key)],
        )
        .await;
        replay.expect(StatusCode::CREATED);
        assert_eq!(replay.body, created.body);
        assert_eq!(state(h, &client, w, team).await, committed);
        assert_eq!(
            committed["head"]["last_seq"].as_i64().unwrap(),
            before["head"]["last_seq"].as_i64().unwrap() + 1
        );
        assert_eq!(
            committed["number"].as_i64().unwrap(),
            before["number"].as_i64().unwrap() + 1
        );
    }
    let updated = request(
        &h.router,
        Some(&client),
        "PATCH",
        &base,
        Some(json!({"name":"Accepted after capacity recovery"})),
        &[("if-match", etag)],
    )
    .await;
    updated.expect(StatusCode::OK);
    assert_eq!(updated.body["name"], "Accepted after capacity recovery");
}
