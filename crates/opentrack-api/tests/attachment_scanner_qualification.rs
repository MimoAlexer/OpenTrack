#![cfg(target_os = "linux")]
//! Installed ClamAV 1.5+ profile; launcher checks its version before compilation.
//! A private benign file-hash signature does not qualify official CVD trust or feeds.
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use opentrack_adapters::local_blob::{LocalBlobConfig, LocalBlobStore, ScannerConfig};
use opentrack_api::AttachmentService;
use opentrack_domain::AttachmentLimits;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use support::{Harness, ORIGIN};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use tower::ServiceExt;

struct Clamd {
    root: PathBuf,
    binary: PathBuf,
    child: Option<Child>,
}
impl Clamd {
    fn new(detected: &[u8]) -> Self {
        let binary = PathBuf::from(
            std::env::var_os("OPENTRACK_TEST_CLAMD_BIN")
                .expect("Explicit trusted OPENTRACK_TEST_CLAMD_BIN is required"),
        );
        assert!(
            binary.is_absolute() && binary.is_file(),
            "A real clamd executable is required"
        );
        let root = PathBuf::from(
            std::env::var_os("OPENTRACK_TEST_CLAMD_ROOT")
                .expect("Use scripts/qualify-attachment-scanner.sh for private daemon cleanup"),
        );
        assert!(
            root.parent()
                .is_some_and(|parent| parent == std::path::Path::new("/tmp"))
                && root.file_name().is_some_and(|name| name
                    .to_string_lossy()
                    .starts_with("opentrack-scanner-qualification-"))
                && fs::symlink_metadata(&root).unwrap().is_dir()
                && fs::metadata(&root).unwrap().permissions().mode() & 0o777 == 0o700
                && fs::read_dir(&root).unwrap().next().is_none(),
            "An empty private launcher-created scanner directory is required"
        );
        for directory in ["signatures", "temporary", "blobs", "certificates"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(directory))
                .unwrap();
        }
        // Official format: https://docs.clamav.net/manual/Signatures/HashSignatures.html
        fs::write(
            root.join("signatures/qualification.hsb"),
            format!(
                "{}:{}:OpenTrack.Qualification\n",
                digest(detected),
                detected.len()
            ),
        )
        .unwrap();
        // https://github.com/Cisco-Talos/clamav/blob/main/etc/clamd.conf.sample
        fs::write(root.join("clamd.conf"), format!(
            "Foreground yes\nLocalSocket {}\nLocalSocketMode 600\nFixStaleSocket no\nDatabaseDirectory {}\nTemporaryDirectory {}\nCVDCertsDirectory {}\nLogSyslog no\nMaxThreads 1\nMaxQueue 2\nStreamMaxLength 1M\nMaxScanSize 1M\nMaxFileSize 1M\nMaxScanTime 2000\nReadTimeout 3\nCommandReadTimeout 3\nSelfCheck 0\nConcurrentDatabaseReload no\nDisableCache yes\n",
            root.join("clamd.sock").display(), root.join("signatures").display(), root.join("temporary").display(), root.join("certificates").display()
        )).unwrap();
        Self {
            root,
            binary,
            child: None,
        }
    }
    fn socket(&self) -> PathBuf {
        self.root.join("clamd.sock")
    }
    async fn start(&mut self) {
        assert!(self.child.is_none(), "Only one owned scanner may run");
        let socket = self.socket();
        if socket.exists() {
            fs::remove_file(&socket).unwrap();
        }
        let mut command = Command::new(&self.binary);
        command
            .arg("--foreground")
            .arg("--config-file")
            .arg(self.root.join("clamd.conf"))
            .current_dir(&self.root)
            .env_clear()
            .env("LANG", "C")
            .env("TZ", "UTC")
            .env("TMPDIR", self.root.join("temporary"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(libraries) = std::env::var_os("OPENTRACK_TEST_CLAMD_LIBRARY_PATH") {
            assert!(
                std::env::split_paths(&libraries).all(|path| path.is_absolute() && path.is_dir()),
                "Scanner library directories must exist and be absolute"
            );
            command.env("LD_LIBRARY_PATH", libraries);
        }
        self.child = Some(
            command
                .spawn()
                .expect("Cannot start the explicitly configured scanner"),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                self.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                "Installed clamd exited before readiness"
            );
            if ping(&socket).await {
                assert_eq!(
                    fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Installed clamd readiness deadline exceeded"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            assert!(
                terminate(&mut child),
                "Owned scanner termination deadline exceeded"
            );
        }
    }
}
impl Drop for Clamd {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            terminate(&mut child);
        }
    }
}
fn terminate(child: &mut Child) -> bool {
    let _ = child.kill();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if child.try_wait().is_ok_and(|status| status.is_some()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}
async fn ping(socket: &std::path::Path) -> bool {
    tokio::time::timeout(Duration::from_millis(200), async {
        let mut connection = UnixStream::connect(socket).await?;
        connection.write_all(b"zPING\0").await?;
        let mut reply = [0; 5];
        connection.read_exact(&mut reply).await?;
        Ok::<_, std::io::Error>(reply == *b"PONG\0")
    })
    .await
    .is_ok_and(|result| result.unwrap_or(false))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
struct Client {
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
    fn expect(&self, expected: StatusCode) {
        assert_eq!(self.status, expected);
    }
    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
}
async fn request(
    router: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    bytes: Vec<u8>,
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
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        router
            .clone()
            .oneshot(builder.body(Body::from(bytes)).unwrap()),
    )
    .await
    .expect("Scanner HTTP deadline exceeded")
    .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    Reply {
        status,
        headers,
        bytes,
    }
}
async fn post(
    router: &Router,
    client: Option<&Client>,
    path: &str,
    body: Value,
    key: &str,
) -> Reply {
    request(
        router,
        client,
        "POST",
        path,
        serde_json::to_vec(&body).unwrap(),
        &[
            ("content-type", "application/json"),
            ("idempotency-key", key),
        ],
    )
    .await
}
async fn upload(
    router: &Router,
    client: &Client,
    base: &str,
    team: &str,
    bytes: &[u8],
    key: &str,
) -> (String, Reply) {
    let reservation = post(router, Some(client), &format!("{base}/attachments"), json!({"team_id":team,"original_name":"qualification.bin","size_bytes":bytes.len(),"sha256":digest(bytes)}), key).await;
    reservation.expect(StatusCode::CREATED);
    let path = format!(
        "{base}/attachments/{}",
        reservation.json()["id"].as_str().unwrap()
    );
    let reply = request(
        router,
        Some(client),
        "PUT",
        &format!("{path}/content"),
        bytes.to_vec(),
        &[
            ("if-match", reservation.etag()),
            ("content-length", &bytes.len().to_string()),
            ("content-type", "application/octet-stream"),
        ],
    )
    .await;
    assert_eq!(reply.json()["id"], reservation.json()["id"]);
    (path, reply)
}
async fn unavailable(router: &Router, client: &Client, path: &str) {
    request(
        router,
        Some(client),
        "GET",
        &format!("{path}/content"),
        vec![],
        &[],
    )
    .await
    .expect(StatusCode::NOT_FOUND);
}
async fn download(router: &Router, client: &Client, path: &str, bytes: &[u8]) {
    let reply = request(
        router,
        Some(client),
        "GET",
        &format!("{path}/content"),
        vec![],
        &[],
    )
    .await;
    reply.expect(StatusCode::OK);
    assert_eq!(reply.headers["cache-control"], "no-store");
    assert_eq!(reply.headers["x-content-type-options"], "nosniff");
    assert_eq!(reply.bytes, bytes);
    assert_eq!(digest(&reply.bytes), digest(bytes));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Requires explicitly trusted installed clamd and isolated PostgreSQL; use scripts/qualify-attachment-scanner.sh"]
async fn installed_clamd_clean_detection_and_outage_recovery() {
    let detected: Vec<u8> = (0..4096)
        .map(|index| ((index * 37 + 0xa7) % 256) as u8)
        .collect();
    let daemon = Clamd::new(&detected);
    let store = LocalBlobStore::open(LocalBlobConfig {
        root: daemon.root.join("blobs"),
        forbidden_roots: vec![std::env::current_dir().unwrap()],
        max_upload_bytes: 1024 * 1024,
        upload_idle_timeout: Duration::from_secs(5),
        scanner: ScannerConfig {
            socket: Some(daemon.socket()),
            required: true,
            timeout: Duration::from_secs(3),
        },
    })
    .await
    .unwrap();
    let h = Harness::new()
        .await
        .with_attachments(AttachmentService::new(
            store,
            AttachmentLimits {
                antivirus_required: true,
                ..AttachmentLimits::default()
            },
        ));
    let copy = h.clone();
    let run = tokio::spawn(async move {
        let mut daemon = daemon;
        tokio::time::timeout(
            Duration::from_secs(75),
            exercise(&copy, &mut daemon, &detected),
        )
        .await
        .expect("Installed scanner qualification deadline exceeded");
    })
    .await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness, daemon: &mut Clamd, detected: &[u8]) {
    daemon.start().await;
    let router = &h.router;
    let setup = post(router, None, "/api/v1/setup", json!({"setup_token":h.setup_token,"email":"owner@example.test","display_name":"Example Owner","password":"Installed scanner qualification 724!","workspace_name":"Scanner qualification","team_name":"Engineering","team_key":"ENG"}), "scanner-setup").await;
    setup.expect(StatusCode::CREATED);
    let session = setup.json();
    let client = Client {
        cookie: setup.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: session["csrf_token"].as_str().unwrap().into(),
    };
    let base = format!(
        "/api/v1/workspaces/{}",
        session["workspaces"][0]["id"].as_str().unwrap()
    );
    let bootstrap = request(
        router,
        Some(&client),
        "GET",
        &format!("{base}/bootstrap"),
        vec![],
        &[],
    )
    .await;
    bootstrap.expect(StatusCode::OK);
    let boot = bootstrap.json();
    let team = boot["teams"][0]["id"].as_str().unwrap();
    let mut clean = detected.to_vec();
    clean[0] ^= 0xff;
    let (clean_path, clean_reply) =
        upload(router, &client, &base, team, &clean, "scanner-clean").await;
    clean_reply.expect(StatusCode::OK);
    assert_eq!(clean_reply.json()["state"], "ready");
    assert_eq!(clean_reply.json()["scan_status"], "clean");
    download(router, &client, &clean_path, &clean).await;
    let (detected_path, rejected) =
        upload(router, &client, &base, team, detected, "scanner-detected").await;
    rejected.expect(StatusCode::OK);
    assert_eq!(rejected.json()["state"], "rejected");
    assert_eq!(rejected.json()["scan_status"], "infected");
    unavailable(router, &client, &detected_path).await;
    daemon.stop();
    assert!(
        !ping(&daemon.socket()).await,
        "Only the stopped owned daemon may serve this socket"
    );
    let (recovery_path, quarantined) =
        upload(router, &client, &base, team, &clean, "scanner-outage").await;
    quarantined.expect(StatusCode::ACCEPTED);
    assert_eq!(quarantined.json()["state"], "quarantined");
    assert_eq!(quarantined.json()["scan_status"], "unavailable");
    unavailable(router, &client, &recovery_path).await;
    let retried = request(
        router,
        Some(&client),
        "POST",
        &format!("{recovery_path}/finalize"),
        vec![],
        &[],
    )
    .await;
    retried.expect(StatusCode::ACCEPTED);
    assert_eq!(retried.json()["id"], quarantined.json()["id"]);
    assert_eq!(retried.json()["state"], "quarantined");
    assert_eq!(retried.json()["scan_status"], "unavailable");
    unavailable(router, &client, &recovery_path).await;
    daemon.start().await;
    let recovered = request(
        router,
        Some(&client),
        "POST",
        &format!("{recovery_path}/finalize"),
        vec![],
        &[],
    )
    .await;
    recovered.expect(StatusCode::OK);
    assert_eq!(recovered.json()["id"], quarantined.json()["id"]);
    assert_eq!(recovered.json()["state"], "ready");
    assert_eq!(recovered.json()["scan_status"], "clean");
    download(router, &client, &recovery_path, &clean).await;
    let replay = request(
        router,
        Some(&client),
        "POST",
        &format!("{recovery_path}/finalize"),
        vec![],
        &[],
    )
    .await;
    replay.expect(StatusCode::OK);
    assert_eq!(replay.json(), recovered.json());
    let list = request(
        router,
        Some(&client),
        "GET",
        &format!("{base}/attachments?team_id={team}"),
        vec![],
        &[],
    )
    .await;
    list.expect(StatusCode::OK);
    assert_eq!(list.json()["data"].as_array().unwrap().len(), 3);
    daemon.stop();
}
