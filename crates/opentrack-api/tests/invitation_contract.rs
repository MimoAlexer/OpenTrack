//! Real HTTP invitation replacement integrity with restricted PostgreSQL access.
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use opentrack_api::{ApiState, router};
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use std::{path::PathBuf, sync::Arc};
use support::Harness;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://invitation.contract.invalid";
const PASSWORD: &str = "HTTP invitation integrity password 842!";
const RETIRED: &str = "invitation.expired_replaced";

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("Expected JSON response")
    }
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).unwrap().to_str().unwrap()
    }
    fn status(&self, expected: StatusCode) {
        assert_eq!(self.status, expected);
    }
    fn problem(&self, status: StatusCode, code: &str) {
        self.status(status);
        assert_eq!(self.header("content-type"), "application/problem+json");
        let body = self.json();
        assert_eq!(body["code"], code);
        assert_eq!(body["status"], status.as_u16());
        assert_eq!(body["request_id"], self.header("x-request-id"));
        assert_eq!(body["type"], format!("/api/v1/problems/{code}"));
        assert!(!self.headers.contains_key("location"));
        assert!(!self.headers.contains_key("set-cookie"));
        assert!(!self.headers.contains_key("x-opentrack-session-invalidated"));
    }
    fn excludes(&self, values: &[&str]) {
        let body = String::from_utf8_lossy(&self.body);
        for value in values {
            assert!(
                !body.contains(value),
                "Response must not expose hidden identifiers or credentials"
            );
        }
    }
}
struct Client {
    id: Uuid,
    cookie: String,
    csrf: String,
}
impl Client {
    fn from(reply: &Reply) -> Self {
        let body = reply.json();
        let cookie = reply.header("set-cookie");
        for flag in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
            assert!(cookie.contains(flag));
        }
        Self {
            id: Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap(),
            cookie: cookie.split(';').next().unwrap().to_owned(),
            csrf: body["csrf_token"].as_str().unwrap().to_owned(),
        }
    }
    fn headers(&self) -> Vec<(&str, &str)> {
        vec![
            ("origin", ORIGIN),
            ("cookie", &self.cookie),
            ("x-csrf-token", &self.csrf),
        ]
    }
}
struct Issued {
    id: Uuid,
    metadata: Value,
    token: String,
    etag: String,
    location: String,
}
impl Issued {
    fn from(reply: &Reply) -> Self {
        reply.status(StatusCode::CREATED);
        let value = reply.json();
        let metadata = value["invitation"].clone();
        let id = Uuid::parse_str(metadata["id"].as_str().unwrap()).unwrap();
        let etag = reply.header("etag").to_owned();
        assert_eq!(
            etag,
            format!(
                "\"invitation:{id}:{}\"",
                metadata["version"].as_str().unwrap()
            )
        );
        Self {
            id,
            metadata,
            token: value["token"].as_str().unwrap().to_owned(),
            etag,
            location: reply.header("location").to_owned(),
        }
    }
}
fn make_router(h: &Harness) -> Router {
    router(ApiState {
        attachments: None,
        app: h.app.clone(),
        origins: Arc::new(vec![ORIGIN.into()]),
        secure_cookie: true,
        static_dir: Arc::new(PathBuf::from("/unused-invitation-contract-static")),
    })
}
async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    for (key, value) in headers {
        builder = builder.header(*key, *value);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let bytes = body
        .map(|v| serde_json::to_vec(&v).unwrap())
        .unwrap_or_default();
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
    Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).unwrap();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    let reply = Reply {
        status,
        headers,
        body,
    };
    reply.excludes(&[PASSWORD]);
    reply
}
async fn create(router: &Router, owner: &Client, w: Uuid, key: &str, body: Value) -> Reply {
    let mut headers = owner.headers();
    headers.push(("idempotency-key", key));
    request(
        router,
        "POST",
        &format!("/api/v1/workspaces/{w}/invitations"),
        Some(body),
        &headers,
    )
    .await
}
fn invitation(email: &str, role: &str, teams: &[Uuid]) -> Value {
    json!({"email":email,"role":role,"team_ids":teams})
}
async fn expire(h: &Harness, actor: Uuid, w: Uuid, id: Uuid) {
    let mut tx = h.context(actor, w).await;
    sqlx::query("UPDATE invitations SET expires_at=clock_timestamp()-interval '1 second' WHERE workspace_id=$1 AND id=$2")
        .bind(w).bind(id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn snapshot(h: &Harness, actor: Uuid, w: Uuid, old: Uuid) -> Value {
    let mut tx = h.context(actor, w).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object('old',(SELECT jsonb_build_object('version',version,'revoked_at',revoked_at,'accepted_at',accepted_at) FROM invitations WHERE workspace_id=$1 AND id=$2),'invites',(SELECT count(*) FROM invitations WHERE workspace_id=$1),'audit',(SELECT count(*) FROM audit_entries WHERE workspace_id=$1),'events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1),'retries',(SELECT count(*) FROM idempotency_records WHERE workspace_id=$1))")
        .bind(w).bind(old).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}
async fn retirement(h: &Harness, actor: Uuid, w: Uuid, old: Uuid) -> (i64, i64) {
    let mut tx = h.context(actor, w).await;
    let result = sqlx::query_as("SELECT (SELECT count(*) FROM audit_entries WHERE workspace_id=$1 AND target_id=$2 AND action=$3 AND actor_id=$4 AND actor_kind='account'),(SELECT count(*) FROM workspace_events WHERE workspace_id=$1 AND entity_id=$2)")
        .bind(w).bind(old).bind(RETIRED).bind(actor).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}
async fn invalid_token(router: &Router, token: &str) {
    let headers = [("origin", ORIGIN)];
    for (path, body) in [
        ("/api/v1/invitations/preview", json!({"token":token})),
        (
            "/api/v1/invitations/accept",
            json!({"token":token,"display_name":"Must not be created","password":PASSWORD}),
        ),
    ] {
        let reply = request(router, "POST", path, Some(body), &headers).await;
        reply.problem(StatusCode::NOT_FOUND, "not_found");
        reply.excludes(&[token]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_invitation_integrity_contract() {
    let h = Harness::new().await;
    let runner = h.clone();
    let outcome = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let router = make_router(h);
    let setup = request(&router, "POST", "/api/v1/setup", Some(json!({"setup_token":h.setup_token,
        "email":"invitation-owner@contract.invalid","password":PASSWORD,"display_name":"Invitation owner",
        "workspace_name":"Invitation contracts","team_name":"Engineering","team_key":"ENG"})), &[("origin", ORIGIN)]).await;
    setup.status(StatusCode::CREATED);
    let owner = Client::from(&setup);
    let w = Uuid::parse_str(setup.json()["workspaces"][0]["id"].as_str().unwrap()).unwrap();
    normal_replacement(h, &router, &owner, w).await;
    hidden_replacement(h, &router, &owner, w).await;
    rollback_replacement(h, &router, &owner, w).await;
}
async fn normal_replacement(h: &Harness, router: &Router, owner: &Client, w: Uuid) {
    let body = invitation("replacement@contract.invalid", "member", &[]);
    let old = Issued::from(&create(router, owner, w, "normal-initial", body.clone()).await);
    expire(h, owner.id, w, old.id).await;
    invalid_token(router, &old.token).await;
    let before = snapshot(h, owner.id, w, old.id).await;
    let (_, old_events) = retirement(h, owner.id, w, old.id).await;
    let reply = create(router, owner, w, "normal-replacement", body.clone()).await;
    let replacement = Issued::from(&reply);
    assert_ne!(replacement.id, old.id);
    reply.excludes(&[&old.id.to_string(), &old.token]);
    let current = request(router, "GET", &replacement.location, None, &owner.headers()).await;
    current.status(StatusCode::OK);
    assert_eq!(current.json(), replacement.metadata);
    assert_eq!(current.header("etag"), replacement.etag);
    let after = snapshot(h, owner.id, w, old.id).await;
    assert_eq!(
        after["old"]["version"].as_i64().unwrap(),
        before["old"]["version"].as_i64().unwrap() + 1
    );
    assert!(after["old"]["revoked_at"].is_string());
    assert_eq!(retirement(h, owner.id, w, old.id).await, (1, old_events));

    let replay = create(router, owner, w, "normal-replacement", body.clone()).await;
    replay.status(StatusCode::CREATED);
    assert_eq!(replay.json()["invitation"], replacement.metadata);
    assert!(replay.json()["token"].is_null());
    replay.excludes(&[&replacement.token, &old.token]);
    assert_eq!(replay.header("etag"), replacement.etag);
    assert_eq!(replay.header("location"), replacement.location);
    assert_eq!(snapshot(h, owner.id, w, old.id).await, after);
    let conflict = create(router, owner, w, "normal-active-conflict", body).await;
    conflict.problem(StatusCode::CONFLICT, "conflict");
    conflict.excludes(&[&old.token, &replacement.token]);
    assert_eq!(snapshot(h, owner.id, w, old.id).await, after);
    invalid_token(router, &old.token).await;
    let accepted = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":replacement.token,
        "display_name":"Replacement member","password":PASSWORD})),
        &[("origin", ORIGIN)],
    )
    .await;
    accepted.status(StatusCode::OK);
    Client::from(&accepted);
    invalid_token(router, &replacement.token).await;
}
async fn hidden_replacement(h: &Harness, router: &Router, owner: &Client, w: Uuid) {
    let admin_invite = Issued::from(
        &create(
            router,
            owner,
            w,
            "private-admin-invite",
            invitation("private-admin@contract.invalid", "admin", &[]),
        )
        .await,
    );
    let accepted = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":admin_invite.token,
        "display_name":"Private team administrator","password":PASSWORD})),
        &[("origin", ORIGIN)],
    )
    .await;
    accepted.status(StatusCode::OK);
    let admin = Client::from(&accepted);
    let mut headers = admin.headers();
    headers.push(("idempotency-key", "private-team-create"));
    let created = request(
        router,
        "POST",
        &format!("/api/v1/workspaces/{w}/teams"),
        Some(json!({"name":"Hidden invitation team","key":"HID","private":true})),
        &headers,
    )
    .await;
    created.status(StatusCode::CREATED);
    let private = Uuid::parse_str(created.json()["id"].as_str().unwrap()).unwrap();
    let old = Issued::from(
        &create(
            router,
            &admin,
            w,
            "private-old-invitation",
            invitation("hidden-replacement@contract.invalid", "member", &[private]),
        )
        .await,
    );
    expire(h, admin.id, w, old.id).await;
    let hidden = request(router, "GET", &old.location, None, &owner.headers()).await;
    hidden.problem(StatusCode::NOT_FOUND, "not_found");
    hidden.excludes(&[&old.id.to_string(), &private.to_string(), &old.token]);
    let (_, old_events) = retirement(h, owner.id, w, old.id).await;
    let reply = create(
        router,
        owner,
        w,
        "public-replacement",
        invitation("hidden-replacement@contract.invalid", "viewer", &[]),
    )
    .await;
    let replacement = Issued::from(&reply);
    reply.excludes(&[
        &old.id.to_string(),
        &private.to_string(),
        &old.token,
        "Hidden invitation team",
    ]);
    assert_eq!(replacement.metadata["team_ids"], json!([]));
    let listed = request(
        router,
        "GET",
        &format!("/api/v1/workspaces/{w}/invitations?state=all"),
        None,
        &owner.headers(),
    )
    .await;
    listed.status(StatusCode::OK);
    listed.excludes(&[&old.id.to_string(), &private.to_string(), &old.token]);
    assert!(
        listed.json()["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == replacement.id.to_string())
    );
    assert_eq!(
        retirement(h, owner.id, w, old.id).await,
        (1, old_events),
        "Old private IDs remain out of newly emitted events"
    );
    let old_by_admin = request(router, "GET", &old.location, None, &admin.headers()).await;
    old_by_admin.status(StatusCode::OK);
    assert!(old_by_admin.json()["revoked_at"].is_string());
    invalid_token(router, &old.token).await;
}
async fn rollback_replacement(h: &Harness, router: &Router, owner: &Client, w: Uuid) {
    let body = invitation("rollback-replacement@contract.invalid", "member", &[]);
    let old = Issued::from(&create(router, owner, w, "rollback-initial", body.clone()).await);
    expire(h, owner.id, w, old.id).await;
    let before = snapshot(h, owner.id, w, old.id).await;
    // Fixture-only trigger definitions use fixed strings, never external input.
    for (table, predicate, behavior) in [
        (
            "audit_entries",
            "NEW.action='invitation.expired_replaced'",
            "RAISE EXCEPTION 'Contract retirement audit failure' USING ERRCODE='P0001';",
        ),
        (
            "audit_entries",
            "NEW.action='invitation.expired_replaced'",
            "RETURN NULL;",
        ),
        (
            "audit_entries",
            "NEW.action='invitation.created'",
            "RETURN NULL;",
        ),
        (
            "workspace_events",
            "NEW.event_type='invitation.created'",
            "RETURN NULL;",
        ),
    ] {
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION reject_invitation_effect() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {predicate} THEN {behavior} END IF; RETURN NEW; END $$; CREATE TRIGGER zz_contract_reject_invitation BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION reject_invitation_effect();")))
            .execute(&h.owner).await.unwrap();
        let failed = create(router, owner, w, "rollback-retry", body.clone()).await;
        failed.problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
        failed.excludes(&[
            &old.token,
            "Contract retirement audit failure",
            "reject_invitation_effect",
        ]);
        assert_eq!(
            snapshot(h, owner.id, w, old.id).await,
            before,
            "A missing required effect rolls back retirement, replacement, audit, retry and stream state"
        );
        sqlx::raw_sql(AssertSqlSafe(format!("DROP TRIGGER zz_contract_reject_invitation ON {table}; DROP FUNCTION reject_invitation_effect();")))
            .execute(&h.owner).await.unwrap();
    }
    let created = create(router, owner, w, "rollback-retry", body).await;
    Issued::from(&created);
    assert_eq!(retirement(h, owner.id, w, old.id).await.0, 1);
}
