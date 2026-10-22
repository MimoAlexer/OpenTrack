//! Real PostgreSQL HTTP checks for issue graph, move and historical-key boundaries.
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
async fn get(router: &Router, client: &Client, path: &str) -> Reply {
    let reply = request(router, Some(client), "GET", path, None, &[]).await;
    reply.expect(StatusCode::OK);
    reply
}
async fn create(router: &Router, client: &Client, base: &str, team: &str, title: &str) -> Reply {
    let reply = request(
        router,
        Some(client),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":team,"title":title})),
        &[("idempotency-key", title)],
    )
    .await;
    reply.expect(StatusCode::CREATED);
    assert_eq!(reply.body["etag"], reply.etag());
    reply
}
async fn patch(router: &Router, client: &Client, path: &str, tag: &str, body: Value) -> Reply {
    let reply = request(
        router,
        Some(client),
        "PATCH",
        path,
        Some(body),
        &[("if-match", tag)],
    )
    .await;
    reply.expect(StatusCode::OK);
    assert_eq!(reply.body["etag"], reply.etag());
    reply
}

#[tokio::test]
async fn graph_and_moves_preserve_http_preconditions_and_visibility() {
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
        "setup_token":h.setup_token,"email":"owner@graph-http.invalid", "display_name":"Graph owner",
        "password":"Graph HTTP verification passphrase 2026!", "workspace_name":"Graph contracts",
        "team_name":"Engineering","team_key":"ENG"
    })), &[]).await;
    setup.expect(StatusCode::CREATED);
    let owner = client(&setup);
    let w = setup.body["workspaces"][0]["id"].as_str().unwrap();
    let base = format!("/api/v1/workspaces/{w}");
    let boot = get(router, &owner, &format!("{base}/bootstrap")).await;
    let team = boot.body["teams"][0]["id"].as_str().unwrap();
    let a = create(router, &owner, &base, team, "Parent issue").await;
    let b = create(router, &owner, &base, team, "Related issue").await;
    let a_id = a.body["id"].as_str().unwrap();
    let b_id = b.body["id"].as_str().unwrap();
    let a_path = format!("{base}/issues/{a_id}");
    let b_path = format!("{base}/issues/{b_id}");
    let parent_path = format!("{b_path}/parent");
    let relations_path = format!("{a_path}/relations");
    let empty = get(router, &owner, &parent_path).await;
    assert!(empty.body["parent"].is_null());
    assert!(empty.body["can_change"].as_bool().unwrap());
    assert_ne!(empty.etag(), b.etag());
    request(
        router,
        None,
        "GET",
        &parent_path,
        None,
        &[("if-none-match", "*")],
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let unchanged = request(
        router,
        Some(&owner),
        "GET",
        &parent_path,
        None,
        &[("if-none-match", &format!("W/{}", empty.etag()))],
    )
    .await;
    unchanged.expect(StatusCode::NOT_MODIFIED);
    assert!(unchanged.body.is_null());
    assert_eq!(unchanged.etag(), empty.etag());

    let parent_body = json!({"parent_id":a_id,"parent_etag":a.etag()});
    request(
        router,
        Some(&owner),
        "PATCH",
        &parent_path,
        Some(parent_body.clone()),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let bad_csrf = Client {
        csrf: "incorrect".into(),
        ..owner.clone()
    };
    request(
        router,
        Some(&bad_csrf),
        "PATCH",
        &parent_path,
        Some(parent_body.clone()),
        &[("if-match", b.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    for invalid in [
        json!({}),
        json!({"parent_id":null,"parent_etag":null}),
        json!({"parent_id":a_id}),
        json!({"parent_id":null,"surprise":true}),
    ] {
        request(
            router,
            Some(&owner),
            "PATCH",
            &parent_path,
            Some(invalid),
            &[("if-match", b.etag())],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    let renamed_a = patch(
        router,
        &owner,
        &a_path,
        a.etag(),
        json!({"title":"Renamed parent"}),
    )
    .await;
    let target_conflict = request(
        router,
        Some(&owner),
        "PATCH",
        &parent_path,
        Some(parent_body),
        &[("if-match", b.etag())],
    )
    .await;
    target_conflict.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(
        target_conflict.body["details"]["current"]["target"]["id"],
        a_id
    );
    assert_eq!(
        target_conflict.body["details"]["current"]["target"]["etag"],
        renamed_a.etag()
    );
    let attached = patch(
        router,
        &owner,
        &parent_path,
        b.etag(),
        json!({"parent_id":a_id,"parent_etag":renamed_a.etag()}),
    )
    .await;
    let linked = get(router, &owner, &parent_path).await;
    assert_eq!(linked.body["parent"]["target"]["id"], a_id);
    let parent_after = get(router, &owner, &a_path).await;
    assert_ne!(parent_after.etag(), renamed_a.etag());
    let children = get(router, &owner, &format!("{a_path}/children")).await;
    assert_eq!(children.body["data"][0]["id"], b_id);
    assert_eq!(children.body["issue_etag"], parent_after.etag());
    assert!(children.body.get("total").is_none());

    // A metadata-only target change must invalidate the parent's projection even
    // though the child aggregate did not change.
    patch(
        router,
        &owner,
        &a_path,
        parent_after.etag(),
        json!({"title":"Current parent title"}),
    )
    .await;
    let changed = request(
        router,
        Some(&owner),
        "GET",
        &parent_path,
        None,
        &[("if-none-match", linked.etag())],
    )
    .await;
    changed.expect(StatusCode::OK);
    assert_ne!(changed.etag(), linked.etag());
    assert_eq!(changed.body["issue_etag"], attached.etag());
    assert_eq!(
        changed.body["parent"]["target"]["title"],
        "Current parent title"
    );
    let detached = patch(
        router,
        &owner,
        &parent_path,
        attached.etag(),
        json!({"parent_id":null}),
    )
    .await;
    assert!(get(router, &owner, &parent_path).await.body["parent"].is_null());

    let current_a = get(router, &owner, &a_path).await;
    let relation_body = json!({"kind":"blocks","target_id":b_id,"target_etag":detached.etag()});
    request(
        router,
        Some(&owner),
        "POST",
        &relations_path,
        Some(relation_body.clone()),
        &[("if-match", current_a.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let created = request(
        router,
        Some(&owner),
        "POST",
        &relations_path,
        Some(relation_body.clone()),
        &[
            ("if-match", current_a.etag()),
            ("idempotency-key", "block-receipt"),
        ],
    )
    .await;
    created.expect(StatusCode::CREATED);
    assert_eq!(created.body["issue"]["etag"], created.etag());
    assert_eq!(created.body["relation"]["kind"], "blocks");
    let replay = request(
        router,
        Some(&owner),
        "POST",
        &relations_path,
        Some(relation_body.clone()),
        &[
            ("if-match", current_a.etag()),
            ("idempotency-key", "block-receipt"),
        ],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body, created.body);
    assert_eq!(replay.etag(), created.etag());
    request(
        router,
        Some(&owner),
        "POST",
        &relations_path,
        Some(relation_body),
        &[
            ("if-match", created.etag()),
            ("idempotency-key", "block-receipt"),
        ],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let relative = get(router, &owner, &format!("{b_path}/relations")).await;
    assert_eq!(relative.body["data"][0]["kind"], "blocked_by");
    assert_eq!(relative.body["data"][0]["target"]["id"], a_id);
    let relation_path = created.headers["location"].to_str().unwrap();
    let relation = get(router, &owner, relation_path).await;
    assert_eq!(relation.body, created.body["relation"]);
    request(
        router,
        Some(&owner),
        "GET",
        relation_path,
        None,
        &[("if-none-match", relation.etag())],
    )
    .await
    .expect(StatusCode::NOT_MODIFIED);
    let current_b = get(router, &owner, &b_path).await;
    patch(
        router,
        &owner,
        &b_path,
        current_b.etag(),
        json!({"title":"Current related title"}),
    )
    .await;
    let changed_relation = request(
        router,
        Some(&owner),
        "GET",
        relation_path,
        None,
        &[("if-none-match", relation.etag())],
    )
    .await;
    changed_relation.expect(StatusCode::OK);
    assert_eq!(
        changed_relation.body["target"]["title"],
        "Current related title"
    );
    assert_ne!(changed_relation.etag(), relation.etag());
    request(router, Some(&owner), "DELETE", relation_path, None, &[])
        .await
        .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let removed = request(
        router,
        Some(&owner),
        "DELETE",
        relation_path,
        None,
        &[("if-match", created.etag())],
    )
    .await;
    removed.expect(StatusCode::OK);
    assert_eq!(removed.body["etag"], removed.etag());
    request(router, Some(&owner), "GET", relation_path, None, &[])
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");

    // Child creation is one command and checks the chosen parent's validator.
    let child = request(router, Some(&owner), "POST", &format!("{base}/issues"),
        Some(json!({"team_id":team,"title":"Child created atomically","parent_id":a_id,"parent_etag":removed.etag()})),
        &[("idempotency-key","create-child")]).await;
    child.expect(StatusCode::CREATED);
    let child_path = format!("{base}/issues/{}", child.body["id"].as_str().unwrap());
    assert_eq!(
        get(router, &owner, &format!("{child_path}/parent"))
            .await
            .body["parent"]["target"]["id"],
        a_id
    );
    assert!(child.body["assignee_id"].is_null());
    assert!(child.body["label_ids"].as_array().unwrap().is_empty());

    let destination = request(
        router,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private delivery","key":"PVT","private":true})),
        &[("idempotency-key", "private-destination")],
    )
    .await;
    destination.expect(StatusCode::CREATED);
    let dest = destination.body["id"].as_str().unwrap();
    let preview_path = format!("{a_path}/move-preview?destination_team_id={dest}");
    let blocked = get(router, &owner, &preview_path).await;
    assert!(
        blocked.body["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "children")
    );
    assert!(blocked.body.get("new_key").is_none());
    patch(
        router,
        &owner,
        &format!("{child_path}/parent"),
        child.etag(),
        json!({"parent_id":null}),
    )
    .await;

    // A member can create in the public source team, but old aliases and stored
    // receipts must not disclose the issue once moved into a private team.
    let invited = request(
        router,
        Some(&owner),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":"member@graph-http.invalid","role":"member","team_ids":[]})),
        &[("idempotency-key", "graph-member")],
    )
    .await;
    invited.expect(StatusCode::CREATED);
    let accepted = request(router, None, "POST", "/api/v1/invitations/accept", Some(json!({
        "token":invited.body["token"],"display_name":"Graph member","password":"Graph member verification passphrase 2026!"
    })), &[]).await;
    accepted.expect(StatusCode::OK);
    let member = client(&accepted);
    let movable = create(router, &member, &base, team, "Move receipt privacy").await;
    let moving_id = movable.body["id"].as_str().unwrap();
    let moving_path = format!("{base}/issues/{moving_id}");
    let old_key = movable.body["key"].as_str().unwrap();
    let destinations = get(router, &owner, &format!("{moving_path}/move-destinations")).await;
    assert!(
        destinations.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == dest && v["can_move_issue"] == true)
    );
    let member_destinations =
        get(router, &member, &format!("{moving_path}/move-destinations")).await;
    assert!(!member_destinations.body.to_string().contains(dest));
    let preview = get(
        router,
        &owner,
        &format!("{moving_path}/move-preview?destination_team_id={dest}"),
    )
    .await;
    assert_eq!(preview.body["audience_change"], "public_to_private");
    assert!(preview.body["blockers"].as_array().unwrap().is_empty());
    let move_body = json!({"destination_team_id":dest,"status_id":preview.body["destination_default_status_id"],
        "assignee_id":null,"label_ids":[],"preview_token":preview.body["preview_token"]});
    let mut omitted = move_body.clone();
    omitted.as_object_mut().unwrap().remove("assignee_id");
    request(
        router,
        Some(&owner),
        "POST",
        &format!("{moving_path}/move"),
        Some(omitted),
        &[
            ("if-match", movable.etag()),
            ("idempotency-key", "move-omitted"),
        ],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let moved = request(
        router,
        Some(&owner),
        "POST",
        &format!("{moving_path}/move"),
        Some(move_body.clone()),
        &[
            ("if-match", movable.etag()),
            ("idempotency-key", "move-once"),
        ],
    )
    .await;
    moved.expect(StatusCode::OK);
    assert_eq!(moved.body["id"], moving_id);
    assert_eq!(moved.body["team_id"], dest);
    assert_eq!(moved.body["etag"], moved.etag());
    assert_ne!(moved.body["key"], old_key);
    assert!(!moved.headers.contains_key("location"));
    let replay = request(
        router,
        Some(&owner),
        "POST",
        &format!("{moving_path}/move"),
        Some(move_body),
        &[
            ("if-match", movable.etag()),
            ("idempotency-key", "move-once"),
        ],
    )
    .await;
    replay.expect(StatusCode::OK);
    assert_eq!(replay.body, moved.body);
    let alias_path = format!("{base}/issues/by-key/{old_key}");
    let alias = get(router, &owner, &alias_path).await;
    assert_eq!(alias.body, moved.body);
    assert_eq!(alias.etag(), moved.etag());
    assert!(!alias.headers.contains_key("location"));
    for path in [
        &alias_path,
        &moving_path,
        &format!("{moving_path}/parent"),
        &format!("{moving_path}/relations"),
    ] {
        let denied = request(
            router,
            Some(&member),
            "GET",
            path,
            None,
            &[("if-none-match", "*")],
        )
        .await;
        denied.problem(StatusCode::NOT_FOUND, "not_found");
        assert!(!denied.body.to_string().contains(dest));
        assert!(
            !denied
                .body
                .to_string()
                .contains(moved.body["key"].as_str().unwrap())
        );
    }
    request(
        router,
        Some(&member),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":team,"title":"Move receipt privacy"})),
        &[("idempotency-key", "Move receipt privacy")],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let lookup = get(
        router,
        &owner,
        &format!("{base}/issues/lookup?team_id={dest}&q={old_key}"),
    )
    .await;
    assert_eq!(lookup.body["data"][0]["id"], moving_id);
    let source_lookup = get(
        router,
        &member,
        &format!("{base}/issues/lookup?team_id={team}&q={old_key}"),
    )
    .await;
    assert!(source_lookup.body["data"].as_array().unwrap().is_empty());
}
