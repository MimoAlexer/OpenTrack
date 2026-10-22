//! Real PostgreSQL HTTP checks for label transport and issue reference validators.
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
async fn labels_assignees_and_projection_conflicts_over_http() {
    let h = Harness::new().await;
    let test = h.clone();
    let outcome = tokio::spawn(async move { exercise(&test).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let router = &h.router;
    let setup = request(router, None, "POST", "/api/v1/setup", Some(json!({
        "setup_token":h.setup_token,"email":"owner@taxonomy.invalid",
        "display_name":"Taxonomy owner","password":"Taxonomy HTTP verification passphrase 2026!",
        "workspace_name":"Taxonomy contracts","team_name":"Engineering","team_key":"ENG"
    })), &[]).await;
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
    let user = &setup.body["user"]["id"];
    let w = setup.body["workspaces"][0]["id"].as_str().unwrap();
    let base = format!("/api/v1/workspaces/{w}");
    let boot = request(
        router,
        Some(&client),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    boot.expect(StatusCode::OK);
    let t = boot.body["teams"][0]["id"].as_str().unwrap();
    let labels = format!("{base}/labels");
    request(router, None, "GET", &labels, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let invalid_csrf = Client {
        csrf: "wrong-token".into(),
        ..client.clone()
    };
    request(
        router,
        Some(&invalid_csrf),
        "POST",
        &labels,
        Some(json!({"name":"Blocked"})),
        &[("idempotency-key", "blocked")],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        router,
        Some(&client),
        "POST",
        &labels,
        Some(json!({"name":"Missing key"})),
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let shared = request(
        router,
        Some(&client),
        "POST",
        &labels,
        Some(json!({"name":"Customer report","color":"blue"})),
        &[("idempotency-key", "shared-label")],
    )
    .await;
    shared.expect(StatusCode::CREATED);
    assert!(shared.body["team_id"].is_null());
    assert_eq!(shared.body["can_manage"], true);
    let shared_id = shared.body["id"].as_str().unwrap();
    let label_path = format!("{labels}/{shared_id}");
    assert_eq!(shared.headers["location"], label_path);
    let replay = request(
        router,
        Some(&client),
        "POST",
        &labels,
        Some(json!({"name":"Customer report","color":"blue"})),
        &[("idempotency-key", "shared-label")],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body, shared.body);
    assert_eq!(replay.etag(), shared.etag());
    let team = request(
        router,
        Some(&client),
        "POST",
        &labels,
        Some(json!({"team_id":t,"name":"Regression"})),
        &[("idempotency-key", "team-label")],
    )
    .await;
    team.expect(StatusCode::CREATED);
    assert_eq!(team.body["team_id"], t);
    assert_eq!(team.body["color"], "gray");
    let team_id = team.body["id"].as_str().unwrap();
    let applicable = request(
        router,
        Some(&client),
        "GET",
        &format!("{labels}?scope=applicable&team_id={t}"),
        None,
        &[],
    )
    .await;
    applicable.expect(StatusCode::OK);
    assert_eq!(applicable.body["data"].as_array().unwrap().len(), 2);
    for query in [
        "scope=team".to_owned(),
        format!("scope=workspace&team_id={t}"),
        "limit=101".into(),
        "state=unknown".into(),
    ] {
        request(
            router,
            Some(&client),
            "GET",
            &format!("{labels}?{query}"),
            None,
            &[],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    request(
        router,
        Some(&client),
        "PATCH",
        &label_path,
        Some(json!({"name":"Edited"})),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    for patch in [
        json!({"name":null}),
        json!({"color":null}),
        json!({"archived":null}),
        json!({"team_id":t}),
    ] {
        request(
            router,
            Some(&client),
            "PATCH",
            &label_path,
            Some(patch),
            &[("if-match", shared.etag())],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    let candidates_path = format!("{base}/teams/{t}/assignees");
    let candidates = request(router, Some(&client), "GET", &candidates_path, None, &[]).await;
    candidates.expect(StatusCode::OK);
    assert_eq!(candidates.body["data"][0]["id"], *user);
    assert_eq!(candidates.body["self_assignable"], true);
    assert_eq!(candidates.body["data"][0].as_object().unwrap().len(), 2);
    let empty = request(
        router,
        Some(&client),
        "GET",
        &format!("{candidates_path}?q=absent-name"),
        None,
        &[],
    )
    .await;
    empty.expect(StatusCode::OK);
    assert_eq!(empty.body["data"], json!([]));
    assert_eq!(empty.body["self_assignable"], true);
    request(
        router,
        Some(&client),
        "GET",
        &format!("{candidates_path}?limit=21"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let issues = format!("{base}/issues");
    let created = request(router, Some(&client), "POST", &issues, Some(json!({
        "team_id":t,"title":"Check labels over HTTP","assignee_id":user,"label_ids":[team_id,shared_id]
    })), &[("idempotency-key","labelled-issue")]).await;
    created.expect(StatusCode::CREATED);
    assert_eq!(created.body["etag"], created.etag());
    assert_eq!(created.body["assignee"]["id"], *user);
    assert_eq!(created.body["assignee"]["assignable"], true);
    assert_eq!(created.body["labels"].as_array().unwrap().len(), 2);
    let issue_path = created.headers["location"].to_str().unwrap();
    let filtered = request(
        router,
        Some(&client),
        "GET",
        &format!("{issues}?label_ids={shared_id},{team_id}&label_mode=all&sort=priority"),
        None,
        &[],
    )
    .await;
    filtered.expect(StatusCode::OK);
    assert_eq!(filtered.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(filtered.body["data"][0]["etag"], created.etag());
    for query in [
        "label_ids=invalid",
        "label_ids=",
        "label_mode=wrong",
        "sort=wrong",
    ] {
        let reply = request(
            router,
            Some(&client),
            "GET",
            &format!("{issues}?{query}"),
            None,
            &[],
        )
        .await;
        assert!(matches!(
            reply.status,
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
        ));
        assert_eq!(reply.headers["content-type"], "application/problem+json");
    }
    let renamed = request(
        router,
        Some(&client),
        "PATCH",
        &label_path,
        Some(json!({"name":"Customer feedback"})),
        &[("if-match", shared.etag())],
    )
    .await;
    renamed.expect(StatusCode::OK);
    assert_ne!(renamed.etag(), shared.etag());
    let label_conflict = request(
        router,
        Some(&client),
        "PATCH",
        &label_path,
        Some(json!({"name":"Stale edit"})),
        &[("if-match", shared.etag())],
    )
    .await;
    label_conflict.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(label_conflict.body["details"]["current"], renamed.body);
    let current = request(router, Some(&client), "GET", issue_path, None, &[]).await;
    current.expect(StatusCode::OK);
    assert_eq!(current.body["version"], created.body["version"]);
    assert_ne!(current.etag(), created.etag());
    assert_eq!(current.body["etag"], current.etag());
    let conflict = request(
        router,
        Some(&client),
        "PATCH",
        issue_path,
        Some(json!({"title":"Stale issue edit"})),
        &[("if-match", created.etag())],
    )
    .await;
    conflict.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(conflict.body["details"]["current"], current.body);
    let archived = request(
        router,
        Some(&client),
        "PATCH",
        &label_path,
        Some(json!({"archived":true})),
        &[("if-match", renamed.etag())],
    )
    .await;
    archived.expect(StatusCode::OK);
    assert!(archived.body["archived_at"].is_string());
    let historical = request(router, Some(&client), "GET", issue_path, None, &[]).await;
    historical.expect(StatusCode::OK);
    assert_ne!(historical.etag(), current.etag());
    assert!(
        historical.body["labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|label| label["id"] == shared_id && label["archived_at"].is_string())
    );
    let retained = request(
        router,
        Some(&client),
        "PATCH",
        issue_path,
        Some(json!({"title":"Retain historical labels","label_ids":[shared_id,team_id]})),
        &[("if-match", historical.etag())],
    )
    .await;
    retained.expect(StatusCode::OK);
    assert_eq!(retained.body["etag"], retained.etag());
    request(router, Some(&client), "POST", &issues, Some(json!({"team_id":t,"title":"Cannot newly apply archived label","label_ids":[shared_id]})), &[("idempotency-key","archived-label-rejected")]).await.problem(StatusCode::UNPROCESSABLE_ENTITY,"validation_failed");
    let removed = request(
        router,
        Some(&client),
        "PATCH",
        issue_path,
        Some(json!({"label_ids":[]})),
        &[("if-match", retained.etag())],
    )
    .await;
    removed.expect(StatusCode::OK);
    assert_eq!(removed.body["label_ids"], json!([]));
    assert_eq!(removed.body["labels"], json!([]));
    assert_eq!(removed.body["etag"], removed.etag());
    let restored = request(
        router,
        Some(&client),
        "PATCH",
        &label_path,
        Some(json!({"archived":false,"name":"Restored feedback"})),
        &[("if-match", archived.etag())],
    )
    .await;
    restored.expect(StatusCode::OK);
    assert!(restored.body["archived_at"].is_null());
    let fetched = request(router, Some(&client), "GET", &label_path, None, &[]).await;
    fetched.expect(StatusCode::OK);
    assert_eq!(fetched.body, restored.body);
    assert_eq!(fetched.etag(), restored.etag());
    let activity = request(
        router,
        Some(&client),
        "GET",
        &format!("{issue_path}/activity"),
        None,
        &[],
    )
    .await;
    activity.expect(StatusCode::OK);
    let events = activity.body["data"].as_array().unwrap();
    assert_eq!(events.len(), 3, "Failed writes must not append activity");
    assert_eq!(events[0]["changes"]["source"], "manual");
    assert_eq!(
        events[0]["changes"]["label_ids"]["before"],
        retained.body["label_ids"]
    );
    assert_eq!(events[0]["changes"]["label_ids"]["after"], json!([]));
    assert_eq!(events[2]["changes"]["assignee_id"]["before"], Value::Null);
    assert_eq!(events[2]["changes"]["assignee_id"]["after"], *user);
}
