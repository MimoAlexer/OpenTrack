//! Real PostgreSQL transport checks for structured descriptions and discussion.
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

fn document(text: &str) -> Value {
    json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]}})
}

#[tokio::test]
async fn independent_documents_and_idempotent_discussion_over_http() {
    let h = Harness::new().await;
    let test = h.clone();
    let outcome = tokio::spawn(async move { exercise(&test).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let r = &h.router;
    let setup=request(r,None,"POST","/api/v1/setup",Some(json!({
        "setup_token":h.setup_token,"email":"owner@documents.invalid","display_name":"Document owner",
        "password":"Documents HTTP verification passphrase 2026!","workspace_name":"Documents HTTP",
        "team_name":"Engineering","team_key":"ENG"
    })),&[]).await;
    setup.expect(StatusCode::CREATED);
    let client = Client {
        cookie: setup.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: setup.body["csrf_token"].as_str().unwrap().into(),
    };
    let w = setup.body["workspaces"][0]["id"].as_str().unwrap();
    let base = format!("/api/v1/workspaces/{w}");
    let boot = request(
        r,
        Some(&client),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    boot.expect(StatusCode::OK);
    let team = boot.body["teams"][0]["id"].as_str().unwrap();
    let created=request(r,Some(&client),"POST",&format!("{base}/issues"),Some(json!({"team_id":team,"title":"Editor verification","document":document("Original body")})),&[("idempotency-key","document-http-issue")]).await;
    created.expect(StatusCode::CREATED);
    let issue = format!("{base}/issues/{}", created.body["id"].as_str().unwrap());
    let initial = request(
        r,
        Some(&client),
        "GET",
        &format!("{issue}/document"),
        None,
        &[],
    )
    .await;
    initial.expect(StatusCode::OK);
    assert_eq!(initial.body["plain_text"], "Original body");
    assert_eq!(initial.body["schema_supported"], true);
    assert_eq!(initial.body["etag"], initial.etag());
    let doc = format!("{base}/documents/{}", initial.body["id"].as_str().unwrap());
    request(r, None, "GET", &doc, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        r,
        Some(&client),
        "PATCH",
        &doc,
        Some(document("Blocked")),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let bad = Client {
        csrf: "bad".into(),
        ..client.clone()
    };
    request(
        r,
        Some(&bad),
        "PATCH",
        &doc,
        Some(document("Blocked")),
        &[("if-match", initial.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        r,
        Some(&client),
        "PATCH",
        &doc,
        Some(json!({"schema_version":2,"body":{"type":"doc"}})),
        &[("if-match", initial.etag())],
    )
    .await
    .problem(
        StatusCode::UNPROCESSABLE_ENTITY,
        "unsupported_document_schema",
    );
    let changed = request(
        r,
        Some(&client),
        "PATCH",
        &doc,
        Some(document("Saved body")),
        &[("if-match", initial.etag())],
    )
    .await;
    changed.expect(StatusCode::OK);
    assert_ne!(changed.etag(), initial.etag());
    let conflict = request(
        r,
        Some(&client),
        "PATCH",
        &doc,
        Some(document("Keep my draft")),
        &[("if-match", initial.etag())],
    )
    .await;
    conflict.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(
        conflict.body["details"]["current"]["plain_text"],
        "Saved body"
    );
    let latest = request(r, Some(&client), "GET", &issue, None, &[]).await;
    latest.expect(StatusCode::OK);
    assert_eq!(latest.body["version"], created.body["version"]);
    assert_eq!(latest.body["description"], "Saved body");
    assert_ne!(latest.etag(), created.etag());
    let revisions = request(
        r,
        Some(&client),
        "GET",
        &format!("{doc}/revisions?limit=1"),
        None,
        &[],
    )
    .await;
    revisions.expect(StatusCode::OK);
    assert_eq!(revisions.body["data"].as_array().unwrap().len(), 1);
    assert!(revisions.body["data"][0].get("body").is_none());
    let version = revisions.body["data"][0]["revision"].as_str().unwrap();
    request(
        r,
        Some(&client),
        "GET",
        &format!("{doc}/revisions/{version}"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::OK);
    let comments = format!("{issue}/comments");
    request(
        r,
        Some(&client),
        "POST",
        &comments,
        Some(document("First comment")),
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let posted = request(
        r,
        Some(&client),
        "POST",
        &comments,
        Some(document("First comment")),
        &[("idempotency-key", "document-http-comment")],
    )
    .await;
    posted.expect(StatusCode::CREATED);
    let comment = format!("{base}/comments/{}", posted.body["id"].as_str().unwrap());
    assert_eq!(posted.headers["location"], comment);
    let replay = request(
        r,
        Some(&client),
        "POST",
        &comments,
        Some(document("First comment")),
        &[("idempotency-key", "document-http-comment")],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body, posted.body);
    let edited = request(
        r,
        Some(&client),
        "PATCH",
        &comment,
        Some(document("Edited comment")),
        &[("if-match", posted.etag())],
    )
    .await;
    edited.expect(StatusCode::OK);
    assert!(!edited.body["edited_at"].is_null());
    request(
        r,
        Some(&client),
        "DELETE",
        &comment,
        None,
        &[("if-match", posted.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    request(
        r,
        Some(&client),
        "DELETE",
        &comment,
        None,
        &[("if-match", edited.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    let page = request(r, Some(&client), "GET", &comments, None, &[]).await;
    page.expect(StatusCode::OK);
    assert_eq!(page.body["data"].as_array().unwrap().len(), 1);
    assert!(!page.body["data"][0]["deleted_at"].is_null());
    assert!(page.body["data"][0]["document"].is_null());
    request(
        r,
        Some(&client),
        "GET",
        &format!("{issue}/activity-page?limit=1"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::OK);
    request(
        r,
        Some(&client),
        "GET",
        &format!("{comments}?limit=101"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}
