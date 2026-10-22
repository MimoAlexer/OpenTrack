use opentrack_desktop::{
    attachments::{CancelRequest, DownloadRequest, UploadRequest},
    credentials::{CredentialRef, Vault},
    persistence::{Configuration, Persistence, SavedServer},
    protocol::NativeError,
    state::{Expected, Runtime, SelectRequest},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use uuid::Uuid;
use zeroize::Zeroizing;

struct TestVault;
impl Vault for TestVault {
    fn get(&self, _: &str, _: &CredentialRef) -> Result<Option<Zeroizing<String>>, NativeError> {
        Ok(Some(Zeroizing::new("fixture-native-bearer".into())))
    }
    fn set(&self, _: &str, _: &CredentialRef, _: &str) -> Result<(), NativeError> {
        Ok(())
    }
    fn delete(&self, _: &str, _: &CredentialRef) -> Result<(), NativeError> {
        Ok(())
    }
}
struct Fixture {
    runtime: Arc<Runtime>,
    root: PathBuf,
}
impl Fixture {
    fn new(origin: &str) -> Self {
        let root = std::env::temp_dir().join(format!("ot-attach-native-{}", Uuid::new_v4()));
        let persistence = Persistence::new(root.clone()).unwrap();
        persistence
            .save(&Configuration {
                selected: Some(origin.into()),
                servers: vec![SavedServer {
                    origin: origin.into(),
                    development_http: true,
                    credential: Some(CredentialRef {
                        user_id: Uuid::new_v4(),
                        session_id: Uuid::new_v4(),
                    }),
                }],
            })
            .unwrap();
        let runtime = Runtime::new(
            persistence,
            Arc::new(TestVault),
            Arc::new(|_| {}),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
        Self { runtime, root }
    }
    fn file(&self, bytes: &[u8]) -> PathBuf {
        let path = self.root.join("chosen-file.bin");
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[derive(Debug)]
struct Response {
    status: u16,
    mime: &'static str,
    body: Vec<u8>,
}
struct Request {
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    reply: oneshot::Sender<Response>,
}
struct Server {
    origin: String,
    requests: mpsc::Receiver<Request>,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (send, requests) = mpsc::channel(4);
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                let mut chunk = [0; 8192];
                let header_end = loop {
                    let count = socket.read(&mut chunk).await.unwrap_or(0);
                    if count == 0 {
                        return;
                    }
                    data.extend_from_slice(&chunk[..count]);
                    if let Some(end) = data.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        break end + 4;
                    }
                    assert!(data.len() < 65536);
                };
                let head = String::from_utf8(data[..header_end].to_vec()).unwrap();
                let mut lines = head.split("\r\n");
                let path = lines
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_string();
                let headers: BTreeMap<_, _> = lines
                    .filter_map(|line| {
                        line.split_once(':').map(|(key, value)| {
                            (key.to_ascii_lowercase(), value.trim().to_string())
                        })
                    })
                    .collect();
                let length = headers
                    .get("content-length")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                assert!(length <= 2 * 1024 * 1024);
                while data.len() < header_end + length {
                    let count = socket.read(&mut chunk).await.unwrap_or(0);
                    if count == 0 {
                        return;
                    }
                    data.extend_from_slice(&chunk[..count]);
                }
                let body = data[header_end..header_end + length].to_vec();
                let (reply, receive) = oneshot::channel();
                if send
                    .send(Request {
                        path,
                        headers,
                        body,
                        reply,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                if let Ok(response) = receive.await {
                    let header = format!(
                        "HTTP/1.1 {} Response\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.status,
                        response.mime,
                        response.body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(&response.body).await;
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
        tokio::time::timeout(Duration::from_secs(5), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn upload(handle: Uuid) -> UploadRequest {
    UploadRequest {
        expected_generation: "1".into(),
        workspace_id: Uuid::new_v4(),
        attachment_id: Uuid::new_v4(),
        handle,
        etag: "\"attachment-fixture\"".into(),
        operation_id: Uuid::new_v4(),
    }
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[tokio::test]
async fn picked_descriptor_uploads_exact_bytes_without_disclosing_path_and_rejects_modified_file() {
    let mut server = Server::new().await;
    let fixture = Fixture::new(&server.origin);
    let bytes = vec![b'x'; 150_000];
    let path = fixture.file(&bytes);
    let picked = fixture
        .runtime
        .accept_attachment_selection(
            Expected {
                expected_generation: "1".into(),
            },
            vec![path.clone()],
        )
        .await
        .unwrap()
        .remove(0);
    assert_eq!(picked.sha256, sha(&bytes));
    let public = serde_json::to_string(&picked).unwrap();
    assert!(!public.contains(fixture.root.to_str().unwrap()));
    assert!(public.contains("chosen-file.bin"));
    let retained = fixture.root.join("retained.bin");
    fs::rename(&path, &retained).unwrap();
    fs::write(&path, b"path substitution").unwrap();
    let request = upload(picked.handle);
    let operation = request.operation_id;
    let runtime = fixture.runtime.clone();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let output = progress.clone();
    let task = tokio::spawn(async move {
        runtime
            .attachment_upload(
                request,
                Arc::new(move |event| output.lock().unwrap().push(event)),
            )
            .await
    });
    let received = server.next().await;
    assert!(received.path.ends_with("/content"));
    assert_eq!(received.body, bytes);
    assert_eq!(
        received.headers["authorization"],
        "Bearer fixture-native-bearer"
    );
    assert_eq!(received.headers["content-length"], "150000");
    received
        .reply
        .send(Response {
            status: 200,
            mime: "application/json",
            body: br#"{"state":"ready"}"#.to_vec(),
        })
        .unwrap();
    assert_eq!(task.await.unwrap().unwrap().status, 200);
    assert!(
        progress
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.operation_id == operation && p.sent_bytes == 150_000)
    );
    fs::write(retained, b"changed after picker").unwrap();
    let error = fixture
        .runtime
        .attachment_upload(upload(picked.handle), Arc::new(|_| {}))
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "selected_file_changed");
    assert!(server.requests.try_recv().is_err());
}

#[tokio::test]
async fn handle_boundaries_reject_symlinks_too_many_files_and_stale_generations() {
    let server = Server::new().await;
    let fixture = Fixture::new(&server.origin);
    let path = fixture.file(b"selected");
    let alias = fixture.root.join("symlink");
    symlink(&path, &alias).unwrap();
    assert!(
        fixture
            .runtime
            .accept_attachment_selection(
                Expected {
                    expected_generation: "1".into()
                },
                vec![alias]
            )
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .runtime
            .accept_attachment_selection(
                Expected {
                    expected_generation: "1".into()
                },
                vec![path.clone(); 17]
            )
            .await
            .err()
            .unwrap()
            .code,
        "file_handle_limit"
    );
    let picked = fixture
        .runtime
        .accept_attachment_selection(
            Expected {
                expected_generation: "1".into(),
            },
            vec![path],
        )
        .await
        .unwrap()
        .remove(0);
    let state = fixture
        .runtime
        .select(SelectRequest {
            expected_generation: "1".into(),
            origin: "http://127.0.0.1:1".into(),
            development_http: true,
        })
        .await
        .unwrap();
    assert_eq!(
        fixture
            .runtime
            .attachment_upload(upload(picked.handle), Arc::new(|_| {}))
            .await
            .err()
            .unwrap()
            .code,
        "stale_generation"
    );
    let mut request = upload(picked.handle);
    request.expected_generation = state.generation;
    assert_eq!(
        fixture
            .runtime
            .attachment_upload(request, Arc::new(|_| {}))
            .await
            .err()
            .unwrap()
            .code,
        "file_handle_expired"
    );
}

#[tokio::test]
async fn cancellation_before_upload_registration_prevents_network_dispatch_and_is_bounded() {
    let mut server = Server::new().await;
    let fixture = Fixture::new(&server.origin);
    let picked = fixture
        .runtime
        .accept_attachment_selection(
            Expected {
                expected_generation: "1".into(),
            },
            vec![fixture.file(b"cancel before registration")],
        )
        .await
        .unwrap()
        .remove(0);
    let request = upload(picked.handle);
    fixture
        .runtime
        .attachment_cancel(CancelRequest {
            expected_generation: "1".into(),
            operation_id: request.operation_id,
        })
        .await
        .unwrap();
    assert_eq!(
        fixture
            .runtime
            .attachment_upload(request, Arc::new(|_| {}))
            .await
            .err()
            .unwrap()
            .code,
        "attachment_cancelled"
    );
    assert!(server.requests.try_recv().is_err());
    for _ in 1..64 {
        fixture
            .runtime
            .attachment_cancel(CancelRequest {
                expected_generation: "1".into(),
                operation_id: Uuid::new_v4(),
            })
            .await
            .unwrap();
    }
    assert_eq!(
        fixture
            .runtime
            .attachment_cancel(CancelRequest {
                expected_generation: "1".into(),
                operation_id: Uuid::new_v4(),
            })
            .await
            .unwrap_err()
            .code,
        "attachment_cancel_queue_full"
    );
    assert_eq!(
        fixture
            .runtime
            .attachment_upload(upload(picked.handle), Arc::new(|_| {}))
            .await
            .err()
            .unwrap()
            .code,
        "attachment_cancel_queue_full"
    );
    assert!(server.requests.try_recv().is_err());
    assert_eq!(
        fixture
            .runtime
            .attachment_cancel(CancelRequest {
                expected_generation: "0".into(),
                operation_id: Uuid::new_v4(),
            })
            .await
            .unwrap_err()
            .code,
        "stale_generation"
    );
}

#[tokio::test]
async fn cancellation_and_server_change_discard_late_upload_receipts() {
    for switch in [false, true] {
        let mut server = Server::new().await;
        let fixture = Fixture::new(&server.origin);
        let path = fixture.file(b"bytes");
        let picked = fixture
            .runtime
            .accept_attachment_selection(
                Expected {
                    expected_generation: "1".into(),
                },
                vec![path],
            )
            .await
            .unwrap()
            .remove(0);
        let request = upload(picked.handle);
        let id = request.operation_id;
        let runtime = fixture.runtime.clone();
        let task =
            tokio::spawn(async move { runtime.attachment_upload(request, Arc::new(|_| {})).await });
        let received = server.next().await;
        if switch {
            fixture
                .runtime
                .select(SelectRequest {
                    expected_generation: "1".into(),
                    origin: "http://127.0.0.1:1".into(),
                    development_http: true,
                })
                .await
                .unwrap();
        } else {
            fixture
                .runtime
                .attachment_cancel(CancelRequest {
                    expected_generation: "1".into(),
                    operation_id: id,
                })
                .await
                .unwrap();
        }
        assert_eq!(
            task.await.unwrap().err().unwrap().code,
            "attachment_cancelled"
        );
        let _ = received.reply.send(Response {
            status: 200,
            mime: "application/json",
            body: br#"{"state":"ready"}"#.to_vec(),
        });
    }
}

#[tokio::test]
async fn downloads_verify_length_and_hash_before_private_atomic_publication() {
    for corrupt in [false, true] {
        let mut server = Server::new().await;
        let fixture = Fixture::new(&server.origin);
        let destination = fixture.root.join("saved.bin");
        let chosen = destination.clone();
        let runtime = fixture.runtime.clone();
        let attachment = Uuid::new_v4();
        let data = b"verified attachment";
        let task = tokio::spawn(async move {
            runtime
                .attachment_download(
                    DownloadRequest {
                        expected_generation: "1".into(),
                        workspace_id: Uuid::new_v4(),
                        attachment_id: attachment,
                        original_name: "ignored-client-hint.bin".into(),
                    },
                    move |name| async move {
                        assert_eq!(name, "server-name.bin");
                        Some(chosen)
                    },
                )
                .await
        });
        let metadata = server.next().await;
        metadata.reply.send(Response{status:200,mime:"application/json",body:serde_json::to_vec(&json!({"id":attachment,"state":"ready","original_name":"server-name.bin","size_bytes":data.len(),"sha256":sha(data)})).unwrap()}).unwrap();
        let content = server.next().await;
        assert!(content.path.ends_with("/content"));
        assert_eq!(
            content.headers["authorization"],
            "Bearer fixture-native-bearer"
        );
        let bytes = if corrupt {
            vec![b'x'; data.len()]
        } else {
            data.to_vec()
        };
        content
            .reply
            .send(Response {
                status: 200,
                mime: "application/octet-stream",
                body: bytes,
            })
            .unwrap();
        let result = task.await.unwrap();
        if corrupt {
            assert_eq!(result.err().unwrap().code, "attachment_integrity");
            assert!(!destination.exists());
        } else {
            assert!(result.unwrap().saved);
            assert_eq!(fs::read(&destination).unwrap(), data);
            assert_eq!(fs::metadata(&destination).unwrap().mode() & 0o7777, 0o600);
        }
        assert!(!fs::read_dir(&fixture.root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".part")
        }));
    }
}
