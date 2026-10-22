//! Scoped mention projections and personal subscription transport with real PostgreSQL.
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
    user: String,
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
        user: reply.body["user"]["id"].as_str().unwrap().into(),
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
const PASSWORD: &str = "Mention HTTP verification password 2026!";
async fn invited(r: &Router, owner: &Client, base: &str, role: &str) -> Client {
    let invite = request(
        r,
        Some(owner),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":format!("{role}@mentions.invalid"),"role":role})),
        &[("idempotency-key", &format!("mentions-invite-{role}"))],
    )
    .await;
    invite.expect(StatusCode::CREATED);
    let accepted=request(r,None,"POST","/api/v1/invitations/accept",Some(json!({"token":invite.body["token"],"display_name":format!("Mention {role}"),"password":PASSWORD})),&[]).await;
    accepted.expect(StatusCode::OK);
    client(&accepted)
}
fn body(ids: &[&str]) -> Value {
    json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":ids.iter().map(|id|json!({"type":"userMention","attrs":{"id":id}})).collect::<Vec<_>>()}]}})
}
#[tokio::test]
async fn mentions_and_personal_subscription_transport() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let r = &h.router;
    let setup=request(r,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@mentions.invalid","display_name":"Mention owner","password":PASSWORD,"workspace_name":"Mentions","team_name":"Engineering","team_key":"ENG"})),&[]).await;
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
    let t = boot.body["teams"][0]["id"].as_str().unwrap();
    let viewer = invited(r, &owner, &base, "viewer").await;
    let member = invited(r, &owner, &base, "member").await;
    let picker = format!("{base}/mention-candidates?team_id={t}");
    request(r, None, "GET", &picker, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    for query in [
        "".to_owned(),
        format!("?team_id={t}&issue_id={}", uuid::Uuid::new_v4()),
        format!("?team_id={t}&limit=51"),
    ] {
        request(
            r,
            Some(&owner),
            "GET",
            &format!("{base}/mention-candidates{query}"),
            None,
            &[],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    let candidates = request(r, Some(&viewer), "GET", &picker, None, &[]).await;
    candidates.expect(StatusCode::OK);
    let data = candidates.body["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert!(data.iter().any(|v| v["id"] == viewer.user));
    assert!(data.iter().all(|v| v.as_object().unwrap().len() == 2));
    let first = request(
        r,
        Some(&owner),
        "GET",
        &format!("{picker}&limit=1"),
        None,
        &[],
    )
    .await;
    first.expect(StatusCode::OK);
    assert_eq!(first.body["has_more"], true);
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", first.body["next_cursor"].as_str().unwrap())
        .finish();
    let next = request(
        r,
        Some(&owner),
        "GET",
        &format!("{picker}&limit=1&{encoded}"),
        None,
        &[],
    )
    .await;
    next.expect(StatusCode::OK);
    assert_ne!(first.body["data"][0]["id"], next.body["data"][0]["id"]);
    request(
        r,
        Some(&viewer),
        "GET",
        &format!("{picker}&limit=1&{encoded}"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let created = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":t,"title":"Mention HTTP"})),
        &[("idempotency-key", "mention-http-issue-create")],
    )
    .await;
    created.expect(StatusCode::CREATED);
    let issue_url = created.headers["location"].to_str().unwrap();
    let subscription_url = format!("{issue_url}/subscription");
    let own = request(r, Some(&owner), "GET", &subscription_url, None, &[]).await;
    own.expect(StatusCode::OK);
    assert_eq!(own.body["sources"], json!(["creator"]));
    assert_eq!(own.body["subscribed"], true);
    assert_eq!(own.body["etag"], own.etag());
    let absent = request(r, Some(&viewer), "GET", &subscription_url, None, &[]).await;
    absent.expect(StatusCode::OK);
    assert_eq!(absent.body["version"], "0");
    request(
        r,
        Some(&viewer),
        "PATCH",
        &subscription_url,
        Some(json!({"preference":"unsubscribed"})),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let bad = Client {
        csrf: "wrong".into(),
        ..viewer.clone()
    };
    request(
        r,
        Some(&bad),
        "PATCH",
        &subscription_url,
        Some(json!({"preference":"unsubscribed"})),
        &[("if-match", absent.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let changed = request(
        r,
        Some(&viewer),
        "PATCH",
        &subscription_url,
        Some(json!({"preference":"unsubscribed"})),
        &[("if-match", absent.etag())],
    )
    .await;
    changed.expect(StatusCode::OK);
    assert_eq!(changed.body["subscribed"], false);
    let stale = request(
        r,
        Some(&viewer),
        "PATCH",
        &subscription_url,
        Some(json!({"preference":"subscribed"})),
        &[("if-match", absent.etag())],
    )
    .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale.body["details"]["current"], changed.body);
    assert_eq!(
        request(r, Some(&owner), "GET", issue_url, None, &[])
            .await
            .body,
        created.body,
        "Personal preference does not alter issue metadata"
    );
    let before = request(
        r,
        Some(&owner),
        "GET",
        &format!("{issue_url}/document"),
        None,
        &[],
    )
    .await;
    before.expect(StatusCode::OK);
    let document_url = format!("{base}/documents/{}", before.body["id"].as_str().unwrap());
    let content = body(&[&viewer.user, &member.user]);
    let saved = request(
        r,
        Some(&owner),
        "PATCH",
        &document_url,
        Some(content.clone()),
        &[("if-match", before.etag())],
    )
    .await;
    saved.expect(StatusCode::OK);
    assert_eq!(saved.body["body"], content["body"]);
    assert_eq!(saved.body["mentions"].as_array().unwrap().len(), 2);
    assert!(
        saved.body["plain_text"]
            .as_str()
            .unwrap()
            .contains("@someone")
    );
    let pref = request(r, Some(&viewer), "GET", &subscription_url, None, &[]).await;
    pref.expect(StatusCode::OK);
    assert_eq!(pref.body["subscribed"], false);
    assert_eq!(pref.body["sources"], json!(["mention"]));
    let own_again = request(r, Some(&owner), "GET", &subscription_url, None, &[]).await;
    assert_eq!(
        own_again.body, own.body,
        "Another person's preference/source does not change my validator"
    );
    let comment = request(
        r,
        Some(&member),
        "POST",
        &format!("{issue_url}/comments"),
        Some(body(&[&viewer.user])),
        &[("idempotency-key", "mention-only-http-comment")],
    )
    .await;
    comment.expect(StatusCode::CREATED);
    let replay = request(
        r,
        Some(&member),
        "POST",
        &format!("{issue_url}/comments"),
        Some(body(&[&viewer.user])),
        &[("idempotency-key", "mention-only-http-comment")],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body, comment.body);
    let mut invalid = content.clone();
    invalid["body"]["content"][0]["content"][0]["attrs"]["label"] = json!("Forged name");
    request(
        r,
        Some(&owner),
        "PATCH",
        &document_url,
        Some(invalid),
        &[("if-match", saved.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let private = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private mentions","key":"PRI","private":true})),
        &[("idempotency-key", "mentions-private-team")],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_team = private.body["id"].as_str().unwrap();
    request(
        r,
        Some(&viewer),
        "GET",
        &format!("{base}/mention-candidates?team_id={private_team}"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let private_issue = request(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":private_team,"title":"Private mention context"})),
        &[("idempotency-key", "mention-private-issue")],
    )
    .await;
    private_issue.expect(StatusCode::CREATED);
    request(
        r,
        Some(&viewer),
        "GET",
        &format!(
            "{}/subscription",
            private_issue.headers["location"].to_str().unwrap()
        ),
        None,
        &[],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let membership_url = format!("{base}/members/{}", viewer.user);
    let membership = request(r, Some(&owner), "GET", &membership_url, None, &[]).await;
    membership.expect(StatusCode::OK);
    request(
        r,
        Some(&owner),
        "PATCH",
        &membership_url,
        Some(json!({"active":false})),
        &[("if-match", membership.etag())],
    )
    .await
    .expect(StatusCode::OK);
    let redacted = request(
        r,
        Some(&owner),
        "GET",
        &document_url,
        None,
        &[("if-none-match", saved.etag())],
    )
    .await;
    redacted.expect(StatusCode::OK);
    assert_ne!(redacted.etag(), saved.etag());
    assert_eq!(redacted.body["version"], saved.body["version"]);
    let unavailable = redacted.body["mentions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == viewer.user)
        .unwrap();
    assert_eq!(unavailable["available"], false);
    assert!(unavailable["display_name"].is_null());
    let mut retained = content.clone();
    retained["body"]["content"][0]["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"text","text":" Continue editing historical references"}));
    request(
        r,
        Some(&owner),
        "PATCH",
        &document_url,
        Some(retained),
        &[("if-match", redacted.etag())],
    )
    .await
    .expect(StatusCode::OK);
    let final_picker = request(r, Some(&owner), "GET", &picker, None, &[]).await;
    assert!(
        !final_picker.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == viewer.user)
    );
}
