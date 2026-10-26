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
use sha2::{Digest, Sha256};
use sqlx::AssertSqlSafe;
use std::{path::PathBuf, sync::Arc};
use support::Harness;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://issue-order-ingress.contract.invalid";
const PASSWORD: &str = "Synthetic order ingress password 2026!";

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
    fn maintenance(&self, job: Uuid, team: Uuid, status: Uuid) {
        self.problem(StatusCode::CONFLICT, "issue_order_maintenance");
        let body = self.json();
        assert_eq!(body["details"]["applied"], false);
        let entries = body["details"]["maintenance"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["id"], json!(job));
        assert_eq!(entries[0]["team_id"], json!(team));
        assert_eq!(entries[0]["status_id"], json!(status));
        assert_eq!(entries[0]["state"], "queued");
        assert_eq!(entries[0]["staged_count"], "0");
        assert_eq!(entries[0]["applied_count"], "0");
        assert!(entries[0]["total_count"].is_null());
        assert!(entries[0]["etag"].is_string());
    }
}
fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}
fn version(value: &Value) -> u64 {
    value.as_str().unwrap().parse().unwrap()
}

struct Fixture {
    router: Router,
    cookie: String,
    csrf: String,
    owner: Uuid,
    workspace: Uuid,
    teams: [Uuid; 2],
    statuses: Vec<Value>,
}
impl Fixture {
    async fn new(h: &Harness) -> Self {
        let router = router(ApiState {
            app: h.app.clone(),
            attachments: None,
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(PathBuf::from("/unused-issue-order-ingress-static")),
        });
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup")
                    .header("origin", ORIGIN)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({"setup_token":h.setup_token,
                "email":"owner@example.test","display_name":"Example Owner","password":PASSWORD,
                "workspace_name":"Order ingress","team_name":"Source team","team_key":"SRC"}))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let session: Value = serde_json::from_slice(&bytes).unwrap();
        let mut fixture = Self {
            router,
            cookie,
            csrf: session["csrf_token"].as_str().unwrap().into(),
            owner: id(&session["user"]["id"]),
            workspace: id(&session["workspaces"][0]["id"]),
            teams: [Uuid::nil(); 2],
            statuses: Vec::new(),
        };
        let initial = fixture.get("/bootstrap").await.json();
        fixture.teams[0] = id(&initial["teams"][0]["id"]);
        let team = fixture
            .request(
                "POST",
                "/teams",
                Some(json!({"name":"Destination team","key":"DST","private":false})),
                &[("idempotency-key", "destination-team")],
            )
            .await;
        team.expect(StatusCode::CREATED);
        fixture.teams[1] = id(&team.json()["id"]);
        fixture.statuses = fixture.get("/bootstrap").await.json()["statuses"]
            .as_array()
            .unwrap()
            .clone();
        fixture
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Reply {
        let mut request = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}{path}", self.workspace))
            .header("origin", ORIGIN)
            .header("cookie", &self.cookie)
            .header("x-csrf-token", &self.csrf);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = self
            .router
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
    async fn get(&self, path: &str) -> Reply {
        let reply = self.request("GET", path, None, &[]).await;
        reply.expect(StatusCode::OK);
        reply
    }
    fn status(&self, team: Uuid, category: &str) -> Uuid {
        id(&self
            .statuses
            .iter()
            .find(|status| status["team_id"] == json!(team) && status["category"] == category)
            .unwrap()["id"])
    }
    async fn create(&self, team: Uuid, status: Uuid, assignee: Option<Uuid>) -> Reply {
        let key = Uuid::new_v4().to_string();
        let reply = self.request("POST","/issues",Some(json!({"team_id":team,"status_id":status,
            "title":"Ingress contract issue","description":"Original plain description","assignee_id":assignee})),&[("idempotency-key",&key)]).await;
        reply.expect(StatusCode::CREATED);
        reply
    }
    async fn order(&self, team: Uuid) -> String {
        self.get(&format!("/issues?team_id={team}&sort=manual"))
            .await
            .json()["order_version"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn position(&self, issue: Uuid, etag: &str, key: &str, body: Value) -> Reply {
        self.request(
            "POST",
            &format!("/issues/{issue}/position"),
            Some(body),
            &[("if-match", etag), ("idempotency-key", key)],
        )
        .await
    }
    async fn guard(&self, h: &Harness, team: Uuid, status: Uuid) -> Uuid {
        let job = Uuid::new_v4();
        let mut tx = h.context(self.owner, self.workspace).await;
        sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'issues.order_normalize',$3,$2::text)")
            .bind(self.workspace).bind(job).bind(self.owner).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO issue_order_normalizations(workspace_id,id,team_id,status_id,original_requested_by) VALUES($1,$2,$3,$4,$5)")
            .bind(self.workspace).bind(job).bind(team).bind(status).bind(self.owner).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO issue_order_guards(workspace_id,team_id,status_id,job_id) VALUES($1,$2,$3,$4)")
            .bind(self.workspace).bind(team).bind(status).bind(job).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        job
    }
    async fn snapshot(&self, h: &Harness, receipts: bool) -> Vec<Value> {
        let mut tx = h.context(self.owner, self.workspace).await;
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
            "issue_key_aliases",
            "issue_labels",
            "issue_relations",
            "project_issues",
            "cycle_issues",
            "issue_subscriptions",
            "subscription_events",
            "jobs",
            "issue_order_normalizations",
            "issue_order_guards",
            "issue_order_staging",
            "idempotency_records",
        ] {
            if table == "idempotency_records" && !receipts {
                continue;
            }
            let projection = if table == "issues" {
                "to_jsonb(x)-'rank'||jsonb_build_object('rank',x.rank::text)"
            } else {
                "to_jsonb(x)"
            };
            result.push(sqlx::query_scalar(AssertSqlSafe(format!("SELECT coalesce(jsonb_agg(data ORDER BY data::text),'[]'::jsonb) FROM (SELECT {projection} data FROM {table} x WHERE workspace_id=$1) rows")))
                .bind(self.workspace).fetch_one(&mut *tx).await.unwrap());
        }
        tx.commit().await.unwrap();
        result
    }
    async fn receipt(&self, h: &Harness, key: &str) -> (i64, i64) {
        let mut tx = h.context(self.owner, self.workspace).await;
        let counts = sqlx::query_as("SELECT count(*),count(response_body) FROM idempotency_records WHERE workspace_id=$1 AND principal_id=$2 AND key_hash=$3")
            .bind(self.workspace).bind(self.owner).bind(format!("{:x}",Sha256::digest(key.as_bytes()))).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        counts
    }
    async fn move_body(&self, issue: Uuid, destination: Uuid, status: Uuid) -> Value {
        let preview = self
            .get(&format!(
                "/issues/{issue}/move-preview?destination_team_id={destination}"
            ))
            .await
            .json();
        assert!(preview["blockers"].as_array().unwrap().is_empty());
        json!({"destination_team_id":destination,"status_id":status,"assignee_id":null,
            "label_ids":[],"preview_token":preview["preview_token"]})
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn team_move_checks_both_guarded_groups_without_transfer_effects() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let [source_team, destination_team] = f.teams;
    let source_status = f.status(source_team, "backlog");
    let destination_status = f.status(destination_team, "backlog");
    let outgoing = f.create(source_team, source_status, None).await;
    let incoming = f.create(destination_team, destination_status, None).await;
    let outgoing_id = id(&outgoing.json()["id"]);
    let incoming_id = id(&incoming.json()["id"]);
    let job = f.guard(&h, source_team, source_status).await;
    for (issue, etag, destination, status, key) in [
        (
            outgoing_id,
            outgoing.etag(),
            destination_team,
            destination_status,
            "move-from-active-group",
        ),
        (
            incoming_id,
            incoming.etag(),
            source_team,
            source_status,
            "move-into-active-group",
        ),
    ] {
        let body = f.move_body(issue, destination, status).await;
        let before = f.snapshot(&h, true).await;
        f.request(
            "POST",
            &format!("/issues/{issue}/move"),
            Some(body),
            &[("if-match", etag), ("idempotency-key", key)],
        )
        .await
        .maintenance(job, source_team, source_status);
        assert_eq!(f.snapshot(&h, true).await, before);
        assert_eq!(f.receipt(&h, key).await, (0, 0));
    }
    let done = f.status(source_team, "completed");
    let body = f.move_body(incoming_id, source_team, done).await;
    let moved = f
        .request(
            "POST",
            &format!("/issues/{incoming_id}/move"),
            Some(body),
            &[
                ("if-match", incoming.etag()),
                ("idempotency-key", "move-to-unfrozen-group"),
            ],
        )
        .await;
    moved.expect(StatusCode::OK);
    assert_eq!(moved.json()["id"], json!(incoming_id));
    assert_eq!(moved.json()["team_id"], json!(source_team));
    assert_eq!(moved.json()["status_id"], json!(done));
    assert_eq!(
        version(&moved.json()["version"]),
        version(&incoming.json()["version"]) + 1
    );
    let progress = f
        .get(&format!(
            "/teams/{source_team}/issue-order-maintenance/{job}"
        ))
        .await
        .json();
    assert_eq!(progress["state"], "queued");
    assert_eq!(progress["staged_count"], "0");
    h.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_same_status_noop_preserves_history_and_reopening_requires_assignee_repair() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let member = Uuid::new_v4();
    let team = f.teams[0];
    let mut tx = h.context(f.owner, f.workspace).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,'assignee@example.test','Example Assignee',password_hash FROM users WHERE id=$2")
        .bind(member).bind(f.owner).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'member')")
        .bind(f.workspace)
        .bind(member)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut terminal = Vec::new();
    for category in ["completed", "canceled"] {
        let status = f.status(team, category);
        let issue = f.create(team, status, Some(member)).await;
        terminal.push((category, status, id(&issue.json()["id"])));
    }
    let mut tx = h.context(f.owner, f.workspace).await;
    sqlx::query("UPDATE workspace_members SET role='viewer' WHERE workspace_id=$1 AND user_id=$2")
        .bind(f.workspace)
        .bind(member)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for (category, status, issue) in terminal {
        let current = f.get(&format!("/issues/{issue}")).await;
        assert_eq!(current.json()["assignee_id"], json!(member));
        assert_eq!(current.json()["assignee"]["assignable"], false);
        let order = f.order(team).await;
        let before = f.snapshot(&h, false).await;
        let noop_key = format!("same-terminal-status-{category}");
        let noop = f
            .position(
                issue,
                current.etag(),
                &noop_key,
                json!({"order_version":order,"status_id":status}),
            )
            .await;
        noop.expect(StatusCode::OK);
        assert_eq!(noop.json()["issue"], current.json());
        assert_eq!(noop.json()["order_version"], order);
        assert_eq!(f.snapshot(&h, false).await, before);
        assert_eq!(f.receipt(&h, &noop_key).await, (1, 1));
        let before = f.snapshot(&h, true).await;
        let denied_key = format!("reopen-ineligible-{category}");
        f.position(
            issue,
            current.etag(),
            &denied_key,
            json!({"order_version":order,"status_id":f.status(team,"backlog")}),
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
        assert_eq!(f.snapshot(&h, true).await, before);
        assert_eq!(f.receipt(&h, &denied_key).await, (0, 0));
        let document = f.get(&format!("/issues/{issue}/document")).await.json();
        let repaired = f
            .request(
                "PATCH",
                &format!("/issues/{issue}"),
                Some(json!({"assignee_id":null})),
                &[("if-match", current.etag())],
            )
            .await;
        repaired.expect(StatusCode::OK);
        let order = f.order(team).await;
        let reopened = f
            .position(
                issue,
                repaired.etag(),
                &format!("reopen-repaired-{category}"),
                json!({"order_version":order,"status_id":f.status(team,"backlog")}),
            )
            .await;
        reopened.expect(StatusCode::OK);
        assert_eq!(
            reopened.json()["issue"]["status_id"],
            json!(f.status(team, "backlog"))
        );
        assert!(reopened.json()["issue"]["assignee_id"].is_null());
        assert_eq!(
            version(&reopened.json()["issue"]["version"]),
            version(&repaired.json()["version"]) + 1
        );
        assert_eq!(
            f.get(&format!("/issues/{issue}/document")).await.json(),
            document
        );
    }
    h.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compound_document_status_rejection_is_atomic_and_same_status_text_remains_writable() {
    let h = Harness::new().await;
    let f = Fixture::new(&h).await;
    let team = f.teams[0];
    let open = f.status(team, "backlog");
    let original = f.create(team, open, None).await;
    let issue = id(&original.json()["id"]);
    let original_document = f.get(&format!("/issues/{issue}/document")).await.json();
    let document = id(&original_document["id"]);
    let job = f.guard(&h, team, open).await;
    let before = f.snapshot(&h, true).await;
    for body in [
        json!({"description":"Unapplied transition text","status_id":f.status(team,"completed")}),
        json!({"description":"Unapplied archive text","archived":true}),
    ] {
        f.request(
            "PATCH",
            &format!("/issues/{issue}"),
            Some(body),
            &[("if-match", original.etag())],
        )
        .await
        .maintenance(job, team, open);
        assert_eq!(f.snapshot(&h, true).await, before);
        assert_eq!(
            f.get(&format!("/issues/{issue}/document")).await.json(),
            original_document
        );
    }
    let mut tx = h.context(f.owner, f.workspace).await;
    let prior_head: i64 =
        sqlx::query_scalar("SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1")
            .bind(f.workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let prior_revisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_revisions WHERE workspace_id=$1 AND document_id=$2",
    )
    .bind(f.workspace)
    .bind(document)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let prior_activity: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_entries WHERE workspace_id=$1 AND entity_id=$2",
    )
    .bind(f.workspace)
    .bind(issue)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let text = "Allowed same-status description";
    let saved = f
        .request(
            "PATCH",
            &format!("/issues/{issue}"),
            Some(json!({"description":text,"status_id":open})),
            &[("if-match", original.etag())],
        )
        .await;
    saved.expect(StatusCode::OK);
    assert_eq!(saved.json()["status_id"], json!(open));
    assert_eq!(
        version(&saved.json()["version"]),
        version(&original.json()["version"]) + 1
    );
    let saved_document = f.get(&format!("/issues/{issue}/document")).await.json();
    assert_eq!(
        version(&saved_document["version"]),
        version(&original_document["version"]) + 1
    );
    assert_eq!(
        saved_document["body"],
        json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]})
    );
    let mut tx = h.context(f.owner, f.workspace).await;
    let revisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_revisions WHERE workspace_id=$1 AND document_id=$2",
    )
    .bind(f.workspace)
    .bind(document)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(revisions, prior_revisions + 1);
    let activity: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_entries WHERE workspace_id=$1 AND entity_id=$2",
    )
    .bind(f.workspace)
    .bind(issue)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(activity, prior_activity + 1);
    let events: Vec<(String,Uuid,Uuid,Uuid)> = sqlx::query_as("SELECT event_type,entity_id,actor_id,mutation_id FROM workspace_events WHERE workspace_id=$1 AND seq>$2 ORDER BY event_type")
        .bind(f.workspace).bind(prior_head).fetch_all(&mut *tx).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        (&events[0].0, events[0].1),
        (&"document.updated".to_string(), document)
    );
    assert_eq!(
        (&events[1].0, events[1].1),
        (&"issue.updated".to_string(), issue)
    );
    assert!(
        events
            .iter()
            .all(|event| event.2 == f.owner && event.3 == events[0].3)
    );
    let head: i64 =
        sqlx::query_scalar("SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1")
            .bind(f.workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(head, prior_head + 2);
    let guard: (String,i64,i64) = sqlx::query_as("SELECT j.state,n.staged_count,n.applied_count FROM issue_order_guards g JOIN jobs j ON j.workspace_id=g.workspace_id AND j.id=g.job_id JOIN issue_order_normalizations n ON n.workspace_id=g.workspace_id AND n.id=g.job_id WHERE g.workspace_id=$1 AND g.job_id=$2")
        .bind(f.workspace).bind(job).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(guard, ("queued".into(), 0, 0));
    tx.commit().await.unwrap();
    h.cleanup().await;
}
