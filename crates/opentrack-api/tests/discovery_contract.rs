//! Saved discovery and search transport with real PostgreSQL.
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

#[derive(Clone)]
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
    #[track_caller]
    fn expect(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{}", self.body);
    }
    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
    #[track_caller]
    fn problem(&self, status: StatusCode, code: &str) {
        self.expect(status);
        assert_eq!(self.body["code"], code);
        assert_eq!(self.headers["content-type"], "application/problem+json");
        assert!(!self.headers.contains_key("etag"));
        assert!(!self.headers.contains_key("location"));
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

fn client(reply: &Reply) -> Client {
    Client {
        cookie: reply.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: reply.body["csrf_token"].as_str().unwrap().into(),
    }
}
const PASSWORD: &str = "Preference HTTP verification password 2026!";
async fn invited(r: &Router, owner: &Client, base: &str, role: &str) -> Client {
    let invite = request(
        r,
        Some(owner),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":format!("{role}@preferences.invalid"),"role":role})),
        &[("idempotency-key", &format!("preferences-invite-{role}"))],
    )
    .await;
    invite.expect(StatusCode::CREATED);
    let accepted=request(r,None,"POST","/api/v1/invitations/accept",Some(json!({"token":invite.body["token"],"display_name":format!("Example {role}"),"password":PASSWORD})),&[]).await;
    accepted.expect(StatusCode::OK);
    client(&accepted)
}
#[tokio::test]
async fn discovery_http_scope_body_protocol_and_preconditions() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let r = &h.router;
    let setup=request(r,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@example.test","password":PASSWORD,"display_name":"Example Owner","workspace_name":"Discovery","team_name":"Public","team_key":"PUB"})),&[]).await;
    setup.expect(StatusCode::CREATED);
    let a = client(&setup);
    let w = setup.body["workspaces"][0]["id"].as_str().unwrap();
    let base = format!("/api/v1/workspaces/{w}");
    let b = invited(r, &a, &base, "member").await;
    let definition = json!({"name":"Private title","kind":"search","visibility":"personal","definition":{"query":{"version":1,"filter":null,"q":"nebula"},"layout":"list","group_by":"status","columns":["key","title"]}});
    let path = format!("{base}/saved-searches");
    let created = request(
        r,
        Some(&a),
        "POST",
        &path,
        Some(definition.clone()),
        &[("idempotency-key", "http-save-1")],
    )
    .await;
    created.expect(StatusCode::CREATED);
    let mut malicious = definition.clone();
    malicious["visibility"] = json!("workspace");
    request(
        r,
        Some(&a),
        "POST",
        &path,
        Some(malicious),
        &[("idempotency-key", "http-private-only")],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let id = created.body["id"].as_str().unwrap();
    let item = format!("{path}/{id}");
    let other = request(r, Some(&b), "GET", &path, None, &[]).await;
    other.expect(StatusCode::OK);
    assert_eq!(other.body["data"], json!([]));
    for (method, url, body, extra) in [
        ("GET", item.clone(), None, vec![]),
        ("POST", format!("{item}/execute"), Some(json!({})), vec![]),
        (
            "PATCH",
            item.clone(),
            Some(definition.clone()),
            vec![
                ("if-match", created.etag()),
                ("idempotency-key", "http-other-rename"),
            ],
        ),
        (
            "DELETE",
            item.clone(),
            None,
            vec![
                ("if-match", created.etag()),
                ("idempotency-key", "http-other-delete"),
            ],
        ),
    ] {
        request(r, Some(&b), method, &url, body, &extra)
            .await
            .problem(StatusCode::NOT_FOUND, "not_found");
    }
    request(r, Some(&a), "GET", &format!("{base}/views/{id}"), None, &[])
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    request(
        r,
        Some(&a),
        "PATCH",
        &item,
        Some(definition.clone()),
        &[("idempotency-key", "http-no-etag")],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(
        r,
        Some(&a),
        "POST",
        &format!("{base}/views"),
        Some(definition.clone()),
        &[("idempotency-key", "http-wrong-kind")],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let query = json!({"query":{"version":1,"filter":null,"q":"nebula"},"types":["issue"]});
    request(
        r,
        Some(&a),
        "POST",
        &format!("{base}/search/query?limit=1"),
        Some(query.clone()),
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let oversized =
        json!({"query":{"version":1,"filter":null,"q":"x".repeat(1024*1024)},"types":["issue"]});
    request(
        r,
        Some(&a),
        "POST",
        &format!("{base}/search/query"),
        Some(oversized),
        &[],
    )
    .await
    .problem(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
    let mut no_csrf = a.clone();
    no_csrf.csrf.clear();
    request(
        r,
        Some(&no_csrf),
        "POST",
        &format!("{base}/search/query"),
        Some(query.clone()),
        &[],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        r,
        Some(&a),
        "POST",
        &format!("{base}/search/query"),
        Some(query),
        &[],
    )
    .await
    .expect(StatusCode::OK);
    request(
        r,
        Some(&a),
        "GET",
        &format!("{base}/search/suggestions?q=nebula&limit=21"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    request(
        r,
        Some(&a),
        "GET",
        &format!("{base}/search?q=nebula&unknown=true"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::BAD_REQUEST);
    request(
        r,
        Some(&a),
        "PUT",
        &format!("{item}/favorite"),
        Some(json!({"favorite":true})),
        &[("idempotency-key", "http-favorite-1")],
    )
    .await
    .expect(StatusCode::OK);
    let mut rename = definition;
    rename["name"] = json!("Renamed search");
    let renamed = request(
        r,
        Some(&a),
        "PATCH",
        &item,
        Some(rename.clone()),
        &[
            ("if-match", created.etag()),
            ("idempotency-key", "http-rename-1"),
        ],
    )
    .await;
    renamed.expect(StatusCode::OK);
    assert_eq!(renamed.body["favorite"], true);
    request(
        r,
        Some(&a),
        "PATCH",
        &item,
        Some(rename),
        &[
            ("if-match", created.etag()),
            ("idempotency-key", "http-rename-2"),
        ],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    for _ in 0..2 {
        request(
            r,
            Some(&a),
            "DELETE",
            &item,
            None,
            &[
                ("if-match", renamed.etag()),
                ("idempotency-key", "http-delete-1"),
            ],
        )
        .await
        .expect(StatusCode::NO_CONTENT);
    }
    request(
        r,
        Some(&a),
        "DELETE",
        &format!("{base}/views/{id}"),
        None,
        &[
            ("if-match", renamed.etag()),
            ("idempotency-key", "http-delete-1"),
        ],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let view_body = json!({"name":"Canonical view","kind":"view","visibility":"personal","definition":renamed.body["definition"]});
    let view = request(
        r,
        Some(&a),
        "POST",
        &format!("{base}/views"),
        Some(view_body),
        &[("idempotency-key", "http-delete-view-create")],
    )
    .await;
    view.expect(StatusCode::CREATED);
    let view_id = view.body["id"].as_str().unwrap();
    for _ in 0..2 {
        request(
            r,
            Some(&a),
            "DELETE",
            &format!("{base}/views/{view_id}"),
            None,
            &[
                ("if-match", view.etag()),
                ("idempotency-key", "http-delete-view-1"),
            ],
        )
        .await
        .expect(StatusCode::NO_CONTENT);
    }
    request(
        r,
        Some(&a),
        "DELETE",
        &format!("{base}/saved-searches/{view_id}"),
        None,
        &[
            ("if-match", view.etag()),
            ("idempotency-key", "http-delete-view-1"),
        ],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
}
