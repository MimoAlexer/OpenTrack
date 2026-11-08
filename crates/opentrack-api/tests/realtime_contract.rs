//! Durable replay and authorization contracts through PostgreSQL and real HTTP.
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use futures_util::StreamExt;
use opentrack_store as store;
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use support::{Harness, ORIGIN};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "Realtime contract verification password 739!";
const DEADLINE: Duration = Duration::from_secs(12);

#[derive(Clone)]
struct Client {
    cookie: String,
    csrf: String,
    actor: Uuid,
}

#[derive(Clone)]
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
    path: String,
}

impl Reply {
    fn expect(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{}: {}", self.path, self.body);
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
            actor: id(&self.body["user"]["id"]),
        }
    }
}

fn id(value: &Value) -> Uuid {
    value.as_str().unwrap().parse().unwrap()
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
    if method == "POST" {
        builder = builder.header("idempotency-key", Uuid::new_v4().to_string());
    }
    for (name, value) in extra {
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
    assert_eq!(headers["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        path: path.to_owned(),
        body: if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    }
}

struct Fixture {
    owner: Client,
    workspace: Uuid,
    team: Uuid,
    base: String,
}

async fn setup(h: &Harness) -> Fixture {
    let reply = request(
        &h.router,
        None,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "setup_token": h.setup_token, "email":"realtime-owner@example.test",
            "password":PASSWORD, "display_name":"Example Owner",
            "workspace_name":"Realtime contracts", "team_name":"Engineering", "team_key":"ENG"
        })),
        &[],
    )
    .await;
    reply.expect(StatusCode::CREATED);
    let owner = reply.client();
    let workspace = id(&reply.body["workspaces"][0]["id"]);
    let base = format!("/api/v1/workspaces/{workspace}");
    let bootstrap = request(
        &h.router,
        Some(&owner),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    bootstrap.expect(StatusCode::OK);
    Fixture {
        owner,
        workspace,
        team: id(&bootstrap.body["teams"][0]["id"]),
        base,
    }
}

async fn bootstrap(router: &Router, client: &Client, base: &str) -> Value {
    let reply = request(
        router,
        Some(client),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    reply.expect(StatusCode::OK);
    opaque(reply.body["realtime_cursor"].as_str().unwrap());
    reply.body
}

fn opaque(cursor: &str) {
    assert!(!cursor.is_empty());
    assert!(cursor.len() <= 4096, "A cursor must be bounded");
    assert!(cursor.parse::<i64>().is_err(), "A sequence is not a cursor");
    assert!(serde_json::from_str::<Value>(cursor).is_err());
}

fn page_path(base: &str, cursor: &str, limit: u32) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", cursor)
        .append_pair("limit", &limit.to_string())
        .finish();
    format!("{base}/events/page?{query}")
}

async fn page(router: &Router, client: &Client, base: &str, cursor: &str, limit: u32) -> Value {
    let reply = request(
        router,
        Some(client),
        "GET",
        &page_path(base, cursor, limit),
        None,
        &[],
    )
    .await;
    reply.expect(StatusCode::OK);
    assert_eq!(
        reply
            .body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["cursor", "events", "has_more"]
    );
    opaque(reply.body["cursor"].as_str().unwrap());
    reply.body
}

async fn issue(router: &Router, client: &Client, base: &str, team: Uuid, title: &str) -> Reply {
    let reply = request(
        router,
        Some(client),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":team,"title":title})),
        &[],
    )
    .await;
    reply.expect(StatusCode::CREATED);
    reply
}

async fn invite(router: &Router, f: &Fixture, email: &str, role: &str) -> Client {
    let invitation = request(
        router,
        Some(&f.owner),
        "POST",
        &format!("{}/invitations", f.base),
        Some(json!({"email":email,"role":role,"team_ids":[]})),
        &[],
    )
    .await;
    invitation.expect(StatusCode::CREATED);
    let accepted = request(
        router,
        None,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":invitation.body["token"],"display_name":"Example Member","password":PASSWORD})),
        &[],
    )
    .await;
    accepted.expect(StatusCode::OK);
    accepted.client()
}

async fn native_bearer(router: &Router, browser: &Client) -> (String, Uuid) {
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let challenge = opentrack_domain::native_code_challenge(VERIFIER).unwrap();
    let state = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let start = request(
        router,
        None,
        "POST",
        "/api/v1/auth/native/requests",
        Some(json!({
            "code_challenge":challenge,"state":state,"device_name":"Realtime native contract",
            "callback_uri":"http://127.0.0.1:38419/opentrack/native-callback"
        })),
        &[],
    )
    .await;
    // Handoff responses contain secrets. Status failures deliberately omit
    // response bodies so generated credentials cannot enter test output.
    assert_eq!(start.status, StatusCode::CREATED);
    assert!(!start.headers.contains_key("set-cookie"));
    let approval_path = format!(
        "/api/v1/auth/native/requests/{}",
        start.body["id"].as_str().unwrap()
    );
    let consent = request(router, Some(browser), "GET", &approval_path, None, &[]).await;
    assert_eq!(consent.status, StatusCode::OK);
    assert_eq!(consent.body["device_name"], "Realtime native contract");
    let approved = request(
        router,
        Some(browser),
        "POST",
        &format!("{approval_path}/approve"),
        Some(json!({"confirmation_code":start.body["confirmation_code"]})),
        &[],
    )
    .await;
    assert_eq!(approved.status, StatusCode::OK);
    let callback = url::Url::parse(approved.body["callback_uri"].as_str().unwrap()).unwrap();
    let code = callback
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .into_owned();
    let grant = request(
        router,
        None,
        "POST",
        "/api/v1/auth/native/exchange",
        Some(json!({"request_id":start.body["id"],"code":code,"code_verifier":VERIFIER})),
        &[],
    )
    .await;
    assert_eq!(grant.status, StatusCode::OK);
    assert!(!grant.headers.contains_key("set-cookie"));
    assert_eq!(grant.body["token_type"], "Bearer");
    assert_eq!(grant.body["session"]["audience"], "desktop");
    let token = grant.body["access_token"].as_str().unwrap();
    assert!(opentrack_domain::native_hex_secret(token));
    (format!("Bearer {token}"), id(&grant.body["session_id"]))
}

struct Server {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    async fn start(router: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { address, task }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug)]
struct SseEvent {
    name: String,
    id: Option<String>,
    data: Value,
}

struct TcpSse {
    reader: BufReader<tokio::net::TcpStream>,
    pending: String,
    finished: bool,
}

impl TcpSse {
    async fn open(server: &Server, client: &Client, base: &str, cursor: &str) -> Self {
        tokio::time::timeout(DEADLINE, async {
            let mut socket = tokio::net::TcpStream::connect(server.address).await.unwrap();
            socket.write_all(format!(
                "GET {base}/events HTTP/1.1\r\nHost: {}\r\nOrigin: {ORIGIN}\r\nCookie: {}\r\nAccept: text/event-stream\r\nLast-Event-ID: {cursor}\r\n\r\n",
                server.address, client.cookie
            ).as_bytes()).await.unwrap();
            let mut reader = BufReader::new(socket);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line.split_whitespace().nth(1), Some("200"), "{line}");
            let mut headers = BTreeMap::new();
            loop {
                line.clear();
                reader.read_line(&mut line).await.unwrap();
                if line == "\r\n" { break; }
                let (name, value) = line.trim_end().split_once(':').unwrap();
                headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
            }
            assert!(headers["content-type"].starts_with("text/event-stream"));
            assert_eq!(headers["cache-control"], "no-store");
            assert_eq!(headers["transfer-encoding"], "chunked");
            Self { reader, pending: String::new(), finished: false }
        }).await.expect("SSE headers exceeded the loopback deadline")
    }

    async fn next(&mut self) -> Option<SseEvent> {
        tokio::time::timeout(DEADLINE, self.read_event())
            .await
            .expect("Expected a bounded SSE delivery or stream closure")
    }

    async fn expect_event(&mut self, name: &str) -> SseEvent {
        tokio::time::timeout(DEADLINE, async {
            loop {
                let event = self
                    .read_event()
                    .await
                    .expect("Stream closed before its expected event");
                if event.name == name {
                    return event;
                }
                assert_eq!(
                    event.name, "checkpoint",
                    "Unexpected SSE event while waiting for {name}"
                );
                assert_eq!(event.data, json!({}));
                opaque(event.id.as_deref().unwrap());
            }
        })
        .await
        .expect("Expected SSE event exceeded its delivery deadline")
    }

    async fn read_event(&mut self) -> Option<SseEvent> {
        loop {
            if let Some(end) = self.pending.find("\n\n") {
                let frame = self.pending[..end].to_owned();
                self.pending.drain(..end + 2);
                let mut name = "message".to_owned();
                let mut id = None;
                let mut data = Vec::new();
                for line in frame.lines() {
                    let Some((field, value)) = line.split_once(':') else {
                        continue;
                    };
                    let value = value.strip_prefix(' ').unwrap_or(value);
                    match field {
                        "event" => name = value.to_owned(),
                        "id" => id = Some(value.to_owned()),
                        "data" => data.push(value),
                        _ => {}
                    }
                }
                if data.is_empty() {
                    continue;
                }
                return Some(SseEvent {
                    name,
                    id,
                    data: serde_json::from_str(&data.join("\n")).unwrap(),
                });
            }
            if self.finished {
                return None;
            }
            let mut line = String::new();
            let read = self.reader.read_line(&mut line).await.unwrap();
            if read == 0 {
                self.finished = true;
                continue;
            }
            let length = usize::from_str_radix(line.trim().split(';').next().unwrap(), 16).unwrap();
            if length == 0 {
                self.finished = true;
                continue;
            }
            assert!(length <= 1024 * 1024);
            let mut chunk = vec![0; length + 2];
            self.reader.read_exact(&mut chunk).await.unwrap();
            assert_eq!(&chunk[length..], b"\r\n");
            self.pending
                .push_str(std::str::from_utf8(&chunk[..length]).unwrap());
        }
    }
}

fn invalidation(event: &SseEvent, workspace: Uuid, entity: Uuid) {
    assert_eq!(event.name, "invalidation");
    opaque(event.id.as_deref().unwrap());
    assert_eq!(event.data["workspace_id"], workspace.to_string());
    assert_eq!(event.data["entity_id"], entity.to_string());
    assert_eq!(event.data["entity_type"], "issue");
    assert_eq!(event.data["schema_version"], 1);
    Uuid::parse_str(event.data["event_id"].as_str().unwrap()).unwrap();
    Uuid::parse_str(event.data["mutation_id"].as_str().unwrap()).unwrap();
    let object = event.data.as_object().unwrap();
    for key in object.keys() {
        assert!(
            [
                "event_id",
                "schema_version",
                "type",
                "workspace_id",
                "entity_type",
                "entity_id",
                "entity_version",
                "actor_id",
                "occurred_at",
                "mutation_id",
                "changed_fields"
            ]
            .contains(&key.as_str()),
            "Unexpected wire field {key}"
        );
    }
    assert!(!object.contains_key("seq"));
    assert!(!object.contains_key("team_id"));
}

async fn update(
    router: &Router,
    client: &Client,
    base: &str,
    original: &Reply,
    title: &str,
) -> Reply {
    let reply = request(
        router,
        Some(client),
        "PATCH",
        &format!("{base}/issues/{}", original.body["id"].as_str().unwrap()),
        Some(json!({"title":title})),
        &[("if-match", original.headers["etag"].to_str().unwrap())],
    )
    .await;
    reply.expect(StatusCode::OK);
    reply
}

async fn staged_update(h: &Harness, f: &Fixture, entity: Uuid, title: &str) -> store::Tx {
    let mut tx = store::begin(&h.app().pool).await.unwrap();
    store::context(&mut tx, f.owner.actor, Some(f.workspace))
        .await
        .unwrap();
    sqlx::query("UPDATE issues SET title=$3,version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(f.workspace)
        .bind(entity)
        .bind(title)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

async fn append(tx: &mut store::Tx, f: &Fixture, entity: Uuid, version: i64) {
    store::append_event(
        tx,
        store::Event {
            workspace: f.workspace,
            team: f.team,
            entity,
            version,
            actor: f.owner.actor,
            kind: "issue.updated",
            mutation: Uuid::new_v4(),
            changes: json!({"title":true}),
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_http_nodes_replay_reversed_commits_rollback_and_reconnect() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { ordered_replay(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn ordered_replay(h: &Harness) {
    let f = setup(h).await;
    let second = h.independent_app().await;
    let router_b = opentrack_api::router(h.api_state(second.clone()));
    let other_team = request(
        &h.router,
        Some(&f.owner),
        "POST",
        &format!("{}/teams", f.base),
        Some(json!({"name":"Independent aggregate team","key":"IAT","private":false})),
        &[],
    )
    .await;
    other_team.expect(StatusCode::CREATED);
    let a = issue(
        &h.router,
        &f.owner,
        &f.base,
        f.team,
        "First transaction source",
    )
    .await;
    let b = issue(
        &h.router,
        &f.owner,
        &f.base,
        id(&other_team.body["id"]),
        "Second transaction source",
    )
    .await;
    let snapshot = bootstrap(&h.router, &f.owner, &f.base).await;
    let cursor = snapshot["realtime_cursor"].as_str().unwrap();
    let server_a = Server::start(h.router.clone()).await;
    let server_b = Server::start(router_b.clone()).await;
    let mut stream_a = TcpSse::open(&server_a, &f.owner, &f.base, cursor).await;
    let mut stream_b = TcpSse::open(&server_b, &f.owner, &f.base, cursor).await;

    // A starts first and changes a different aggregate, but B commits first.
    let mut transaction_a = staged_update(h, &f, id(&a.body["id"]), "Delayed first update").await;
    let b = update(&router_b, &f.owner, &f.base, &b, "Committed second update").await;
    let first_a = stream_a.expect_event("invalidation").await;
    let first_b = stream_b.expect_event("invalidation").await;
    invalidation(&first_a, f.workspace, id(&b.body["id"]));
    invalidation(&first_b, f.workspace, id(&b.body["id"]));
    assert_eq!(first_a.data["event_id"], first_b.data["event_id"]);
    append(&mut transaction_a, &f, id(&a.body["id"]), 2).await;
    transaction_a.commit().await.unwrap();
    let delayed = stream_a.expect_event("invalidation").await;
    invalidation(&delayed, f.workspace, id(&a.body["id"]));
    assert_eq!(
        stream_b.expect_event("invalidation").await.data["event_id"],
        delayed.data["event_id"]
    );
    let replay = page(
        &router_b,
        &f.owner,
        &f.base,
        first_a.id.as_deref().unwrap(),
        100,
    )
    .await;
    assert_eq!(replay["events"].as_array().unwrap().len(), 1);
    assert_eq!(replay["events"][0]["event_id"], delayed.data["event_id"]);

    // B reaches the head while A owns its sequence reservation. A rollback must
    // unblock B without leaving a replay gap or exposing A's staged mutation.
    let mut rollback = staged_update(h, &f, id(&a.body["id"]), "Rolled back title").await;
    append(&mut rollback, &f, id(&a.body["id"]), 3).await;
    let waiting_router = router_b.clone();
    let waiting_client = f.owner.clone();
    let waiting_base = f.base.clone();
    let waiting_issue = b.clone();
    let waiting = tokio::spawn(async move {
        update(
            &waiting_router,
            &waiting_client,
            &waiting_base,
            &waiting_issue,
            "After rollback",
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !waiting.is_finished(),
        "A concurrent writer must wait for the transactional head reservation"
    );
    rollback.rollback().await.unwrap();
    let reconnect_cursor = delayed.id.as_deref().unwrap();
    drop(stream_a);
    let mut reconnected = TcpSse::open(&server_b, &f.owner, &f.base, reconnect_cursor).await;
    let b = waiting.await.unwrap();
    let after_rollback = reconnected.expect_event("invalidation").await;
    invalidation(&after_rollback, f.workspace, id(&b.body["id"]));
    let replay = page(&h.router, &f.owner, &f.base, reconnect_cursor, 1).await;
    assert_eq!(replay["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        replay["events"][0]["event_id"],
        after_rollback.data["event_id"]
    );
    assert_eq!(replay["has_more"], false);
    let converged = page(
        &router_b,
        &f.owner,
        &f.base,
        replay["cursor"].as_str().unwrap(),
        100,
    )
    .await;
    assert_eq!(converged["events"], json!([]));
    assert_eq!(converged["has_more"], false);
    let canonical = bootstrap(&router_b, &f.owner, &f.base).await;
    assert!(
        canonical["issues"]
            .as_array()
            .unwrap()
            .iter()
            .all(|issue| issue["title"] != "Rolled back title")
    );
    drop((stream_b, reconnected, server_a, server_b));
    second.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hidden_events_checkpoint_without_identifiers_and_cursors_bind_scope_and_session() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { hidden_replay(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn hidden_replay(h: &Harness) {
    let f = setup(h).await;
    let admin = invite(&h.router, &f, "realtime-admin@example.test", "admin").await;
    let private = request(
        &h.router,
        Some(&admin),
        "POST",
        &format!("{}/teams", f.base),
        Some(json!({"name":"Restricted source team","key":"RST","private":true})),
        &[],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_team = id(&private.body["id"]);
    request(
        &h.router,
        Some(&f.owner),
        "GET",
        &format!("{}/teams/{private_team}", f.base),
        None,
        &[],
    )
    .await
    .expect(StatusCode::NOT_FOUND);
    let snapshot = bootstrap(&h.router, &f.owner, &f.base).await;
    let cursor = snapshot["realtime_cursor"].as_str().unwrap();
    let server = Server::start(h.router.clone()).await;
    let mut stream = TcpSse::open(&server, &f.owner, &f.base, cursor).await;
    let hidden = issue(
        &h.router,
        &admin,
        &f.base,
        private_team,
        "Restricted issue content",
    )
    .await;
    let checkpoint = stream.next().await.unwrap();
    assert_eq!(checkpoint.name, "checkpoint");
    assert_eq!(checkpoint.data, json!({}), "A checkpoint has no metadata");
    let advanced = checkpoint.id.as_deref().unwrap();
    opaque(advanced);
    assert_ne!(advanced, cursor);
    let encoded = serde_json::to_string(&checkpoint.data).unwrap();
    for secret in [
        private_team.to_string(),
        id(&hidden.body["id"]).to_string(),
        admin.actor.to_string(),
        "Restricted issue content".into(),
    ] {
        assert!(!encoded.contains(&secret));
        assert!(!advanced.contains(&secret));
    }
    let hidden_page = page(&h.router, &f.owner, &f.base, cursor, 1).await;
    assert_eq!(hidden_page["events"], json!([]));
    assert_eq!(hidden_page["has_more"], false);
    assert_ne!(hidden_page["cursor"], cursor);
    let visible = issue(
        &h.router,
        &f.owner,
        &f.base,
        f.team,
        "Public source content",
    )
    .await;
    let event = stream.expect_event("invalidation").await;
    invalidation(&event, f.workspace, id(&visible.body["id"]));
    assert!(
        !serde_json::to_string(&event.data)
            .unwrap()
            .contains("Public source content")
    );
    let visible_page = page(&h.router, &f.owner, &f.base, advanced, 1).await;
    assert_eq!(visible_page["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        visible_page["events"][0]["event_id"],
        event.data["event_id"]
    );
    assert_eq!(visible_page["has_more"], false);

    // An event's recorded public team cannot authorize its now-private source.
    // The old audience must clear cached state without receiving the hidden ID.
    let moving = format!("{}/issues/{}", f.base, visible.body["id"].as_str().unwrap());
    let preview = request(
        &h.router,
        Some(&admin),
        "GET",
        &format!("{moving}/move-preview?destination_team_id={private_team}"),
        None,
        &[],
    )
    .await;
    preview.expect(StatusCode::OK);
    request(
        &h.router, Some(&admin), "POST", &format!("{moving}/move"),
        Some(json!({
            "destination_team_id":private_team,"status_id":preview.body["destination_default_status_id"],
            "assignee_id":null,"label_ids":[],"preview_token":preview.body["preview_token"]
        })), &[("if-match",visible.headers["etag"].to_str().unwrap())],
    ).await.expect(StatusCode::OK);
    let reset = stream.expect_event("reset_required").await;
    assert_eq!(reset.data, json!({"reason":"scope_changed"}));
    assert!(reset.id.is_none());
    assert!(stream.next().await.is_none());
    let denied = request(
        &h.router,
        Some(&f.owner),
        "GET",
        &page_path(&f.base, event.id.as_deref().unwrap(), 100),
        None,
        &[],
    )
    .await;
    denied.expect(StatusCode::CONFLICT);
    assert!(
        !denied
            .body
            .to_string()
            .contains(visible.body["id"].as_str().unwrap())
    );
    assert!(!denied.body.to_string().contains(&private_team.to_string()));
    request(&h.router, Some(&f.owner), "GET", &moving, None, &[])
        .await
        .expect(StatusCode::NOT_FOUND);

    let login = request(
        &h.router,
        None,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":"realtime-owner@example.test","password":PASSWORD})),
        &[],
    )
    .await;
    login.expect(StatusCode::OK);
    let other_session = login.client();
    let organizations = request(
        &h.router,
        Some(&f.owner),
        "GET",
        "/api/v1/organizations",
        None,
        &[],
    )
    .await;
    organizations.expect(StatusCode::OK);
    let second = request(
        &h.router, Some(&f.owner), "POST", "/api/v1/workspaces",
        Some(json!({"organization_id":organizations.body[0]["id"],"name":"Other replay scope","team_name":"Operations","team_key":"OPS"})), &[],
    ).await;
    second.expect(StatusCode::CREATED);
    let other_base = format!("/api/v1/workspaces/{}", second.body["id"].as_str().unwrap());
    let mut modified = cursor.as_bytes().to_vec();
    let middle = modified.len() / 2;
    modified[middle] = if modified[middle] == b'A' { b'B' } else { b'A' };
    let modified = String::from_utf8(modified).unwrap();
    for (client, base, rejected) in [
        (&f.owner, f.base.as_str(), "not-a-cursor"),
        (&f.owner, f.base.as_str(), modified.as_str()),
        (&other_session, f.base.as_str(), cursor),
        (&f.owner, other_base.as_str(), cursor),
    ] {
        let reply = request(
            &h.router,
            Some(client),
            "GET",
            &page_path(base, rejected, 100),
            None,
            &[],
        )
        .await;
        reply.expect(StatusCode::CONFLICT);
        assert_eq!(reply.headers["content-type"], "application/problem+json");
        assert!(
            !serde_json::to_string(&reply.body)
                .unwrap()
                .contains(rejected)
        );
        request(
            &h.router,
            Some(client),
            "GET",
            &format!("{base}/events"),
            None,
            &[("last-event-id", rejected)],
        )
        .await
        .expect(StatusCode::CONFLICT);
    }
    request(
        &h.router,
        None,
        "GET",
        &page_path(&f.base, cursor, 100),
        None,
        &[],
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
    request(
        &h.router,
        None,
        "GET",
        &format!("{}/events", f.base),
        None,
        &[("last-event-id", cursor)],
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
    for limit in [0, 101] {
        request(
            &h.router,
            Some(&f.owner),
            "GET",
            &page_path(&f.base, cursor, limit),
            None,
            &[],
        )
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    }
    for headers in [
        vec![],
        vec![("last-event-id", "")],
        vec![("last-event-id", cursor), ("last-event-id", cursor)],
    ] {
        request(
            &h.router,
            Some(&f.owner),
            "GET",
            &format!("{}/events", f.base),
            None,
            &headers,
        )
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", cursor)
        .finish();
    request(
        &h.router,
        Some(&f.owner),
        "GET",
        &format!("{}/events?{query}", f.base),
        None,
        &[("last-event-id", "different-opaque-value")],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    native_replay(h, &f).await;
    drop((stream, server));
}

async fn native_replay(h: &Harness, f: &Fixture) {
    let (bearer, session) = native_bearer(&h.router, &f.owner).await;
    let authorization = [("authorization", bearer.as_str())];
    let native_bootstrap = request(
        &h.router,
        None,
        "GET",
        &format!("{}/bootstrap", f.base),
        None,
        &authorization,
    )
    .await;
    native_bootstrap.expect(StatusCode::OK);
    let native_cursor = native_bootstrap.body["realtime_cursor"].as_str().unwrap();
    opaque(native_cursor);
    let web_bootstrap = bootstrap(&h.router, &f.owner, &f.base).await;
    let web_cursor = web_bootstrap["realtime_cursor"].as_str().unwrap();
    let created = issue(&h.router, &f.owner, &f.base, f.team, "Native replay source").await;
    let native_page = request(
        &h.router,
        None,
        "GET",
        &page_path(&f.base, native_cursor, 100),
        None,
        &authorization,
    )
    .await;
    native_page.expect(StatusCode::OK);
    assert!(!native_page.headers.contains_key("set-cookie"));
    assert_eq!(native_page.body["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        native_page.body["events"][0]["entity_id"],
        created.body["id"]
    );
    assert_eq!(native_page.body["events"][0]["type"], "issue.created");
    assert_eq!(native_page.body["has_more"], false);
    opaque(native_page.body["cursor"].as_str().unwrap());
    request(
        &h.router,
        None,
        "GET",
        &page_path(&f.base, web_cursor, 100),
        None,
        &authorization,
    )
    .await
    .expect(StatusCode::CONFLICT);
    request(
        &h.router,
        Some(&f.owner),
        "GET",
        &page_path(&f.base, native_cursor, 100),
        None,
        &[],
    )
    .await
    .expect(StatusCode::CONFLICT);
    request(
        &h.router,
        Some(&f.owner),
        "POST",
        &format!("/api/v1/auth/sessions/{session}/revoke"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    request(
        &h.router,
        None,
        "GET",
        &page_path(&f.base, native_cursor, 100),
        None,
        &authorization,
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
    request(
        &h.router,
        None,
        "GET",
        &format!("{}/bootstrap", f.base),
        None,
        &authorization,
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn already_open_http_streams_end_after_role_and_session_revocation() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { revoked_streams(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn reset_and_close(stream: &mut TcpSse) {
    let reset = stream.expect_event("reset_required").await;
    assert_eq!(reset.name, "reset_required");
    assert_eq!(reset.data.as_object().unwrap().len(), 1);
    assert!(reset.data["reason"].as_str().is_some());
    assert!(
        reset.id.is_none(),
        "A reset cannot acknowledge unconsumed changes"
    );
    assert!(
        stream.next().await.is_none(),
        "A reset must close the stream"
    );
}

async fn revoked_streams(h: &Harness) {
    let f = setup(h).await;
    let member = invite(&h.router, &f, "realtime-member@example.test", "member").await;
    let server = Server::start(h.router.clone()).await;
    let initial = bootstrap(&h.router, &member, &f.base).await;
    let mut stream = TcpSse::open(
        &server,
        &member,
        &f.base,
        initial["realtime_cursor"].as_str().unwrap(),
    )
    .await;
    let path = format!("{}/members/{}", f.base, member.actor);
    let membership = request(&h.router, Some(&f.owner), "GET", &path, None, &[]).await;
    membership.expect(StatusCode::OK);
    request(
        &h.router,
        Some(&f.owner),
        "PATCH",
        &path,
        Some(json!({"role":"viewer"})),
        &[("if-match", membership.headers["etag"].to_str().unwrap())],
    )
    .await
    .expect(StatusCode::OK);
    reset_and_close(&mut stream).await;
    request(
        &h.router,
        Some(&member),
        "GET",
        &page_path(&f.base, initial["realtime_cursor"].as_str().unwrap(), 100),
        None,
        &[],
    )
    .await
    .expect(StatusCode::CONFLICT);

    let login = request(
        &h.router,
        None,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":"realtime-owner@example.test","password":PASSWORD})),
        &[],
    )
    .await;
    login.expect(StatusCode::OK);
    let other_session = login.client();
    let initial = bootstrap(&h.router, &f.owner, &f.base).await;
    let mut stream = TcpSse::open(
        &server,
        &f.owner,
        &f.base,
        initial["realtime_cursor"].as_str().unwrap(),
    )
    .await;
    let sessions = request(
        &h.router,
        Some(&f.owner),
        "GET",
        "/api/v1/auth/sessions",
        None,
        &[],
    )
    .await;
    sessions.expect(StatusCode::OK);
    let current = sessions.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["current"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    request(
        &h.router,
        Some(&other_session),
        "POST",
        &format!("/api/v1/auth/sessions/{current}/revoke"),
        None,
        &[],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    reset_and_close(&mut stream).await;
    request(
        &h.router,
        Some(&f.owner),
        "GET",
        &page_path(&f.base, initial["realtime_cursor"].as_str().unwrap(), 100),
        None,
        &[],
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
    drop((stream, server));
}

async fn open_unpolled(
    router: &Router,
    client: &Client,
    base: &str,
    cursor: &str,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("{base}/events"))
                .header("cookie", &client.cookie)
                .header("origin", ORIGIN)
                .header("accept", "text/event-stream")
                .header("last-event-id", cursor)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polling_only_node_survives_missing_notifications_and_caps_sessions_across_nodes() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { polling_and_limits(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn polling_and_limits(h: &Harness) {
    use opentrack_api::{RealtimeNode, router_with_realtime_node};

    let f = setup(h).await;
    let second = h.independent_app().await;
    let node_a = Arc::new(RealtimeNode::polling_only(h.app().clone()));
    let node_b = Arc::new(RealtimeNode::new(second.clone()));
    let router_a = router_with_realtime_node(h.api_state(h.app().clone()), node_a.clone());
    let router_b = router_with_realtime_node(h.api_state(second.clone()), node_b.clone());
    let organizations = request(
        &router_a,
        Some(&f.owner),
        "GET",
        "/api/v1/organizations",
        None,
        &[],
    )
    .await;
    organizations.expect(StatusCode::OK);
    let other_workspace = request(
        &router_a, Some(&f.owner), "POST", "/api/v1/workspaces",
        Some(json!({"organization_id":organizations.body[0]["id"],"name":"Other stream scope","team_name":"Operations","team_key":"OPS"})), &[],
    ).await;
    other_workspace.expect(StatusCode::CREATED);
    let other_base = format!(
        "/api/v1/workspaces/{}",
        other_workspace.body["id"].as_str().unwrap()
    );
    let other_snapshot = bootstrap(&router_b, &f.owner, &other_base).await;
    let other_cursor = other_snapshot["realtime_cursor"].as_str().unwrap();
    let snapshot = bootstrap(&router_a, &f.owner, &f.base).await;
    let cursor = snapshot["realtime_cursor"].as_str().unwrap();
    let server = Server::start(router_a.clone()).await;
    let mut stream = TcpSse::open(&server, &f.owner, &f.base, cursor).await;
    let committed = std::time::Instant::now();
    let created = issue(
        &router_b,
        &f.owner,
        &f.base,
        f.team,
        "Persisted without listener wakeup",
    )
    .await;
    let event = stream.expect_event("invalidation").await;
    invalidation(&event, f.workspace, id(&created.body["id"]));
    // Includes loopback scheduling and SQL time while leaving a small CI allowance.
    assert!(
        committed.elapsed() < Duration::from_secs(4),
        "The two-second safety poll must deliver without NOTIFY"
    );
    drop((stream, server));
    tokio::time::timeout(DEADLINE, async {
        while node_a.stats().active_streams != 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Dropping the HTTP connection must release the stream lease");

    let snapshot = bootstrap(&router_a, &f.owner, &f.base).await;
    let cursor = snapshot["realtime_cursor"].as_str().unwrap();
    let first = open_unpolled(&router_a, &f.owner, &f.base, cursor).await;
    let second_stream = open_unpolled(&router_b, &f.owner, &f.base, cursor).await;
    let third = open_unpolled(&router_a, &f.owner, &f.base, cursor).await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second_stream.status(), StatusCode::OK);
    assert_eq!(third.status(), StatusCode::OK);
    let rejected = open_unpolled(&router_b, &f.owner, &other_base, other_cursor).await;
    assert_eq!(
        rejected.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "The fourth connection must count streams on both independent nodes"
    );
    assert_eq!(
        rejected.headers()["content-type"],
        "application/problem+json"
    );
    drop((rejected, third));
    let replacement = tokio::time::timeout(DEADLINE, async {
        loop {
            let response = open_unpolled(&router_b, &f.owner, &other_base, other_cursor).await;
            if response.status() == StatusCode::OK {
                break response;
            }
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("A dropped body must free its installation-wide session slot");

    // The response bodies remain unpolled: no kernel send buffer can hide server
    // queue growth. Populate durable rows, then let both nodes replay them.
    let mut tx = store::begin(&h.app().pool).await.unwrap();
    store::context(&mut tx, f.owner.actor, Some(f.workspace))
        .await
        .unwrap();
    let entity = id(&created.body["id"]);
    sqlx::query("UPDATE issues SET version=301,title='Final slow-consumer source' WHERE workspace_id=$1 AND id=$2")
        .bind(f.workspace).bind(entity).execute(&mut *tx).await.unwrap();
    let events: Vec<_> = (2..=301)
        .map(|version| store::Event {
            workspace: f.workspace,
            team: f.team,
            entity,
            version,
            actor: f.owner.actor,
            kind: "issue.updated",
            mutation: Uuid::new_v4(),
            changes: json!({"title":true}),
        })
        .collect();
    for event in &events {
        store::append_activity(&mut tx, event).await.unwrap();
    }
    for chunk in events.chunks(100) {
        store::append_stream_events(&mut tx, chunk).await.unwrap();
    }
    tx.commit().await.unwrap();
    tokio::time::timeout(DEADLINE, async {
        loop {
            if node_a.stats().overflowed_streams > 0 && node_b.stats().overflowed_streams > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("An unpolled client must overflow and disconnect");
    for stats in [node_a.stats(), node_b.stats()] {
        assert!(
            stats.max_queued_events <= 256,
            "Each client queue is bounded to 256 events"
        );
        assert!(
            stats.max_queued_bytes <= 1024 * 1024,
            "Each client queue is bounded to 1 MiB"
        );
        assert!(stats.queued_events <= 3 * 256);
        assert!(stats.queued_bytes <= 3 * 1024 * 1024);
        assert!(stats.max_queued_events > 0);
        assert!(stats.max_queued_bytes > 0);
    }
    // Overflow must preserve durable convergence rather than acknowledge a drop.
    let mut replay_cursor = cursor.to_owned();
    let mut replayed = Vec::new();
    loop {
        let batch = page(&router_b, &f.owner, &f.base, &replay_cursor, 100).await;
        replayed.extend(
            batch["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| event["event_id"].as_str().unwrap().to_owned()),
        );
        replay_cursor = batch["cursor"].as_str().unwrap().to_owned();
        if batch["has_more"] == false {
            break;
        }
    }
    assert_eq!(replayed.len(), 300);
    replayed.sort();
    replayed.dedup();
    assert_eq!(
        replayed.len(),
        300,
        "Replay must retain every event despite slow-client overflow"
    );
    let mut overflow_body = first.into_body().into_data_stream();
    let drained = tokio::time::timeout(DEADLINE, async {
        let mut bytes = Vec::new();
        while let Some(chunk) = overflow_body.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
            assert!(
                bytes.len() <= 1024 * 1024 + 4096,
                "A closed overflow body remains bounded"
            );
        }
        bytes
    })
    .await
    .expect("An overflowed stream must close after its bounded queue drains");
    let terminal = std::str::from_utf8(&drained).unwrap();
    assert!(terminal.contains("event: reset_required\n"));
    let data = terminal
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(data).unwrap(),
        json!({"reason":"reset_required"})
    );
    assert!(
        !terminal.lines().any(|line| line.starts_with("id:")),
        "Overflow cannot acknowledge discarded queued events"
    );
    drop((second_stream, replacement, overflow_body));
    node_a.shutdown();
    node_b.shutdown();
    second.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_data_and_head_share_a_snapshot_during_concurrent_commit() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { bootstrap_snapshot(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn bootstrap_snapshot(h: &Harness) {
    let f = setup(h).await;
    let original = issue(
        &h.router,
        &f.owner,
        &f.base,
        f.team,
        "Before bootstrap snapshot",
    )
    .await;
    let entity = id(&original.body["id"]);
    let head_oid: i64 =
        sqlx::query_scalar("SELECT 'workspace_stream_heads'::regclass::oid::bigint")
            .fetch_one(h.owner_pool())
            .await
            .unwrap();
    let mut writer = store::begin(h.owner_pool()).await.unwrap();
    store::context(&mut writer, f.owner.actor, Some(f.workspace))
        .await
        .unwrap();
    sqlx::query("LOCK TABLE workspace_stream_heads IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *writer)
        .await
        .unwrap();
    let router = h.router.clone();
    let owner = f.owner.clone();
    let base = f.base.clone();
    let reading = tokio::spawn(async move { bootstrap(&router, &owner, &base).await });
    tokio::time::timeout(Duration::from_millis(900), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE relation::bigint=$1 AND NOT granted)",
            )
            .bind(head_oid)
            .fetch_one(&h.app().pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            assert!(
                !reading.is_finished(),
                "Bootstrap must read its replay head before returning"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Bootstrap did not reach its stream-head read");
    // The same real transaction that releases the head lock commits its source
    // mutation and event, after bootstrap has already read its issue rows.
    sqlx::query("UPDATE issues SET title='During bootstrap snapshot',version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(f.workspace).bind(entity).execute(&mut *writer).await.unwrap();
    append(&mut writer, &f, entity, 2).await;
    writer.commit().await.unwrap();
    let snapshot = reading.await.unwrap();
    let item = snapshot["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["id"] == entity.to_string())
        .unwrap();
    assert_eq!(item["title"], "Before bootstrap snapshot");
    assert_eq!(item["version"], "1");
    let replay = page(
        &h.router,
        &f.owner,
        &f.base,
        snapshot["realtime_cursor"].as_str().unwrap(),
        100,
    )
    .await;
    assert_eq!(
        replay["events"].as_array().unwrap().len(),
        1,
        "The cursor cannot skip a mutation excluded from its snapshot"
    );
    assert_eq!(replay["events"][0]["entity_id"], entity.to_string());
    assert_eq!(replay["events"][0]["entity_version"], "2");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_snapshot_cannot_resurrect_a_session_revoked_while_waiting_for_its_guard() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { bootstrap_revocation(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn bootstrap_revocation(h: &Harness) {
    let f = setup(h).await;
    issue(
        &h.router,
        &f.owner,
        &f.base,
        f.team,
        "Revocation race source",
    )
    .await;
    let token = f.owner.cookie.split_once('=').unwrap().1;
    let actor = h.app().authenticate(token).await.unwrap();
    let token_hash = actor.token_hash.clone();
    let mut revoking = store::begin(h.owner_pool()).await.unwrap();
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoking)
        .await
        .unwrap();
    store::context_exclusive(&mut revoking, f.owner.actor, Some(f.workspace))
        .await
        .unwrap();
    let app = h.app().clone();
    let workspace = f.workspace;
    let reading = tokio::spawn(async move { app.bootstrap(&actor, workspace).await });
    tokio::time::timeout(Duration::from_millis(900), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE $1=ANY(pg_blocking_pids(a.pid)))")
                .bind(holder).fetch_one(&h.app().pool).await.unwrap();
            if waiting { break; }
            assert!(!reading.is_finished(), "Bootstrap must wait for the account guard");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("Bootstrap did not block behind the account guard");
    // Keep the users row unchanged. The session row alone changes after the
    // reader's repeatable-read snapshot began but before its guard is acquired.
    let revoked = sqlx::query("UPDATE sessions SET revoked_at=clock_timestamp() WHERE user_id=$1 AND token_hash=$2 AND revoked_at IS NULL")
        .bind(f.owner.actor).bind(token_hash).execute(&mut *revoking).await.unwrap();
    assert_eq!(revoked.rows_affected(), 1);
    revoking.commit().await.unwrap();
    let result = tokio::time::timeout(DEADLINE, reading)
        .await
        .expect("Bootstrap retry must be bounded")
        .unwrap();
    match result {
        Err(opentrack_domain::Error::Unauthorized) => {}
        Err(error) => panic!("Expected unauthorized after session revocation, got {error:?}"),
        Ok(_) => {
            panic!("A revoked session received workspace bootstrap data from its old snapshot")
        }
    }
    request(
        &h.router,
        Some(&f.owner),
        "GET",
        &format!("{}/bootstrap", f.base),
        None,
        &[],
    )
    .await
    .expect(StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_listener_disconnect_and_reconnect_preserve_durable_http_delivery() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { listener_reconnect(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn listener_reconnect(h: &Harness) {
    use opentrack_api::{RealtimeNode, router_with_realtime_node};

    let f = setup(h).await;
    let writer = h.independent_app().await;
    let writer_router = opentrack_api::router(h.api_state(writer.clone()));
    let node = Arc::new(RealtimeNode::new(h.app().clone()));
    let reader_router = router_with_realtime_node(h.api_state(h.app().clone()), node.clone());
    let snapshot = bootstrap(&reader_router, &f.owner, &f.base).await;
    let cursor = snapshot["realtime_cursor"].as_str().unwrap();
    let server = Server::start(reader_router.clone()).await;
    let mut stream = TcpSse::open(&server, &f.owner, &f.base, cursor).await;
    tokio::time::timeout(DEADLINE, async {
        loop {
            let stats = node.stats();
            if stats.listener_connected && stats.listener_connections == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The dedicated PostgreSQL listener must connect before fault injection");

    // A no-op interruption must fail the disconnected assertion. The writer is
    // a separate App/pool and uses the production mutation route during the gap.
    node.interrupt_listener();
    tokio::time::timeout(Duration::from_secs(3), async {
        while node.stats().listener_connected {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Interrupting the live PostgreSQL listener must actually disconnect it");
    let during_gap = issue(
        &writer_router,
        &f.owner,
        &f.base,
        f.team,
        "Committed while listener disconnected",
    )
    .await;
    assert!(
        !node.stats().listener_connected,
        "The controlled gap must include the durable mutation commit"
    );
    tokio::time::timeout(DEADLINE, async {
        loop {
            let stats = node.stats();
            if stats.listener_connected
                && stats.listener_connections >= 2
                && stats.listener_reconnects >= 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The same node must reconnect its dedicated PostgreSQL listener");
    let gap_event = stream.expect_event("invalidation").await;
    invalidation(&gap_event, f.workspace, id(&during_gap.body["id"]));

    let after_reconnect = issue(
        &writer_router,
        &f.owner,
        &f.base,
        f.team,
        "Committed after listener reconnect",
    )
    .await;
    let reconnected_event = stream.expect_event("invalidation").await;
    invalidation(
        &reconnected_event,
        f.workspace,
        id(&after_reconnect.body["id"]),
    );
    assert_ne!(
        gap_event.data["event_id"],
        reconnected_event.data["event_id"]
    );
    let replay = page(&writer_router, &f.owner, &f.base, cursor, 100).await;
    assert_eq!(replay["events"].as_array().unwrap().len(), 2);
    assert_eq!(replay["events"][0]["event_id"], gap_event.data["event_id"]);
    assert_eq!(
        replay["events"][1]["event_id"],
        reconnected_event.data["event_id"]
    );
    assert_eq!(replay["has_more"], false);
    let reconnect = page(
        &reader_router,
        &f.owner,
        &f.base,
        gap_event.id.as_deref().unwrap(),
        100,
    )
    .await;
    assert_eq!(reconnect["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        reconnect["events"][0]["event_id"],
        reconnected_event.data["event_id"]
    );
    assert_eq!(reconnect["has_more"], false);
    node.shutdown();
    drop((stream, server));
    writer.pool.close().await;
}
