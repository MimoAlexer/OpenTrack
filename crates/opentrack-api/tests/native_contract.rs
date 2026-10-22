//! Real PostgreSQL HTTP native/web audience and one-use handoff checks.
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
use url::Url;

const VERIFIER: &str = "eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHg";
const CHALLENGE: &str = "r2OTv5p8bMf1Vxfo6zus0GD5OCMULnxAyfo2HEtvEp0";
const STATE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}
impl Reply {
    fn status(&self, status: StatusCode) {
        assert_eq!(self.status, status);
    }
    fn problem(&self, status: StatusCode, code: &str) {
        self.status(status);
        assert_eq!(self.body["code"], code);
        assert_eq!(self.headers["content-type"], "application/problem+json");
        assert!(!self.headers.contains_key("set-cookie"));
    }
}
async fn req(
    router: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let result = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map(|body| serde_json::to_vec(&body).unwrap())
                        .unwrap_or_default(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = result.status();
    let headers = result.headers().clone();
    let bytes = to_bytes(result.into_body(), 1024 * 1024).await.unwrap();
    if path.starts_with("/api/") {
        assert_eq!(headers["cache-control"], "no-store");
    }
    if path.starts_with("/api/v1/auth/native/") {
        assert_eq!(headers["referrer-policy"], "no-referrer");
        assert!(!headers.contains_key("etag"));
        assert!(!headers.contains_key("set-cookie"));
    }
    Reply {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}
async fn setup(h: &Harness) -> Reply {
    let reply = req(&h.router, "POST", "/api/v1/setup", &[("origin", ORIGIN)], Some(json!({
        "setup_token": h.setup_token, "email":"owner@native-http.invalid", "display_name":"Native owner",
        "password":"Native HTTP local fixture passphrase 2026!", "workspace_name":"Native contracts",
        "team_name":"Engineering", "team_key":"ENG"
    }))).await;
    reply.status(StatusCode::CREATED);
    assert_eq!(reply.body["audience"], "web");
    reply
}
fn cookie(reply: &Reply) -> String {
    reply.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}
fn start_body() -> Value {
    json!({"code_challenge":CHALLENGE,"callback_uri":"http://127.0.0.1:38419/opentrack/native-callback","state":STATE,"device_name":"Linux test desktop"})
}
async fn grant(h: &Harness, setup: &Reply) -> Reply {
    let start = req(
        &h.router,
        "POST",
        "/api/v1/auth/native/requests",
        &[],
        Some(start_body()),
    )
    .await;
    start.status(StatusCode::CREATED);
    assert!(!start.headers.contains_key("location"));
    let id = start.body["id"].as_str().unwrap();
    assert_eq!(
        start.body["authorization_path"],
        format!("/auth/native/approve?request_id={id}")
    );
    let path = format!("/api/v1/auth/native/requests/{id}");
    req(&h.router, "GET", &path, &[], None)
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let cookie = cookie(setup);
    let csrf = setup.body["csrf_token"].as_str().unwrap();
    let browser = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf),
    ];
    let consent = req(&h.router, "GET", &path, &browser, None).await;
    consent.status(StatusCode::OK);
    assert_eq!(consent.body["device_name"], "Linux test desktop");
    assert!(consent.body.get("code_challenge").is_none());
    let body = json!({"confirmation_code":start.body["confirmation_code"]});
    req(
        &h.router,
        "POST",
        &format!("{path}/approve"),
        &[("cookie", cookie.as_str()), ("origin", ORIGIN)],
        Some(body.clone()),
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    req(
        &h.router,
        "POST",
        &format!("{path}/approve"),
        &[
            ("cookie", cookie.as_str()),
            ("origin", "https://untrusted.invalid"),
            ("x-csrf-token", csrf),
        ],
        Some(body.clone()),
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let approved = req(
        &h.router,
        "POST",
        &format!("{path}/approve"),
        &browser,
        Some(body),
    )
    .await;
    approved.status(StatusCode::OK);
    let callback = Url::parse(approved.body["callback_uri"].as_str().unwrap()).unwrap();
    assert_eq!(
        callback.origin().ascii_serialization(),
        "http://127.0.0.1:38419"
    );
    assert_eq!(callback.path(), "/opentrack/native-callback");
    let query: std::collections::HashMap<_, _> = callback.query_pairs().collect();
    assert_eq!(query.len(), 3);
    assert_eq!(query["state"], STATE);
    assert_eq!(query["request_id"], id);
    let exchange = json!({"request_id":id,"code":query["code"],"code_verifier":VERIFIER});
    let mut wrong = exchange.clone();
    wrong["code_verifier"] = json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    req(
        &h.router,
        "POST",
        "/api/v1/auth/native/exchange",
        &[],
        Some(wrong),
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "native_auth_invalid");
    let grant = req(
        &h.router,
        "POST",
        "/api/v1/auth/native/exchange",
        &[],
        Some(exchange.clone()),
    )
    .await;
    grant.status(StatusCode::OK);
    assert_eq!(grant.body["token_type"], "Bearer");
    assert_eq!(grant.body["session"]["audience"], "desktop");
    assert_eq!(grant.body["session"]["csrf_token"], "");
    req(
        &h.router,
        "POST",
        "/api/v1/auth/native/exchange",
        &[],
        Some(exchange),
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "native_auth_invalid");
    grant
}

#[tokio::test]
async fn native_handoff_and_shared_issue_workflow() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { workflow(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn workflow(h: &Harness) {
    let setup = setup(h).await;
    let cookie = cookie(&setup);
    let capabilities = req(&h.router, "GET", "/api/v1/client-capabilities", &[], None).await;
    capabilities.status(StatusCode::OK);
    assert_eq!(capabilities.body["native_auth_version"], 1);
    let grant = grant(h, &setup).await;
    let bearer = format!("Bearer {}", grant.body["access_token"].as_str().unwrap());
    let native = [("authorization", bearer.as_str())];
    req(&h.router, "GET", "/api/v1/auth/session", &native, None)
        .await
        .status(StatusCode::OK);
    let w = setup.body["workspaces"][0]["id"].as_str().unwrap();
    let base = format!("/api/v1/workspaces/{w}");
    let boot = req(
        &h.router,
        "GET",
        &format!("{base}/bootstrap"),
        &native,
        None,
    )
    .await;
    boot.status(StatusCode::OK);
    let issue = req(
        &h.router,
        "POST",
        &format!("{base}/issues"),
        &[
            ("authorization", bearer.as_str()),
            ("idempotency-key", "native-shared-issue"),
        ],
        Some(json!({"team_id":boot.body["teams"][0]["id"],"title":"Shared from desktop"})),
    )
    .await;
    issue.status(StatusCode::CREATED);
    let read = req(
        &h.router,
        "GET",
        &format!("{base}/issues/{}", issue.body["id"].as_str().unwrap()),
        &[("cookie", cookie.as_str())],
        None,
    )
    .await;
    read.status(StatusCode::OK);
    assert_eq!(read.body["title"], "Shared from desktop");
    let sessions = req(
        &h.router,
        "GET",
        "/api/v1/auth/sessions",
        &[("cookie", cookie.as_str())],
        None,
    )
    .await;
    sessions.status(StatusCode::OK);
    assert!(
        sessions.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == grant.body["session_id"]
                && s["audience"] == "desktop"
                && s["device_name"] == "Linux test desktop")
    );
    let native_cookie = format!(
        "opentrack_session={}",
        grant.body["access_token"].as_str().unwrap()
    );
    req(
        &h.router,
        "GET",
        "/api/v1/auth/session",
        &[("cookie", native_cookie.as_str())],
        None,
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let web_bearer = format!(
        "Bearer {}",
        cookie.strip_prefix("opentrack_session=").unwrap()
    );
    req(
        &h.router,
        "GET",
        "/api/v1/auth/session",
        &[("authorization", web_bearer.as_str())],
        None,
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    req(
        &h.router,
        "GET",
        "/api/v1/auth/session",
        &[
            ("cookie", cookie.as_str()),
            ("authorization", bearer.as_str()),
        ],
        None,
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let other = self::grant(h, &setup).await;
    let other_bearer = format!("Bearer {}", other.body["access_token"].as_str().unwrap());
    let other_native = [("authorization", other_bearer.as_str())];
    let revoked = req(
        &h.router,
        "POST",
        &format!(
            "/api/v1/auth/sessions/{}/revoke",
            other.body["session_id"].as_str().unwrap()
        ),
        &other_native,
        None,
    )
    .await;
    revoked.status(StatusCode::NO_CONTENT);
    assert_eq!(revoked.headers["x-opentrack-session-invalidated"], "true");
    assert!(!revoked.headers.contains_key("set-cookie"));
    req(
        &h.router,
        "GET",
        "/api/v1/auth/session",
        &other_native,
        None,
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let logout = req(&h.router, "POST", "/api/v1/auth/logout", &native, None).await;
    logout.status(StatusCode::NO_CONTENT);
    assert!(!logout.headers.contains_key("set-cookie"));
    req(&h.router, "GET", "/api/v1/auth/session", &native, None)
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    req(
        &h.router,
        "GET",
        "/api/v1/auth/session",
        &[("cookie", cookie.as_str())],
        None,
    )
    .await
    .status(StatusCode::OK);
}

#[tokio::test]
async fn browser_only_routes_reject_native_credentials_and_consent_is_private() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { audience(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn audience(h: &Harness) {
    let setup = setup(h).await;
    let cookie = cookie(&setup);
    // A syntactically valid supplied bearer must be rejected before dispatching
    // a browser-only operation, even when no business input is valid.
    let bearer = format!("Bearer {}", "a".repeat(64));
    for path in [
        "/api/v1/setup",
        "/api/v1/auth/login",
        "/api/v1/auth/recovery/complete",
        "/api/v1/invitations/preview",
        "/api/v1/invitations/accept",
    ] {
        req(
            &h.router,
            "POST",
            path,
            &[("origin", ORIGIN), ("authorization", bearer.as_str())],
            Some(json!({})),
        )
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    req(
        &h.router,
        "POST",
        "/api/v1/auth/native/requests",
        &[("cookie", cookie.as_str())],
        Some(start_body()),
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    req(
        &h.router,
        "POST",
        "/api/v1/auth/native/exchange",
        &[("cookie", cookie.as_str())],
        Some(json!({})),
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    req(
        &h.router,
        "GET",
        "/api/v1/auth/native/requests/00000000-0000-4000-8000-000000000001",
        &[("authorization", bearer.as_str())],
        None,
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let document = req(
        &h.router,
        "GET",
        "/auth/native/approve?request_id=00000000-0000-4000-8000-000000000001",
        &[("accept", "text/html")],
        None,
    )
    .await;
    document.status(StatusCode::OK);
    assert_eq!(document.headers["cache-control"], "no-store");
    assert_eq!(document.headers["referrer-policy"], "no-referrer");
}
