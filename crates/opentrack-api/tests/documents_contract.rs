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

    // A future writer's stored JSON must stay opaque to this server version.
    let private_team = request(
        r,
        Some(&client),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Future reference team","key":"FUT","private":true})),
        &[("idempotency-key", "future-reference-team")],
    )
    .await;
    private_team.expect(StatusCode::CREATED);
    let private_issue = request(
        r,
        Some(&client),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":private_team.body["id"],"title":"Private future reference control"})),
        &[("idempotency-key", "future-reference-issue")],
    )
    .await;
    private_issue.expect(StatusCode::CREATED);
    let invitation = request(
        r,
        Some(&client),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":"future-reader@documents.invalid","role":"viewer","team_ids":[]})),
        &[("idempotency-key", "future-reference-reader")],
    )
    .await;
    invitation.expect(StatusCode::CREATED);
    let accepted = request(
        r,
        None,
        "POST",
        "/api/v1/invitations/accept",
        Some(
            json!({"token":invitation.body["token"],"display_name":"Example Reader",
            "password":"Future schema HTTP verification passphrase 2026!"}),
        ),
        &[],
    )
    .await;
    accepted.expect(StatusCode::OK);
    let reader = Client {
        cookie: accepted.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: accepted.body["csrf_token"].as_str().unwrap().into(),
    };
    let opaque = json!({
        "type":"futureDoc",
        "attrs":{"unknown":[null,true,7,{"literal":"<script>not executable</script> & \\ \" café"}]},
        "content":[{"type":"futureWidget","attrs":{"future":true,"nested":{"retain":"exactly"}},
            "content":[{"type":"issueReference","attrs":{"id":private_issue.body["id"]}}]}],
    });
    let fallback = "Future-format plain text: <script>inert fallback</script> & café";
    let workspace = w.parse::<uuid::Uuid>().unwrap();
    let actor = setup.body["user"]["id"]
        .as_str()
        .unwrap()
        .parse::<uuid::Uuid>()
        .unwrap();
    let document_id = initial.body["id"]
        .as_str()
        .unwrap()
        .parse::<uuid::Uuid>()
        .unwrap();
    let mut tx = h.lock_team(workspace, team.parse().unwrap(), actor).await;
    let stored: String = sqlx::query_scalar("UPDATE documents SET schema_version=99,body=$3,plain_text=$4,version=version+1,updated_by_user_id=$5,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 RETURNING body::text")
        .bind(workspace).bind(document_id).bind(&opaque).bind(fallback).bind(actor)
        .fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();

    let future = request(r, Some(&client), "GET", &doc, None, &[]).await;
    future.expect(StatusCode::OK);
    assert_eq!(future.body["schema_version"], 99);
    assert_eq!(future.body["schema_supported"], false);
    assert_eq!(future.body["can_edit"], false);
    assert_eq!(future.body["body"], opaque);
    assert_eq!(future.body["plain_text"], fallback);
    assert_eq!(future.body["etag"], future.etag());
    assert_ne!(future.etag(), changed.etag());
    assert_eq!(future.body["issue_references"][0]["available"], true);
    assert_eq!(
        future.body["issue_references"][0]["title"],
        private_issue.body["title"]
    );
    let future_issue = request(r, Some(&client), "GET", &issue, None, &[]).await;
    future_issue.expect(StatusCode::OK);
    assert_eq!(future_issue.body["description"], fallback);
    let history_path = format!("{doc}/revisions");
    let history = request(r, Some(&client), "GET", &history_path, None, &[]).await;
    history.expect(StatusCode::OK);
    let future_revision_path = format!(
        "{doc}/revisions/{}",
        future.body["version"].as_str().unwrap()
    );
    let revision = request(r, Some(&client), "GET", &future_revision_path, None, &[]).await;
    revision.expect(StatusCode::OK);
    assert_eq!(revision.body["schema_version"], 99);
    assert_eq!(revision.body["body"], opaque);
    assert_eq!(revision.body["plain_text"], fallback);

    for path in [
        format!("{issue}/document"),
        doc.clone(),
        future_revision_path.clone(),
    ] {
        let visible = request(r, Some(&reader), "GET", &path, None, &[]).await;
        visible.expect(StatusCode::OK);
        assert_eq!(visible.body["schema_version"], 99);
        assert_eq!(visible.body["body"], opaque);
        assert_eq!(visible.body["plain_text"], fallback);
        assert_eq!(
            visible.body["issue_references"],
            json!([{
                "id":private_issue.body["id"],"available":false,"key":null,"title":null,
            }])
        );
        if path != future_revision_path {
            assert_eq!(visible.body["schema_supported"], false);
            assert_eq!(visible.body["can_edit"], false);
        }
    }
    request(
        r,
        Some(&reader),
        "GET",
        &format!(
            "{base}/issues/{}",
            private_issue.body["id"].as_str().unwrap()
        ),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");

    // Unsupported stored schemas reject edits before stale-version comparison.
    for etag in [changed.etag(), future.etag()] {
        request(
            r,
            Some(&client),
            "PATCH",
            &doc,
            Some(document("Cannot downgrade stored future JSON")),
            &[("if-match", etag)],
        )
        .await
        .problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_document_schema",
        );
    }
    request(
        r,
        Some(&client),
        "PATCH",
        &doc,
        Some(json!({"schema_version":99,"body":opaque})),
        &[("if-match", future.etag())],
    )
    .await
    .problem(
        StatusCode::UNPROCESSABLE_ENTITY,
        "unsupported_document_schema",
    );
    request(r, Some(&client), "PATCH", &issue,
        Some(json!({"title":"Must roll back with the rejected description","description":"Cannot flatten stored future JSON"})),
        &[("if-match", future_issue.etag())])
        .await.problem(StatusCode::UNPROCESSABLE_ENTITY, "unsupported_document_schema");
    assert_eq!(
        request(r, Some(&client), "GET", &doc, None, &[]).await.body,
        future.body
    );
    assert_eq!(
        request(r, Some(&client), "GET", &issue, None, &[])
            .await
            .body,
        future_issue.body
    );
    assert_eq!(
        request(r, Some(&client), "GET", &history_path, None, &[])
            .await
            .body,
        history.body
    );
    assert_eq!(
        request(r, Some(&client), "GET", &future_revision_path, None, &[])
            .await
            .body,
        revision.body
    );
    let mut tx = h.lock_team(workspace, team.parse().unwrap(), actor).await;
    let retained: String =
        sqlx::query_scalar("SELECT body::text FROM documents WHERE workspace_id=$1 AND id=$2")
            .bind(workspace)
            .bind(document_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(retained, stored);
    tx.commit().await.unwrap();
}
