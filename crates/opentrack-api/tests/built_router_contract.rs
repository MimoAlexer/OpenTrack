//! Exercise the production router, including the frontend's emitted files.
#[allow(dead_code)]
mod support;

use axum::Router;
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Reply {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

async fn request(address: SocketAddr, method: &str, path: &str, accept: &str) -> Reply {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: {accept}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        const LIMIT: u64 = 16 * 1024 * 1024;
        socket.take(LIMIT).read_to_end(&mut bytes).await.unwrap();
        assert!(bytes.len() < LIMIT as usize, "HTTP response exceeded its bound");
        let boundary = bytes.windows(4).position(|value| value == b"\r\n\r\n").unwrap();
        let mut lines = std::str::from_utf8(&bytes[..boundary]).unwrap().split("\r\n");
        let status = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
        let headers = lines.map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        }).collect();
        Reply { status, headers, body: bytes[boundary + 4..].to_vec() }
    }).await.expect("Loopback HTTP request exceeded its deadline")
}

async fn start(
    router: Router,
) -> (
    SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    (address, stop, server)
}

#[tokio::test]
async fn real_router_separates_navigation_health_capabilities_and_missing_resources() {
    let harness = support::Harness::new().await;
    let fixture = harness.clone();
    let result = tokio::spawn(async move { boundaries(&fixture).await }).await;
    harness.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn boundaries(harness: &support::Harness) {
    let (address, stop, server) = start(harness.router.clone()).await;
    for (path, status) in [("/health/live", "live"), ("/health/ready", "ready")] {
        let reply = request(address, "GET", path, "application/json").await;
        assert_eq!(reply.status, 200);
        assert_eq!(reply.json(), json!({"status": status}));
        assert_eq!(reply.headers["cache-control"], "no-store");
    }
    let capabilities = request(address, "GET", "/api/v1/capabilities", "application/json").await;
    assert_eq!(capabilities.status, 200);
    assert_eq!(capabilities.headers["cache-control"], "no-store");
    assert_eq!(
        capabilities.json(),
        json!({
            "api_version": "v1", "version": env!("CARGO_PKG_VERSION"),
            "features": {"issues": true, "projects": true, "attachments": false, "ai": false, "agents": false, "workspace_settings": true},
            "limits": {"title_characters": 256, "page_size": 100, "description_bytes": 262144, "max_upload_bytes": 0, "workspace_quota_bytes": 0}
        })
    );
    let contract = request(address, "GET", "/api/v1/openapi.json", "application/json").await;
    assert_eq!(contract.status, 200);
    assert_eq!(contract.headers["cache-control"], "no-store");
    let snapshot: Value =
        serde_json::from_str(include_str!("../../../docs/api/openapi.json")).unwrap();
    assert_eq!(
        contract.json(),
        snapshot,
        "The live schema must match the committed generator input"
    );
    assert_eq!(
        snapshot["paths"]["/api/v1/capabilities"]["get"]["security"],
        json!([{}])
    );
    assert_eq!(
        snapshot["paths"]["/api/v1/capabilities"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/Capabilities"
    );
    configured_capabilities(harness).await;

    for accept in [
        "text/html",
        "application/json, text/html;q=0.5",
        "TEXT/HTML; charset=utf-8; Q=1.000",
    ] {
        let get = request(
            address,
            "GET",
            "/workspace/issues/ENG-42?tab=activity",
            accept,
        )
        .await;
        assert_eq!(get.status, 200, "{accept}");
        assert_eq!(get.body, support::INDEX.as_bytes());
        assert_eq!(get.headers["cache-control"], "no-cache");
        assert_eq!(get.headers["vary"], "Accept");
        let head = request(address, "HEAD", "/workspace/issues/ENG-42", accept).await;
        assert_eq!(head.status, 200);
        assert!(head.body.is_empty());
        for header in ["content-type", "cache-control", "content-length", "vary"] {
            assert_eq!(head.headers[header], get.headers[header]);
        }
    }
    for accept in [
        "application/json",
        "*/*",
        "text/htmlish",
        "text/html;q=0",
        "text/html;q=0.000,*/*;q=1",
        "text/html;q=invalid",
        "text/html;q=1.001",
        "text/html;q=0.1234",
        "text/html;q=.5",
        "text/html;q=0.5;q=1",
        "text/html;broken",
        "text/html;q=",
    ] {
        let reply = request(address, "GET", "/workspace/issues/ENG-42", accept).await;
        assert_eq!(reply.status, 404, "{accept} must not select the HTML shell");
        assert_eq!(reply.headers["content-type"], "application/problem+json");
    }
    for path in [
        "/api",
        "/api/missing",
        "/api/v1/missing",
        "/api/v2/missing",
        "/health",
        "/health/missing",
        "/assets/missing",
        "/assets/missing.js",
        "/missing.css",
        "/theme.missing.js",
    ] {
        let reply = request(address, "GET", path, "text/html").await;
        assert_eq!(
            reply.status, 404,
            "{path} must stay separate from the HTML shell"
        );
        assert_ne!(reply.body, support::INDEX.as_bytes());
        assert_eq!(reply.headers["cache-control"], "no-store");
    }
    assert_eq!(
        request(address, "POST", "/workspace/issues/ENG-42", "text/html")
            .await
            .status,
        405
    );
    let missing = PathBuf::from("work").join(format!("missing-static-{}", uuid::Uuid::new_v4()));
    let (missing_address, missing_stop, missing_server) =
        start(harness.serving_static(missing)).await;
    let unavailable = request(missing_address, "GET", "/", "text/html").await;
    assert_eq!(unavailable.status, 503);
    assert_eq!(unavailable.headers["cache-control"], "no-store");
    missing_stop.send(()).unwrap();
    missing_server.await.unwrap();
    harness.close_runtime().await;
    let unavailable = request(address, "GET", "/health/ready", "application/json").await;
    assert_eq!(unavailable.status, 503);
    assert_eq!(unavailable.json(), json!({"status": "unavailable"}));
    assert_eq!(unavailable.headers["cache-control"], "no-store");
    assert_eq!(
        request(address, "GET", "/health/live", "application/json")
            .await
            .status,
        200
    );
    assert_eq!(
        request(address, "GET", "/api/v1/capabilities", "application/json")
            .await
            .status,
        200
    );
    stop.send(()).unwrap();
    server.await.unwrap();
}

async fn configured_capabilities(harness: &support::Harness) {
    use opentrack_adapters::local_blob::{LocalBlobConfig, LocalBlobStore, ScannerConfig};
    use opentrack_domain::AttachmentLimits;
    let root =
        std::env::temp_dir().join(format!("opentrack-capabilities-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let store = LocalBlobStore::open(LocalBlobConfig {
        root: root.clone(),
        forbidden_roots: vec![std::env::current_dir().unwrap()],
        max_upload_bytes: 4096,
        upload_idle_timeout: Duration::from_secs(5),
        scanner: ScannerConfig::default(),
    })
    .await
    .unwrap();
    let configured = harness
        .clone()
        .with_attachments(opentrack_api::AttachmentService::new(
            store,
            AttachmentLimits {
                max_upload_bytes: 4096,
                workspace_quota_bytes: 8192,
                ..AttachmentLimits::default()
            },
        ));
    let (address, stop, server) = start(configured.router.clone()).await;
    let reply = request(address, "GET", "/api/v1/capabilities", "application/json").await;
    assert_eq!(reply.status, 200);
    let capability = reply.json();
    assert_eq!(capability["features"]["attachments"], true);
    assert_eq!(capability["limits"]["max_upload_bytes"], 4096);
    assert_eq!(capability["limits"]["workspace_quota_bytes"], 8192);
    assert!(
        !String::from_utf8(reply.body)
            .unwrap()
            .contains(root.to_str().unwrap())
    );
    stop.send(()).unwrap();
    server.await.unwrap();
    drop(configured);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
#[ignore = "requires the production frontend: npm --prefix web run build"]
async fn production_browser_files_are_served_by_the_real_router() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    assert!(
        directory.join("index.html").is_file(),
        "Build web/ before this explicit contract check"
    );
    let harness = support::Harness::new().await;
    let fixture = harness.clone();
    let result = tokio::spawn(async move { built_files(&fixture, directory).await }).await;
    harness.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn built_files(harness: &support::Harness, directory: PathBuf) {
    let (address, stop, server) = start(harness.serving_static(directory.clone())).await;
    let index = request(address, "GET", "/", "text/html").await;
    assert_eq!(index.status, 200);
    assert_eq!(
        index.body,
        std::fs::read(directory.join("index.html")).unwrap()
    );
    let html = std::str::from_utf8(&index.body).unwrap();
    assert!(html.contains("id=\"root\""));
    assert!(
        !html.contains("/src/main.tsx"),
        "Fetch the emitted production entrypoint"
    );
    let references: Vec<_> = html
        .split('"')
        .filter(|part| part.starts_with('/'))
        .collect();
    assert!(references.contains(&"/theme.js"));
    assert!(
        references
            .iter()
            .any(|part| part.starts_with("/assets/") && part.ends_with(".js"))
    );
    assert!(
        references
            .iter()
            .any(|part| part.starts_with("/assets/") && part.ends_with(".css"))
    );
    let mut files = vec![directory.join("theme.js")];
    let mut pending = vec![directory.join("assets")];
    while let Some(parent) = pending.pop() {
        for entry in std::fs::read_dir(parent).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    for reference in &references {
        assert!(
            files.iter().any(|path| path
                .strip_prefix(&directory)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
                == reference.trim_start_matches('/')),
            "Every HTML dependency must be among the served files: {reference}"
        );
    }
    assert!(
        files.iter().any(|file| file
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("worker")),
        "Exercise emitted document-worker chunks too"
    );
    for file in files {
        let path = format!(
            "/{}",
            file.strip_prefix(&directory)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        let reply = request(address, "GET", &path, "*/*").await;
        assert_eq!(reply.status, 200, "{path}");
        assert_eq!(reply.body, std::fs::read(&file).unwrap(), "{path}");
        assert_eq!(reply.headers["x-content-type-options"], "nosniff");
        let content_type = reply.headers["content-type"].as_str();
        match file.extension().and_then(|value| value.to_str()) {
            Some("js") => assert!(matches!(
                content_type,
                "text/javascript" | "application/javascript"
            )),
            Some("css") => assert_eq!(content_type, "text/css"),
            Some("map") => assert!(matches!(content_type, "application/json" | "text/plain")),
            _ => panic!("Review the MIME contract for the newly emitted asset {path}"),
        }
        assert_eq!(
            reply.headers["cache-control"],
            if path == "/theme.js" {
                "no-cache"
            } else {
                "public, max-age=31536000, immutable"
            }
        );
        let head = request(address, "HEAD", &path, "*/*").await;
        assert_eq!(head.status, 200, "{path}");
        assert!(head.body.is_empty());
        for header in ["content-type", "content-length", "cache-control"] {
            assert_eq!(
                head.headers[header], reply.headers[header],
                "{path}: {header}"
            );
        }
    }
    let deep = request(address, "GET", "/workspace/issues/ENG-42", "text/html").await;
    assert_eq!(deep.status, 200);
    assert_eq!(deep.body, index.body);
    for path in [
        "/api/v1/not-found",
        "/assets/not-found.js",
        "/assets/not-found",
        "/not-found.css",
    ] {
        assert_eq!(request(address, "GET", path, "text/html").await.status, 404);
    }
    stop.send(()).unwrap();
    server.await.unwrap();
}
