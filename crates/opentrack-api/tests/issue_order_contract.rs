#[allow(dead_code)]
#[path = "support/mod.rs"]
mod http_support;
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use opentrack_api::{ApiState, router};
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::AssertSqlSafe;
use std::{path::PathBuf, sync::Arc};
use support::Harness;
use tower::ServiceExt;
use url::form_urlencoded;
use uuid::Uuid;

const ORIGIN: &str = "https://issue-order.contract.invalid";
const PASSWORD: &str = "Synthetic issue ordering password 2026!";

#[derive(Clone)]
struct Client {
    id: Uuid,
    cookie: String,
    csrf: String,
}
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap()
    }
    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
    #[track_caller]
    fn expect(&self, status: StatusCode) {
        assert_eq!(self.status, status);
        assert_eq!(self.headers["cache-control"], "no-store");
    }
    #[track_caller]
    fn problem(&self, status: StatusCode, code: &str) {
        self.expect(status);
        assert_eq!(self.json()["code"], code);
        assert_eq!(self.headers["content-type"], "application/problem+json");
        assert!(!self.headers.contains_key("etag"));
        assert!(!self.headers.contains_key("set-cookie"));
    }
    fn client(&self) -> Client {
        let body = self.json();
        Client {
            id: id(&body["user"]["id"]),
            cookie: self.headers["set-cookie"]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .into(),
            csrf: body["csrf_token"].as_str().unwrap().into(),
        }
    }
}
fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}
async fn request(
    router: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
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
    for (name, value) in headers {
        builder = builder.header(*name, *value);
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
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    assert!(!String::from_utf8_lossy(&bytes).contains(PASSWORD));
    Reply {
        status,
        headers,
        bytes,
    }
}
async fn get(router: &Router, client: &Client, path: &str) -> Reply {
    let reply = request(router, Some(client), "GET", path, None, &[]).await;
    reply.expect(StatusCode::OK);
    reply
}
async fn login(router: &Router, email: &str) -> Client {
    let reply = request(
        router,
        None,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":email,"password":PASSWORD})),
        &[],
    )
    .await;
    reply.expect(StatusCode::OK);
    reply.client()
}

struct Fixture {
    router: Router,
    owner: Client,
    manager: Client,
    member: Client,
    viewer: Client,
    workspace: Uuid,
    other_workspace: Uuid,
    team: Uuid,
    private_team: Uuid,
    statuses: [Uuid; 2],
    private_statuses: [Uuid; 2],
}
impl Fixture {
    async fn new(h: &Harness) -> Self {
        let router = router(ApiState {
            app: h.app.clone(),
            attachments: None,
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(PathBuf::from("/unused-issue-order-contract-static")),
        });
        let setup = request(
            &router,
            None,
            "POST",
            "/api/v1/setup",
            Some(json!({"setup_token":h.setup_token,"email":"owner@example.test",
                "display_name":"Example Owner","password":PASSWORD,
                "workspace_name":"Issue order contracts","team_name":"Engineering","team_key":"ENG"})),
            &[],
        )
        .await;
        setup.expect(StatusCode::CREATED);
        let owner = setup.client();
        let workspace = id(&setup.json()["workspaces"][0]["id"]);
        let bootstrap = get(
            &router,
            &owner,
            &format!("/api/v1/workspaces/{workspace}/bootstrap"),
        )
        .await;
        let team = id(&bootstrap.json()["teams"][0]["id"]);
        let private = request(
            &router,
            Some(&owner),
            "POST",
            &format!("/api/v1/workspaces/{workspace}/teams"),
            Some(json!({"name":"Private engineering","key":"PRV","private":true})),
            &[("idempotency-key", "private-order-team")],
        )
        .await;
        private.expect(StatusCode::CREATED);
        let private_team = id(&private.json()["id"]);
        let mut tx = h.context(owner.id, workspace).await;
        for (email, name, role, private_access) in [
            ("manager@example.test", "Example Manager", "admin", false),
            ("member@example.test", "Example Member", "member", true),
            ("viewer@example.test", "Example Viewer", "viewer", true),
        ] {
            let user = Uuid::new_v4();
            sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,$3,password_hash FROM users WHERE id=$4")
                .bind(user).bind(email).bind(name).bind(owner.id).execute(&mut *tx).await.unwrap();
            sqlx::query(
                "INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3)",
            )
            .bind(workspace)
            .bind(user)
            .bind(role)
            .execute(&mut *tx)
            .await
            .unwrap();
            if private_access {
                sqlx::query(
                    "INSERT INTO team_members(workspace_id,team_id,user_id) VALUES($1,$2,$3)",
                )
                .bind(workspace)
                .bind(private_team)
                .bind(user)
                .execute(&mut *tx)
                .await
                .unwrap();
            }
        }
        let organization: Uuid =
            sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id=$1")
                .bind(workspace)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let mut groups = Vec::new();
        for team_id in [team, private_team] {
            let statuses: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 ORDER BY position,id LIMIT 2")
                .bind(workspace).bind(team_id).fetch_all(&mut *tx).await.unwrap();
            groups.push(<[Uuid; 2]>::try_from(statuses).unwrap());
        }
        let isolated: bool = sqlx::query_scalar("SELECT NOT has_table_privilege(current_user,'issue_order_staging','SELECT,INSERT,UPDATE,DELETE')")
            .fetch_one(&h.app.pool).await.unwrap();
        assert!(isolated);
        tx.commit().await.unwrap();
        let actor = h
            .app
            .authenticate(owner.cookie.split_once('=').unwrap().1)
            .await
            .unwrap();
        let other_workspace = h.app.create_workspace(&actor, "other-order-workspace", serde_json::from_value(json!({
            "organization_id":organization,"name":"Other workspace","team_name":"Other engineering","team_key":"OTH"
        })).unwrap()).await.unwrap().id;
        Self {
            manager: login(&router, "manager@example.test").await,
            member: login(&router, "member@example.test").await,
            viewer: login(&router, "viewer@example.test").await,
            router,
            owner,
            workspace,
            other_workspace,
            team,
            private_team,
            statuses: groups[0],
            private_statuses: groups[1],
        }
    }
    fn base(&self) -> String {
        format!("/api/v1/workspaces/{}", self.workspace)
    }
    fn issue_path(&self, issue: Uuid) -> String {
        format!("{}/issues/{issue}", self.base())
    }
    fn manual_path(&self, team: Uuid) -> String {
        format!("{}/issues?team_id={team}&sort=manual", self.base())
    }
    async fn manual(&self, client: &Client, team: Uuid) -> Value {
        let body = get(&self.router, client, &self.manual_path(team))
            .await
            .json();
        let token = body["order_version"].as_str().unwrap();
        let (order, workflow) = token.split_once(':').unwrap();
        for component in [order, workflow] {
            let value: i64 = component.parse().unwrap();
            assert!(value > 0);
            assert_eq!(component, value.to_string());
        }
        body
    }
    async fn position(
        &self,
        client: &Client,
        issue: Uuid,
        etag: &str,
        key: &str,
        body: Value,
    ) -> Reply {
        request(
            &self.router,
            Some(client),
            "POST",
            &format!("{}/position", self.issue_path(issue)),
            Some(body),
            &[("if-match", etag), ("idempotency-key", key)],
        )
        .await
    }
    async fn seed(&self, h: &Harness, team: Uuid, status: Uuid, count: i32) -> Vec<Uuid> {
        let mut tx = h.context(self.owner.id, self.workspace).await;
        let start: i64 = sqlx::query_scalar(
            "SELECT next_issue_number FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(self.workspace)
        .bind(team)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let inserted = sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank) SELECT gen_random_uuid(),$1,$2,$3,$4+n-1,'Order issue '||n,$5,n::numeric*1024 FROM generate_series(1,$6::integer) n")
            .bind(self.workspace).bind(team).bind(self.owner.id).bind(start).bind(status).bind(count).execute(&mut *tx).await.unwrap();
        assert_eq!(inserted.rows_affected(), count as u64);
        let ids = sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND number>=$3 AND number<$3+$4 ORDER BY number")
            .bind(self.workspace).bind(team).bind(start).bind(i64::from(count)).fetch_all(&mut *tx).await.unwrap();
        sqlx::query("UPDATE teams SET next_issue_number=next_issue_number+$3 WHERE workspace_id=$1 AND id=$2")
            .bind(self.workspace).bind(team).bind(i64::from(count)).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        ids
    }
    async fn rank(&self, h: &Harness, issue: Uuid, rank: &str) {
        let mut tx = h.context(self.owner.id, self.workspace).await;
        sqlx::query("UPDATE issues SET rank=$3::numeric WHERE workspace_id=$1 AND id=$2")
            .bind(self.workspace)
            .bind(issue)
            .bind(rank)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    async fn snapshot(&self, h: &Harness, physical: bool) -> Vec<Value> {
        let mut tx = h.context(self.owner.id, self.workspace).await;
        let mut result = Vec::new();
        for table in [
            "issues",
            "teams",
            "documents",
            "document_revisions",
            "activity_entries",
            "audit_entries",
            "workspace_events",
            "workspace_stream_heads",
            "issue_labels",
            "issue_subscriptions",
            "subscription_events",
        ] {
            let projection = match (table, physical) {
                ("issues", false) => "to_jsonb(x)-'rank'",
                ("teams", false) => "to_jsonb(x)-'ordering_version'",
                _ => "to_jsonb(x)",
            };
            let operation_audits = if table == "audit_entries" {
                " AND NOT (x.action IN ('issue.order_maintenance_requested','issue.order_maintenance_resumed') AND EXISTS(SELECT 1 FROM issue_order_normalizations n WHERE n.workspace_id=x.workspace_id AND n.id=x.target_id))"
            } else {
                ""
            };
            result.push(sqlx::query_scalar(AssertSqlSafe(format!("SELECT coalesce(jsonb_agg(data ORDER BY data::text),'[]'::jsonb) FROM (SELECT {projection} data FROM {table} x WHERE workspace_id=$1{operation_audits}) rows")))
                .bind(self.workspace).fetch_one(&mut *tx).await.unwrap());
        }
        tx.commit().await.unwrap();
        result
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn position_rejects_rank_injection_and_enforces_source_authority_first() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let source = f.seed(&h, f.private_team, f.private_statuses[0], 1).await[0];
    let issue = get(&f.router, &f.member, &f.issue_path(source)).await;
    f.position(
        &f.member,
        source,
        issue.etag(),
        "untrusted-rank",
        json!({"order_version":"1:1","rank":"0"}),
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let manual = f.manual(&f.member, f.private_team).await;
    let body = json!({"order_version":manual["order_version"]});
    let before = f.snapshot(&h, true).await;
    for invalid in [
        json!({"order_version":"0:1"}),
        json!({"order_version":"01:1"}),
        json!({"order_version":manual["order_version"],"status_id":null}),
        json!({"order_version":manual["order_version"],"before_id":source}),
    ] {
        f.position(
            &f.member,
            source,
            issue.etag(),
            &Uuid::new_v4().to_string(),
            invalid,
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    for (client, status, code) in [
        (&f.manager, StatusCode::NOT_FOUND, "not_found"),
        (&f.viewer, StatusCode::FORBIDDEN, "forbidden"),
    ] {
        let denied = f
            .position(
                client,
                source,
                issue.etag(),
                &Uuid::new_v4().to_string(),
                body.clone(),
            )
            .await;
        denied.problem(status, code);
        assert!(!denied.json().to_string().contains(&source.to_string()));
    }
    request(
        &f.router,
        None,
        "POST",
        &format!("{}/position", f.issue_path(source)),
        Some(body.clone()),
        &[
            ("if-match", issue.etag()),
            ("idempotency-key", "anonymous-position"),
        ],
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let wrong_csrf = Client {
        csrf: "incorrect".into(),
        ..f.member.clone()
    };
    f.position(
        &wrong_csrf,
        source,
        issue.etag(),
        "invalid-csrf",
        body.clone(),
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        &f.router,
        Some(&f.member),
        "POST",
        &format!("{}/position", f.issue_path(source)),
        Some(body.clone()),
        &[("idempotency-key", "missing-precondition")],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(
        &f.router,
        Some(&f.member),
        "POST",
        &format!("{}/position", f.issue_path(source)),
        Some(body.clone()),
        &[("if-match", issue.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    request(
        &f.router,
        Some(&f.owner),
        "POST",
        &format!(
            "/api/v1/workspaces/{}/issues/{source}/position",
            f.other_workspace
        ),
        Some(body),
        &[
            ("if-match", issue.etag()),
            ("idempotency-key", "wrong-workspace"),
        ],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(f.snapshot(&h, true).await, before);
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query("UPDATE issues SET title='Changed remotely',version=version+1 WHERE workspace_id=$1 AND id=$2").bind(f.workspace).bind(source).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let stale = f
        .position(
            &f.member,
            source,
            issue.etag(),
            "source-before-generation",
            json!({"order_version":manual["order_version"],"before_id":Uuid::new_v4()}),
        )
        .await;
    stale.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(stale.json()["details"]["current"]["id"], json!(source));
    let current = get(&f.router, &f.member, &f.issue_path(source)).await;
    f.position(
        &f.member,
        source,
        current.etag(),
        "current-source-stale-order",
        json!({"order_version":manual["order_version"],"before_id":Uuid::new_v4()}),
    )
    .await
    .expect(StatusCode::CONFLICT);
    h.cleanup().await;
}

fn page_path(base: &str, team: Uuid, cursor: Option<&str>, extra: &[(&str, &str)]) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("team_id", &team.to_string())
        .append_pair("sort", "manual")
        .append_pair("limit", "2");
    if let Some(cursor) = cursor {
        query.append_pair("cursor", cursor);
    }
    for (key, value) in extra {
        query.append_pair(key, value);
    }
    format!("{base}/issues?{}", query.finish())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_pages_bind_query_principal_tenant_and_both_generations() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let issues = f.seed(&h, f.team, f.statuses[0], 5).await;
    for (issue, rank) in issues.iter().zip(["-5", "2", "2", "10", "100"]) {
        f.rank(&h, *issue, rank).await;
    }
    let first_path = page_path(&f.base(), f.team, None, &[]);
    let first = get(&f.router, &f.owner, &first_path).await.json();
    assert!(first["has_more"].as_bool().unwrap());
    let cursor = first["next_cursor"].as_str().unwrap();
    let mut actual: Vec<Uuid> = first["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| id(&row["id"]))
        .collect();
    let mut next = Some(cursor.to_string());
    for _ in 0..3 {
        let Some(token) = next.take() else {
            break;
        };
        let page = get(
            &f.router,
            &f.owner,
            &page_path(&f.base(), f.team, Some(&token), &[]),
        )
        .await
        .json();
        assert_eq!(page["order_version"], first["order_version"]);
        actual.extend(
            page["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| id(&row["id"])),
        );
        next = page["next_cursor"].as_str().map(str::to_string);
        assert_eq!(page["has_more"].as_bool().unwrap(), next.is_some());
    }
    assert!(next.is_none());
    let mut tx = h.context(f.owner.id, f.workspace).await;
    let expected: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND archived_at IS NULL AND deleted_at IS NULL ORDER BY rank,id")
        .bind(f.workspace).bind(f.team).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(actual, expected);
    let valid = page_path(&f.base(), f.team, Some(cursor), &[]);
    let changed_query = page_path(&f.base(), f.team, Some(cursor), &[("q", "different")]);
    let other_team = page_path(&f.base(), f.private_team, Some(cursor), &[]);
    let other_workspace = page_path(
        &format!("/api/v1/workspaces/{}", f.other_workspace),
        f.team,
        Some(cursor),
        &[],
    );
    for (client, path) in [
        (&f.member, valid.as_str()),
        (&f.owner, changed_query.as_str()),
        (&f.owner, other_team.as_str()),
    ] {
        request(&f.router, Some(client), "GET", path, None, &[])
            .await
            .expect(StatusCode::UNPROCESSABLE_ENTITY);
    }
    request(
        &f.router,
        Some(&f.owner),
        "GET",
        &other_workspace,
        None,
        &[],
    )
    .await
    .expect(StatusCode::NOT_FOUND);
    for path in [
        format!("{}/issues?sort=manual", f.base()),
        format!("{}&archived=true", f.manual_path(f.team)),
    ] {
        request(&f.router, Some(&f.owner), "GET", &path, None, &[])
            .await
            .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query(
        "UPDATE teams SET workflow_version=workflow_version+1 WHERE workspace_id=$1 AND id=$2",
    )
    .bind(f.workspace)
    .bind(f.team)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    request(&f.router, Some(&f.owner), "GET", &valid, None, &[])
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let refreshed = get(&f.router, &f.owner, &first_path).await.json();
    assert_ne!(refreshed["order_version"], first["order_version"]);
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query("UPDATE issues SET title='Cursor ordering changed',version=version+1 WHERE workspace_id=$1 AND id=$2").bind(f.workspace).bind(issues[0]).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    request(
        &f.router,
        Some(&f.owner),
        "GET",
        &page_path(&f.base(), f.team, refreshed["next_cursor"].as_str(), &[]),
        None,
        &[],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let refreshed = get(&f.router, &f.owner, &first_path).await.json();
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(f.workspace)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    request(
        &f.router,
        Some(&f.owner),
        "GET",
        &page_path(&f.base(), f.team, refreshed["next_cursor"].as_str(), &[]),
        None,
        &[],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    h.cleanup().await;
}

const MAX_RANK: &str = "99999999999999999999.999999999999999999";

fn row_ids(page: &Value) -> Vec<Uuid> {
    page["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| id(&row["id"]))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn position_uses_full_group_adjacency_and_reauthorizes_matching_replays() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let rows = f.seed(&h, f.team, f.statuses[0], 5).await;
    let [a, b, c, d, source]: [Uuid; 5] = rows.try_into().unwrap();
    let original = get(&f.router, &f.member, &f.issue_path(source)).await;
    let manual = f.manual(&f.member, f.team).await;
    let body = json!({"order_version":manual["order_version"],"before_id":c,"after_id":b});
    let moved = f
        .position(
            &f.member,
            source,
            original.etag(),
            "between-live-neighbors",
            body.clone(),
        )
        .await;
    moved.expect(StatusCode::OK);
    assert_eq!(moved.json()["issue"]["version"], "2");
    assert_eq!(
        row_ids(&f.manual(&f.member, f.team).await),
        vec![a, b, source, c, d]
    );
    let settled = f.snapshot(&h, true).await;
    let replay = f
        .position(
            &f.member,
            source,
            original.etag(),
            "between-live-neighbors",
            body.clone(),
        )
        .await;
    replay.expect(StatusCode::OK);
    assert_eq!(replay.json(), moved.json());
    assert_eq!(f.snapshot(&h, true).await, settled);
    let current = get(&f.router, &f.member, &f.issue_path(source)).await;
    let manual = f.manual(&f.member, f.team).await;
    let noop = f
        .position(
            &f.member,
            source,
            current.etag(),
            "source-excluded-noop",
            json!({"order_version":manual["order_version"],"before_id":c,"after_id":b}),
        )
        .await;
    noop.expect(StatusCode::OK);
    assert_eq!(noop.json()["issue"], current.json());
    assert_eq!(noop.json()["order_version"], manual["order_version"]);
    assert_eq!(f.snapshot(&h, true).await, settled);
    f.position(
        &f.member,
        source,
        current.etag(),
        "nonadjacent-live-neighbors",
        json!({"order_version":manual["order_version"],"before_id":c,"after_id":a}),
    )
    .await
    .expect(StatusCode::CONFLICT);
    assert_eq!(f.snapshot(&h, true).await, settled);
    f.position(
        &f.member,
        source,
        current.etag(),
        "between-live-neighbors",
        json!({"order_version":manual["order_version"],"before_id":d}),
    )
    .await
    .expect(StatusCode::CONFLICT);
    assert_eq!(f.snapshot(&h, true).await, settled);
    for (key, anchors, expected) in [
        (
            "successor-only",
            json!({"before_id":d}),
            vec![a, b, c, source, d],
        ),
        (
            "predecessor-only",
            json!({"after_id":a}),
            vec![a, source, b, c, d],
        ),
        (
            "append-without-anchors",
            json!({}),
            vec![a, b, c, d, source],
        ),
    ] {
        let issue = get(&f.router, &f.member, &f.issue_path(source)).await;
        let manual = f.manual(&f.member, f.team).await;
        let mut body = anchors;
        body["order_version"] = manual["order_version"].clone();
        f.position(&f.member, source, issue.etag(), key, body)
            .await
            .expect(StatusCode::OK);
        assert_eq!(row_ids(&f.manual(&f.member, f.team).await), expected);
    }
    let route = format!("issues.position:{source}");
    let key_hash = format!("{:x}", Sha256::digest(b"between-live-neighbors"));
    let mut tx = h.context(f.owner.id, f.workspace).await;
    let expired_receipt: Value = sqlx::query_scalar("UPDATE idempotency_records SET expires_at=clock_timestamp()-interval '1 second' WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4 RETURNING to_jsonb(idempotency_records)")
        .bind(f.workspace).bind(f.member.id).bind(&route).bind(&key_hash)
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(expired_receipt["response_body"], moved.json());
    tx.commit().await.unwrap();
    let before_expired_retry = f.snapshot(&h, true).await;
    let expired = f
        .position(
            &f.member,
            source,
            original.etag(),
            "between-live-neighbors",
            body,
        )
        .await;
    expired.problem(StatusCode::CONFLICT, "conflict");
    assert_ne!(expired.json()["details"]["applied"], json!(false));
    assert_eq!(f.snapshot(&h, true).await, before_expired_retry);
    let mut tx = h.context(f.owner.id, f.workspace).await;
    let retained: Value = sqlx::query_scalar("SELECT to_jsonb(r) FROM idempotency_records r WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4")
        .bind(f.workspace).bind(f.member.id).bind(&route).bind(&key_hash)
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(retained, expired_receipt);
    tx.commit().await.unwrap();
    let private = f.seed(&h, f.private_team, f.private_statuses[0], 3).await;
    let private_source = private[2];
    let issue = get(&f.router, &f.member, &f.issue_path(private_source)).await;
    let manual = f.manual(&f.member, f.private_team).await;
    let body = json!({"order_version":manual["order_version"],"before_id":private[0]});
    f.position(
        &f.member,
        private_source,
        issue.etag(),
        "private-recorded-team",
        body.clone(),
    )
    .await
    .expect(StatusCode::OK);
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query("UPDATE issues SET team_id=$3,status_id=$4,number=1000,version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(f.workspace).bind(private_source).bind(f.team).bind(f.statuses[0]).execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(f.workspace)
        .bind(f.private_team)
        .bind(f.member.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    get(&f.router, &f.member, &f.issue_path(private_source)).await;
    let settled = f.snapshot(&h, true).await;
    f.position(
        &f.member,
        private_source,
        issue.etag(),
        "private-recorded-team",
        body,
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(f.snapshot(&h, true).await, settled);
    h.cleanup().await;
}

fn maintenance(reply: &Reply) -> Value {
    reply.problem(StatusCode::CONFLICT, "issue_order_maintenance");
    let body = reply.json();
    assert_eq!(body["details"]["applied"], false);
    let values = body["details"]["maintenance"].as_array().unwrap();
    assert_eq!(values.len(), 1);
    let progress = values[0].clone();
    assert!(progress["etag"].is_string());
    for forbidden in [
        "original_requested_by",
        "requested_by",
        "lease_owner",
        "lease_token",
    ] {
        assert!(progress.get(forbidden).is_none());
    }
    progress
}
async fn receipt(h: &Harness, f: &Fixture, principal: Uuid, route: &str, key: &str) -> (i64, i64) {
    let mut tx = h.context(f.owner.id, f.workspace).await;
    let counts = sqlx::query_as("SELECT count(*),count(response_body) FROM idempotency_records WHERE workspace_id=$1 AND principal_id=$2 AND ($3='' OR route=$3) AND key_hash=$4")
        .bind(f.workspace).bind(principal).bind(route).bind(format!("{:x}",Sha256::digest(key.as_bytes()))).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    counts
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_exhaustion_commits_one_typed_intent_without_creating_an_issue() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let target = f.seed(&h, f.team, f.statuses[0], 501).await;
    f.rank(&h, *target.last().unwrap(), MAX_RANK).await;
    let before = f.snapshot(&h, true).await;
    let key = "create-order-exhaustion";
    let body = json!({"team_id":f.team,"status_id":f.statuses[0],"title":"Unapplied create","description":"This must not create a document"});
    let attempted = request(
        &f.router,
        Some(&f.member),
        "POST",
        &format!("{}/issues", f.base()),
        Some(body.clone()),
        &[("idempotency-key", key)],
    )
    .await;
    let progress = maintenance(&attempted);
    assert_eq!(progress["state"], "queued");
    assert_eq!(progress["phase"], "staging");
    assert_eq!(progress["staged_count"], "0");
    assert_eq!(progress["applied_count"], "0");
    assert!(progress["total_count"].is_null());
    assert_eq!(f.snapshot(&h, true).await, before);
    assert_eq!(
        receipt(&h, &f, f.member.id, "issues.create", key).await,
        (1, 0)
    );
    let replay = request(
        &f.router,
        Some(&f.member),
        "POST",
        &format!("{}/issues", f.base()),
        Some(body),
        &[("idempotency-key", key)],
    )
    .await;
    assert_eq!(maintenance(&replay)["id"], progress["id"]);
    assert_eq!(f.snapshot(&h, true).await, before);
    assert_eq!(
        receipt(&h, &f, f.member.id, "issues.create", key).await,
        (1, 0)
    );
    let mut tx = h.context(f.owner.id, f.workspace).await;
    let intent: (i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM jobs WHERE workspace_id=$1 AND kind='issues.order_normalize'),(SELECT count(*) FROM issue_order_normalizations WHERE workspace_id=$1),(SELECT count(*) FROM issue_order_guards WHERE workspace_id=$1),(SELECT count(*) FROM audit_entries a JOIN issue_order_normalizations n ON n.workspace_id=a.workspace_id AND n.id=a.target_id WHERE a.workspace_id=$1 AND a.action='issue.order_maintenance_requested' AND a.actor_id=$2)")
        .bind(f.workspace).bind(f.member.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(intent, (1, 1, 1, 1));
    tx.commit().await.unwrap();
    h.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_event_failure_rolls_back_position_and_same_key_retry_is_safe() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let rows = f.seed(&h, f.team, f.statuses[0], 3).await;
    let source = rows[2];
    let issue = get(&f.router, &f.member, &f.issue_path(source)).await;
    let manual = f.manual(&f.member, f.team).await;
    let body = json!({"order_version":manual["order_version"],"before_id":rows[0]});
    let before = f.snapshot(&h, true).await;
    sqlx::raw_sql("CREATE FUNCTION suppress_order_event() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_order_event BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION suppress_order_event();")
        .execute(&h.owner).await.unwrap();
    f.position(
        &f.member,
        source,
        issue.etag(),
        "suppressed-order-event",
        body.clone(),
    )
    .await
    .problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    assert_eq!(f.snapshot(&h, true).await, before);
    assert_eq!(
        receipt(&h, &f, f.member.id, "", "suppressed-order-event")
            .await
            .1,
        0
    );
    sqlx::raw_sql("DROP TRIGGER suppress_order_event ON workspace_events; DROP FUNCTION suppress_order_event();").execute(&h.owner).await.unwrap();
    let moved = f
        .position(
            &f.member,
            source,
            issue.etag(),
            "suppressed-order-event",
            body.clone(),
        )
        .await;
    moved.expect(StatusCode::OK);
    assert_eq!(moved.json()["issue"]["version"], "2");
    assert_eq!(
        receipt(&h, &f, f.member.id, "", "suppressed-order-event").await,
        (1, 1)
    );
    let settled = f.snapshot(&h, true).await;
    let replay = f
        .position(
            &f.member,
            source,
            issue.etag(),
            "suppressed-order-event",
            body,
        )
        .await;
    replay.expect(StatusCode::OK);
    assert_eq!(replay.json(), moved.json());
    assert_eq!(f.snapshot(&h, true).await, settled);
    h.cleanup().await;
}

async fn progress(f: &Fixture, client: &Client, path: &str) -> Value {
    let reply = get(&f.router, client, path).await;
    let body = reply.json();
    assert_eq!(body["etag"], reply.etag());
    for forbidden in [
        "original_requested_by",
        "requested_by",
        "lease_owner",
        "lease_token",
    ] {
        assert!(body.get(forbidden).is_none());
    }
    let unchanged = request(
        &f.router,
        Some(client),
        "GET",
        path,
        None,
        &[("if-none-match", reply.etag())],
    )
    .await;
    unchanged.expect(StatusCode::NOT_MODIFIED);
    assert_eq!(unchanged.etag(), reply.etag());
    assert!(unchanged.bytes.is_empty());
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_position_intent_survives_mid_apply_access_loss_and_manager_resume() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let target = f.seed(&h, f.private_team, f.private_statuses[0], 501).await;
    let source = f.seed(&h, f.private_team, f.private_statuses[1], 1).await[0];
    f.rank(&h, *target.last().unwrap(), MAX_RANK).await;
    let issue = get(&f.router, &f.member, &f.issue_path(source)).await;
    let manual = f.manual(&f.member, f.private_team).await;
    let before = f.snapshot(&h, true).await;
    let attempted = f
        .position(
            &f.member,
            source,
            issue.etag(),
            "large-unapplied-position",
            json!({"order_version":manual["order_version"],"status_id":f.private_statuses[0]}),
        )
        .await;
    let queued = maintenance(&attempted);
    assert_eq!(queued["state"], "queued");
    assert_eq!(queued["phase"], "staging");
    assert!(queued["total_count"].is_null());
    assert_eq!(f.snapshot(&h, true).await, before);
    let job = id(&queued["id"]);
    let path = format!(
        "{}/teams/{}/issue-order-maintenance/{job}",
        f.base(),
        f.private_team
    );
    assert_eq!(
        maintenance(
            &request(
                &f.router,
                Some(&f.member),
                "GET",
                &f.manual_path(f.private_team),
                None,
                &[]
            )
            .await
        )["id"],
        json!(job)
    );
    for sort in ["created", "priority"] {
        get(
            &f.router,
            &f.member,
            &format!("{}/issues?team_id={}&sort={sort}", f.base(), f.private_team),
        )
        .await;
    }
    let worker = Worker::connect(
        &h.worker_url().await,
        WorkerOptions {
            max_connections: 2,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap();
    let before_worker = f.snapshot(&h, false).await;
    let mut staged = 0;
    let mut applied = 0;
    let mut old_claim = None;
    for _ in 0..7 {
        let claim = worker.claim(f.workspace).await.unwrap().unwrap();
        assert_eq!(claim.id, job);
        assert_eq!(claim.kind, "issues.order_normalize");
        assert_eq!(
            worker.run_claim(&claim).await.unwrap(),
            WorkerTick::Progress
        );
        let current = progress(&f, &f.owner, &path).await;
        let next_staged: i64 = current["staged_count"].as_str().unwrap().parse().unwrap();
        let next_applied: i64 = current["applied_count"].as_str().unwrap().parse().unwrap();
        assert!((0..=100).contains(&(next_staged - staged)));
        assert!((0..=100).contains(&(next_applied - applied)));
        assert!(next_staged == staged || next_applied == applied);
        assert_eq!(
            current["total_count"].is_null(),
            current["phase"] == "staging"
        );
        staged = next_staged;
        applied = next_applied;
        old_claim = Some(claim);
    }
    assert_eq!((staged, applied), (501, 100));
    assert_eq!(f.snapshot(&h, false).await, before_worker);
    sqlx::query("UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1")
        .bind(f.member.id)
        .execute(&h.owner)
        .await
        .unwrap();
    let claim = worker.claim(f.workspace).await.unwrap().unwrap();
    assert_eq!(worker.run_claim(&claim).await.unwrap(), WorkerTick::Blocked);
    let blocked = progress(&f, &f.owner, &path).await;
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["phase"], "applying");
    assert_eq!(blocked["staged_count"], "501");
    assert_eq!(blocked["applied_count"], "100");
    assert_eq!(blocked["total_count"], "501");
    assert_eq!(blocked["error_code"], "access_lost");
    request(
        &f.router,
        Some(&f.member),
        "GET",
        &path,
        None,
        &[("if-none-match", queued["etag"].as_str().unwrap())],
    )
    .await
    .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        &f.router,
        Some(&f.manager),
        "GET",
        &path,
        None,
        &[("if-none-match", blocked["etag"].as_str().unwrap())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let viewer = progress(&f, &f.viewer, &path).await;
    assert_eq!(viewer["can_resume"], false);
    let resume = format!("{path}/resume");
    request(
        &f.router,
        Some(&f.viewer),
        "POST",
        &resume,
        None,
        &[("if-match", viewer["etag"].as_str().unwrap())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query("UPDATE workspace_members SET role='admin' WHERE workspace_id=$1 AND user_id=$2")
        .bind(f.workspace)
        .bind(f.viewer.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let promoted = login(&f.router, "viewer@example.test").await;
    let changed = request(
        &f.router,
        Some(&promoted),
        "GET",
        &path,
        None,
        &[("if-none-match", viewer["etag"].as_str().unwrap())],
    )
    .await;
    changed.expect(StatusCode::OK);
    assert_eq!(changed.json()["can_resume"], true);
    assert_ne!(changed.etag(), viewer["etag"].as_str().unwrap());
    request(
        &f.router,
        Some(&f.manager),
        "POST",
        &resume,
        None,
        &[("if-match", blocked["etag"].as_str().unwrap())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id) VALUES($1,$2,$3)")
        .bind(f.workspace)
        .bind(f.private_team)
        .bind(f.manager.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let manager = login(&f.router, "manager@example.test").await;
    let permitted = progress(&f, &manager, &path).await;
    assert_eq!(permitted["can_resume"], true);
    request(&f.router, Some(&manager), "POST", &resume, None, &[])
        .await
        .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(
        &f.router,
        Some(&manager),
        "POST",
        &resume,
        None,
        &[("if-match", queued["etag"].as_str().unwrap())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    request(
        &f.router,
        Some(&manager),
        "POST",
        &resume,
        None,
        &[("if-match", viewer["etag"].as_str().unwrap())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let resumed = request(
        &f.router,
        Some(&manager),
        "POST",
        &resume,
        None,
        &[("if-match", permitted["etag"].as_str().unwrap())],
    )
    .await;
    resumed.expect(StatusCode::OK);
    assert_eq!(resumed.json()["etag"], resumed.etag());
    assert_eq!(resumed.json()["state"], "queued");
    assert_eq!(resumed.json()["staged_count"], "501");
    assert_eq!(resumed.json()["applied_count"], "100");
    let mut tx = h.context(f.owner.id, f.workspace).await;
    let actors: (Uuid,Uuid) = sqlx::query_as("SELECT n.original_requested_by,j.requested_by FROM issue_order_normalizations n JOIN jobs j USING(workspace_id,id) WHERE n.workspace_id=$1 AND n.id=$2")
        .bind(f.workspace).bind(job).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(actors, (f.member.id, manager.id));
    let audits: Vec<(String,Uuid)> = sqlx::query_as("SELECT action,actor_id FROM audit_entries WHERE workspace_id=$1 AND target_id=$2 AND action IN('issue.order_maintenance_requested','issue.order_maintenance_resumed') ORDER BY action")
        .bind(f.workspace).bind(job).fetch_all(&mut *tx).await.unwrap();
    assert_eq!(
        audits,
        vec![
            ("issue.order_maintenance_requested".into(), f.member.id),
            ("issue.order_maintenance_resumed".into(), manager.id)
        ]
    );
    tx.commit().await.unwrap();
    let before_stale_claim = f.snapshot(&h, true).await;
    assert_eq!(
        worker.run_claim(&old_claim.unwrap()).await.unwrap(),
        WorkerTick::LostLease
    );
    assert_eq!(f.snapshot(&h, true).await, before_stale_claim);
    let before_completion = f.snapshot(&h, false).await;
    let mut completed = false;
    for _ in 0..6 {
        let claim = worker.claim(f.workspace).await.unwrap().unwrap();
        assert_eq!(claim.id, job);
        assert_eq!(claim.requested_by, Some(manager.id));
        let result = worker.run_claim(&claim).await.unwrap();
        assert!(matches!(
            result,
            WorkerTick::Progress | WorkerTick::Completed
        ));
        let current = progress(&f, &manager, &path).await;
        let next: i64 = current["applied_count"].as_str().unwrap().parse().unwrap();
        assert!((0..=100).contains(&(next - applied)));
        assert_eq!(current["staged_count"], "501");
        assert_eq!(current["total_count"], "501");
        applied = next;
        if result == WorkerTick::Completed {
            assert_eq!(current["state"], "succeeded");
            assert_eq!(current["phase"], "completed");
            completed = true;
            break;
        }
    }
    assert!(completed);
    assert_eq!(applied, 501);
    assert_eq!(f.snapshot(&h, false).await, before_completion);
    let unchanged = get(&f.router, &manager, &f.issue_path(source)).await;
    for field in [
        "id",
        "workspace_id",
        "team_id",
        "status_id",
        "version",
        "body_version",
        "title",
    ] {
        assert_eq!(unchanged.json()[field], issue.json()[field]);
    }
    let manual = f.manual(&manager, f.private_team).await;
    f.position(
        &manager,
        source,
        unchanged.etag(),
        "reviewed-after-normalization",
        json!({"order_version":manual["order_version"],"status_id":f.private_statuses[0]}),
    )
    .await
    .expect(StatusCode::OK);
    worker.pool.close().await;
    h.cleanup().await;
}

#[tokio::test]
async fn http_fixture_grants_keep_ordering_staging_private() {
    let h = http_support::Harness::new().await;
    h.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_order_maintenance_blocks_membership_changes_but_allows_text() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let rows = f.seed(&h, f.team, f.statuses[0], 3).await;
    let removed = get(&f.router, &f.owner, &f.issue_path(rows[2])).await;
    request(
        &f.router,
        Some(&f.owner),
        "DELETE",
        &f.issue_path(rows[2]),
        None,
        &[("if-match", removed.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    let restore = get(
        &f.router,
        &f.owner,
        &format!("{}/deleted", f.issue_path(rows[2])),
    )
    .await;
    let job = Uuid::new_v4();
    let mut tx = h.context(f.owner.id, f.workspace).await;
    sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'issues.order_normalize',$3,$2::text)")
        .bind(f.workspace).bind(job).bind(f.member.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issue_order_normalizations(workspace_id,id,team_id,status_id,original_requested_by) VALUES($1,$2,$3,$4,$5)")
        .bind(f.workspace).bind(job).bind(f.team).bind(f.statuses[0]).bind(f.member.id).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO issue_order_guards(workspace_id,team_id,status_id,job_id) VALUES($1,$2,$3,$4)",
    )
    .bind(f.workspace)
    .bind(f.team)
    .bind(f.statuses[0])
    .bind(job)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let issue = get(&f.router, &f.member, &f.issue_path(rows[0])).await;
    request(
        &f.router,
        Some(&f.member),
        "PATCH",
        &f.issue_path(rows[0]),
        Some(json!({"title":"Editable during normalization"})),
        &[("if-match", issue.etag())],
    )
    .await
    .expect(StatusCode::OK);
    let current = get(&f.router, &f.member, &f.issue_path(rows[0])).await;
    let before = f.snapshot(&h, true).await;
    for body in [json!({"status_id":f.statuses[1]}), json!({"archived":true})] {
        let reply = request(
            &f.router,
            Some(&f.member),
            "PATCH",
            &f.issue_path(rows[0]),
            Some(body),
            &[("if-match", current.etag())],
        )
        .await;
        assert_eq!(maintenance(&reply)["id"], json!(job));
        assert_eq!(f.snapshot(&h, true).await, before);
    }
    let owner = get(&f.router, &f.owner, &f.issue_path(rows[0])).await;
    let reply = request(
        &f.router,
        Some(&f.owner),
        "DELETE",
        &f.issue_path(rows[0]),
        None,
        &[("if-match", owner.etag())],
    )
    .await;
    assert_eq!(maintenance(&reply)["id"], json!(job));
    let reply = request(
        &f.router,
        Some(&f.owner),
        "POST",
        &format!("{}/restore", f.issue_path(rows[2])),
        Some(json!({"status_id":f.statuses[0],"assignee_id":null,"label_ids":[]})),
        &[("if-match", restore.etag())],
    )
    .await;
    assert_eq!(maintenance(&reply)["id"], json!(job));
    assert_eq!(f.snapshot(&h, true).await, before);
    h.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn five_hundred_rows_normalize_inline_without_fake_issue_history() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let target = f.seed(&h, f.team, f.statuses[0], 500).await;
    let source = f.seed(&h, f.team, f.statuses[1], 1).await[0];
    f.rank(&h, *target.last().unwrap(), MAX_RANK).await;
    let before = f.snapshot(&h, false).await;
    let issue = get(&f.router, &f.member, &f.issue_path(source)).await;
    let manual = f.manual(&f.member, f.team).await;
    let moved = f
        .position(
            &f.member,
            source,
            issue.etag(),
            "inline-five-hundred",
            json!({"order_version":manual["order_version"],"status_id":f.statuses[0]}),
        )
        .await;
    moved.expect(StatusCode::OK);
    assert_eq!(moved.json()["issue"]["status_id"], json!(f.statuses[0]));
    assert_eq!(moved.json()["issue"]["version"], "2");
    assert_ne!(moved.json()["order_version"], manual["order_version"]);
    let after = f.snapshot(&h, false).await;
    let old: Vec<_> = before[0]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["id"] != json!(source))
        .cloned()
        .collect();
    let new: Vec<_> = after[0]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["id"] != json!(source))
        .cloned()
        .collect();
    assert_eq!(new, old);
    let mut tx = h.context(f.owner.id, f.workspace).await;
    let untouched: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM activity_entries WHERE workspace_id=$1 AND entity_id=ANY($2)) AND NOT EXISTS(SELECT 1 FROM workspace_events WHERE workspace_id=$1 AND entity_id=ANY($2))")
        .bind(f.workspace).bind(&target).fetch_one(&mut *tx).await.unwrap();
    assert!(untouched);
    let jobs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM issue_order_normalizations WHERE workspace_id=$1")
            .bind(f.workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(jobs, 0);
    tx.commit().await.unwrap();
    h.cleanup().await;
}
