//! Recipient-only inbox HTTP auth, CSRF, preconditions and wire-schema coverage.
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
use uuid::Uuid;

#[derive(Clone)]
struct Client {
    cookie: String,
    csrf: String,
    user: Uuid,
}
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}
impl Reply {
    fn expect(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{}", self.body);
    }
    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
    fn client(&self) -> Client {
        Client {
            cookie: self.headers["set-cookie"]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .into(),
            csrf: self.body["csrf_token"].as_str().unwrap().into(),
            user: serde_json::from_value(self.body["user"]["id"].clone()).unwrap(),
        }
    }
}
async fn call(
    r: &Router,
    c: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    etag: Option<&str>,
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN)
        .header(
            "idempotency-key",
            format!("notification-http-{}", Uuid::new_v4()),
        );
    if let Some(c) = c {
        request = request
            .header("cookie", &c.cookie)
            .header("x-csrf-token", &c.csrf);
    }
    if let Some(etag) = etag {
        request = request.header("if-match", etag);
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = r
        .clone()
        .oneshot(
            request
                .body(Body::from(
                    body.map(|b| serde_json::to_vec(&b).unwrap())
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
async fn notifications_http_recipient_scope_csrf_and_preconditions() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let r = &h.router;
    let setup=call(r,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@notifications.invalid","display_name":"Example Owner","password":"Notification HTTP verification password 2026!","workspace_name":"Inbox","team_name":"Engineering","team_key":"ENG"})),None).await;
    setup.expect(StatusCode::CREATED);
    let owner = setup.client();
    let w: Uuid = serde_json::from_value(setup.body["workspaces"][0]["id"].clone()).unwrap();
    let base = format!("/api/v1/workspaces/{w}");
    let bootstrap = call(
        r,
        Some(&owner),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        None,
    )
    .await;
    let t: Uuid = serde_json::from_value(bootstrap.body["teams"][0]["id"].clone()).unwrap();
    // The invitation creation header is required independently of notification commands.
    let request = Request::builder()
        .method("POST")
        .uri(format!("{base}/invitations"))
        .header("origin", ORIGIN)
        .header("cookie", &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header("idempotency-key", "notification-viewer-invite")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"email":"viewer@notifications.invalid","role":"viewer"}).to_string(),
        ))
        .unwrap();
    let invite = r.clone().oneshot(request).await.unwrap();
    assert_eq!(invite.status(), StatusCode::CREATED);
    let invite: Value =
        serde_json::from_slice(&to_bytes(invite.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    let accepted=call(r,None,"POST","/api/v1/invitations/accept",Some(json!({"token":invite["token"],"display_name":"Example Viewer","password":"Notification HTTP verification password 2026!"})),None).await;
    accepted.expect(StatusCode::OK);
    let viewer = accepted.client();
    let source = call(
        r,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":t,"title":"Deleted source title"})),
        None,
    )
    .await;
    source.expect(StatusCode::CREATED);
    let source_id: Uuid = serde_json::from_value(source.body["id"].clone()).unwrap();
    call(
        r,
        Some(&owner),
        "DELETE",
        &format!("{base}/issues/{source_id}"),
        None,
        Some(source.etag()),
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    let notification = Uuid::new_v4();
    let mut fixture = h.lock_team(w, t, owner.user).await;
    // Source labels deliberately remain absent for this missing-source fixture.
    sqlx::query("INSERT INTO notification_heads(workspace_id,user_id,last_seq) VALUES($1,$2,1)")
        .bind(w)
        .bind(viewer.user)
        .execute(&mut *fixture)
        .await
        .unwrap();
    sqlx::query("INSERT INTO notifications(workspace_id,user_id,id,inbox_seq,event_id,entity_kind,entity_id,reason,origin_team_id,actor_id) VALUES($1,$2,$3,1,$4,'issue',$5,'mention',$6,$7)").bind(w).bind(viewer.user).bind(notification).bind(Uuid::new_v4()).bind(source_id).bind(t).bind(owner.user).execute(&mut *fixture).await.unwrap();
    fixture.commit().await.unwrap();
    let list = format!("{base}/notifications");
    let item_path = format!("{list}/{notification}");
    let mark_path = format!("{list}/mark-read");
    let prefs = format!("{base}/notification-preferences");
    for path in [&list, &prefs] {
        call(r, None, "GET", path, None, None)
            .await
            .expect(StatusCode::UNAUTHORIZED);
    }
    assert_eq!(
        call(r, Some(&owner), "GET", &list, None, None).await.body["items"],
        json!([])
    );
    let page = call(r, Some(&viewer), "GET", &list, None, None).await;
    page.expect(StatusCode::OK);
    assert_eq!(page.body["items"][0]["seq"], "1");
    assert_eq!(page.body["items"][0]["version"], "1");
    assert_eq!(page.body["next_cursor"], Value::Null);
    assert_eq!(page.body["unread_count"], 1);
    assert!(!page.headers.contains_key("etag"));
    assert_eq!(page.body["items"][0]["source"]["available"], false);
    assert!(page.body["items"][0]["source"].get("title").is_none());
    let tag = page.body["items"][0]["etag"].as_str().unwrap();
    call(
        r,
        Some(&owner),
        "PATCH",
        &item_path,
        Some(json!({"action":"read"})),
        Some(tag),
    )
    .await
    .expect(StatusCode::NOT_FOUND);
    call(
        r,
        Some(&viewer),
        "PATCH",
        &item_path,
        Some(json!({"action":"read"})),
        None,
    )
    .await
    .expect(StatusCode::PRECONDITION_REQUIRED);
    let mut bad = viewer.clone();
    bad.csrf = "invalid".into();
    call(
        r,
        Some(&bad),
        "PATCH",
        &item_path,
        Some(json!({"action":"read"})),
        Some(tag),
    )
    .await
    .expect(StatusCode::FORBIDDEN);
    call(
        r,
        Some(&bad),
        "POST",
        &mark_path,
        Some(json!({"snapshot":page.body["snapshot"]})),
        None,
    )
    .await
    .expect(StatusCode::FORBIDDEN);
    call(
        r,
        Some(&viewer),
        "PATCH",
        &item_path,
        Some(json!({"action":"read","user_id":owner.user})),
        Some(tag),
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    call(
        r,
        Some(&viewer),
        "POST",
        &mark_path,
        Some(json!({"snapshot":page.body["snapshot"],"user_id":owner.user})),
        None,
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    call(
        r,
        Some(&owner),
        "POST",
        &mark_path,
        Some(json!({"snapshot":page.body["snapshot"]})),
        None,
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    for suffix in [
        "?limit=51",
        "?limit=0",
        "?filter=hidden",
        "?cursor=invalid",
        "?user_id=someone",
    ] {
        assert!(
            call(
                r,
                Some(&viewer),
                "GET",
                &format!("{list}{suffix}"),
                None,
                None
            )
            .await
            .status
            .is_client_error()
        );
    }
    let changed = call(
        r,
        Some(&viewer),
        "PATCH",
        &item_path,
        Some(json!({"action":"read"})),
        Some(tag),
    )
    .await;
    changed.expect(StatusCode::OK);
    assert_eq!(changed.body["version"], "2");
    assert_eq!(changed.etag(), changed.body["etag"]);
    let noop = call(
        r,
        Some(&viewer),
        "PATCH",
        &item_path,
        Some(json!({"action":"read"})),
        Some(changed.etag()),
    )
    .await;
    noop.expect(StatusCode::OK);
    assert_eq!(noop.body, changed.body);
    let stale = call(
        r,
        Some(&viewer),
        "PATCH",
        &item_path,
        Some(json!({"action":"unread"})),
        Some(tag),
    )
    .await;
    stale.expect(StatusCode::PRECONDITION_FAILED);
    assert_eq!(stale.body["details"]["current"], changed.body);
    let marked = call(
        r,
        Some(&viewer),
        "POST",
        &mark_path,
        Some(json!({"snapshot":page.body["snapshot"]})),
        None,
    )
    .await;
    marked.expect(StatusCode::OK);
    assert_eq!(
        marked.body,
        json!({"updated_count":0,"unread_count":0,"has_more":false})
    );
    let initial = call(r, Some(&viewer), "GET", &prefs, None, None).await;
    initial.expect(StatusCode::OK);
    assert_eq!(initial.body["version"], "0");
    let request = json!({"assignments":false,"mentions":true,"subscribed_comments":false});
    call(
        r,
        Some(&viewer),
        "PATCH",
        &prefs,
        Some(request.clone()),
        None,
    )
    .await
    .expect(StatusCode::PRECONDITION_REQUIRED);
    call(
        r,
        Some(&bad),
        "PATCH",
        &prefs,
        Some(request.clone()),
        Some(initial.etag()),
    )
    .await
    .expect(StatusCode::FORBIDDEN);
    let mut override_user = request.clone();
    override_user["user_id"] = json!(owner.user);
    call(
        r,
        Some(&viewer),
        "PATCH",
        &prefs,
        Some(override_user),
        Some(initial.etag()),
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let saved = call(
        r,
        Some(&viewer),
        "PATCH",
        &prefs,
        Some(request),
        Some(initial.etag()),
    )
    .await;
    saved.expect(StatusCode::OK);
    assert_eq!(saved.body["version"], "1");
    assert_eq!(
        call(r, Some(&owner), "GET", &prefs, None, None).await.body["assignments"],
        true
    );
}

#[test]
fn notification_openapi_preserves_private_commands_and_decimal_counters() {
    let doc = opentrack_api::openapi();
    let schemas = &doc["components"]["schemas"];
    for name in [
        "UpdateNotification",
        "MarkNotificationsRead",
        "UpdateNotificationPreferences",
        "NotificationQuery",
    ] {
        assert_eq!(schemas[name]["additionalProperties"], false);
    }
    for name in ["NotificationItem", "NotificationPreferences"] {
        assert_eq!(schemas[name]["properties"]["version"]["type"], "string");
    }
    assert_eq!(
        schemas["NotificationItem"]["properties"]["seq"]["type"],
        "string"
    );
    assert_eq!(
        schemas["NotificationsMarkedRead"]["properties"]["has_more"]["type"],
        "boolean"
    );
    assert_eq!(
        schemas["NotificationFilter"]["enum"],
        json!(["unread", "all", "archived", "snoozed"])
    );
    let paths = &doc["paths"];
    for (path, method) in [
        ("/api/v1/workspaces/{w}/notifications/{id}", "patch"),
        ("/api/v1/workspaces/{w}/notification-preferences", "patch"),
    ] {
        assert!(
            paths[path][method]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "If-Match" && p["required"] == true)
        );
        assert_eq!(
            paths[path][method]["security"],
            json!([{"cookieAuth":[],"csrfToken":[]},{"nativeBearer":[]}])
        );
        assert!(paths[path][method]["responses"]["200"]["headers"]["ETag"].is_object());
    }
    assert!(paths["/api/v1/workspaces/{w}/notification-preferences"]["get"]["responses"]["200"]["headers"]["ETag"].is_object());
}
