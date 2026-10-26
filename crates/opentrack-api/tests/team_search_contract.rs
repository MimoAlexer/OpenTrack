#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use opentrack_domain::DirectoryQuery;
use serde_json::{Value, json};
use support::{Harness, ORIGIN};
use tower::ServiceExt;
use url::form_urlencoded;
use uuid::Uuid;

const PASSWORD: &str = "Team search verification passphrase 2026!";
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
        }
    }
}
async fn request(
    router: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    etag: Option<&str>,
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(client) = client {
        request = request
            .header("cookie", &client.cookie)
            .header("x-csrf-token", &client.csrf);
    }
    if method == "POST" {
        request = request.header("idempotency-key", Uuid::new_v4().to_string());
    }
    if let Some(etag) = etag {
        request = request.header("if-match", etag);
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            request
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
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Reply {
        status,
        headers,
        body,
    }
}
fn query(base: &str, term: &str, limit: u32, state: &str, cursor: Option<&str>) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("q", term)
        .append_pair("limit", &limit.to_string())
        .append_pair("state", state);
    if let Some(cursor) = cursor {
        query.append_pair("cursor", cursor);
    }
    format!("{base}/teams?{}", query.finish())
}
async fn team(
    router: &Router,
    client: &Client,
    base: &str,
    name: &str,
    key: &str,
    private: bool,
) -> Reply {
    let reply = request(
        router,
        Some(client),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":name,"key":key,"private":private})),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED);
    reply
}
async fn list(router: &Router, client: &Client, path: &str) -> Value {
    let reply = request(router, Some(client), "GET", path, None, None).await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.body
}
async fn invalid(router: &Router, client: &Client, path: &str) {
    let reply = request(router, Some(client), "GET", path, None, None).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.body["code"], "validation_failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_team_search_preserves_visibility_and_signed_cursor_context() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    assert!(
        serde_json::from_value::<DirectoryQuery>(json!({"q":"not a generic directory field"}))
            .is_err()
    );
    let setup = request(&h.router, None, "POST", "/api/v1/setup", Some(json!({
        "setup_token":h.setup_token,"email":"team-owner@example.test","password":PASSWORD,"display_name":"Example Owner",
        "workspace_name":"Team discovery contracts","team_name":"Engineering","team_key":"ENG"
    })), None).await;
    assert_eq!(setup.status, StatusCode::CREATED);
    let owner = setup.client();
    let workspace = setup.body["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Team discovery contracts")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let base = format!("/api/v1/workspaces/{workspace}");
    let alpha = team(&h.router, &owner, &base, "Needle Alpha", "NDA", false).await;
    team(&h.router, &owner, &base, "Needle Beta", "NDB", false).await;
    let literal = team(&h.router, &owner, &base, "Release 100%_ready", "LIT", false).await;
    let unicode = "\u{1f680}".repeat(100);
    let unicode_team = team(&h.router, &owner, &base, &unicode, "UNI", false).await;
    let invited = request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":"team-admin@example.test","role":"admin","team_ids":[]})),
        None,
    )
    .await;
    assert_eq!(invited.status, StatusCode::CREATED);
    let accepted = request(&h.router, None, "POST", "/api/v1/invitations/accept", Some(json!({"token":invited.body["token"],"display_name":"Example Administrator","password":PASSWORD})), None).await;
    assert_eq!(accepted.status, StatusCode::OK);
    let admin = accepted.client();
    let private = team(&h.router, &admin, &base, "Needle Private", "NDP", true).await;
    let hidden = request(
        &h.router,
        Some(&owner),
        "GET",
        &format!("{base}/teams/{}", private.body["id"].as_str().unwrap()),
        None,
        None,
    )
    .await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    let owner_matches = list(
        &h.router,
        &owner,
        &query(&base, "nEeDlE", 20, "active", None),
    )
    .await;
    assert_eq!(owner_matches["data"].as_array().unwrap().len(), 2);
    assert!(
        owner_matches["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != private.body["id"])
    );
    let default_page = list(&h.router, &owner, &format!("{base}/teams")).await;
    assert_eq!(default_page["data"].as_array().unwrap().len(), 5);
    assert_eq!(default_page["has_more"], false);
    assert!(default_page["next_cursor"].is_null());
    assert!(
        default_page["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != private.body["id"])
    );
    assert_eq!(
        list(
            &h.router,
            &admin,
            &query(&base, "needle", 20, "active", None)
        )
        .await["data"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        list(&h.router, &owner, &query(&base, "nda", 20, "active", None)).await["data"][0]["id"],
        alpha.body["id"]
    );
    assert_eq!(
        list(&h.router, &owner, &query(&base, "%_", 20, "active", None)).await["data"][0]["id"],
        literal.body["id"]
    );
    assert_eq!(
        list(
            &h.router,
            &owner,
            &query(&base, &unicode, 20, "active", None)
        )
        .await["data"][0]["id"],
        unicode_team.body["id"]
    );
    invalid(
        &h.router,
        &owner,
        &query(&base, &(unicode + "\u{1f680}"), 20, "active", None),
    )
    .await;
    invalid(
        &h.router,
        &owner,
        &query(&base, "invalid\0query", 20, "active", None),
    )
    .await;
    invalid(
        &h.router,
        &owner,
        &query(&base, "needle", 0, "active", None),
    )
    .await;
    invalid(
        &h.router,
        &owner,
        &query(&base, "needle", 101, "active", None),
    )
    .await;
    invalid(
        &h.router,
        &owner,
        &query(&base, "needle", 20, "unknown", None),
    )
    .await;

    let first = list(
        &h.router,
        &owner,
        &query(&base, "needle", 1, "active", None),
    )
    .await;
    assert_eq!(first["data"].as_array().unwrap().len(), 1);
    assert_eq!(first["has_more"], true);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = list(
        &h.router,
        &owner,
        &query(&base, "  needle  ", 1, "active", Some(cursor)),
    )
    .await;
    assert_eq!(second["data"].as_array().unwrap().len(), 1);
    assert_eq!(second["has_more"], false);
    assert_ne!(first["data"][0]["id"], second["data"][0]["id"]);
    for path in [
        query(&base, "different", 1, "active", Some(cursor)),
        query(&base, "needle", 2, "active", Some(cursor)),
        query(&base, "needle", 1, "all", Some(cursor)),
    ] {
        invalid(&h.router, &owner, &path).await;
    }
    invalid(
        &h.router,
        &admin,
        &query(&base, "needle", 1, "active", Some(cursor)),
    )
    .await;
    let mut tampered = cursor.to_owned();
    let last = tampered.pop().unwrap();
    tampered.push(if last == '0' { '1' } else { '0' });
    invalid(
        &h.router,
        &owner,
        &query(&base, "needle", 1, "active", Some(&tampered)),
    )
    .await;
    invalid(
        &h.router,
        &owner,
        &query(
            &base,
            "needle",
            1,
            "active",
            Some("00000000-0000-4000-8000-000000000001|unsigned"),
        ),
    )
    .await;
    let organizations = request(
        &h.router,
        Some(&owner),
        "GET",
        "/api/v1/organizations",
        None,
        None,
    )
    .await;
    let other = request(&h.router, Some(&owner), "POST", "/api/v1/workspaces", Some(json!({"organization_id":organizations.body[0]["id"],"name":"Second discovery scope","team_name":"Other team","team_key":"OTH"})), None).await;
    assert_eq!(other.status, StatusCode::CREATED);
    invalid(
        &h.router,
        &owner,
        &query(
            &format!("/api/v1/workspaces/{}", other.body["id"].as_str().unwrap()),
            "needle",
            1,
            "active",
            Some(cursor),
        ),
    )
    .await;

    let alpha_path = format!("{base}/teams/{}", alpha.body["id"].as_str().unwrap());
    assert_eq!(
        request(&h.router, Some(&admin), "GET", &alpha_path, None, None)
            .await
            .status,
        StatusCode::OK
    );
    let changed = request(
        &h.router,
        Some(&owner),
        "PATCH",
        &alpha_path,
        Some(json!({"private":true})),
        Some(alpha.headers["etag"].to_str().unwrap()),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    invalid(
        &h.router,
        &owner,
        &query(&base, "needle", 1, "active", Some(cursor)),
    )
    .await;
    assert_eq!(
        request(&h.router, Some(&admin), "GET", &alpha_path, None, None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let remaining = list(
        &h.router,
        &admin,
        &query(&base, "needle", 20, "active", None),
    )
    .await;
    assert!(
        remaining["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != alpha.body["id"])
    );
    let archived = request(
        &h.router,
        Some(&owner),
        "PATCH",
        &alpha_path,
        Some(json!({"archived":true})),
        Some(changed.headers["etag"].to_str().unwrap()),
    )
    .await;
    assert_eq!(archived.status, StatusCode::OK);
    assert_eq!(
        list(&h.router, &owner, &query(&base, "nda", 20, "active", None)).await["data"],
        json!([])
    );
    assert_eq!(
        list(
            &h.router,
            &owner,
            &query(&base, "nda", 20, "archived", None)
        )
        .await["data"][0]["id"],
        alpha.body["id"]
    );
}
