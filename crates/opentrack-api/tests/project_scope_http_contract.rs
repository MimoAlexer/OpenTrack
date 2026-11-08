//! HTTP preconditions, durable scope commands and safe replay against PostgreSQL.
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
struct Client {
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
        assert_eq!(self.status, status, "{}", self.body)
    }
}
async fn request(
    router: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut r = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(c) = client {
        r = r
            .header("cookie", &c.cookie)
            .header("x-csrf-token", &c.csrf)
    }
    for (k, v) in headers {
        r = r.header(*k, *v)
    }
    if body.is_some() {
        r = r.header("content-type", "application/json")
    }
    let response = router
        .clone()
        .oneshot(
            r.body(Body::from(
                body.map(|v| serde_json::to_vec(&v).unwrap())
                    .unwrap_or_default(),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    Reply {
        status,
        headers,
        body: if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scope_routes_enforce_preconditions_and_replay_the_confirmed_result() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic())
    }
}
async fn exercise(h: &Harness) {
    let setup=request(&h.router,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"scope-owner@example.test","display_name":"Example Owner","password":"Scope HTTP contract password 482!","workspace_name":"Scope contract","team_name":"Delivery","team_key":"SCO"})),&[]).await;
    setup.expect(StatusCode::CREATED);
    let c = Client {
        cookie: setup.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: setup.body["csrf_token"].as_str().unwrap().into(),
    };
    let base = format!(
        "/api/v1/workspaces/{}",
        setup.body["workspaces"][0]["id"].as_str().unwrap()
    );
    let boot = request(
        &h.router,
        Some(&c),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    boot.expect(StatusCode::OK);
    let team = boot.body["teams"][0]["id"].as_str().unwrap();
    let directory = format!("{base}/teams/{team}/projects");
    let project = request(
        &h.router,
        Some(&c),
        "POST",
        &directory,
        Some(json!({"name":"Reviewed scope"})),
        &[("idempotency-key", "http-scope-project")],
    )
    .await;
    project.expect(StatusCode::CREATED);
    let project_path = format!("{directory}/{}", project.body["id"].as_str().unwrap());
    let issue=request(&h.router,Some(&c),"POST",&format!("{base}/issues"),Some(json!({"team_id":team,"title":"Saved scope source","project_id":project.body["id"],"project_etag":project.body["etag"]})),&[("idempotency-key","http-scope-linked-issue")]).await;
    issue.expect(StatusCode::CREATED);
    let scope = request(
        &h.router,
        Some(&c),
        "GET",
        &format!("{project_path}/scope-history"),
        None,
        &[],
    )
    .await;
    scope.expect(StatusCode::OK);
    assert_eq!(scope.body["current"][0]["id"], issue.body["id"]);
    request(
        &h.router,
        None,
        "GET",
        &format!("{project_path}/scope-history"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
    request(
        &h.router,
        Some(&c),
        "GET",
        &format!("{project_path}/scope-history?limit=26"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    request(
        &h.router,
        Some(&c),
        "GET",
        &format!("{project_path}/scope-history?unknown=true"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::BAD_REQUEST);
    let body = json!({"label":"Delivery baseline","reason":"Reviewed deliverable"});
    let token = scope.body["scope_etag"].as_str().unwrap();
    request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{project_path}/scope-baselines"),
        Some(body.clone()),
        &[("idempotency-key", "http-scope-agree")],
    )
    .await
    .expect(StatusCode::PRECONDITION_REQUIRED);
    let agreed = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{project_path}/scope-baselines"),
        Some(body.clone()),
        &[("if-match", token), ("idempotency-key", "http-scope-agree")],
    )
    .await;
    agreed.expect(StatusCode::CREATED);
    assert_eq!(agreed.body["comparison"]["unchanged"], 1);
    assert_ne!(
        agreed.headers["etag"].to_str().unwrap(),
        agreed.body["scope_etag"].as_str().unwrap()
    );
    let replay = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{project_path}/scope-baselines"),
        Some(body),
        &[("if-match", token), ("idempotency-key", "http-scope-agree")],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body["baseline"]["id"], agreed.body["baseline"]["id"]);
    let change =
        json!({"issue_id":issue.body["id"],"action":"postponed","reason":"Await dependency"});
    request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{project_path}/scope-changes"),
        Some(change.clone()),
        &[("if-match", token), ("idempotency-key", "http-scope-stale")],
    )
    .await
    .expect(StatusCode::PRECONDITION_FAILED);
    let changed = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{project_path}/scope-changes"),
        Some(change),
        &[
            ("if-match", agreed.body["scope_etag"].as_str().unwrap()),
            ("idempotency-key", "http-scope-postpone"),
        ],
    )
    .await;
    changed.expect(StatusCode::OK);
    assert_eq!(changed.body["current"], json!([]));
    assert_eq!(
        changed.body["comparison"]["postponed"][0]["reason"],
        "Await dependency"
    );
    let live = request(
        &h.router,
        Some(&c),
        "GET",
        &format!("{base}/issues/{}", issue.body["id"].as_str().unwrap()),
        None,
        &[],
    )
    .await;
    live.expect(StatusCode::OK);
    assert_eq!(live.body["status_id"], issue.body["status_id"]);

    let update = json!({"body":"Delivery update"});
    request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{project_path}/updates"),
        Some(update.clone()),
        &[("idempotency-key", "http-expiring-update")],
    )
    .await
    .expect(StatusCode::CREATED);
    let mut tx = h
        .lock_team(
            setup.body["workspaces"][0]["id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            team.parse().unwrap(),
            setup.body["user"]["id"].as_str().unwrap().parse().unwrap(),
        )
        .await;
    sqlx::query("UPDATE idempotency_records SET expires_at=now()-interval '1 day'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for (route, key, body, changed) in [
        (
            "scope-baselines",
            "http-scope-agree",
            json!({"label":"Delivery baseline","reason":"Reviewed deliverable"}),
            json!({"label":"Other baseline","reason":"Reviewed deliverable"}),
        ),
        (
            "scope-changes",
            "http-scope-postpone",
            json!({"issue_id":issue.body["id"],"action":"postponed","reason":"Await dependency"}),
            json!({"issue_id":issue.body["id"],"action":"postponed","reason":"Different reason"}),
        ),
        (
            "updates",
            "http-expiring-update",
            update,
            json!({"body":"Other update"}),
        ),
    ] {
        let path = format!("{project_path}/{route}");
        let headers = [("if-match", token), ("idempotency-key", key)];
        let expired = request(&h.router, Some(&c), "POST", &path, Some(body), &headers).await;
        expired.expect(StatusCode::CONFLICT);
        assert_eq!(expired.body["code"], "conflict");
        assert_eq!(expired.body["details"]["retry_window_expired"], true);
        let mismatch = request(&h.router, Some(&c), "POST", &path, Some(changed), &headers).await;
        mismatch.expect(StatusCode::CONFLICT);
        assert!(mismatch.body["details"].is_null());
    }
    let other_route = request(
        &h.router,
        Some(&c),
        "POST",
        &directory,
        Some(json!({"name":"Reviewed scope"})),
        &[("idempotency-key", "http-scope-project")],
    )
    .await;
    other_route.expect(StatusCode::CONFLICT);
    assert!(other_route.body["details"].is_null());
}
