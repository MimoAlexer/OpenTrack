//! Project defaults and issue creation through HTTP and real PostgreSQL.
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
        assert_eq!(self.status, status);
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

#[tokio::test]
async fn template_project_default_and_atomic_create_follow_current_authority() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let r = &h.router;
    let setup = request(
        r,
        None,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "setup_token":h.setup_token,"email":"owner@example.test","display_name":"Example Owner",
            "password":"HTTP contract example password 2026!","workspace_name":"Example workspace",
            "team_name":"Example team","team_key":"EXM"
        })),
        &[],
    )
    .await;
    setup.expect(StatusCode::CREATED);
    let owner = client(&setup);
    let base = format!(
        "/api/v1/workspaces/{}",
        setup.body["workspaces"][0]["id"].as_str().unwrap()
    );
    let bootstrap = request(
        r,
        Some(&owner),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    bootstrap.expect(StatusCode::OK);
    let public_team = bootstrap.body["teams"][0]["id"].as_str().unwrap();

    let invite = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":"member@example.test","role":"member","team_ids":[]})),
        &[("idempotency-key", "http-project-invite")],
    )
    .await;
    invite.expect(StatusCode::CREATED);
    let accepted = request(r, None, "POST", "/api/v1/invitations/accept", Some(json!({
        "token":invite.body["token"],"display_name":"Example Member","password":"HTTP contract member password 2026!"
    })), &[]).await;
    accepted.expect(StatusCode::OK);
    let member_id = accepted.body["user"]["id"].as_str().unwrap();

    let team = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private example","key":"PRV","private":true})),
        &[("idempotency-key", "http-project-private-team")],
    )
    .await;
    team.expect(StatusCode::CREATED);
    let team_id = team.body["id"].as_str().unwrap();
    let team_path = format!("{base}/teams/{team_id}");
    let grant_path = format!("{team_path}/members/{member_id}");
    let grant = request(
        r,
        Some(&owner),
        "PUT",
        &grant_path,
        Some(json!({"role":"contributor"})),
        &[("if-none-match", "*")],
    )
    .await;
    grant.expect(StatusCode::CREATED);
    let login = request(
        r,
        None,
        "POST",
        "/api/v1/auth/login",
        Some(json!({
            "email":"member@example.test","password":"HTTP contract member password 2026!"
        })),
        &[],
    )
    .await;
    login.expect(StatusCode::OK);
    let member = client(&login);

    let made = request(
        r,
        Some(&owner),
        "POST",
        &format!("{team_path}/projects"),
        Some(json!({"name":"Private project example"})),
        &[("idempotency-key", "http-project-default")],
    )
    .await;
    made.expect(StatusCode::CREATED);
    let project_id = made.body["id"].as_str().unwrap();
    let project_url = made.headers["location"].to_str().unwrap();
    let definition = json!({"name":"Example template","title":"Example issue",
        "defaults":{"status_id":null,"priority":2,"label_ids":[],"project_id":project_id},
        "document":{"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph"}]}}});
    let template = request(
        r,
        Some(&owner),
        "POST",
        &format!("{team_path}/templates"),
        Some(definition.clone()),
        &[("idempotency-key", "http-project-template")],
    )
    .await;
    template.expect(StatusCode::CREATED);
    assert_eq!(template.body["defaults"]["project_id"], project_id);
    let preview_path = format!("{}/preview", template.headers["location"].to_str().unwrap());
    let preview = request(r, Some(&member), "GET", &preview_path, None, &[]).await;
    preview.expect(StatusCode::OK);
    assert!(!preview.headers.contains_key("etag"));
    assert_eq!(preview.body["resolved_project"]["id"], project_id);
    assert_eq!(preview.body["can_create"], true);
    let project_etag = preview.body["resolved_project"]["etag"].as_str().unwrap();
    let body = json!({"team_id":team_id,"title":"Example issue",
        "template":{"id":template.body["id"],"version":template.body["version"]},
        "status_id":preview.body["resolved_status"]["id"],
        "document":{"schema_version":1,"body":template.body["document"]["body"]},
        "project_id":project_id,"project_etag":project_etag});
    let issue_url = format!("{base}/issues");
    let key = "http-project-issue";
    let created = request(
        r,
        Some(&member),
        "POST",
        &issue_url,
        Some(body.clone()),
        &[("idempotency-key", key)],
    )
    .await;
    created.expect(StatusCode::CREATED);
    let issue_id = created.body["id"].as_str().unwrap();
    let scoped = request(
        r,
        Some(&member),
        "GET",
        &format!("{project_url}/issues"),
        None,
        &[],
    )
    .await;
    scoped.expect(StatusCode::OK);
    assert_eq!(scoped.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(scoped.body["data"][0]["id"], issue_id);
    let metadata = request(
        r,
        Some(&owner),
        "GET",
        &format!("{project_url}/metadata"),
        None,
        &[],
    )
    .await;
    metadata.expect(StatusCode::OK);
    assert_ne!(metadata.etag(), project_etag);
    let replay = request(
        r,
        Some(&member),
        "POST",
        &issue_url,
        Some(body.clone()),
        &[("idempotency-key", key)],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body["id"], issue_id);
    let after = request(
        r,
        Some(&owner),
        "GET",
        &format!("{project_url}/metadata"),
        None,
        &[],
    )
    .await;
    assert_eq!(after.etag(), metadata.etag());
    let after_scope = request(
        r,
        Some(&owner),
        "GET",
        &format!("{project_url}/issues"),
        None,
        &[],
    )
    .await;
    assert_eq!(after_scope.body["data"].as_array().unwrap().len(), 1);
    let mut changed = body.clone();
    changed["title"] = json!("Different issue");
    request(
        r,
        Some(&member),
        "POST",
        &issue_url,
        Some(changed),
        &[("idempotency-key", key)],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");

    for (index, fields) in [
        json!({"project_id":project_id}),
        json!({"project_etag":project_etag}),
        json!({"project_id":null,"project_etag":project_etag}),
        json!({"project_id":project_id,"project_etag":null}),
        json!({"project_id":project_id,"project_etag":""}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut invalid = json!({"team_id":team_id,"title":"Invalid pair"});
        invalid
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        request(
            r,
            Some(&owner),
            "POST",
            &issue_url,
            Some(invalid),
            &[("idempotency-key", &format!("http-invalid-pair-{index}"))],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    let stale = json!({"team_id":team_id,"title":"Stale project","project_id":project_id,"project_etag":project_etag});
    request(
        r,
        Some(&owner),
        "POST",
        &issue_url,
        Some(stale),
        &[("idempotency-key", "http-project-stale")],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let wrong_team = json!({"team_id":public_team,"title":"Wrong team","project_id":project_id,"project_etag":metadata.etag()});
    let denied = request(
        r,
        Some(&owner),
        "POST",
        &issue_url,
        Some(wrong_team),
        &[("idempotency-key", "http-project-wrong-team")],
    )
    .await;
    denied.problem(StatusCode::NOT_FOUND, "not_found");
    assert!(!denied.body.to_string().contains(project_id));
    assert!(!denied.body.to_string().contains("Private project example"));
    let invalid_default = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/teams/{public_team}/templates"),
        Some(definition),
        &[("idempotency-key", "http-project-wrong-default")],
    )
    .await;
    invalid_default.problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert!(!invalid_default.body.to_string().contains(project_id));

    let archived = request(
        r,
        Some(&owner),
        "PATCH",
        &format!("{project_url}/metadata"),
        Some(json!({"archived":true})),
        &[("if-match", metadata.etag())],
    )
    .await;
    archived.expect(StatusCode::OK);
    let preview_archived = request(r, Some(&member), "GET", &preview_path, None, &[]).await;
    preview_archived.expect(StatusCode::OK);
    assert_eq!(preview_archived.body["resolved_project"]["id"], project_id);
    assert_eq!(preview_archived.body["can_create"], false);
    assert!(
        preview_archived.body["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["field"] == "project_id" && d["code"] == "archived")
    );
    let archived_issue = json!({"team_id":team_id,"title":"Archived project","project_id":project_id,"project_etag":archived.etag()});
    request(
        r,
        Some(&member),
        "POST",
        &issue_url,
        Some(archived_issue),
        &[("idempotency-key", "http-project-archived")],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");

    request(
        r,
        Some(&owner),
        "DELETE",
        &grant_path,
        None,
        &[("if-match", grant.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    for denied in [
        request(r, Some(&member), "GET", &preview_path, None, &[]).await,
        request(
            r,
            Some(&member),
            "POST",
            &issue_url,
            Some(body),
            &[("idempotency-key", key)],
        )
        .await,
    ] {
        denied.problem(StatusCode::NOT_FOUND, "not_found");
        assert!(!denied.body.to_string().contains(project_id));
        assert!(!denied.body.to_string().contains("Private project example"));
    }
    let owner_scope = request(
        r,
        Some(&owner),
        "GET",
        &format!("{project_url}/issues"),
        None,
        &[],
    )
    .await;
    owner_scope.expect(StatusCode::OK);
    assert_eq!(owner_scope.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(owner_scope.body["data"][0]["id"], issue_id);
}
