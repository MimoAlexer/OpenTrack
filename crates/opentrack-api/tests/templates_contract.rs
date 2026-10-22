//! Template transport and compound preview invalidation against real PostgreSQL.
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
        body: serde_json::from_slice(&bytes).unwrap(),
    }
}

#[tokio::test]
async fn reusable_templates_transport_and_reference_preview() {
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
    let setup=request(r,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@templates.invalid","display_name":"Template owner","password":"Template HTTP verification passphrase 2026!","workspace_name":"Templates","team_name":"Engineering","team_key":"ENG"})),&[]).await;
    setup.expect(StatusCode::CREATED);
    let c = Client {
        cookie: setup.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: setup.body["csrf_token"].as_str().unwrap().into(),
    };
    let base = format!(
        "/api/v1/workspaces/{}",
        setup.body["workspaces"][0]["id"].as_str().unwrap()
    );
    let boot = request(r, Some(&c), "GET", &format!("{base}/bootstrap"), None, &[]).await;
    boot.expect(StatusCode::OK);
    let t = boot.body["teams"][0]["id"].as_str().unwrap();
    let team = format!("{base}/teams/{t}");
    let templates = format!("{team}/templates");
    let definition = json!({"name":"Release review","title":"Review release","defaults":{"status_id":null,"priority":2,"label_ids":[]},"document":{"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Verify before shipping"}]}]}}});
    request(r, None, "GET", &templates, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let bad = Client {
        csrf: "invalid".into(),
        ..c.clone()
    };
    request(
        r,
        Some(&bad),
        "POST",
        &templates,
        Some(definition.clone()),
        &[("idempotency-key", "template-invalid-csrf")],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        r,
        Some(&c),
        "POST",
        &templates,
        Some(definition.clone()),
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let mut invalid = definition.clone();
    invalid["document"]["body"]["content"] =
        json!([{"type":"attachment","attrs":{"id":uuid::Uuid::new_v4()}}]);
    request(
        r,
        Some(&c),
        "POST",
        &templates,
        Some(invalid),
        &[("idempotency-key", "template-attachment-invalid")],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let created = request(
        r,
        Some(&c),
        "POST",
        &templates,
        Some(definition.clone()),
        &[("idempotency-key", "template-http-create")],
    )
    .await;
    created.expect(StatusCode::CREATED);
    assert_eq!(created.body["title_mode"], "literal");
    assert_eq!(created.body["etag"], created.etag());
    let location = created.headers["location"].to_str().unwrap();
    assert_eq!(
        request(r, Some(&c), "GET", location, None, &[]).await.body,
        created.body
    );
    let list = request(r, Some(&c), "GET", &templates, None, &[]).await;
    list.expect(StatusCode::OK);
    assert_eq!(list.body["data"].as_array().unwrap().len(), 1);
    for field in ["document", "title", "defaults"] {
        assert!(list.body["data"][0].get(field).is_none());
    }
    let preview_url = format!("{location}/preview");
    let preview = request(
        r,
        Some(&c),
        "GET",
        &preview_url,
        None,
        &[("if-none-match", created.etag())],
    )
    .await;
    preview.expect(StatusCode::OK);
    assert!(!preview.headers.contains_key("etag"));
    assert_eq!(preview.body["can_create"], true);
    assert_eq!(preview.body["materialized_title"], definition["title"]);
    assert!(
        chrono::DateTime::parse_from_rfc3339(preview.body["title_context_at"].as_str().unwrap())
            .is_ok()
    );
    let flow = request(r, Some(&c), "GET", &format!("{team}/workflow"), None, &[]).await;
    let chosen = preview.body["resolved_status"]["id"].clone();
    let other = flow.body["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] != chosen)
        .unwrap()["id"]
        .clone();
    request(
        r,
        Some(&c),
        "PUT",
        &format!("{team}/workflow/default"),
        Some(json!({"status_id":other})),
        &[("if-match", flow.etag())],
    )
    .await
    .expect(StatusCode::OK);
    let changed_preview = request(
        r,
        Some(&c),
        "GET",
        &preview_url,
        None,
        &[("if-none-match", created.etag())],
    )
    .await;
    changed_preview.expect(StatusCode::OK);
    assert_eq!(changed_preview.body["resolved_status"]["id"], other);
    assert_eq!(changed_preview.body["template"]["version"], "1");
    assert!(!changed_preview.headers.contains_key("etag"));
    request(
        r,
        Some(&c),
        "PATCH",
        location,
        Some(json!({"title":"Edited"})),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(
        r,
        Some(&c),
        "PATCH",
        location,
        Some(json!({"title":null})),
        &[("if-match", created.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    request(
        r,
        Some(&c),
        "PATCH",
        location,
        Some(json!({"unknown":true})),
        &[("if-match", created.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    for (invalid, status, code) in [
        (
            json!({"title_mode":null}),
            StatusCode::BAD_REQUEST,
            "bad_request",
        ),
        (
            json!({"title_mode":"expression"}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            json!({"title_mode":"pattern","title":"{{user.name}}"}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            json!({"title_mode":"pattern","title":"{{date}"}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
    ] {
        let reply = request(
            r,
            Some(&c),
            "PATCH",
            location,
            Some(invalid.clone()),
            &[("if-match", created.etag())],
        )
        .await;
        assert_eq!(
            reply.status, status,
            "Invalid input: {invalid}; response: {}",
            reply.body
        );
        reply.problem(status, code);
    }
    let edited = request(
        r,
        Some(&c),
        "PATCH",
        location,
        Some(json!({"title":"{{team.key}} {{team.name}} {{date}}","title_mode":"pattern"})),
        &[("if-match", created.etag())],
    )
    .await;
    edited.expect(StatusCode::OK);
    assert_eq!(edited.body["title_mode"], "pattern");
    let pattern_preview = request(r, Some(&c), "GET", &preview_url, None, &[]).await;
    pattern_preview.expect(StatusCode::OK);
    let context = chrono::DateTime::parse_from_rfc3339(
        pattern_preview.body["title_context_at"].as_str().unwrap(),
    )
    .unwrap()
    .with_timezone(&chrono::Utc);
    assert_eq!(
        pattern_preview.body["materialized_title"],
        format!("ENG Engineering {}", context.format("%Y-%m-%d"))
    );
    let stale = request(
        r,
        Some(&c),
        "PATCH",
        location,
        Some(json!({"title":"Stale"})),
        &[("if-match", created.etag())],
    )
    .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale.body["details"]["current"], edited.body);
    let issue = json!({"team_id":t,"title":"Customized issue","status_id":chosen,"document":definition["document"],"template":{"id":created.body["id"],"version":"1"}});
    let stale_create = request(
        r,
        Some(&c),
        "POST",
        &format!("{base}/issues"),
        Some(issue.clone()),
        &[("idempotency-key", "template-stale-create")],
    )
    .await;
    stale_create.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale_create.body["details"]["current"], edited.body);
    let mut current = issue;
    current["template"]["version"] = edited.body["version"].clone();
    let made = request(
        r,
        Some(&c),
        "POST",
        &format!("{base}/issues"),
        Some(current.clone()),
        &[("idempotency-key", "template-current-create")],
    )
    .await;
    made.expect(StatusCode::CREATED);
    assert_eq!(made.body["title"], "Customized issue");
    for field in [
        "template",
        "template_id",
        "template_version",
        "source_team_id",
    ] {
        assert!(made.body.get(field).is_none());
    }
    let archived = request(
        r,
        Some(&c),
        "PATCH",
        location,
        Some(json!({"archived":true})),
        &[("if-match", edited.etag())],
    )
    .await;
    archived.expect(StatusCode::OK);
    assert_eq!(archived.body["can_use"], false);
    let replay = request(
        r,
        Some(&c),
        "POST",
        &format!("{base}/issues"),
        Some(current),
        &[("idempotency-key", "template-current-create")],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body, made.body);
    let replay_template = request(
        r,
        Some(&c),
        "POST",
        &templates,
        Some(definition),
        &[("idempotency-key", "template-http-create")],
    )
    .await;
    replay_template.expect(StatusCode::CREATED);
    assert_eq!(replay_template.body, archived.body);
    let hidden = request(r, Some(&c), "GET", &templates, None, &[]).await;
    assert!(hidden.body["data"].as_array().unwrap().is_empty());
    let trash = request(
        r,
        Some(&c),
        "GET",
        &format!("{templates}?archived=true&limit=1"),
        None,
        &[],
    )
    .await;
    assert_eq!(trash.body["data"].as_array().unwrap().len(), 1);
    let restore = request(
        r,
        Some(&c),
        "PATCH",
        location,
        Some(json!({"archived":false})),
        &[("if-match", archived.etag())],
    )
    .await;
    restore.expect(StatusCode::OK);
    assert_eq!(restore.body["can_use"], true);
}
