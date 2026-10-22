//! Soft deletion and explicit thirty-day recovery across the HTTP boundary.
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

const ORIGIN: &str = "https://deletion.contract.invalid";
const PASSWORD: &str = "Deletion contract verification password 2026!";

fn cursor_query(cursor: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", cursor)
        .finish()
}

#[derive(Clone)]
struct Client {
    user: Uuid,
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
    fn client(&self) -> Client {
        Client {
            user: Uuid::parse_str(self.body["user"]["id"].as_str().unwrap()).unwrap(),
            cookie: self.headers["set-cookie"]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .into(),
            csrf: self.body["csrf_token"].as_str().unwrap().into(),
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
struct Fixture {
    router: Router,
    owner: Client,
    workspace: Uuid,
    base: String,
    team: String,
    status: String,
}
impl Fixture {
    async fn new(h: &Harness) -> Self {
        let router = router(ApiState {
            attachments: None,
            app: h.app.clone(),
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(PathBuf::from("/unused-deletion-contract-static")),
        });
        let setup = request(
            &router,
            None,
            "POST",
            "/api/v1/setup",
            Some(json!({
                "setup_token":h.setup_token,"email":"owner@deletion.invalid","password":PASSWORD,
                "display_name":"Deletion owner","workspace_name":"Deletion contracts",
                "team_name":"Engineering","team_key":"ENG"
            })),
            &[],
        )
        .await;
        setup.expect(StatusCode::CREATED);
        let owner = setup.client();
        let workspace =
            Uuid::parse_str(setup.body["workspaces"][0]["id"].as_str().unwrap()).unwrap();
        let base = format!("/api/v1/workspaces/{workspace}");
        let bootstrap = request(
            &router,
            Some(&owner),
            "GET",
            &format!("{base}/bootstrap"),
            None,
            &[],
        )
        .await;
        bootstrap.expect(StatusCode::OK);
        let team = bootstrap.body["teams"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let status = bootstrap.body["statuses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["team_id"] == team && v["is_default"] == true)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            router,
            owner,
            workspace,
            base,
            team,
            status,
        }
    }
    fn issue_path(&self, issue: &Reply) -> String {
        format!(
            "{}/issues/{}",
            self.base,
            issue.body["id"].as_str().unwrap()
        )
    }
    fn repair(&self) -> Value {
        json!({"status_id":self.status,"assignee_id":null,"label_ids":[]})
    }
    async fn get(&self, client: &Client, path: &str) -> Reply {
        let response = request(&self.router, Some(client), "GET", path, None, &[]).await;
        response.expect(StatusCode::OK);
        response
    }
    async fn create(&self, client: &Client, title: &str) -> Reply {
        self.create_in(client, &self.team, title).await
    }
    async fn create_in(&self, client: &Client, team: &str, title: &str) -> Reply {
        let response = request(
            &self.router,
            Some(client),
            "POST",
            &format!("{}/issues", self.base),
            Some(json!({"team_id":team,"title":title})),
            &[("idempotency-key", title)],
        )
        .await;
        response.expect(StatusCode::CREATED);
        assert_eq!(response.body["etag"], response.etag());
        response
    }
    async fn invite(&self, name: &str, role: &str) -> Client {
        let invitation = request(
            &self.router,
            Some(&self.owner),
            "POST",
            &format!("{}/invitations", self.base),
            Some(json!({"email":format!("{name}@deletion.invalid"),"role":role,"team_ids":[]})),
            &[("idempotency-key", &format!("deletion-invite-{name}"))],
        )
        .await;
        invitation.expect(StatusCode::CREATED);
        let accepted = request(
            &self.router,
            None,
            "POST",
            "/api/v1/invitations/accept",
            Some(json!({"token":invitation.body["token"],"display_name":name,"password":PASSWORD})),
            &[],
        )
        .await;
        accepted.expect(StatusCode::OK);
        accepted.client()
    }
    async fn login(&self, name: &str) -> Client {
        let response = request(
            &self.router,
            None,
            "POST",
            "/api/v1/auth/login",
            Some(json!({"email":format!("{name}@deletion.invalid"),"password":PASSWORD})),
            &[],
        )
        .await;
        response.expect(StatusCode::OK);
        response.client()
    }
    async fn delete(&self, client: &Client, path: &str, etag: &str) {
        let response = request(
            &self.router,
            Some(client),
            "DELETE",
            path,
            None,
            &[("if-match", etag)],
        )
        .await;
        response.expect(StatusCode::NO_CONTENT);
        assert!(response.body.is_null());
        assert!(!response.headers.contains_key("etag"));
        assert!(!response.headers.contains_key("location"));
    }
    async fn patch(&self, client: &Client, path: &str, etag: &str, body: Value) -> Reply {
        let response = request(
            &self.router,
            Some(client),
            "PATCH",
            path,
            Some(body),
            &[("if-match", etag)],
        )
        .await;
        response.expect(StatusCode::OK);
        response
    }
}

#[tokio::test]
async fn deletion_http_authority_graph_and_trash_contract() {
    let h = Harness::new().await;
    let test = h.clone();
    let outcome = tokio::spawn(async move {
        let f = Fixture::new(&test).await;
        authority_and_graph(&f).await;
        trash_visibility(&f).await;
    })
    .await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn authority_and_graph(f: &Fixture) {
    let author = f.invite("author", "member").await;
    let other = f.invite("other", "member").await;
    let viewer = f.invite("viewer", "viewer").await;
    let admin = f.invite("admin", "admin").await;
    let parent = f.create(&f.owner, "Retained parent").await;
    let source = f.create(&author, "Delete graph source").await;
    let source_path = f.issue_path(&source);
    let preview_path = format!("{source_path}/deletion");
    request(&f.router, None, "GET", &preview_path, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    for (client, expected) in [
        (&author, true),
        (&other, false),
        (&viewer, false),
        (&admin, true),
        (&f.owner, true),
    ] {
        let preview = f.get(client, &preview_path).await;
        assert_eq!(preview.body["can_delete"], expected);
        assert_eq!(preview.body["issue"]["etag"], preview.etag());
        assert_eq!(preview.etag(), source.etag());
    }
    for client in [&other, &viewer] {
        request(
            &f.router,
            Some(client),
            "DELETE",
            &source_path,
            None,
            &[("if-match", source.etag())],
        )
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
    }
    request(&f.router, Some(&author), "DELETE", &source_path, None, &[])
        .await
        .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let bad_csrf = Client {
        csrf: "incorrect".into(),
        ..author.clone()
    };
    request(
        &f.router,
        Some(&bad_csrf),
        "DELETE",
        &source_path,
        None,
        &[("if-match", source.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    f.patch(
        &author,
        &format!("{source_path}/parent"),
        source.etag(),
        json!({"parent_id":parent.body["id"],"parent_etag":parent.etag()}),
    )
    .await;
    let source_now = f.get(&author, &source_path).await;
    let child = request(&f.router, Some(&author), "POST", &format!("{}/issues",f.base),
        Some(json!({"team_id":f.team,"title":"Detached child","parent_id":source.body["id"],"parent_etag":source_now.etag()})),
        &[("idempotency-key","deletion-child")]).await;
    child.expect(StatusCode::CREATED);
    let related = f.create(&other, "Retained relation target").await;
    let source_now = f.get(&author, &source_path).await;
    let relation = request(
        &f.router,
        Some(&author),
        "POST",
        &format!("{source_path}/relations"),
        Some(json!({"kind":"related","target_id":related.body["id"],"target_etag":related.etag()})),
        &[
            ("if-match", source_now.etag()),
            ("idempotency-key", "deletion-relation"),
        ],
    )
    .await;
    relation.expect(StatusCode::CREATED);
    let preview = f.get(&author, &preview_path).await;
    assert_eq!(preview.body["child_count"], 1);
    assert_eq!(preview.body["relationship_count"], 1);
    assert_eq!(preview.body["has_parent"], true);
    let stale = request(
        &f.router,
        Some(&author),
        "DELETE",
        &source_path,
        None,
        &[("if-match", source.etag())],
    )
    .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale.body["details"]["current"], preview.body["issue"]);
    f.delete(&author, &source_path, preview.etag()).await;
    for path in [
        source_path.clone(),
        format!("{source_path}/activity"),
        preview_path,
        format!(
            "{}/issues/by-key/{}",
            f.base,
            source.body["key"].as_str().unwrap()
        ),
    ] {
        request(&f.router, Some(&author), "GET", &path, None, &[])
            .await
            .problem(StatusCode::NOT_FOUND, "not_found");
    }
    // Create receipts cannot expose their old active representation after deletion.
    request(
        &f.router,
        Some(&author),
        "POST",
        &format!("{}/issues", f.base),
        Some(json!({"team_id":f.team,"title":"Delete graph source"})),
        &[("idempotency-key", "Delete graph source")],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    assert!(
        f.get(&author, &format!("{}/parent", f.issue_path(&child)))
            .await
            .body["parent"]
            .is_null()
    );
    assert!(
        f.get(&author, &format!("{}/children", f.issue_path(&parent)))
            .await
            .body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.get(&other, &format!("{}/relations", f.issue_path(&related)))
            .await
            .body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for issue in [&parent, &child, &related] {
        assert_ne!(
            f.get(&author, &f.issue_path(issue)).await.etag(),
            issue.etag()
        );
    }
    let tombstone = f.get(&viewer, &format!("{source_path}/deleted")).await;
    assert_eq!(tombstone.body["can_restore"], false);
    assert_eq!(tombstone.body["issue"]["etag"], tombstone.etag());
    let start =
        chrono::DateTime::parse_from_rfc3339(tombstone.body["deleted_at"].as_str().unwrap())
            .unwrap();
    let end =
        chrono::DateTime::parse_from_rfc3339(tombstone.body["restore_until"].as_str().unwrap())
            .unwrap();
    assert_eq!(end - start, chrono::Duration::days(30));
    for client in [&other, &viewer] {
        request(
            &f.router,
            Some(client),
            "POST",
            &format!("{source_path}/restore"),
            Some(f.repair()),
            &[("if-match", tombstone.etag())],
        )
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
    }
    // A current maintainer can recover another author's issue. The promotion
    // invalidates their old cookie, so use a real fresh password login.
    request(
        &f.router,
        Some(&f.owner),
        "PUT",
        &format!("{}/teams/{}/members/{}", f.base, f.team, other.user),
        Some(json!({"role":"maintainer"})),
        &[("if-none-match", "*")],
    )
    .await
    .expect(StatusCode::CREATED);
    let maintainer = f.login("other").await;
    let tombstone = f.get(&maintainer, &format!("{source_path}/deleted")).await;
    assert_eq!(tombstone.body["can_restore"], true);
    let restored = request(
        &f.router,
        Some(&maintainer),
        "POST",
        &format!("{source_path}/restore"),
        Some(f.repair()),
        &[("if-match", tombstone.etag())],
    )
    .await;
    restored.expect(StatusCode::OK);
    assert_eq!(restored.body["id"], source.body["id"]);
    assert_eq!(restored.body["key"], source.body["key"]);
    assert_eq!(restored.body["etag"], restored.etag());
    assert!(f.get(&author, &format!("{source_path}/parent")).await.body["parent"].is_null());
    assert!(
        f.get(&author, &format!("{source_path}/children"))
            .await
            .body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.get(&author, &format!("{source_path}/relations"))
            .await
            .body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    request(
        &f.router,
        Some(&author),
        "GET",
        &format!("{source_path}/deleted"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    f.delete(&maintainer, &source_path, restored.etag()).await;
    let tombstone = f.get(&admin, &format!("{source_path}/deleted")).await;
    let restored = request(
        &f.router,
        Some(&admin),
        "POST",
        &format!("{source_path}/restore"),
        Some(f.repair()),
        &[("if-match", tombstone.etag())],
    )
    .await;
    restored.expect(StatusCode::OK);
    f.delete(&admin, &source_path, restored.etag()).await;
}

async fn trash_visibility(f: &Fixture) {
    let reader = f.login("viewer").await;
    let outsider_admin = f.login("admin").await;
    let private_team = request(
        &f.router,
        Some(&f.owner),
        "POST",
        &format!("{}/teams", f.base),
        Some(json!({"name":"Private work","key":"PVT","private":true})),
        &[("idempotency-key", "deletion-private")],
    )
    .await;
    private_team.expect(StatusCode::CREATED);
    let private_id = private_team.body["id"].as_str().unwrap();
    let secret = f
        .create_in(&f.owner, private_id, "Private deleted title")
        .await;
    let path = f.issue_path(&secret);
    for suffix in ["", "/deletion", "/deleted"] {
        request(
            &f.router,
            Some(&reader),
            "GET",
            &format!("{path}{suffix}"),
            None,
            &[],
        )
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    }
    request(
        &f.router,
        Some(&reader),
        "DELETE",
        &path,
        None,
        &[("if-match", secret.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    f.delete(&f.owner, &path, secret.etag()).await;
    let tombstone = f.get(&f.owner, &format!("{path}/deleted")).await;
    request(
        &f.router,
        Some(&reader),
        "GET",
        &format!("{path}/deleted"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    request(
        &f.router,
        Some(&reader),
        "POST",
        &format!("{path}/restore"),
        Some(json!({"status_id":secret.body["status_id"],"assignee_id":null,"label_ids":[]})),
        &[("if-match", tombstone.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    // Workspace administration does not grant access to a private team's trash.
    for suffix in ["", "/deletion", "/deleted"] {
        request(
            &f.router,
            Some(&outsider_admin),
            "GET",
            &format!("{path}{suffix}"),
            None,
            &[],
        )
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    }
    request(
        &f.router,
        Some(&outsider_admin),
        "POST",
        &format!("{path}/restore"),
        Some(json!({"status_id":secret.body["status_id"],"assignee_id":null,"label_ids":[]})),
        &[("if-match", tombstone.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    for title in ["Trash page first", "Trash page second", "Trash page third"] {
        let issue = f.create(&f.owner, title).await;
        f.delete(&f.owner, &f.issue_path(&issue), issue.etag())
            .await;
    }
    let list = format!("{}/issues/deleted", f.base);
    let visible = f.get(&reader, &list).await;
    assert!(!visible.body.to_string().contains("Private deleted title"));
    assert!(
        !visible
            .body
            .to_string()
            .contains(secret.body["id"].as_str().unwrap())
    );
    assert!(
        visible.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["can_restore"] == false)
    );
    request(
        &f.router,
        Some(&reader),
        "GET",
        &format!("{list}?team_id={private_id}"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let query = format!("{list}?team_id={}&q=Trash%20page&limit=1", f.team);
    let first = f.get(&reader, &query).await;
    assert_eq!(first.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(first.body["has_more"], true);
    let cursor = cursor_query(first.body["next_cursor"].as_str().unwrap());
    let next_path = format!("{query}&{cursor}");
    let second = f.get(&reader, &next_path).await;
    assert_ne!(
        first.body["data"][0]["issue"]["id"],
        second.body["data"][0]["issue"]["id"]
    );
    let third = f
        .get(
            &reader,
            &format!(
                "{query}&{}",
                cursor_query(second.body["next_cursor"].as_str().unwrap())
            ),
        )
        .await;
    assert_eq!(third.body["has_more"], false);
    assert!(third.body["next_cursor"].is_null());
    request(&f.router, Some(&f.owner), "GET", &next_path, None, &[])
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    request(
        &f.router,
        Some(&reader),
        "GET",
        &format!("{list}?limit=1&q=different&{cursor}"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let ordinary = f
        .get(&reader, &format!("{}/issues?q=Trash%20page", f.base))
        .await;
    assert!(ordinary.body["data"].as_array().unwrap().is_empty());
    let archived = f
        .get(
            &reader,
            &format!("{}/issues?archived=true&q=Trash%20page", f.base),
        )
        .await;
    assert!(archived.body["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn deletion_http_explicit_repairs_deadline_and_current_authority() {
    let h = Harness::new().await;
    let test = h.clone();
    let outcome = tokio::spawn(async move { repair_and_deadline(&test).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn repair_and_deadline(h: &Harness) {
    let f = Fixture::new(h).await;
    let author = f.invite("repair-author", "member").await;
    let issue = f.create(&author, "Repair explicitly").await;
    let path = f.issue_path(&issue);
    let label = request(
        &f.router,
        Some(&f.owner),
        "POST",
        &format!("{}/labels", f.base),
        Some(json!({"name":"Retained historical label","color":"blue"})),
        &[("idempotency-key", "repair-label")],
    )
    .await;
    label.expect(StatusCode::CREATED);
    let assigned = f
        .patch(
            &author,
            &path,
            issue.etag(),
            json!({"assignee_id":author.user,"label_ids":[label.body["id"]]}),
        )
        .await;
    f.delete(&author, &path, assigned.etag()).await;
    let original_tombstone = f.get(&author, &format!("{path}/deleted")).await;
    let restore_path = format!("{path}/restore");
    request(
        &f.router,
        Some(&author),
        "POST",
        &restore_path,
        Some(f.repair()),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    for body in [
        json!({}),
        json!({"status_id":f.status,"label_ids":[]}),
        json!({"status_id":f.status,"assignee_id":null}),
        json!({"status_id":f.status,"assignee_id":null,"label_ids":[],"title":"Unexpected edit"}),
    ] {
        request(
            &f.router,
            Some(&author),
            "POST",
            &restore_path,
            Some(body),
            &[("if-match", original_tombstone.etag())],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    // Losing contribution authority must override historical authorship.
    let member_path = format!("{}/members/{}", f.base, author.user);
    let membership = f.get(&f.owner, &member_path).await;
    f.patch(
        &f.owner,
        &member_path,
        membership.etag(),
        json!({"role":"viewer"}),
    )
    .await;
    f.patch(
        &f.owner,
        &format!("{}/labels/{}", f.base, label.body["id"].as_str().unwrap()),
        label.etag(),
        json!({"archived":true}),
    )
    .await;
    let historical = f.get(&author, &format!("{path}/deleted")).await;
    assert_eq!(historical.body["can_restore"], false);
    assert_eq!(historical.body["issue"]["assignee"]["assignable"], false);
    assert!(!historical.body["issue"]["labels"][0]["archived_at"].is_null());
    assert_ne!(historical.etag(), original_tombstone.etag());
    request(
        &f.router,
        Some(&author),
        "POST",
        &restore_path,
        Some(f.repair()),
        &[("if-match", historical.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let stale = request(
        &f.router,
        Some(&f.owner),
        "POST",
        &restore_path,
        Some(f.repair()),
        &[("if-match", original_tombstone.etag())],
    )
    .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale.body["details"]["current"], historical.body["issue"]);
    request(
        &f.router,
        Some(&f.owner),
        "POST",
        &restore_path,
        Some(
            json!({"status_id":f.status,"assignee_id":author.user,"label_ids":[label.body["id"]]}),
        ),
        &[("if-match", historical.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let repaired = request(
        &f.router,
        Some(&f.owner),
        "POST",
        &restore_path,
        Some(json!({"status_id":f.status,"assignee_id":null,"label_ids":[label.body["id"]]})),
        &[("if-match", historical.etag())],
    )
    .await;
    repaired.expect(StatusCode::OK);
    assert!(repaired.body["assignee_id"].is_null());
    assert_eq!(repaired.body["label_ids"], json!([label.body["id"]]));
    assert!(!repaired.body["labels"][0]["archived_at"].is_null());
    assert_eq!(repaired.body["etag"], repaired.etag());
    // Archive is a separate read-only state; it is never an implicit deletion.
    let archived = f
        .patch(&f.owner, &path, repaired.etag(), json!({"archived":true}))
        .await;
    assert_eq!(
        f.get(&f.owner, &format!("{path}/deletion")).await.body["can_delete"],
        false
    );
    request(
        &f.router,
        Some(&f.owner),
        "DELETE",
        &path,
        None,
        &[("if-match", archived.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let live = f
        .patch(&f.owner, &path, archived.etag(), json!({"archived":false}))
        .await;
    f.delete(&f.owner, &path, live.etag()).await;
    // Tombstones remain readable after the deadline; restoration does not.
    let id = Uuid::parse_str(issue.body["id"].as_str().unwrap()).unwrap();
    let mut tx = h.context(f.owner.user, f.workspace).await;
    assert_eq!(sqlx::query("UPDATE issues SET deleted_at=clock_timestamp()-interval '31 days' WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NOT NULL")
        .bind(f.workspace).bind(id).execute(&mut *tx).await.unwrap().rows_affected(),1);
    tx.commit().await.unwrap();
    let expired = f.get(&f.owner, &format!("{path}/deleted")).await;
    assert_eq!(expired.body["can_restore"], false);
    assert!(
        chrono::DateTime::parse_from_rfc3339(expired.body["restore_until"].as_str().unwrap())
            .unwrap()
            < chrono::Utc::now()
    );
    let trash = f
        .get(
            &f.owner,
            &format!("{}/issues/deleted?q=Repair%20explicitly", f.base),
        )
        .await;
    assert_eq!(trash.body["data"][0]["issue"]["id"], issue.body["id"]);
    assert_eq!(trash.body["data"][0]["can_restore"], false);
    request(
        &f.router,
        Some(&f.owner),
        "POST",
        &restore_path,
        Some(f.repair()),
        &[("if-match", expired.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    request(&f.router, Some(&f.owner), "GET", &path, None, &[])
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(
        f.get(&f.owner, &format!("{path}/deleted")).await.body,
        expired.body
    );
}
