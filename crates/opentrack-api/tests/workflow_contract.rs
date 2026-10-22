//! HTTP contracts use the real restricted PostgreSQL application, not mocked routes.
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use opentrack_api::{ApiState, router};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use support::Harness;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://workflow.contract.invalid";
const PASSWORD: &str = "Workflow HTTP verification passphrase 2026!";
struct Client {
    cookie: String,
    csrf: String,
    id: Uuid,
}
impl Client {
    fn from(reply: &Reply) -> Self {
        let body = reply.json();
        Self {
            cookie: reply.header("set-cookie").split(';').next().unwrap().into(),
            csrf: body["csrf_token"].as_str().unwrap().into(),
            id: id(&body["user"]["id"]),
        }
    }
}
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
    fn header(&self, key: &str) -> &str {
        self.headers[key].to_str().unwrap()
    }
    #[track_caller]
    fn expect(&self, expected: StatusCode) {
        assert_eq!(self.status, expected, "Unexpected HTTP status");
    }
    #[track_caller]
    fn problem(&self, status: StatusCode, code: &str) {
        self.expect(status);
        assert_eq!(self.json()["code"], code);
        assert_eq!(self.header("content-type"), "application/problem+json");
        assert!(!self.headers.contains_key("location"));
        assert!(!self.headers.contains_key("set-cookie"));
        assert!(!self.headers.contains_key("etag"));
    }
}
fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
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
    let body = body
        .map(|value| serde_json::to_vec(&value).unwrap())
        .unwrap_or_default();
    let reply = router
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = reply.status();
    let headers = reply.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
    let body = to_bytes(reply.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    assert!(!String::from_utf8_lossy(&body).contains(PASSWORD));
    Reply {
        status,
        headers,
        body,
    }
}
async fn workflow(router: &Router, client: &Client, base: &str) -> Reply {
    let reply = request(
        router,
        Some(client),
        "GET",
        &format!("{base}/workflow"),
        None,
        &[],
    )
    .await;
    reply.expect(StatusCode::OK);
    reply
}
async fn invite(router: &Router, owner: &Client, w: Uuid, email: &str, role: &str) -> Client {
    let reply = request(
        router,
        Some(owner),
        "POST",
        &format!("/api/v1/workspaces/{w}/invitations"),
        Some(json!({"email":email,"role":role,"team_ids":[]})),
        &[("idempotency-key", email)],
    )
    .await;
    reply.expect(StatusCode::CREATED);
    let accepted = request(
        router,
        None,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":reply.json()["token"],"display_name":email,"password":PASSWORD})),
        &[],
    )
    .await;
    accepted.expect(StatusCode::OK);
    Client::from(&accepted)
}
async fn login(router: &Router, email: &str) -> Client {
    let reply = request(
        router,
        None,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":email,"password":PASSWORD})),
        &[],
    )
    .await;
    reply.expect(StatusCode::OK);
    Client::from(&reply)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_workflow_versions_defaults_and_replacement_contract() {
    let h = Harness::new().await;
    let test = h.clone();
    let outcome = tokio::spawn(async move { exercise(&test).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let router = router(ApiState {
        attachments: None,
        app: h.app.clone(),
        origins: Arc::new(vec![ORIGIN.into()]),
        secure_cookie: true,
        static_dir: Arc::new(PathBuf::from("/unused-workflow-contract-static")),
    });
    let setup = request(
        &router,
        None,
        "POST",
        "/api/v1/setup",
        Some(json!({"setup_token":h.setup_token,
        "email":"owner@workflow.invalid","display_name":"Workflow owner","password":PASSWORD,
        "workspace_name":"Workflow contracts","team_name":"Engineering","team_key":"ENG"})),
        &[],
    )
    .await;
    setup.expect(StatusCode::CREATED);
    let owner = Client::from(&setup);
    let w = id(&setup.json()["workspaces"][0]["id"]);
    let boot = request(
        &router,
        Some(&owner),
        "GET",
        &format!("/api/v1/workspaces/{w}/bootstrap"),
        None,
        &[],
    )
    .await;
    boot.expect(StatusCode::OK);
    let t = id(&boot.json()["teams"][0]["id"]);
    let base = format!("/api/v1/workspaces/{w}/teams/{t}");
    let initial = workflow(&router, &owner, &base).await;
    assert_eq!(initial.json()["can_manage"], true);
    let initial_body = initial.json();
    let original_default = initial_body["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["is_default"] == true)
        .unwrap()["id"]
        .clone();
    assert_eq!(
        initial_body["statuses"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["is_default"] == true)
            .count(),
        1
    );
    let unchanged = request(
        &router,
        Some(&owner),
        "GET",
        &format!("{base}/workflow"),
        None,
        &[("if-none-match", initial.header("etag"))],
    )
    .await;
    unchanged.expect(StatusCode::NOT_MODIFIED);
    assert!(unchanged.body.is_empty());
    let weak = format!("W/{}", initial.header("etag"));
    request(
        &router,
        Some(&owner),
        "GET",
        &format!("{base}/workflow"),
        None,
        &[("if-none-match", &weak)],
    )
    .await
    .expect(StatusCode::NOT_MODIFIED);
    request(
        &router,
        None,
        "GET",
        &format!("{base}/workflow"),
        None,
        &[("if-none-match", "*")],
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");

    let wrong_csrf = Client {
        cookie: owner.cookie.clone(),
        csrf: "wrong-csrf".into(),
        id: owner.id,
    };
    request(
        &router,
        Some(&wrong_csrf),
        "PUT",
        &format!("{base}/workflow/default"),
        Some(json!({"status_id":original_default})),
        &[("if-match", initial.header("etag"))],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(workflow(&router, &owner, &base).await.json(), initial_body);
    let body = json!({"name":"Ready for review","category":"backlog","color":"purple"});
    request(
        &router,
        Some(&owner),
        "POST",
        &format!("{base}/statuses"),
        Some(body.clone()),
        &[("idempotency-key", "missing-version")],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(&router,Some(&owner),"POST",&format!("{base}/statuses"),Some(json!({"name":"Unsafe color","category":"backlog","color":"url(https://example.invalid)"})),&[("idempotency-key","invalid-color"),("if-match",initial.header("etag"))]).await.problem(StatusCode::UNPROCESSABLE_ENTITY,"validation_failed");
    let created = request(
        &router,
        Some(&owner),
        "POST",
        &format!("{base}/statuses"),
        Some(body.clone()),
        &[
            ("idempotency-key", "new-review"),
            ("if-match", initial.header("etag")),
        ],
    )
    .await;
    created.expect(StatusCode::CREATED);
    let status_path = created.header("location").to_owned();
    let new_status = request(&router, Some(&owner), "GET", &status_path, None, &[]).await;
    new_status.expect(StatusCode::OK);
    let source = id(&new_status.json()["id"]);
    let replay = request(
        &router,
        Some(&owner),
        "POST",
        &format!("{base}/statuses"),
        Some(body),
        &[
            ("idempotency-key", "new-review"),
            ("if-match", initial.header("etag")),
        ],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.json(), created.json());
    assert_eq!(replay.header("location"), status_path);
    request(
        &router,
        Some(&owner),
        "POST",
        &format!("{base}/statuses"),
        Some(json!({"name":"Different","category":"backlog","color":"purple"})),
        &[
            ("idempotency-key", "new-review"),
            ("if-match", initial.header("etag")),
        ],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let renamed = request(
        &router,
        Some(&owner),
        "PATCH",
        &status_path,
        Some(json!({"name":"Ready"})),
        &[("if-match", created.header("etag"))],
    )
    .await;
    renamed.expect(StatusCode::OK);
    let stale = request(
        &router,
        Some(&owner),
        "PATCH",
        &status_path,
        Some(json!({"name":"Keep my draft"})),
        &[("if-match", created.header("etag"))],
    )
    .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale.json()["details"]["current"], renamed.json());
    let default = request(
        &router,
        Some(&owner),
        "PUT",
        &format!("{base}/workflow/default"),
        Some(json!({"status_id":source})),
        &[("if-match", renamed.header("etag"))],
    )
    .await;
    default.expect(StatusCode::OK);
    // Place the chosen default after the other backlog status. Position must not
    // substitute for is_default when the server accepts an omitted status_id.
    let mut statuses = default.json()["statuses"].as_array().unwrap().clone();
    statuses.sort_by_key(|s| {
        (
            match s["category"].as_str().unwrap() {
                "triage" => 0,
                "backlog" => 1,
                "unstarted" => 2,
                "started" => 3,
                "completed" => 4,
                _ => 5,
            },
            s["id"] == json!(source),
        )
    });
    let order: Vec<Value> = statuses.iter().map(|s| s["id"].clone()).collect();
    let reordered = request(
        &router,
        Some(&owner),
        "PUT",
        &format!("{base}/workflow/order"),
        Some(json!({"status_ids":order})),
        &[("if-match", default.header("etag"))],
    )
    .await;
    reordered.expect(StatusCode::OK);
    let issue = request(
        &router,
        Some(&owner),
        "POST",
        &format!("/api/v1/workspaces/{w}/issues"),
        Some(json!({"team_id":t,"title":"Uses actual default"})),
        &[("idempotency-key", "workflow-issue")],
    )
    .await;
    issue.expect(StatusCode::CREATED);
    assert_eq!(issue.json()["status_id"], json!(source));
    let issue_path = issue.header("location").to_owned();
    let reset = request(
        &router,
        Some(&owner),
        "PUT",
        &format!("{base}/workflow/default"),
        Some(json!({"status_id":original_default})),
        &[("if-match", reordered.header("etag"))],
    )
    .await;
    reset.expect(StatusCode::OK);
    request(
        &router,
        Some(&owner),
        "POST",
        &format!("{status_path}/archive"),
        None,
        &[("if-match", reset.header("etag"))],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let preview = request(
        &router,
        Some(&owner),
        "GET",
        &format!(
            "{status_path}/replacement-preview?replacement_status_id={}",
            original_default.as_str().unwrap()
        ),
        None,
        &[],
    )
    .await;
    preview.expect(StatusCode::OK);
    assert_eq!(preview.json()["issue_count"], "1");
    let replacement_body = json!({"replacement_status_id":original_default});
    let accepted = request(
        &router,
        Some(&owner),
        "POST",
        &format!("{status_path}/replace"),
        Some(replacement_body.clone()),
        &[
            ("if-match", reset.header("etag")),
            ("idempotency-key", "replace-review"),
        ],
    )
    .await;
    accepted.expect(StatusCode::ACCEPTED);
    assert_eq!(accepted.json()["state"], "queued");
    assert_eq!(accepted.json()["matched_count"], "1");
    let job_path = accepted.header("location").to_owned();
    let job = id(&accepted.json()["id"]);
    let replay = request(
        &router,
        Some(&owner),
        "POST",
        &format!("{status_path}/replace"),
        Some(replacement_body),
        &[
            ("if-match", reset.header("etag")),
            ("idempotency-key", "replace-review"),
        ],
    )
    .await;
    replay.expect(StatusCode::ACCEPTED);
    assert_eq!(replay.json(), accepted.json());
    assert_eq!(replay.header("location"), job_path);
    let during = workflow(&router, &owner, &base).await;
    let during_body = during.json();
    let retiring = during_body["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == json!(source))
        .unwrap();
    assert_eq!(retiring["replacement_job_id"], json!(job));
    assert!(retiring["retiring_at"].is_string());
    request(
        &router,
        Some(&owner),
        "POST",
        &format!("/api/v1/workspaces/{w}/issues"),
        Some(json!({"team_id":t,"title":"Must not use retiring status","status_id":source})),
        &[("idempotency-key", "retiring-create")],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let edited = request(
        &router,
        Some(&owner),
        "PATCH",
        &issue_path,
        Some(json!({"title":"Unrelated edit during replacement"})),
        &[("if-match", issue.header("etag"))],
    )
    .await;
    edited.expect(StatusCode::OK);
    assert_eq!(edited.json()["status_id"], json!(source));
    request(
        &router,
        Some(&owner),
        "GET",
        &job_path,
        None,
        &[("if-none-match", accepted.header("etag"))],
    )
    .await
    .expect(StatusCode::NOT_MODIFIED);
    request(
        &router,
        Some(&owner),
        "POST",
        &format!("{job_path}/resume"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(
        &router,
        Some(&owner),
        "POST",
        &format!("{job_path}/resume"),
        None,
        &[("if-match", accepted.header("etag"))],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    directory_and_permissions(&router, &owner, w, &base).await;
}

async fn directory_and_permissions(router: &Router, owner: &Client, w: Uuid, base: &str) {
    let current = workflow(router, owner, base).await;
    let created = request(
        router,
        Some(owner),
        "POST",
        &format!("{base}/statuses"),
        Some(json!({"name":"Unused archive","category":"triage","color":"gray"})),
        &[
            ("if-match", current.header("etag")),
            ("idempotency-key", "unused-status"),
        ],
    )
    .await;
    created.expect(StatusCode::CREATED);
    let location = created.header("location").to_owned();
    let archived = request(
        router,
        Some(owner),
        "POST",
        &format!("{location}/archive"),
        None,
        &[("if-match", created.header("etag"))],
    )
    .await;
    archived.expect(StatusCode::OK);
    let history = request(
        router,
        Some(owner),
        "GET",
        &format!("{base}/statuses?state=archived&limit=1"),
        None,
        &[],
    )
    .await;
    history.expect(StatusCode::OK);
    assert_eq!(history.json()["data"].as_array().unwrap().len(), 1);
    let restored = request(
        router,
        Some(owner),
        "POST",
        &format!("{location}/restore"),
        None,
        &[("if-match", archived.header("etag"))],
    )
    .await;
    restored.expect(StatusCode::OK);
    let email = "member@workflow.invalid";
    let member = invite(router, owner, w, email, "member").await;
    let read = workflow(router, &member, base).await;
    assert_eq!(read.json()["can_manage"], false);
    request(
        router,
        Some(&member),
        "PATCH",
        &location,
        Some(json!({"name":"Forbidden rename"})),
        &[("if-match", read.header("etag"))],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let member_path = format!("{base}/members/{}", member.id);
    let promoted = request(
        router,
        Some(owner),
        "PUT",
        &member_path,
        Some(json!({"role":"maintainer"})),
        &[("if-none-match", "*")],
    )
    .await;
    promoted.expect(StatusCode::CREATED);
    let member = login(router, email).await;
    let manager = workflow(router, &member, base).await;
    assert_eq!(manager.json()["can_manage"], true);
    request(
        router,
        Some(owner),
        "PUT",
        &member_path,
        Some(json!({"role":"contributor"})),
        &[("if-match", promoted.header("etag"))],
    )
    .await
    .expect(StatusCode::OK);
    let changed = request(
        router,
        Some(&member),
        "GET",
        &format!("{base}/workflow"),
        None,
        &[("if-none-match", manager.header("etag"))],
    )
    .await;
    changed.expect(StatusCode::OK);
    assert_eq!(changed.json()["can_manage"], false);
    assert_eq!(changed.json()["version"], manager.json()["version"]);
    let administrator = invite(router, owner, w, "admin@workflow.invalid", "admin").await;
    let private = request(
        router,
        Some(owner),
        "POST",
        &format!("/api/v1/workspaces/{w}/teams"),
        Some(json!({"name":"Private work","key":"PVT","private":true})),
        &[("idempotency-key", "private-workflow")],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_id = id(&private.json()["id"]);
    let hidden = format!("/api/v1/workspaces/{w}/teams/{private_id}/workflow");
    request(
        router,
        Some(&administrator),
        "GET",
        &hidden,
        None,
        &[("if-none-match", "*")],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    request(
        router,
        Some(&administrator),
        "GET",
        &format!("/api/v1/workspaces/{w}/teams/{private_id}/statuses?state=all"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
}
