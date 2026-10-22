//! Session management through the real HTTP router and restricted PostgreSQL role.
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

const EMAIL: &str = "sessions-http@contract.invalid";
const PASSWORD: &str = "HTTP session contract passphrase 772!";
const SESSIONS: &str = "/api/v1/auth/sessions";

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("Expected JSON response")
    }
    fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap()
    }
    fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("Missing {name}"))
            .to_str()
            .unwrap()
    }
    fn status(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{}", self.text());
    }
    fn problem(&self, status: StatusCode, code: &str) {
        self.status(status);
        assert_eq!(self.header("content-type"), "application/problem+json");
        let value = self.json();
        assert_eq!(value["status"], status.as_u16());
        assert_eq!(value["code"], code);
        assert_eq!(value["type"], format!("/api/v1/problems/{code}"));
        assert_eq!(value["request_id"], self.header("x-request-id"));
        assert_eq!(
            value["instance"],
            format!("urn:uuid:{}", self.header("x-request-id"))
        );
        assert!(value["field_errors"].is_array());
    }
    fn cookie(&self) -> String {
        let header = self.header("set-cookie");
        for part in ["HttpOnly", "SameSite=Lax", "Path=/", "Secure"] {
            assert!(header.contains(part));
        }
        header.split(';').next().unwrap().to_owned()
    }
}

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    for (key, value) in headers {
        builder = builder.header(*key, *value);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let payload = body
        .map(|v| serde_json::to_vec(&v).unwrap())
        .unwrap_or_default();
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(payload)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
    Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).unwrap();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    Reply {
        status,
        headers,
        body,
    }
}

fn authenticated<'a>(cookie: &'a str, csrf: &'a str) -> [(&'a str, &'a str); 3] {
    [
        ("origin", ORIGIN),
        ("cookie", cookie),
        ("x-csrf-token", csrf),
    ]
}

async fn login(router: &Router, previous: Option<&str>, password: &str) -> Reply {
    let mut headers = vec![("origin", ORIGIN)];
    if let Some(previous) = previous {
        headers.push(("cookie", previous));
    }
    request(
        router,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":EMAIL,"password":password})),
        &headers,
    )
    .await
}

async fn current_id(router: &Router, cookie: &str) -> String {
    let response = request(router, "GET", SESSIONS, None, &[("cookie", cookie)]).await;
    response.status(StatusCode::OK);
    response.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["current"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_http_session_management_contracts() {
    let harness = Harness::new().await;
    let runner = harness.clone();
    let outcome = tokio::spawn(async move { exercise(&runner).await }).await;
    harness.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let router = &h.router;
    let setup=request(router,"POST","/api/v1/setup",Some(json!({
        "setup_token":h.setup_token,"email":EMAIL,"password":PASSWORD,
        "display_name":"Session owner","workspace_name":"HTTP sessions","team_name":"Engineering","team_key":"ENG"
    })),&[("origin",ORIGIN)]).await;
    setup.status(StatusCode::CREATED);
    let original = setup.cookie();
    let session = setup.json();
    let csrf = session["csrf_token"].as_str().unwrap();
    let w = session["workspaces"][0]["id"].as_str().unwrap();
    let second = login(router, None, PASSWORD).await;
    second.status(StatusCode::OK);
    let second_cookie = second.cookie();
    let third = login(router, None, PASSWORD).await;
    third.status(StatusCode::OK);
    let third_cookie = third.cookie();
    let second_id = current_id(router, &second_cookie).await;
    let original_headers = authenticated(&original, csrf);
    request(router, "GET", SESSIONS, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let list = request(router, "GET", SESSIONS, None, &original_headers).await;
    list.status(StatusCode::OK);
    let list = list.json();
    let data = list["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data.iter().filter(|s| s["current"] == true).count(), 1);
    for session in data {
        let mut keys: Vec<_> = session
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "audience",
                "auth_time",
                "created_at",
                "current",
                "device_name",
                "expires_at",
                "id",
                "idle_expires_at",
                "last_seen_at"
            ]
        );
        assert_eq!(session["audience"], "web");
        assert!(session["device_name"].is_null());
        Uuid::parse_str(session["id"].as_str().unwrap()).unwrap();
    }
    for (query, status, code) in [
        (
            "limit=0",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "limit=101",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        ("limit=nope", StatusCode::BAD_REQUEST, "bad_request"),
        (
            "cursor=invalid",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
    ] {
        request(
            router,
            "GET",
            &format!("{SESSIONS}?{query}"),
            None,
            &original_headers,
        )
        .await
        .problem(status, code);
    }
    let first = request(
        router,
        "GET",
        &format!("{SESSIONS}?limit=1"),
        None,
        &original_headers,
    )
    .await;
    first.status(StatusCode::OK);
    let first = first.json();
    assert_eq!(first["has_more"], true);
    let cursor = first["next_cursor"].as_str().unwrap();
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("limit", "1")
        .append_pair("cursor", cursor)
        .finish();
    let next = request(
        router,
        "GET",
        &format!("{SESSIONS}?{encoded}"),
        None,
        &original_headers,
    )
    .await;
    next.status(StatusCode::OK);
    assert_ne!(first["data"][0]["id"], next.json()["data"][0]["id"]);

    let revoke_path = format!("{SESSIONS}/{second_id}/revoke");
    for headers in [
        vec![("cookie", original.as_str()), ("origin", ORIGIN)],
        vec![
            ("cookie", original.as_str()),
            ("origin", ORIGIN),
            ("x-csrf-token", "wrong"),
        ],
        vec![
            ("cookie", original.as_str()),
            ("origin", "https://foreign.invalid"),
            ("x-csrf-token", csrf),
        ],
    ] {
        request(router, "POST", &revoke_path, None, &headers)
            .await
            .problem(StatusCode::FORBIDDEN, "forbidden");
    }
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &second_cookie)],
    )
    .await
    .status(StatusCode::OK);
    request(
        router,
        "POST",
        &format!("{SESSIONS}/invalid/revoke"),
        None,
        &original_headers,
    )
    .await
    .problem(StatusCode::BAD_REQUEST, "bad_request");
    request(
        router,
        "POST",
        &format!("{SESSIONS}/{}/revoke", Uuid::new_v4()),
        None,
        &original_headers,
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");

    let invite = request(
        router,
        "POST",
        &format!("/api/v1/workspaces/{w}/invitations"),
        Some(json!({"email":"foreign-session@contract.invalid","role":"member"})),
        &[
            &original_headers[..],
            &[("idempotency-key", "session-foreign-account")],
        ]
        .concat(),
    )
    .await;
    invite.status(StatusCode::CREATED);
    let invite = invite.json();
    let token = invite["token"].as_str().unwrap();
    let accept = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":token,"display_name":"Other account","password":PASSWORD})),
        &[("origin", ORIGIN)],
    )
    .await;
    accept.status(StatusCode::OK);
    let foreign = accept.cookie();
    let foreign_id = current_id(router, &foreign).await;
    request(
        router,
        "POST",
        &format!("{SESSIONS}/{foreign_id}/revoke"),
        None,
        &original_headers,
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &foreign)],
    )
    .await
    .status(StatusCode::OK);

    let switched = request(
        router,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":"foreign-session@contract.invalid","password":PASSWORD})),
        &[("origin", ORIGIN), ("cookie", &original)],
    )
    .await;
    switched.status(StatusCode::OK);
    assert_ne!(switched.json()["user"]["id"], session["user"]["id"]);
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &original)],
    )
    .await
    .status(StatusCode::OK);
    // Signing into another account must not revoke the supplied foreign user's
    // session; only a same-user prior cookie participates in login rotation.
    let failed = login(router, Some(&original), "incorrect password").await;
    failed.problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert!(failed.headers.get("set-cookie").is_none());
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &original)],
    )
    .await
    .status(StatusCode::OK);
    let fresh = login(router, Some(&original), PASSWORD).await;
    fresh.status(StatusCode::OK);
    let fresh_cookie = fresh.cookie();
    assert_ne!(fresh_cookie, original);
    let fresh_session = fresh.json();
    let fresh_csrf = fresh_session["csrf_token"].as_str().unwrap();
    assert_ne!(fresh_csrf, csrf);
    let fresh_headers = authenticated(&fresh_cookie, fresh_csrf);
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &original)],
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &second_cookie)],
    )
    .await
    .status(StatusCode::OK);
    let revoked = request(router, "POST", &revoke_path, None, &fresh_headers).await;
    revoked.status(StatusCode::NO_CONTENT);
    assert!(revoked.body.is_empty());
    assert!(revoked.headers.get("set-cookie").is_none());
    request(router, "POST", &revoke_path, None, &fresh_headers)
        .await
        .status(StatusCode::NO_CONTENT);
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &second_cookie)],
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        router,
        "POST",
        &format!("{SESSIONS}/revoke-others"),
        None,
        &[("cookie", &fresh_cookie), ("origin", ORIGIN)],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let others = request(
        router,
        "POST",
        &format!("{SESSIONS}/revoke-others"),
        None,
        &fresh_headers,
    )
    .await;
    others.status(StatusCode::NO_CONTENT);
    assert!(others.headers.get("set-cookie").is_none());
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &third_cookie)],
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &fresh_cookie)],
    )
    .await
    .status(StatusCode::OK);
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &foreign)],
    )
    .await
    .status(StatusCode::OK);
    let current = current_id(router, &fresh_cookie).await;
    let logout = request(
        router,
        "POST",
        &format!("{SESSIONS}/{current}/revoke"),
        None,
        &fresh_headers,
    )
    .await;
    logout.status(StatusCode::NO_CONTENT);
    logout.cookie();
    assert!(logout.header("set-cookie").contains("Max-Age=0"));
    request(router, "GET", SESSIONS, None, &[("cookie", &fresh_cookie)])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
}
