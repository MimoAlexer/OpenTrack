//! Caller-owned shortcut preference transport with real PostgreSQL.
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
    user: String,
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
        user: reply.body["user"]["id"].as_str().unwrap().into(),
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
async fn preferences_transport_has_caller_scope_csrf_and_strong_preconditions() {
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
    let setup=request(r,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@preferences.invalid","display_name":"Example Owner","password":PASSWORD,"workspace_name":"Preferences","team_name":"Engineering","team_key":"ENG"})),&[]).await;
    setup.expect(StatusCode::CREATED);
    let owner = client(&setup);
    let base = format!(
        "/api/v1/workspaces/{}",
        setup.body["workspaces"][0]["id"].as_str().unwrap()
    );
    let path = format!("{base}/preferences");
    let viewer = invited(r, &owner, &base, "viewer").await;
    request(r, None, "GET", &path, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let original = request(r, Some(&owner), "GET", &path, None, &[]).await;
    original.expect(StatusCode::OK);
    assert_eq!(original.body["shortcut_mode"], "enabled");
    assert_eq!(original.body["version"], "0");
    assert_eq!(original.body["etag"], original.etag());
    request(
        r,
        Some(&owner),
        "PATCH",
        &path,
        Some(json!({"shortcut_mode":"disabled"})),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let mut no_csrf = owner.clone();
    no_csrf.csrf = "wrong".into();
    request(
        r,
        Some(&no_csrf),
        "PATCH",
        &path,
        Some(json!({"shortcut_mode":"disabled"})),
        &[("if-match", original.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    for invalid in [
        json!({}),
        json!({"shortcut_mode":null}),
        json!({"shortcut_mode":"script"}),
        json!({"shortcut_mode":"disabled","user_id":viewer.user}),
        json!({"shortcut_mode":"disabled","workspace_id":uuid::Uuid::new_v4()}),
    ] {
        let expected = if invalid.get("shortcut_mode").is_some_and(Value::is_null) {
            (StatusCode::BAD_REQUEST, "bad_request")
        } else {
            (StatusCode::UNPROCESSABLE_ENTITY, "validation_failed")
        };
        let rejected = request(
            r,
            Some(&owner),
            "PATCH",
            &path,
            Some(invalid.clone()),
            &[("if-match", original.etag())],
        )
        .await;
        assert_eq!(
            rejected.status, expected.0,
            "Invalid preference {invalid}: {}",
            rejected.body
        );
        rejected.problem(expected.0, expected.1);
    }
    let saved = request(
        r,
        Some(&owner),
        "PATCH",
        &path,
        Some(json!({"shortcut_mode":"disabled"})),
        &[("if-match", original.etag())],
    )
    .await;
    saved.expect(StatusCode::OK);
    assert_eq!(saved.body["shortcut_mode"], "disabled");
    assert_eq!(saved.body["version"], "1");
    let stale = request(
        r,
        Some(&owner),
        "PATCH",
        &path,
        Some(json!({"shortcut_mode":"enabled"})),
        &[("if-match", original.etag())],
    )
    .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale.body["details"]["current"], saved.body);
    let reloaded = request(r, Some(&owner), "GET", &path, None, &[]).await;
    assert_eq!(reloaded.body, saved.body);
    let personal = request(r, Some(&viewer), "GET", &path, None, &[]).await;
    assert_eq!(personal.body["shortcut_mode"], "enabled");
    assert_ne!(personal.etag(), original.etag());
    request(
        r,
        Some(&viewer),
        "PATCH",
        &path,
        Some(json!({"shortcut_mode":"disabled"})),
        &[("if-match", saved.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    request(
        r,
        Some(&viewer),
        "PATCH",
        &path,
        Some(json!({"shortcut_mode":"disabled"})),
        &[("if-match", personal.etag())],
    )
    .await
    .expect(StatusCode::OK);
    let unavailable = format!("/api/v1/workspaces/{}/preferences", uuid::Uuid::new_v4());
    request(r, Some(&owner), "GET", &unavailable, None, &[])
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    request(
        r,
        Some(&owner),
        "PATCH",
        &unavailable,
        Some(json!({"shortcut_mode":"disabled"})),
        &[("if-match", saved.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    for method in ["POST", "PUT", "DELETE"] {
        request(r, Some(&owner), method, &path, None, &[])
            .await
            .problem(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    }
    request(
        r,
        Some(&owner),
        "PATCH",
        &format!("{path}/{}", viewer.user),
        Some(json!({"shortcut_mode":"disabled"})),
        &[("if-match", saved.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
}
