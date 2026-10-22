//! Recovery through the HTTP boundary and real restricted PostgreSQL identities.
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use opentrack_api::{ApiState, router};
use opentrack_app::{Operator, generate_recovery_code};
use opentrack_store as store;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use support::Harness;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://recovery.contract.invalid";
const EMAIL: &str = "recovery-http@contract.invalid";
const OLD_PASSWORD: &str = "HTTP recovery initial password 384!";
const NEW_PASSWORD: &str = "HTTP recovery replacement password 729!";
const RECOVER: &str = "/api/v1/auth/recovery/complete";

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("Expected JSON")
    }
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).unwrap().to_str().unwrap()
    }
    fn status(&self, expected: StatusCode) {
        assert_eq!(self.status, expected);
    }
    fn problem(&self, expected: StatusCode, code: &str) -> Value {
        self.status(expected);
        assert_eq!(self.header("content-type"), "application/problem+json");
        let value = self.json();
        assert_eq!(value["status"], expected.as_u16());
        assert_eq!(value["code"], code);
        assert_eq!(value["type"], format!("/api/v1/problems/{code}"));
        assert_eq!(value["request_id"], self.header("x-request-id"));
        assert_eq!(
            value["instance"],
            format!("urn:uuid:{}", self.header("x-request-id"))
        );
        assert!(value["field_errors"].is_array());
        assert!(value["retryable"].is_boolean());
        assert!(self.headers.get("location").is_none());
        value
    }
    fn cookie(&self) -> String {
        self.header("set-cookie")
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }
}
fn make_router(h: &Harness) -> Router {
    router(ApiState {
        attachments: None,
        app: h.app.clone(),
        origins: Arc::new(vec![ORIGIN.to_owned()]),
        secure_cookie: true,
        static_dir: Arc::new(PathBuf::from("/unused-recovery-contract-static")),
    })
}
async fn raw(
    router: &Router,
    method: &str,
    path: &str,
    body: Vec<u8>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    for (key, value) in headers {
        builder = builder.header(*key, *value);
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
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
async fn post(router: &Router, path: &str, body: Value, headers: &[(&str, &str)]) -> Reply {
    let mut headers = headers.to_vec();
    headers.push(("content-type", "application/json"));
    raw(
        router,
        "POST",
        path,
        serde_json::to_vec(&body).unwrap(),
        &headers,
    )
    .await
}
async fn complete(router: &Router, token: &str, headers: &[(&str, &str)]) -> Reply {
    let result = post(
        router,
        RECOVER,
        json!({"token":token,"password":NEW_PASSWORD}),
        headers,
    )
    .await;
    let serialized = String::from_utf8_lossy(&result.body);
    assert!(
        !token.starts_with("otrec1.") || !serialized.contains(token),
        "Recovery replies never echo bearer codes"
    );
    assert!(!serialized.contains(NEW_PASSWORD));
    result
}
async fn issue(operator: &Operator, user: Uuid) -> (Uuid, String) {
    let token = generate_recovery_code(user);
    let receipt = operator
        .issue_recovery(
            user,
            Uuid::new_v4(),
            &token,
            30,
            "Verified HTTP contract recovery",
        )
        .await
        .unwrap();
    (receipt.id, token)
}
async fn fixture(h: &Harness, user: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    store::context(&mut tx, user, None).await.unwrap();
    tx
}
fn invalid(reply: Reply, expected_detail: &mut Option<String>) {
    let problem = reply.problem(StatusCode::UNPROCESSABLE_ENTITY, "recovery_invalid");
    let detail = problem["detail"].as_str().unwrap().to_owned();
    assert!(!detail.contains(EMAIL));
    if let Some(expected) = expected_detail {
        assert_eq!(&detail, expected);
    } else {
        *expected_detail = Some(detail);
    }
    assert_eq!(problem["retryable"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_http_recovery_contracts() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let router = make_router(h);
    let origin = [("origin", ORIGIN)];
    let setup=post(&router,"/api/v1/setup",json!({"setup_token":h.setup_token,"email":EMAIL,"password":OLD_PASSWORD,
        "display_name":"HTTP recovery owner","workspace_name":"Recovery","team_name":"Engineering","team_key":"ENG"}),&origin).await;
    setup.status(StatusCode::CREATED);
    let user = Uuid::parse_str(setup.json()["user"]["id"].as_str().unwrap()).unwrap();
    let cookie = setup.cookie();
    let second = post(
        &router,
        "/api/v1/auth/login",
        json!({"email":EMAIL,"password":OLD_PASSWORD}),
        &origin,
    )
    .await;
    second.status(StatusCode::OK);
    let second_cookie = second.cookie();
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let (_, valid) = issue(&operator, user).await;
    complete(&router, &valid, &[])
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
    complete(
        &router,
        &valid,
        &[("origin", "https://foreign.contract.invalid")],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    raw(
        &router,
        "POST",
        RECOVER,
        b"{".to_vec(),
        &[("origin", ORIGIN), ("content-type", "application/json")],
    )
    .await
    .problem(StatusCode::BAD_REQUEST, "bad_request");
    post(
        &router,
        RECOVER,
        json!({"token":valid,"password":NEW_PASSWORD,"extra":"not accepted"}),
        &origin,
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    post(
        &router,
        RECOVER,
        json!({"token":valid,"password":"short"}),
        &origin,
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let mut detail = None;
    invalid(complete(&router, "invalid", &origin).await, &mut detail);
    invalid(
        complete(&router, &generate_recovery_code(Uuid::new_v4()), &origin).await,
        &mut detail,
    );
    invalid(
        complete(&router, &generate_recovery_code(user), &origin).await,
        &mut detail,
    );
    let (expiry, expired) = issue(&operator, user).await;
    invalid(complete(&router, &valid, &origin).await, &mut detail);
    let mut tx = fixture(h, user).await;
    sqlx::query("UPDATE recovery_tokens SET created_at=clock_timestamp()-interval '2 hours',expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1")
        .bind(expiry).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    invalid(complete(&router, &expired, &origin).await, &mut detail);
    let (_, revoked) = issue(&operator, user).await;
    operator
        .revoke_recovery(user, "Cancel HTTP recovery code")
        .await
        .unwrap();
    invalid(complete(&router, &revoked, &origin).await, &mut detail);
    // Disable a separate non-owner account through the real capability. The
    // setup owner must remain enabled, and its sessions stay live until reset.
    let disabled_user = Uuid::new_v4();
    let mut tx = fixture(h, user).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Disabled recovery target',password_hash FROM users WHERE id=$3")
        .bind(disabled_user).bind(format!("{disabled_user}@recovery.invalid")).bind(user)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let (_, disabled) = issue(&operator, disabled_user).await;
    operator
        .disable_account(disabled_user, "Disable HTTP recovery fixture")
        .await
        .unwrap();
    invalid(complete(&router, &disabled, &origin).await, &mut detail);
    operator
        .enable_account(disabled_user, "Restore HTTP recovery fixture")
        .await
        .unwrap();
    invalid(complete(&router, &disabled, &origin).await, &mut detail);
    // A fresh router separates transport contract coverage from admission budgets.
    let router = make_router(h);
    let (_, token) = issue(&operator, user).await;
    // Recovery authority comes from the code. No existing cookie or CSRF token is required.
    let recovered = complete(&router, &token, &origin).await;
    recovered.status(StatusCode::NO_CONTENT);
    assert!(recovered.body.is_empty());
    for attribute in ["Max-Age=0", "HttpOnly", "SameSite=Lax", "Path=/", "Secure"] {
        assert!(recovered.header("set-cookie").contains(attribute));
    }
    assert_eq!(recovered.cookie(), "opentrack_session=");
    invalid(complete(&router, &token, &origin).await, &mut detail);
    for old in [cookie, second_cookie] {
        raw(
            &router,
            "GET",
            "/api/v1/auth/session",
            Vec::new(),
            &[("cookie", &old)],
        )
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    raw(&router, "GET", "/api/v1/auth/session", Vec::new(), &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    post(
        &router,
        "/api/v1/auth/login",
        json!({"email":EMAIL,"password":OLD_PASSWORD}),
        &origin,
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let login = post(
        &router,
        "/api/v1/auth/login",
        json!({"email":EMAIL,"password":NEW_PASSWORD}),
        &origin,
    )
    .await;
    login.status(StatusCode::OK);
    assert!(!login.cookie().ends_with('='));
    assert!(!String::from_utf8_lossy(&login.body).contains(&token));
    raw(
        &router,
        "GET",
        "/api/v1/auth/session",
        Vec::new(),
        &[("cookie", &login.cookie())],
    )
    .await
    .status(StatusCode::OK);

    let router = make_router(h);
    let random = generate_recovery_code(Uuid::new_v4());
    for _ in 0..20 {
        invalid(complete(&router, &random, &origin).await, &mut detail);
    }
    let denied = complete(
        &router,
        &random,
        &[
            ("origin", ORIGIN),
            ("x-forwarded-for", "198.51.100.27"),
            ("x-real-ip", "198.51.100.28"),
        ],
    )
    .await;
    denied.problem(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert!(denied.header("retry-after").parse::<u32>().unwrap() > 0);
}
