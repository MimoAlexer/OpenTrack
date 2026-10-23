#![cfg(target_os = "linux")]

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
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    time::Duration,
};
use support::{Harness, ORIGIN};
use tower::ServiceExt;

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
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(client) = client {
        request = request
            .header("cookie", &client.cookie)
            .header("x-csrf-token", &client.csrf);
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let reply = router
        .clone()
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = reply.status();
    let headers = reply.headers().clone();
    let bytes = to_bytes(reply.into_body(), 3 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    Reply {
        status,
        headers,
        bytes,
    }
}

async fn json_request(
    router: &Router,
    client: Option<&Client>,
    path: &str,
    body: Value,
    headers: &[(&str, &str)],
) -> Reply {
    let mut headers = headers.to_vec();
    headers.push(("content-type", "application/json"));
    request(
        router,
        client,
        "POST",
        path,
        serde_json::to_vec(&body).unwrap(),
        &headers,
    )
    .await
}

#[tokio::test]
#[ignore = "requires scripts/qualify-attachment-storage.sh and its private bounded tmpfs"]
async fn disk_full_keeps_reservation_pending_and_same_attachment_recovers() {
    let namespace = fs::read_link("/proc/self/ns/mnt").unwrap();
    assert!(
        std::env::var_os("OPENTRACK_QUALIFICATION_PARENT_MOUNT")
            .is_some_and(|parent| parent.as_os_str() != namespace.as_os_str()),
        "A distinct launcher mount namespace is required"
    );
    let mount = PathBuf::from(
        std::env::var_os("OPENTRACK_TEST_FULL_DISK_ROOT")
            .expect("Run the storage qualification launcher"),
    );
    let metadata = fs::symlink_metadata(&mount).unwrap();
    assert!(
        mount.is_absolute()
            && metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && fs::canonicalize(&mount).unwrap() == mount
            && fs::read_dir(&mount).unwrap().next().is_none(),
        "A fresh nonsymlink empty storage mount is required"
    );
    let filesystem = std::process::Command::new("findmnt")
        .args(["-n", "-b", "-o", "FSTYPE,SIZE", "--mountpoint"])
        .arg(&mount)
        .output()
        .unwrap();
    assert!(filesystem.status.success());
    assert_eq!(
        String::from_utf8(filesystem.stdout)
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["tmpfs", "4194304"]
    );
    let root = mount.join("blobs");
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let store = LocalBlobStore::open(LocalBlobConfig {
        root: root.clone(),
        forbidden_roots: vec![std::env::current_dir().unwrap()],
        max_upload_bytes: 25 * 1024 * 1024,
        upload_idle_timeout: Duration::from_secs(5),
        scanner: ScannerConfig::default(),
    })
    .await
    .unwrap();
    let harness = Harness::new()
        .await
        .with_attachments(AttachmentService::new(store, AttachmentLimits::default()));
    let copy = harness.clone();
    let outcome = tokio::spawn(async move { exercise(&copy, &mount, &root).await }).await;
    harness.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(harness: &Harness, mount: &Path, root: &Path) {
    let router = &harness.router;
    let setup = json_request(
        router,
        None,
        "/api/v1/setup",
        json!({
            "setup_token": harness.setup_token, "email": "owner@example.test",
            "display_name": "Example Owner", "password": "Storage qualification fixture 246!",
            "workspace_name": "Storage qualification", "team_name": "Engineering", "team_key": "ENG"
        }),
        &[],
    )
    .await;
    assert_eq!(setup.status, StatusCode::CREATED);
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
    assert_eq!(bootstrap.status, StatusCode::OK);
    let team = bootstrap.json()["teams"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let bytes = vec![b'x'; 2 * 1024 * 1024];
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let reserved = json_request(
        router,
        Some(&client),
        &format!("{base}/attachments"),
        json!({
            "team_id": team, "original_name": "storage-qualification.bin",
            "size_bytes": bytes.len(), "sha256": digest
        }),
        &[("idempotency-key", "storage-qualification")],
    )
    .await;
    assert_eq!(reserved.status, StatusCode::CREATED);
    let id = reserved.json()["id"].as_str().unwrap().to_owned();
    let metadata = format!("{base}/attachments/{id}");
    let content = format!("{metadata}/content");
    let length = bytes.len().to_string();
    let filler = mount.join("filler");
    let mut file = fs::File::create_new(&filler).unwrap();
    file.write_all(&vec![b'f'; 3 * 1024 * 1024]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let failed = request(
        router,
        Some(&client),
        "PUT",
        &content,
        bytes.clone(),
        &[
            ("if-match", reserved.etag()),
            ("content-length", &length),
            ("content-type", "application/octet-stream"),
        ],
    )
    .await;
    assert_eq!(failed.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(failed.json()["code"], "dependency_unavailable");
    let pending = request(router, Some(&client), "GET", &metadata, vec![], &[]).await;
    assert_eq!(pending.status, StatusCode::OK);
    assert_eq!(pending.json()["id"], id);
    assert_eq!(pending.json()["state"], "pending");
    assert_ne!(pending.etag(), reserved.etag());
    assert_eq!(
        request(router, Some(&client), "GET", &content, vec![], &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(fs::read_dir(root.join("objects")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(root.join("staging")).unwrap().count(), 0);
    fs::remove_file(filler).unwrap();
    let recovered = request(
        router,
        Some(&client),
        "PUT",
        &content,
        bytes,
        &[
            ("if-match", pending.etag()),
            ("content-length", &length),
            ("content-type", "application/octet-stream"),
        ],
    )
    .await;
    assert_eq!(recovered.status, StatusCode::OK);
    assert_eq!(recovered.json()["id"], id);
    assert_eq!(recovered.json()["state"], "ready");
    assert_eq!(recovered.json()["scan_status"], "not_scanned");
    let downloaded = request(router, Some(&client), "GET", &content, vec![], &[]).await;
    assert_eq!(downloaded.status, StatusCode::OK);
    assert_eq!(downloaded.bytes.len(), 2 * 1024 * 1024);
    assert_eq!(format!("{:x}", Sha256::digest(&downloaded.bytes)), digest);
    assert_eq!(fs::read_dir(root.join("objects")).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root.join("staging")).unwrap().count(), 0);
}
