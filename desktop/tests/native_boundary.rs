use opentrack_desktop::{
    auth::BeginRequest,
    credentials::{CredentialRef, Vault},
    links::ServerOrigin,
    persistence::{Configuration, Persistence, SavedServer},
    protocol::{ApiRequest, Method, NativeError},
    state::{AuthEvent, Expected, Runtime, SelectRequest},
    transport::Transport,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Default)]
struct MemoryVault {
    values: Mutex<BTreeMap<String, String>>,
    fail_set: AtomicBool,
    fail_get: AtomicBool,
    panic_after_set: AtomicBool,
    fail_delete: AtomicBool,
    gate: Mutex<Option<Arc<StorageGate>>>,
    get_gate: Mutex<Option<Arc<StorageGate>>>,
    deleted: tokio::sync::Notify,
}
struct StorageGate {
    entered: tokio::sync::Notify,
    released: Mutex<bool>,
    wake: std::sync::Condvar,
}
impl StorageGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: tokio::sync::Notify::new(),
            released: Mutex::new(false),
            wake: std::sync::Condvar::new(),
        })
    }
}
impl Vault for MemoryVault {
    fn get(
        &self,
        origin: &str,
        reference: &CredentialRef,
    ) -> Result<Option<Zeroizing<String>>, NativeError> {
        if self.fail_get.load(Ordering::SeqCst) {
            return Err(NativeError::storage());
        }
        if let Some(gate) = self.get_gate.lock().unwrap().clone() {
            gate.entered.notify_one();
            let mut released = gate.released.lock().unwrap();
            while !*released {
                released = gate.wake.wait(released).unwrap();
            }
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&reference.key(origin))
            .cloned()
            .map(Zeroizing::new))
    }
    fn set(
        &self,
        origin: &str,
        reference: &CredentialRef,
        secret: &str,
    ) -> Result<(), NativeError> {
        if self.fail_set.load(Ordering::SeqCst) {
            return Err(NativeError::storage());
        }
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.entered.notify_one();
            let mut released = gate.released.lock().unwrap();
            while !*released {
                released = gate.wake.wait(released).unwrap();
            }
        }
        self.values
            .lock()
            .unwrap()
            .insert(reference.key(origin), secret.into());
        assert!(
            !self.panic_after_set.load(Ordering::SeqCst),
            "Injected credential-store failure after write"
        );
        Ok(())
    }
    fn delete(&self, origin: &str, reference: &CredentialRef) -> Result<(), NativeError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(NativeError::storage());
        }
        self.values.lock().unwrap().remove(&reference.key(origin));
        self.deleted.notify_one();
        Ok(())
    }
}
struct Response {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: String,
}
impl Response {
    fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", "application/json".into())],
            body: value.to_string(),
        }
    }
}
struct Request {
    path: String,
    headers: BTreeMap<String, String>,
    body: Value,
    respond: oneshot::Sender<Response>,
}
struct Server {
    origin: String,
    requests: mpsc::Receiver<Request>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sender, requests) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = Vec::new();
                let mut scratch = [0u8; 4096];
                let header_end = loop {
                    let n = socket.read(&mut scratch).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&scratch[..n]);
                    assert!(buffer.len() < 65536);
                    if let Some(n) = buffer.windows(4).position(|x| x == b"\r\n\r\n") {
                        break n + 4;
                    }
                };
                let header = String::from_utf8(buffer[..header_end].to_vec()).unwrap();
                let mut lines = header.split("\r\n");
                let path = lines.next().unwrap().split(' ').nth(1).unwrap().to_string();
                let headers: BTreeMap<_, _> = lines
                    .filter_map(|line| {
                        line.split_once(':')
                            .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
                    })
                    .collect();
                let length = headers
                    .get("content-length")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                while buffer.len() < header_end + length {
                    let n = socket.read(&mut scratch).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&scratch[..n]);
                }
                let body = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&buffer[header_end..header_end + length]).unwrap()
                };
                let (tx, rx) = oneshot::channel();
                if sender
                    .send(Request {
                        path,
                        headers,
                        body,
                        respond: tx,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                if let Ok(response) = rx.await {
                    let mut text = format!(
                        "HTTP/1.1 {} Response\r\nContent-Length: {}\r\nConnection: close\r\n",
                        response.status,
                        response.body.len()
                    );
                    for (k, v) in response.headers {
                        text.push_str(&format!("{k}: {v}\r\n"));
                    }
                    text.push_str("\r\n");
                    text.push_str(&response.body);
                    let _ = socket.write_all(text.as_bytes()).await;
                }
            }
        });
        Self {
            origin,
            requests,
            task,
        }
    }
    async fn next(&mut self) -> Request {
        tokio::time::timeout(Duration::from_secs(10), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
}
struct Fixture {
    runtime: Arc<Runtime>,
    vault: Arc<MemoryVault>,
    events: mpsc::UnboundedReceiver<AuthEvent>,
    directory: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
impl Fixture {
    fn new(server: &Server, credential: Option<CredentialRef>) -> Self {
        let directory =
            std::env::temp_dir().join(format!("opentrack-native-test-{}", Uuid::new_v4()));
        let persistence = Persistence::new(directory.clone()).unwrap();
        persistence
            .save(&Configuration {
                selected: Some(server.origin.clone()),
                servers: vec![SavedServer {
                    origin: server.origin.clone(),
                    development_http: true,
                    credential,
                }],
            })
            .unwrap();
        let vault = Arc::new(MemoryVault::default());
        let (tx, events) = mpsc::unbounded_channel();
        let runtime = Runtime::new(
            persistence,
            vault.clone(),
            Arc::new(move |event| {
                tx.send(event).unwrap();
            }),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
        Self {
            runtime,
            vault,
            events,
            directory,
        }
    }
    async fn event(&mut self) -> AuthEvent {
        tokio::time::timeout(Duration::from_secs(10), self.events.recv())
            .await
            .unwrap()
            .unwrap()
    }
}
fn api(generation: &str) -> ApiRequest {
    ApiRequest {
        expected_generation: generation.into(),
        method: Method::Get,
        path: "/api/v1/auth/session".into(),
        query: None,
        headers: BTreeMap::new(),
        body: None,
    }
}
fn reference() -> CredentialRef {
    CredentialRef {
        user_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
    }
}
#[tokio::test]
async fn concurrent_workspace_requests_queue_instead_of_reporting_a_locked_keyring() {
    use std::{future::Future, task::Poll};
    let mut server = Server::new().await;
    let credential = reference();
    let fixture = Fixture::new(&server, Some(credential.clone()));
    fixture
        .vault
        .set(&server.origin, &credential, &"a".repeat(64))
        .unwrap();
    let gate = StorageGate::new();
    *fixture.vault.get_gate.lock().unwrap() = Some(gate.clone());
    let native = fixture.runtime.clone();
    let first = tokio::spawn(async move { native.api(api("1")).await });
    gate.entered.notified().await;
    let native = fixture.runtime.clone();
    let mut second = Box::pin(async move { native.api(api("1")).await });
    let initial = std::future::poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx))).await;
    *gate.released.lock().unwrap() = true;
    gate.wake.notify_all();
    assert!(
        initial.is_pending(),
        "Parallel reads must wait for the occupied vault lane"
    );
    let second = tokio::spawn(second);
    for _ in 0..2 {
        let request = server.next().await;
        assert_eq!(
            request.headers.get("authorization"),
            Some(&format!("Bearer {}", "a".repeat(64)))
        );
        request
            .respond
            .send(Response::json(200, json!({"ok":true})))
            .ok();
    }
    assert_eq!(first.await.unwrap().unwrap().status, 200);
    assert_eq!(second.await.unwrap().unwrap().status, 200);
}

#[tokio::test]
async fn bounded_keyring_queue_rechecks_server_scope_before_dispatch() {
    use std::{future::Future, task::Poll};
    let mut server = Server::new().await;
    let other = Server::new().await;
    let credential = reference();
    let fixture = Fixture::new(&server, Some(credential.clone()));
    fixture
        .vault
        .set(&server.origin, &credential, &"a".repeat(64))
        .unwrap();
    let gate = StorageGate::new();
    *fixture.vault.get_gate.lock().unwrap() = Some(gate.clone());
    let native = fixture.runtime.clone();
    let first = tokio::spawn(async move { native.api(api("1")).await });
    gate.entered.notified().await;
    let mut waiting = Vec::new();
    let mut all_pending = true;
    for _ in 0..31 {
        let native = fixture.runtime.clone();
        let mut request = Box::pin(async move { native.api(api("1")).await });
        all_pending &= std::future::poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx)))
            .await
            .is_pending();
        waiting.push(request);
    }
    let overflow = fixture.runtime.api(api("1")).await;
    fixture
        .runtime
        .select(SelectRequest {
            expected_generation: "1".into(),
            origin: other.origin.clone(),
            development_http: true,
        })
        .await
        .unwrap();
    *gate.released.lock().unwrap() = true;
    gate.wake.notify_all();
    assert!(all_pending);
    assert_eq!(overflow.err().unwrap().code, "credential_store_busy");
    for request in waiting {
        assert_eq!(request.await.err().unwrap().code, "stale_generation");
    }
    assert_eq!(first.await.unwrap().err().unwrap().code, "stale_generation");
    assert!(server.requests.try_recv().is_err());
}

#[tokio::test]
async fn stale_requests_never_dispatch_to_a_reselected_server() {
    let mut first = Server::new().await;
    let second = Server::new().await;
    let credential = reference();
    let fixture = Fixture::new(&first, Some(credential.clone()));
    fixture
        .vault
        .set(&first.origin, &credential, &"a".repeat(64))
        .unwrap();
    let running = {
        let native = fixture.runtime.clone();
        tokio::spawn(async move { native.api(api("1")).await })
    };
    let request = first.next().await;
    assert_eq!(
        request.headers.get("authorization").unwrap(),
        &format!("Bearer {}", "a".repeat(64))
    );
    assert!(!request.headers.contains_key("cookie"));
    let next = fixture
        .runtime
        .select(SelectRequest {
            expected_generation: "1".into(),
            origin: second.origin.clone(),
            development_http: true,
        })
        .await
        .unwrap();
    assert!(!next.credential_present);
    assert_eq!(next.scope_generation, "2");
    request
        .respond
        .send(Response::json(200, json!({"from":"old"})))
        .ok();
    assert_eq!(
        running.await.unwrap().err().unwrap().code,
        "stale_generation"
    );
    assert_eq!(
        fixture.runtime.api(api("1")).await.err().unwrap().code,
        "stale_generation"
    );
    assert_eq!(
        fixture.runtime.api(api("2")).await.err().unwrap().code,
        "sign_in_required"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), first.requests.recv())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn credential_delete_failure_never_claims_logout_or_forget() {
    let mut server = Server::new().await;
    let credential = reference();
    let fixture = Fixture::new(&server, Some(credential.clone()));
    fixture
        .vault
        .set(&server.origin, &credential, &"b".repeat(64))
        .unwrap();
    fixture.vault.fail_delete.store(true, Ordering::SeqCst);
    let running = {
        let native = fixture.runtime.clone();
        tokio::spawn(async move {
            native
                .logout(Expected {
                    expected_generation: "1".into(),
                })
                .await
        })
    };
    server
        .next()
        .await
        .respond
        .send(Response {
            status: 204,
            headers: vec![],
            body: String::new(),
        })
        .ok();
    assert_eq!(
        running.await.unwrap().err().unwrap().code,
        "credential_storage"
    );
    assert!(fixture.runtime.read().await.credential_present);
    assert_eq!(fixture.runtime.read().await.generation, "1");
    assert_eq!(
        fixture
            .runtime
            .forget(Expected {
                expected_generation: "1".into()
            })
            .await
            .err()
            .unwrap()
            .code,
        "credential_storage"
    );
}
#[tokio::test]
async fn document_reads_allow_bounded_legacy_bodies_without_raising_other_response_limits() {
    for (path, body_size, accepted) in [
        ("/api/v1/documents/123", 1100 * 1024, true),
        (
            "/api/v1/workspaces/123/issues/456/document",
            1100 * 1024,
            true,
        ),
        ("/api/v1/auth/session", 1100 * 1024, false),
        ("/api/v1/documents/123", 3100 * 1024, true),
        ("/api/v1/documents/123", 4 * 1024 * 1024, false),
    ] {
        let mut server = Server::new().await;
        let transport = Transport::new().unwrap();
        let origin = ServerOrigin::parse(&server.origin).unwrap();
        let running = tokio::spawn(async move {
            transport
                .send(
                    &origin,
                    Method::Get,
                    path,
                    None,
                    &BTreeMap::new(),
                    None,
                    None,
                )
                .await
        });
        let body = json!({ "text": "x".repeat(body_size) });
        let expected_length = body.to_string().len();
        server
            .next()
            .await
            .respond
            .send(Response::json(200, body))
            .ok();
        let result = running.await.unwrap();
        if accepted {
            assert_eq!(result.unwrap().bytes.len(), expected_length, "{path}");
        } else {
            assert_eq!(
                result.err().unwrap().code,
                NativeError::server().code,
                "{path}"
            );
        }
    }
}

#[tokio::test]
async fn redirect_bodies_and_credential_headers_never_reach_the_renderer() {
    let mut server = Server::new().await;
    let transport = Transport::new().unwrap();
    let origin = ServerOrigin::parse(&server.origin).unwrap();
    let running = tokio::spawn(async move {
        transport
            .send(
                &origin,
                Method::Get,
                "/api/v1/auth/session",
                None,
                &BTreeMap::new(),
                None,
                Some(&"c".repeat(64)),
            )
            .await
    });
    let request = server.next().await;
    request
        .respond
        .send(Response {
            status: 302,
            headers: vec![(
                "Location",
                "https://other.invalid/api/v1/auth/session".into(),
            )],
            body: String::new(),
        })
        .ok();
    assert_eq!(
        running.await.unwrap().err().unwrap().code,
        "redirect_denied"
    );
    let mut request = api("1");
    request
        .headers
        .insert("Authorization".into(), "Bearer caller-controlled".into());
    assert!(request.check().is_err());
}
async fn start(fixture: &mut Fixture, server: &mut Server) -> (Value, Uuid) {
    let generation = fixture.runtime.read().await.generation;
    let native = fixture.runtime.clone();
    let running = tokio::spawn(async move {
        native
            .begin(BeginRequest {
                expected_generation: generation,
                device_name: "Test desktop".into(),
            })
            .await
    });
    let capabilities = server.next().await;
    assert_eq!(capabilities.path, "/api/v1/client-capabilities");
    assert!(!capabilities.headers.contains_key("authorization"));
    capabilities
        .respond
        .send(Response::json(
            200,
            json!({"api_version":"v1","native_auth_version":1,"desktop_min_protocol":1}),
        ))
        .ok();
    let request = server.next().await;
    assert_eq!(request.path, "/api/v1/auth/native/requests");
    let body = request.body;
    let id = Uuid::new_v4();
    request.respond.send(Response::json(201,json!({"id":id,"authorization_path":format!("/auth/native/approve?request_id={id}"),"confirmation_code":"ABCDEFGH","expires_at":(chrono::Utc::now()+chrono::Duration::minutes(10)).to_rfc3339()}))).ok();
    let reply = running.await.unwrap().unwrap();
    assert_eq!(reply.request_id, id);
    assert_eq!(fixture.event().await.kind, "pending");
    (body, id)
}
fn grant(credential: &CredentialRef, token: &str) -> Value {
    json!({"token_type":"Bearer","access_token":token,"session_id":credential.session_id,"expires_at":(chrono::Utc::now()+chrono::Duration::days(1)).to_rfc3339(),"session":{"user":{"id":credential.user_id,"email":"native@example.test","display_name":"Native"},"csrf_token":"","audience":"desktop","workspaces":[]}})
}
async fn callback(body: &Value, id: Uuid) {
    let mut url = url::Url::parse(body["callback_uri"].as_str().unwrap()).unwrap();
    url.query_pairs_mut()
        .append_pair("request_id", &id.to_string())
        .append_pair("code", &"d".repeat(64))
        .append_pair("state", body["state"].as_str().unwrap());
    let response = reqwest::get(url).await.unwrap();
    assert_eq!(response.status(), 200);
}
#[tokio::test]
async fn pkce_completion_uses_os_store_and_reauthentication_preserves_scope() {
    use base64::Engine;
    use sha2::Digest;
    let mut server = Server::new().await;
    let mut fixture = Fixture::new(&server, None);
    let credential = reference();
    let token = "e".repeat(64);
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    let exchange = server.next().await;
    assert_eq!(exchange.path, "/api/v1/auth/native/exchange");
    assert!(!exchange.headers.contains_key("authorization"));
    assert_eq!(
        body["code_challenge"],
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(
            exchange.body["code_verifier"].as_str().unwrap().as_bytes()
        ))
    );
    exchange
        .respond
        .send(Response::json(200, grant(&credential, &token)))
        .ok();
    let authenticated = fixture.event().await;
    assert_eq!(authenticated.kind, "authenticated");
    let serialized = serde_json::to_string(&authenticated).unwrap();
    assert!(!serialized.contains(&token));
    assert!(!serialized.contains("access_token"));
    assert_eq!(authenticated.scope_generation, "2");
    assert!(
        fixture
            .vault
            .get(&server.origin, &credential)
            .unwrap()
            .is_some()
    );
    let mut replacement = credential.clone();
    replacement.session_id = Uuid::new_v4();
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    server
        .next()
        .await
        .respond
        .send(Response::json(200, grant(&replacement, &"f".repeat(64))))
        .ok();
    let reauthenticated = fixture.event().await;
    assert_eq!(reauthenticated.kind, "authenticated");
    assert_eq!(reauthenticated.scope_generation, "2");
    assert_eq!(reauthenticated.generation, "3");
}
#[tokio::test]
async fn cancellation_is_rejected_once_exchange_may_have_rotated_the_session() {
    let mut server = Server::new().await;
    let mut fixture = Fixture::new(&server, None);
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    let exchange = server.next().await;
    let error = fixture
        .runtime
        .cancel_auth(Expected {
            expected_generation: "1".into(),
        })
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "authentication_finishing");
    assert!(fixture.runtime.read().await.pending.is_some());
    let credential = reference();
    exchange
        .respond
        .send(Response::json(200, grant(&credential, &"f".repeat(64))))
        .ok();
    assert_eq!(fixture.event().await.kind, "authenticated");
    assert!(fixture.runtime.read().await.credential_present);
}
#[tokio::test]
async fn storage_failure_revokes_the_issued_session_and_reports_no_success() {
    let mut server = Server::new().await;
    let mut fixture = Fixture::new(&server, None);
    fixture.vault.fail_set.store(true, Ordering::SeqCst);
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    let credential = reference();
    server
        .next()
        .await
        .respond
        .send(Response::json(200, grant(&credential, &"f".repeat(64))))
        .ok();
    let revoke = server.next().await;
    assert_eq!(revoke.path, "/api/v1/auth/logout");
    assert!(revoke.headers.contains_key("authorization"));
    revoke
        .respond
        .send(Response {
            status: 204,
            headers: vec![],
            body: String::new(),
        })
        .ok();
    let failed = fixture.event().await;
    assert_eq!(failed.kind, "failed");
    assert_eq!(failed.error.unwrap().code, "credential_storage");
    assert!(!fixture.runtime.read().await.credential_present);
    assert!(fixture.vault.values.lock().unwrap().is_empty());
}

#[tokio::test]
async fn self_invalidation_clears_local_credentials_and_preserves_the_command_receipt() {
    let mut server = Server::new().await;
    let credential = reference();
    let mut fixture = Fixture::new(&server, Some(credential.clone()));
    fixture
        .vault
        .set(&server.origin, &credential, &"a".repeat(64))
        .unwrap();
    let running = {
        let native = fixture.runtime.clone();
        tokio::spawn(async move { native.api(api("1")).await })
    };
    let request = server.next().await;
    request
        .respond
        .send(Response {
            status: 200,
            headers: vec![
                ("Content-Type", "application/json".into()),
                ("X-OpenTrack-Session-Invalidated", "true".into()),
                ("Set-Cookie", "server-session=hidden".into()),
                ("Authorization", "Bearer hidden".into()),
                ("ETag", "\"entity:2\"".into()),
            ],
            body: json!({"committed":true}).to_string(),
        })
        .ok();
    let reply = running.await.unwrap().unwrap();
    assert_eq!(reply.generation, "1");
    assert_eq!(reply.body.unwrap()["committed"], true);
    assert_eq!(reply.headers.get("etag").unwrap(), "\"entity:2\"");
    assert!(!reply.headers.contains_key("set-cookie"));
    assert!(!reply.headers.contains_key("authorization"));
    let event = fixture.event().await;
    assert_eq!(event.kind, "signed_out");
    assert_eq!(event.error.unwrap().code, "access_changed");
    assert_eq!(event.generation, "2");
    assert!(!fixture.runtime.read().await.credential_present);
    assert!(
        fixture
            .vault
            .get(&server.origin, &credential)
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn issue_links_remain_untrusted_and_dismissal_cannot_remove_a_newer_link() {
    use opentrack_desktop::state::ClearLinkRequest;
    let server = Server::new().await;
    let fixture = Fixture::new(&server, None);
    let original = fixture.runtime.read().await;
    let issue = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let url = format!(
        "opentrack://open?server=https%3A%2F%2Funtrusted.example&workspace={workspace}&issue={issue}"
    );
    let first = fixture.runtime.accept_link(&url).await.unwrap();
    let second = fixture.runtime.accept_link(&url).await.unwrap();
    assert_ne!(first.id, second.id);
    fixture
        .runtime
        .clear_link(ClearLinkRequest { id: first.id })
        .await
        .unwrap();
    assert_eq!(fixture.runtime.issue_link().await.unwrap().id, second.id);
    assert_eq!(fixture.runtime.read().await.generation, original.generation);
    assert_eq!(
        fixture.runtime.read().await.selected.unwrap().origin,
        server.origin
    );
    fixture
        .runtime
        .clear_link(ClearLinkRequest { id: second.id })
        .await
        .unwrap();
    assert!(fixture.runtime.issue_link().await.is_none());
}
#[test]
fn private_configuration_is_atomic_and_contains_references_only() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("opentrack-config-test-{}", Uuid::new_v4()));
    let persistence = Persistence::new(directory.clone()).unwrap();
    let credential = reference();
    let configuration = Configuration {
        selected: Some("https://server.example".into()),
        servers: vec![SavedServer {
            origin: "https://server.example".into(),
            development_http: false,
            credential: Some(credential.clone()),
        }],
    };
    persistence.save(&configuration).unwrap();
    let text = std::fs::read_to_string(directory.join("connections.toml")).unwrap();
    assert!(text.contains(&credential.session_id.to_string()));
    assert!(!text.contains("token"));
    assert!(!text.contains("password"));
    assert_eq!(
        std::fs::metadata(directory.join("connections.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(persistence.load().unwrap().selected, configuration.selected);
    std::fs::set_permissions(
        directory.join("connections.toml"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(persistence.load().is_err());
    std::fs::remove_file(directory.join("connections.toml")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", directory.join("connections.toml")).unwrap();
    assert!(persistence.load().is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn timed_out_keyring_write_reports_failure_before_late_cleanup_finishes() {
    struct ReleaseOnDrop(Arc<StorageGate>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            *self.0.released.lock().unwrap() = true;
            self.0.wake.notify_all();
        }
    }
    let mut server = Server::new().await;
    let mut fixture = Fixture::new(&server, None);
    let gate = StorageGate::new();
    let release = ReleaseOnDrop(gate.clone());
    *fixture.vault.gate.lock().unwrap() = Some(gate.clone());
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    server
        .next()
        .await
        .respond
        .send(Response::json(200, grant(&reference(), &"d".repeat(64))))
        .ok();
    gate.entered.notified().await;
    let revoke = tokio::time::timeout(Duration::from_secs(12), server.requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(revoke.path, "/api/v1/auth/logout");
    revoke
        .respond
        .send(Response {
            status: 204,
            headers: vec![],
            body: String::new(),
        })
        .ok();
    // Failure must be visible while the OS call is still held. Late cleanup is
    // already scheduled and must not enqueue a second foreground ten-second wait.
    let failure = tokio::time::timeout(Duration::from_secs(2), fixture.events.recv()).await;
    drop(release);
    let failure = failure.unwrap().unwrap();
    assert_eq!(failure.kind, "failed");
    assert_eq!(failure.error.unwrap().code, "credential_storage");
    tokio::time::timeout(Duration::from_secs(2), fixture.vault.deleted.notified())
        .await
        .unwrap();
    assert!(fixture.vault.values.lock().unwrap().is_empty());
    assert!(!fixture.runtime.read().await.credential_present);
}

#[tokio::test]
async fn server_switch_during_keyring_write_revokes_and_removes_the_late_credential() {
    let mut server = Server::new().await;
    let mut fixture = Fixture::new(&server, None);
    let gate = StorageGate::new();
    *fixture.vault.gate.lock().unwrap() = Some(gate.clone());
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    let credential = reference();
    server
        .next()
        .await
        .respond
        .send(Response::json(200, grant(&credential, &"d".repeat(64))))
        .ok();
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    let second = Server::new().await;
    fixture
        .runtime
        .select(SelectRequest {
            expected_generation: "1".into(),
            origin: second.origin.clone(),
            development_http: true,
        })
        .await
        .unwrap();
    *gate.released.lock().unwrap() = true;
    gate.wake.notify_all();
    let revoke = server.next().await;
    assert_eq!(revoke.path, "/api/v1/auth/logout");
    revoke
        .respond
        .send(Response {
            status: 204,
            headers: vec![],
            body: String::new(),
        })
        .ok();
    tokio::time::timeout(Duration::from_secs(5), fixture.vault.deleted.notified())
        .await
        .unwrap();
    assert!(fixture.vault.values.lock().unwrap().is_empty());
    assert!(!fixture.runtime.read().await.credential_present);
    assert!(fixture.events.try_recv().is_err());
}

#[tokio::test]
async fn locked_credential_store_blocks_dispatch_without_downgrading_to_anonymous() {
    let mut server = Server::new().await;
    let credential = reference();
    let fixture = Fixture::new(&server, Some(credential));
    fixture.vault.fail_get.store(true, Ordering::SeqCst);
    assert_eq!(
        fixture.runtime.api(api("1")).await.err().unwrap().code,
        "credential_storage"
    );
    assert!(fixture.runtime.read().await.credential_present);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), server.requests.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn credential_store_write_then_panic_takes_the_revoke_and_cleanup_path() {
    let mut server = Server::new().await;
    let mut fixture = Fixture::new(&server, None);
    fixture.vault.panic_after_set.store(true, Ordering::SeqCst);
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    let credential = reference();
    server
        .next()
        .await
        .respond
        .send(Response::json(200, grant(&credential, &"f".repeat(64))))
        .ok();
    let revoke = server.next().await;
    assert_eq!(revoke.path, "/api/v1/auth/logout");
    revoke
        .respond
        .send(Response {
            status: 204,
            headers: vec![],
            body: String::new(),
        })
        .ok();
    let failed = fixture.event().await;
    assert_eq!(failed.kind, "failed");
    assert_eq!(failed.error.unwrap().code, "credential_storage");
    assert!(fixture.vault.values.lock().unwrap().is_empty());
    assert!(!fixture.runtime.read().await.credential_present);
}
#[tokio::test]
async fn cancellation_before_callback_closes_listener_without_losing_prior_session() {
    let mut server = Server::new().await;
    let credential = reference();
    let mut fixture = Fixture::new(&server, Some(credential.clone()));
    fixture
        .vault
        .set(&server.origin, &credential, &"b".repeat(64))
        .unwrap();
    let (body, _) = start(&mut fixture, &mut server).await;
    let state = fixture
        .runtime
        .cancel_auth(Expected {
            expected_generation: "1".into(),
        })
        .await
        .unwrap();
    assert!(state.credential_present);
    assert!(state.pending.is_none());
    assert_eq!(fixture.event().await.kind, "cancelled");
    let url = url::Url::parse(body["callback_uri"].as_str().unwrap()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if tokio::net::TcpStream::connect((url.host_str().unwrap(), url.port().unwrap()))
                .await
                .is_err()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        fixture
            .vault
            .get(&server.origin, &credential)
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn unconfirmed_reauthentication_forgets_the_possibly_rotated_prior_session() {
    let mut server = Server::new().await;
    let credential = reference();
    let mut fixture = Fixture::new(&server, Some(credential.clone()));
    fixture
        .vault
        .set(&server.origin, &credential, &"a".repeat(64))
        .unwrap();
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    server
        .next()
        .await
        .respond
        .send(Response::json(200, json!({"incomplete":true})))
        .ok();
    let event = fixture.event().await;
    assert_eq!(event.kind, "signed_out");
    assert_eq!(event.error.unwrap().code, "reauthentication_uncertain");
    assert_eq!(event.remote_revocation_confirmed, Some(false));
    assert!(!fixture.runtime.read().await.credential_present);
    assert!(
        fixture
            .vault
            .get(&server.origin, &credential)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn uncertain_old_exchange_invalidates_the_same_credential_after_switching_away_and_back() {
    let mut server = Server::new().await;
    let second = Server::new().await;
    let credential = reference();
    let mut fixture = Fixture::new(&server, Some(credential.clone()));
    fixture
        .vault
        .set(&server.origin, &credential, &"c".repeat(64))
        .unwrap();
    let (body, id) = start(&mut fixture, &mut server).await;
    callback(&body, id).await;
    let exchange = server.next().await;
    fixture
        .runtime
        .select(SelectRequest {
            expected_generation: "1".into(),
            origin: second.origin.clone(),
            development_http: true,
        })
        .await
        .unwrap();
    fixture
        .runtime
        .select(SelectRequest {
            expected_generation: "2".into(),
            origin: server.origin.clone(),
            development_http: true,
        })
        .await
        .unwrap();
    exchange
        .respond
        .send(Response::json(200, json!({"incomplete":true})))
        .ok();
    let event = fixture.event().await;
    assert_eq!(event.kind, "signed_out");
    assert_eq!(event.error.unwrap().code, "reauthentication_uncertain");
    assert_eq!(event.generation, "4");
    assert_eq!(fixture.runtime.read().await.generation, "4");
    assert!(!fixture.runtime.read().await.credential_present);
}

#[tokio::test]
async fn an_untrusted_tls_certificate_is_rejected_without_sending_http_credentials() {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let cert = CertificateDer::from(include_bytes!("fixtures/untrusted-test-cert.der").to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        include_bytes!("fixtures/untrusted-test-key.der").to_vec(),
    ));
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin =
        ServerOrigin::parse(&format!("https://{}", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        assert!(acceptor.accept(socket).await.is_err());
    });
    let result = Transport::new()
        .unwrap()
        .send(
            &origin,
            Method::Get,
            "/api/v1/auth/session",
            None,
            &BTreeMap::new(),
            None,
            Some(&"a".repeat(64)),
        )
        .await;
    assert_eq!(result.err().unwrap().code, "network_error");
    server.await.unwrap();
}
