//! Access elevation response semantics through real HTTP and restricted PostgreSQL.
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

const OWNER_EMAIL: &str = "elevation-owner@contract.invalid";
const MEMBER_EMAIL: &str = "elevation-member@contract.invalid";
const PASSWORD: &str = "HTTP access elevation passphrase 591!";
const INVALIDATED: &str = "x-opentrack-session-invalidated";

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
        assert_eq!(
            self.status,
            expected,
            "{}",
            std::str::from_utf8(&self.body).unwrap()
        );
    }
    fn problem(&self, expected: StatusCode, code: &str) {
        self.status(expected);
        assert_eq!(self.header("content-type"), "application/problem+json");
        assert_eq!(self.json()["code"], code);
        assert_eq!(self.json()["request_id"], self.header("x-request-id"));
        self.unchanged_cookie();
    }
    fn unchanged_cookie(&self) {
        assert!(!self.headers.contains_key("set-cookie"));
        assert!(!self.headers.contains_key(INVALIDATED));
    }
    fn invalidated_cookie(&self) {
        assert_eq!(self.header(INVALIDATED), "true");
        let cookie = self.header("set-cookie");
        assert!(cookie.starts_with("opentrack_session=;"));
        for attribute in ["Max-Age=0", "HttpOnly", "SameSite=Lax", "Path=/", "Secure"] {
            assert!(cookie.contains(attribute));
        }
    }
}

struct Client {
    cookie: String,
    csrf: String,
    id: String,
}
impl Client {
    fn from(reply: &Reply) -> Self {
        let cookie = reply.header("set-cookie");
        assert!(!cookie.contains("Max-Age=0"));
        for attribute in ["HttpOnly", "SameSite=Lax", "Path=/", "Secure"] {
            assert!(cookie.contains(attribute));
        }
        assert!(!reply.headers.contains_key(INVALIDATED));
        let body = reply.json();
        Self {
            cookie: cookie.split(';').next().unwrap().to_owned(),
            csrf: body["csrf_token"].as_str().unwrap().to_owned(),
            id: body["user"]["id"].as_str().unwrap().to_owned(),
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

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let body = body
        .map(|value| serde_json::to_vec(&value).unwrap())
        .unwrap_or_default();
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

async fn read(router: &Router, client: &Client, path: &str) -> Reply {
    let reply = request(router, "GET", path, None, &client.headers()).await;
    reply.status(StatusCode::OK);
    reply.unchanged_cookie();
    reply
}

async fn mutate(
    router: &Router,
    client: &Client,
    method: &str,
    path: &str,
    body: Value,
    precondition: (&str, &str),
) -> Reply {
    let mut headers = client.headers();
    headers.push(precondition);
    request(router, method, path, Some(body), &headers).await
}

async fn login(router: &Router, email: &str) -> Client {
    let reply = request(
        router,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":email,"password":PASSWORD})),
        &[("origin", ORIGIN)],
    )
    .await;
    reply.status(StatusCode::OK);
    Client::from(&reply)
}

async fn cookie_status(router: &Router, client: &Client, valid: bool) {
    let reply = request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &client.headers(),
    )
    .await;
    if valid {
        reply.status(StatusCode::OK);
        assert_eq!(reply.json()["user"]["id"], client.id);
        reply.unchanged_cookie();
    } else {
        reply.problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
}

async fn invite(router: &Router, owner: &Client, workspace: &str, role: &str) -> String {
    let key = Uuid::new_v4().to_string();
    let reply = mutate(
        router,
        owner,
        "POST",
        &format!("/api/v1/workspaces/{workspace}/invitations"),
        json!({"email":MEMBER_EMAIL,"role":role}),
        ("idempotency-key", &key),
    )
    .await;
    reply.status(StatusCode::CREATED);
    reply.unchanged_cookie();
    reply.json()["token"].as_str().unwrap().to_owned()
}

async fn current_session(router: &Router, client: &Client) -> Value {
    read(router, client, "/api/v1/auth/sessions").await.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["current"] == true)
        .unwrap()
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_http_access_elevation_cookie_and_entity_contracts() {
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
    let setup = request(router, "POST", "/api/v1/setup", Some(json!({
        "setup_token":h.setup_token,"email":OWNER_EMAIL,"password":PASSWORD,
        "display_name":"Elevation owner","workspace_name":"Access contracts","team_name":"Engineering","team_key":"ENG"
    })), &[("origin", ORIGIN)]).await;
    setup.status(StatusCode::CREATED);
    let mut owner = Client::from(&setup);
    let workspace = setup.json()["workspaces"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let bootstrap = read(
        router,
        &owner,
        &format!("/api/v1/workspaces/{workspace}/bootstrap"),
    )
    .await;
    let team = bootstrap.json()["teams"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let organizations = read(router, &owner, "/api/v1/organizations").await.json();
    let key = Uuid::new_v4().to_string();
    let second = mutate(router, &owner, "POST", "/api/v1/workspaces", json!({
        "organization_id":organizations[0]["id"],"name":"Invited access","team_name":"Operations","team_key":"OPS"
    }), ("idempotency-key", &key)).await;
    second.status(StatusCode::CREATED);
    second.unchanged_cookie();
    let second_workspace = second.json()["id"].as_str().unwrap().to_owned();

    let token = invite(router, &owner, &workspace, "viewer").await;
    let acceptance = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({
            "token":token,"display_name":"Elevation member","password":PASSWORD
        })),
        &[("origin", ORIGIN)],
    )
    .await;
    acceptance.status(StatusCode::OK);
    let original = Client::from(&acceptance);
    let original_other = login(router, MEMBER_EMAIL).await;
    let path = format!("/api/v1/workspaces/{workspace}/members/{}", original.id);
    let before = read(router, &owner, &path).await;

    // Promotion revokes every target cookie, while the caller retains its session
    // and receives the successful entity with its authoritative next version.
    let promoted = mutate(
        router,
        &owner,
        "PATCH",
        &path,
        json!({"role":"member"}),
        ("if-match", before.header("etag")),
    )
    .await;
    promoted.status(StatusCode::OK);
    promoted.unchanged_cookie();
    assert_eq!(promoted.json()["role"], "member");
    assert_eq!(
        promoted.header("etag"),
        format!(
            "\"member:{workspace}:{}:{}\"",
            original.id,
            promoted.json()["version"].as_str().unwrap()
        )
    );
    cookie_status(router, &original, false).await;
    cookie_status(router, &original_other, false).await;
    cookie_status(router, &owner, true).await;
    let member = login(router, MEMBER_EMAIL).await;
    let member_other = login(router, MEMBER_EMAIL).await;

    let unchanged = mutate(
        router,
        &owner,
        "PATCH",
        &path,
        json!({"role":"member"}),
        ("if-match", promoted.header("etag")),
    )
    .await;
    unchanged.status(StatusCode::OK);
    unchanged.unchanged_cookie();
    let demoted = mutate(
        router,
        &owner,
        "PATCH",
        &path,
        json!({"role":"viewer"}),
        ("if-match", unchanged.header("etag")),
    )
    .await;
    demoted.status(StatusCode::OK);
    demoted.unchanged_cookie();
    cookie_status(router, &member, true).await;
    cookie_status(router, &member_other, true).await;
    let stale = mutate(
        router,
        &owner,
        "PATCH",
        &path,
        json!({"role":"admin"}),
        ("if-match", promoted.header("etag")),
    )
    .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let forbidden = mutate(
        router,
        &member,
        "PATCH",
        &path,
        json!({"role":"admin"}),
        ("if-match", demoted.header("etag")),
    )
    .await;
    forbidden.problem(StatusCode::FORBIDDEN, "forbidden");
    let after = read(router, &owner, &path).await;
    assert_eq!(after.json(), demoted.json());
    assert_eq!(after.header("etag"), demoted.header("etag"));
    cookie_status(router, &member, true).await;
    cookie_status(router, &member_other, true).await;
    cookie_status(router, &owner, true).await;

    // A self-promotion must return success rather than turning a committed write
    // into 401. Its entity and ETag survive the response cookie invalidation.
    let team_path = format!(
        "/api/v1/workspaces/{workspace}/teams/{team}/members/{}",
        owner.id
    );
    let own_membership = read(router, &owner, &team_path).await;
    let lower = mutate(
        router,
        &owner,
        "PUT",
        &team_path,
        json!({"role":"contributor"}),
        ("if-match", own_membership.header("etag")),
    )
    .await;
    lower.status(StatusCode::OK);
    lower.unchanged_cookie();
    let no_change = mutate(
        router,
        &owner,
        "PUT",
        &team_path,
        json!({"role":"contributor"}),
        ("if-match", lower.header("etag")),
    )
    .await;
    no_change.status(StatusCode::OK);
    no_change.unchanged_cookie();
    let owner_other = login(router, OWNER_EMAIL).await;
    let stale_tag = format!("\"team-member:{team}:{}:0\"", owner.id);
    let failed_self = mutate(
        router,
        &owner,
        "PUT",
        &team_path,
        json!({"role":"maintainer"}),
        ("if-match", &stale_tag),
    )
    .await;
    failed_self.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    cookie_status(router, &owner, true).await;
    cookie_status(router, &owner_other, true).await;
    let self_promoted = mutate(
        router,
        &owner,
        "PUT",
        &team_path,
        json!({"role":"maintainer"}),
        ("if-match", no_change.header("etag")),
    )
    .await;
    self_promoted.status(StatusCode::OK);
    self_promoted.invalidated_cookie();
    assert_eq!(self_promoted.json()["user"]["id"], owner.id);
    assert_eq!(self_promoted.json()["role"], "maintainer");
    assert_eq!(
        self_promoted.header("etag"),
        format!(
            "\"team-member:{team}:{}:{}\"",
            owner.id,
            self_promoted.json()["version"].as_str().unwrap()
        )
    );
    cookie_status(router, &owner, false).await;
    cookie_status(router, &owner_other, false).await;
    cookie_status(router, &member, true).await;
    owner = login(router, OWNER_EMAIL).await;
    let persisted = read(router, &owner, &team_path).await;
    assert_eq!(persisted.json(), self_promoted.json());
    assert_eq!(persisted.header("etag"), self_promoted.header("etag"));

    // Invitation acceptance returns a replacement cookie, never the self-change
    // invalidation header. Its existing authentication/absolute lifetime persist.
    let old_session = current_session(router, &member).await;
    let token = invite(router, &owner, &second_workspace, "admin").await;
    let wrong_account = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":token})),
        &owner.headers(),
    )
    .await;
    wrong_account.problem(StatusCode::FORBIDDEN, "forbidden");
    cookie_status(router, &member, true).await;
    cookie_status(router, &member_other, true).await;
    cookie_status(router, &owner, true).await;
    let accepted = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":token})),
        &member.headers(),
    )
    .await;
    accepted.status(StatusCode::OK);
    let rotated = Client::from(&accepted);
    assert_ne!(rotated.cookie, member.cookie);
    assert_ne!(rotated.csrf, member.csrf);
    assert_eq!(rotated.id, member.id);
    assert!(
        accepted.json()["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workspace| workspace["id"] == second_workspace && workspace["role"] == "admin")
    );
    cookie_status(router, &member, false).await;
    cookie_status(router, &member_other, false).await;
    cookie_status(router, &rotated, true).await;
    cookie_status(router, &owner, true).await;
    let new_session = current_session(router, &rotated).await;
    assert_ne!(old_session["id"], new_session["id"]);
    assert_eq!(old_session["auth_time"], new_session["auth_time"]);
    assert_eq!(old_session["expires_at"], new_session["expires_at"]);
    let list = read(router, &rotated, "/api/v1/auth/sessions").await.json();
    assert_eq!(list["data"].as_array().unwrap().len(), 1);
}
