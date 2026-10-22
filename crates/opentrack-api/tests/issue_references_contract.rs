//! Actual Axum routes and current-access rich-reference responses on PostgreSQL.
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
    fn location(&self) -> &str {
        self.headers["location"].to_str().unwrap()
    }
    #[track_caller]
    fn problem(&self, status: StatusCode, code: &str) {
        self.expect(status);
        assert_eq!(self.body["code"], code);
        assert_eq!(self.headers["content-type"], "application/problem+json");
        assert!(!self.headers.contains_key("etag"));
    }
}
async fn request(
    r: &Router,
    c: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(c) = c {
        builder = builder
            .header("cookie", &c.cookie)
            .header("x-csrf-token", &c.csrf);
    }
    for (key, value) in extra {
        builder = builder.header(*key, *value);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = r
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
fn body(ids: &[&str]) -> Value {
    json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":ids.iter().map(|id|json!({"type":"issueReference","attrs":{"id":id}})).collect::<Vec<_>>()}]}})
}
const PASSWORD: &str = "Issue references HTTP verification 2026!";

#[tokio::test]
async fn issue_reference_candidates_and_current_authority_transport() {
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
    let setup=request(r,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@references.invalid","display_name":"Owner","password":PASSWORD,"workspace_name":"References","team_name":"Engineering","team_key":"ENG"})),&[]).await;
    setup.expect(StatusCode::CREATED);
    let owner = client(&setup);
    let base = format!(
        "/api/v1/workspaces/{}",
        setup.body["workspaces"][0]["id"].as_str().unwrap()
    );
    let boot = request(
        r,
        Some(&owner),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    boot.expect(StatusCode::OK);
    let team = boot.body["teams"][0]["id"].as_str().unwrap();
    let invite = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":"member@references.invalid","role":"member"})),
        &[("idempotency-key", "references-invite")],
    )
    .await;
    invite.expect(StatusCode::CREATED);
    let accepted = request(
        r,
        None,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":invite.body["token"],"display_name":"Member","password":PASSWORD})),
        &[],
    )
    .await;
    accepted.expect(StatusCode::OK);
    let member = client(&accepted);
    let private = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private references","key":"PRI","private":true})),
        &[("idempotency-key", "reference-private-team")],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_team = private.body["id"].as_str().unwrap();
    let target = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":team,"title":"Public target"})),
        &[("idempotency-key", "reference-public-target")],
    )
    .await;
    target.expect(StatusCode::CREATED);
    let target_id = target.body["id"].as_str().unwrap();
    let hidden = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":private_team,"title":"Private target"})),
        &[("idempotency-key", "reference-private-target")],
    )
    .await;
    hidden.expect(StatusCode::CREATED);
    let hidden_id = hidden.body["id"].as_str().unwrap();
    let source = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":team,"title":"Source"})),
        &[("idempotency-key", "reference-public-source")],
    )
    .await;
    source.expect(StatusCode::CREATED);
    let source_id = source.body["id"].as_str().unwrap();
    let picker = format!("{base}/issue-reference-candidates?issue_id={source_id}&q=target");
    request(r, None, "GET", &picker, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let choices = request(r, Some(&member), "GET", &picker, None, &[]).await;
    choices.expect(StatusCode::OK);
    assert_eq!(
        choices.body["data"],
        json!([{"id":target_id,"key":target.body["key"],"title":"Public target"}])
    );
    for query in [
        "".to_owned(),
        format!("?team_id={team}&issue_id={source_id}"),
        format!("?team_id={team}&limit=21"),
    ] {
        request(
            r,
            Some(&owner),
            "GET",
            &format!("{base}/issue-reference-candidates{query}"),
            None,
            &[],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    request(
        r,
        Some(&member),
        "GET",
        &format!("{base}/issue-reference-candidates?team_id={private_team}"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let initial = request(
        r,
        Some(&owner),
        "GET",
        &format!("{}/document", source.location()),
        None,
        &[],
    )
    .await;
    initial.expect(StatusCode::OK);
    assert_eq!(initial.body["issue_references"], json!([]));
    let doc_url = format!("{base}/documents/{}", initial.body["id"].as_str().unwrap());
    request(
        r,
        Some(&owner),
        "PATCH",
        &doc_url,
        Some(body(&[target_id])),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(
        r,
        Some(&member),
        "PATCH",
        &doc_url,
        Some(body(&[hidden_id])),
        &[("if-match", initial.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let content = body(&[target_id, hidden_id]);
    let saved = request(
        r,
        Some(&owner),
        "PATCH",
        &doc_url,
        Some(content.clone()),
        &[("if-match", initial.etag())],
    )
    .await;
    saved.expect(StatusCode::OK);
    assert_eq!(saved.body["body"], content["body"]);
    assert_eq!(saved.body["plain_text"], "[issue][issue]");
    let read = request(r, Some(&member), "GET", &doc_url, None, &[]).await;
    read.expect(StatusCode::OK);
    assert_ne!(read.etag(), saved.etag());
    let redacted = read.body["issue_references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == hidden_id)
        .unwrap();
    assert_eq!(
        *redacted,
        json!({"id":hidden_id,"available":false,"key":null,"title":null})
    );
    let mut forged = content.clone();
    forged["body"]["content"][0]["content"][0]["attrs"]["title"] = json!("Forged");
    request(
        r,
        Some(&owner),
        "PATCH",
        &doc_url,
        Some(forged),
        &[("if-match", saved.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let revision_url = format!(
        "{doc_url}/revisions/{}",
        saved.body["version"].as_str().unwrap()
    );
    let revision = request(r, Some(&member), "GET", &revision_url, None, &[]).await;
    revision.expect(StatusCode::OK);
    assert_eq!(
        revision.body["issue_references"],
        read.body["issue_references"]
    );
    let comment = request(
        r,
        Some(&member),
        "POST",
        &format!("{}/comments", source.location()),
        Some(body(&[target_id])),
        &[("idempotency-key", "reference-only-comment")],
    )
    .await;
    comment.expect(StatusCode::CREATED);
    assert_eq!(
        comment.body["document"]["issue_references"][0]["title"],
        "Public target"
    );
    let renamed = request(
        r,
        Some(&owner),
        "PATCH",
        target.location(),
        Some(json!({"title":"Renamed target"})),
        &[("if-match", target.etag())],
    )
    .await;
    renamed.expect(StatusCode::OK);
    let refreshed = request(r, Some(&member), "GET", &doc_url, None, &[]).await;
    refreshed.expect(StatusCode::OK);
    assert_ne!(read.etag(), refreshed.etag());
    assert_eq!(refreshed.body["version"], read.body["version"]);
    let latest_revision = request(r, Some(&member), "GET", &revision_url, None, &[]).await;
    latest_revision.expect(StatusCode::OK);
    assert_ne!(latest_revision.etag(), revision.etag());
    assert_eq!(latest_revision.body["body"], revision.body["body"]);
    let replay = request(
        r,
        Some(&member),
        "POST",
        &format!("{}/comments", source.location()),
        Some(body(&[target_id])),
        &[("idempotency-key", "reference-only-comment")],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body["id"], comment.body["id"]);
    assert_ne!(replay.etag(), comment.etag());
    assert_eq!(
        replay.body["document"]["issue_references"][0]["title"],
        "Renamed target"
    );
    request(
        r,
        Some(&member),
        "PATCH",
        &doc_url,
        Some(content),
        &[("if-match", read.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
}
