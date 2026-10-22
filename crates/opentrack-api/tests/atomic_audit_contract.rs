//! HTTP failure boundaries for mandatory transactional audit and activity writes.
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
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use std::{path::PathBuf, sync::Arc};
use support::Harness;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://atomic-audit.contract.invalid";
const EMAIL: &str = "atomic-audit@contract.invalid";
const PASSWORD: &str = "HTTP atomic audit old passphrase 746!";
const NEW_PASSWORD: &str = "HTTP atomic audit new passphrase 853!";

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("Expected JSON response")
    }
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).unwrap().to_str().unwrap()
    }
    fn status(&self, expected: StatusCode) {
        assert_eq!(self.status, expected);
    }
    fn no_cookie(&self) {
        assert!(!self.headers.contains_key("set-cookie"));
        assert!(!self.headers.contains_key("x-opentrack-session-invalidated"));
    }
    fn problem(&self, status: StatusCode, code: &str) {
        self.status(status);
        self.no_cookie();
        assert_eq!(self.header("content-type"), "application/problem+json");
        let body = self.json();
        assert_eq!(body["code"], code);
        assert_eq!(body["status"], status.as_u16());
        assert_eq!(body["request_id"], self.header("x-request-id"));
        assert!(!self.headers.contains_key("location"));
        assert!(!self.headers.contains_key("etag"));
    }
    fn excludes(&self, secret: &str) {
        assert!(
            !String::from_utf8_lossy(&self.body).contains(secret),
            "Response must not echo a credential"
        );
    }
}
struct Client {
    id: Uuid,
    cookie: String,
    csrf: String,
}
impl Client {
    fn from(reply: &Reply) -> Self {
        let body = reply.json();
        let cookie = reply.header("set-cookie");
        assert!(!cookie.contains("Max-Age=0"));
        for flag in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
            assert!(cookie.contains(flag));
        }
        Self {
            id: Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap(),
            cookie: cookie.split(';').next().unwrap().to_owned(),
            csrf: body["csrf_token"].as_str().unwrap().to_owned(),
        }
    }
    fn headers(&self) -> Vec<(&str, &str)> {
        vec![
            ("origin", ORIGIN),
            ("cookie", &self.cookie),
            ("x-csrf-token", &self.csrf),
        ]
    }
}
fn make_router(h: &Harness) -> Router {
    router(ApiState {
        attachments: None,
        app: h.app.clone(),
        origins: Arc::new(vec![ORIGIN.into()]),
        secure_cookie: true,
        static_dir: Arc::new(PathBuf::from("/unused-atomic-audit-contract-static")),
    })
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
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(
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
    Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).unwrap();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    let reply = Reply {
        status,
        headers,
        body,
    };
    reply.excludes(PASSWORD);
    reply.excludes(NEW_PASSWORD);
    reply
}
async fn read(router: &Router, client: &Client, path: &str) -> Reply {
    let reply = request(router, "GET", path, None, &client.headers()).await;
    reply.status(StatusCode::OK);
    reply.no_cookie();
    reply
}
async fn login(router: &Router, password: &str) -> Client {
    let reply = request(
        router,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":EMAIL,"password":password})),
        &[("origin", ORIGIN)],
    )
    .await;
    reply.status(StatusCode::OK);
    Client::from(&reply)
}
// All fixture SQL interpolation is from fixed identifiers/predicates below or a
// server-generated UUID. No credential, arbitrary input or query value enters it.
async fn suppress(h: &Harness, table: &'static str, predicate: &str) {
    assert!(matches!(
        table,
        "audit_entries" | "account_security_events" | "activity_entries" | "workspace_events"
    ));
    sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION suppress_required_http_effect() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {predicate} THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER zz_suppress_required_http_effect BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION suppress_required_http_effect();")))
        .execute(&h.owner).await.unwrap();
}
async fn unsuppress(h: &Harness, table: &'static str) {
    assert!(matches!(
        table,
        "audit_entries" | "account_security_events" | "activity_entries" | "workspace_events"
    ));
    sqlx::raw_sql(AssertSqlSafe(format!("DROP TRIGGER zz_suppress_required_http_effect ON {table}; DROP FUNCTION suppress_required_http_effect();")))
        .execute(&h.owner).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_atomic_audit_boundaries() {
    let h = Harness::new().await;
    let runner = h.clone();
    let outcome = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let router = make_router(h);
    let setup = json!({"setup_token":h.setup_token,"email":EMAIL,"password":PASSWORD,"display_name":"Audit owner",
        "workspace_name":"Audit boundaries","team_name":"Engineering","team_key":"ENG"});
    suppress(h, "audit_entries", "NEW.action='instance.setup'").await;
    let failed = request(
        &router,
        "POST",
        "/api/v1/setup",
        Some(setup.clone()),
        &[("origin", ORIGIN)],
    )
    .await;
    failed.problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    failed.excludes(&h.setup_token);
    let status = request(&router, "GET", "/api/v1/setup/status", None, &[]).await;
    status.status(StatusCode::OK);
    assert_eq!(status.json()["setup_required"], true);
    unsuppress(h, "audit_entries").await;
    let created = request(
        &router,
        "POST",
        "/api/v1/setup",
        Some(setup),
        &[("origin", ORIGIN)],
    )
    .await;
    created.status(StatusCode::CREATED);
    let owner = Client::from(&created);
    let w = Uuid::parse_str(created.json()["workspaces"][0]["id"].as_str().unwrap()).unwrap();
    bulk_revoke(h, &router, &owner).await;
    issue_mutations(h, &router, &owner, w).await;
    recovery_completion(h, &router, &owner).await;
}
async fn bulk_revoke(h: &Harness, router: &Router, owner: &Client) {
    let second = login(router, PASSWORD).await;
    let third = login(router, PASSWORD).await;
    let listed = read(router, &second, "/api/v1/auth/sessions").await.json();
    let victim = Uuid::parse_str(
        listed["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["current"] == true)
            .unwrap()["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    // Drop one of two required audit rows, detecting partial rather than only
    // all-zero omissions. The HTTP failure must leave every cookie authorized.
    suppress(
        h,
        "account_security_events",
        &format!("NEW.action='session.revoked' AND NEW.session_id='{victim}'::uuid"),
    )
    .await;
    let failed = request(
        router,
        "POST",
        "/api/v1/auth/sessions/revoke-others",
        None,
        &owner.headers(),
    )
    .await;
    failed.problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    for client in [owner, &second, &third] {
        read(router, client, "/api/v1/auth/session").await;
    }
    assert_eq!(
        read(router, owner, "/api/v1/auth/sessions").await.json()["data"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    unsuppress(h, "account_security_events").await;
    let committed = request(
        router,
        "POST",
        "/api/v1/auth/sessions/revoke-others",
        None,
        &owner.headers(),
    )
    .await;
    committed.status(StatusCode::NO_CONTENT);
    committed.no_cookie();
    assert!(committed.body.is_empty());
    read(router, owner, "/api/v1/auth/session").await;
    for client in [&second, &third] {
        request(
            router,
            "GET",
            "/api/v1/auth/session",
            None,
            &client.headers(),
        )
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
}
async fn issue_mutations(h: &Harness, router: &Router, owner: &Client, w: Uuid) {
    let bootstrap = read(router, owner, &format!("/api/v1/workspaces/{w}/bootstrap"))
        .await
        .json();
    let team = bootstrap["teams"][0]["id"].as_str().unwrap();
    let path = format!("/api/v1/workspaces/{w}/issues");
    let body = json!({"team_id":team,"title":"Atomic HTTP issue"});
    let mut create_headers = owner.headers();
    create_headers.push(("idempotency-key", "atomic-http-issue-create"));
    suppress(h, "workspace_events", "NEW.event_type='issue.created'").await;
    let failed = request(router, "POST", &path, Some(body.clone()), &create_headers).await;
    failed.problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    assert!(
        read(router, owner, &path).await.json()["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    unsuppress(h, "workspace_events").await;
    let created = request(router, "POST", &path, Some(body), &create_headers).await;
    created.status(StatusCode::CREATED);
    created.no_cookie();
    assert_eq!(created.json()["key"], "ENG-1");
    let location = created.header("location").to_owned();
    let etag = created.header("etag").to_owned();
    let original = created.json();
    let activity_path = format!("{location}/activity");
    let activity_before = read(router, owner, &activity_path).await.json();
    let mut update_headers = owner.headers();
    update_headers.push(("if-match", &etag));
    suppress(h, "activity_entries", "NEW.event_kind='issue.updated'").await;
    let failed = request(
        router,
        "PATCH",
        &location,
        Some(json!({"title":"Saved only with its activity"})),
        &update_headers,
    )
    .await;
    failed.problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    let unchanged = read(router, owner, &location).await;
    assert_eq!(unchanged.json(), original);
    assert_eq!(unchanged.header("etag"), etag);
    assert_eq!(
        read(router, owner, &activity_path).await.json(),
        activity_before
    );
    unsuppress(h, "activity_entries").await;
    let committed = request(
        router,
        "PATCH",
        &location,
        Some(json!({"title":"Saved only with its activity"})),
        &update_headers,
    )
    .await;
    committed.status(StatusCode::OK);
    committed.no_cookie();
    assert_eq!(committed.json()["version"], "2");
    assert_eq!(committed.json()["title"], "Saved only with its activity");
}
async fn recovery_completion(h: &Harness, router: &Router, owner: &Client) {
    let second = login(router, PASSWORD).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let code = generate_recovery_code(owner.id);
    operator
        .issue_recovery(
            owner.id,
            Uuid::new_v4(),
            &code,
            30,
            "HTTP audit failure boundary",
        )
        .await
        .unwrap();
    suppress(
        h,
        "account_security_events",
        "NEW.action='recovery.completed'",
    )
    .await;
    let body = json!({"token":code,"password":NEW_PASSWORD});
    let failed = request(
        router,
        "POST",
        "/api/v1/auth/recovery/complete",
        Some(body.clone()),
        &owner.headers(),
    )
    .await;
    failed.problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    failed.excludes(&code);
    read(router, owner, "/api/v1/auth/session").await;
    read(router, &second, "/api/v1/auth/session").await;
    let old_password_still_works = login(router, PASSWORD).await;
    unsuppress(h, "account_security_events").await;
    // The same code can be retried because the unsuccessful attempt consumed
    // neither the token nor old sessions and changed no password.
    let completed = request(
        router,
        "POST",
        "/api/v1/auth/recovery/complete",
        Some(body),
        &owner.headers(),
    )
    .await;
    completed.status(StatusCode::NO_CONTENT);
    assert!(completed.body.is_empty());
    let cookie = completed.header("set-cookie");
    assert!(cookie.starts_with("opentrack_session=;"));
    for flag in ["Max-Age=0", "HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(cookie.contains(flag));
    }
    for client in [owner, &second, &old_password_still_works] {
        request(
            router,
            "GET",
            "/api/v1/auth/session",
            None,
            &client.headers(),
        )
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    let replay = request(
        router,
        "POST",
        "/api/v1/auth/recovery/complete",
        Some(json!({"token":code,"password":NEW_PASSWORD})),
        &[("origin", ORIGIN)],
    )
    .await;
    replay.problem(StatusCode::UNPROCESSABLE_ENTITY, "recovery_invalid");
    replay.excludes(&code);
    let fresh = login(router, NEW_PASSWORD).await;
    read(router, &fresh, "/api/v1/auth/session").await;
}
