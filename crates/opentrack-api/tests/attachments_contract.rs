//! Exercise bytes, metadata and document references across the actual HTTP boundary.
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
use std::{path::PathBuf, time::Duration};
use support::{Harness, ORIGIN};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
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
    fn expect(&self, status: StatusCode) {
        assert_eq!(
            self.status,
            status,
            "{}",
            String::from_utf8_lossy(&self.bytes)
        );
    }
    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
}
async fn call(
    r: &Router,
    c: Option<&Client>,
    method: &str,
    path: &str,
    body: Vec<u8>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(c) = c {
        request = request
            .header("cookie", &c.cookie)
            .header("x-csrf-token", &c.csrf);
    }
    for (k, v) in extra {
        request = request.header(*k, *v);
    }
    let response = r
        .clone()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    Reply {
        status,
        headers,
        bytes,
    }
}
async fn json_call(
    r: &Router,
    c: Option<&Client>,
    method: &str,
    path: &str,
    body: Value,
    extra: &[(&str, &str)],
) -> Reply {
    let mut headers = extra.to_vec();
    headers.push(("content-type", "application/json"));
    call(
        r,
        c,
        method,
        path,
        serde_json::to_vec(&body).unwrap(),
        &headers,
    )
    .await
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("opentrack-http-blobs-{}", Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[tokio::test]
async fn streamed_upload_link_download_and_failure_contract() {
    let root = Directory::new();
    let store = LocalBlobStore::open(LocalBlobConfig {
        root: root.0.clone(),
        forbidden_roots: vec![std::env::current_dir().unwrap()],
        max_upload_bytes: 25 * 1024 * 1024,
        upload_idle_timeout: Duration::from_secs(5),
        scanner: ScannerConfig::default(),
    })
    .await
    .unwrap();
    let h = Harness::new()
        .await
        .with_attachments(AttachmentService::new(store, AttachmentLimits::default()));
    let copy = h.clone();
    let blob_root = root.0.clone();
    let outcome = tokio::spawn(async move { exercise(&copy, &blob_root).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness, blob_root: &std::path::Path) {
    let r = &h.router;
    let setup=json_call(r,None,"POST","/api/v1/setup",json!({"setup_token":h.setup_token,"email":"owner@attachments.invalid","display_name":"File owner","password":"Attachments HTTP verification passphrase 2026!","workspace_name":"Files HTTP","team_name":"Engineering","team_key":"ENG"}),&[]).await;
    setup.expect(StatusCode::CREATED);
    let s = setup.json();
    let c = Client {
        cookie: setup.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: s["csrf_token"].as_str().unwrap().into(),
    };
    let base = format!(
        "/api/v1/workspaces/{}",
        s["workspaces"][0]["id"].as_str().unwrap()
    );
    let bootstrap = call(
        r,
        Some(&c),
        "GET",
        &format!("{base}/bootstrap"),
        vec![],
        &[],
    )
    .await
    .json();
    let team = bootstrap["teams"][0]["id"].as_str().unwrap();
    let files = format!("{base}/attachments");
    // More than the ordinary 1MiB JSON limit: body is streamed, never extracted as JSON.
    let bytes = vec![b'X'; 2 * 1024 * 1024 + 17];
    let sha = digest(&bytes);
    let size = bytes.len().to_string();
    let reserved=json_call(r,Some(&c),"POST",&files,json!({"team_id":team,"original_name":"design é.txt","size_bytes":bytes.len(),"sha256":sha,"mime_hint":"text/plain"}),&[("idempotency-key","http-file-1")]).await;
    reserved.expect(StatusCode::CREATED);
    let item = reserved.json();
    let path = format!("{files}/{}", item["id"].as_str().unwrap());
    assert_eq!(reserved.headers["location"], path);
    let metadata = call(r, Some(&c), "GET", &path, vec![], &[]).await;
    metadata.expect(StatusCode::OK);
    assert_eq!(metadata.json(), item);
    let content = format!("{path}/content");
    assert!(item.get("storage_key").is_none());
    assert!(item.get("lease_token").is_none());
    assert_eq!(item["state"], "pending");
    call(r, None, "GET", &content, vec![], &[])
        .await
        .expect(StatusCode::UNAUTHORIZED);
    call(r, Some(&c), "GET", &content, vec![], &[])
        .await
        .expect(StatusCode::NOT_FOUND);
    let bad = Client {
        csrf: "wrong".into(),
        ..c.clone()
    };
    call(
        r,
        Some(&bad),
        "PUT",
        &content,
        bytes.clone(),
        &[("if-match", reserved.etag()), ("content-length", &size)],
    )
    .await
    .expect(StatusCode::FORBIDDEN);
    call(
        r,
        Some(&c),
        "PUT",
        &content,
        bytes.clone(),
        &[("if-match", reserved.etag())],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    call(
        r,
        Some(&c),
        "PUT",
        &content,
        bytes.clone(),
        &[("content-length", &size)],
    )
    .await
    .expect(StatusCode::PRECONDITION_REQUIRED);
    let uploaded = call(
        r,
        Some(&c),
        "PUT",
        &content,
        bytes.clone(),
        &[
            ("if-match", reserved.etag()),
            ("content-length", &size),
            ("content-type", "application/octet-stream"),
        ],
    )
    .await;
    uploaded.expect(StatusCode::OK);
    assert_eq!(uploaded.json()["state"], "ready");
    assert_eq!(uploaded.json()["scan_status"], "not_scanned");
    let replay = call(
        r,
        Some(&c),
        "PUT",
        &content,
        bytes.clone(),
        &[("if-match", reserved.etag()), ("content-length", &size)],
    )
    .await;
    replay.expect(StatusCode::OK);
    assert_eq!(replay.json()["id"], item["id"]);
    let received = call(r, Some(&c), "GET", &content, vec![], &[]).await;
    received.expect(StatusCode::OK);
    assert_eq!(received.bytes, bytes);
    assert_eq!(received.headers["content-type"], "application/octet-stream");
    assert_eq!(received.headers["x-content-type-options"], "nosniff");
    assert_eq!(received.headers["cache-control"], "no-store");
    assert!(
        received.headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("design%20%C3%A9.txt")
    );
    // A missing object is an integrity failure, not an ordinary unavailable link.
    let objects = std::fs::read_dir(blob_root.join("objects"))
        .unwrap()
        .map(|v| v.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(objects.len(), 1);
    let hidden = blob_root.join("test-hidden-object");
    std::fs::rename(&objects[0], &hidden).unwrap();
    let unavailable = call(r, Some(&c), "GET", &content, vec![], &[]).await;
    unavailable.expect(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(unavailable.json()["code"], "dependency_unavailable");
    std::fs::rename(hidden, &objects[0]).unwrap();
    let body = json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"attachment","attrs":{"id":item["id"]}}]}});
    let created = json_call(
        r,
        Some(&c),
        "POST",
        &format!("{base}/issues"),
        json!({"team_id":team,"title":"File-backed issue","document":body}),
        &[("idempotency-key", "http-file-issue")],
    )
    .await;
    created.expect(StatusCode::CREATED);
    let issue = format!("{base}/issues/{}", created.json()["id"].as_str().unwrap());
    let linked = call(r, Some(&c), "GET", &path, vec![], &[]).await;
    linked.expect(StatusCode::OK);
    assert!(linked.json()["document_id"].is_string());
    assert!(linked.json()["expires_at"].is_null());
    json_call(
        r,
        Some(&c),
        "POST",
        &format!("{base}/issues"),
        json!({"team_id":team,"title":"Cannot steal linked bytes","document":body}),
        &[("idempotency-key", "http-file-reuse")],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    call(
        r,
        Some(&c),
        "DELETE",
        &path,
        vec![],
        &[("if-match", linked.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    call(r, Some(&c), "GET", &content, vec![], &[])
        .await
        .expect(StatusCode::NOT_FOUND);
    let retained = call(
        r,
        Some(&c),
        "GET",
        &format!("{issue}/document"),
        vec![],
        &[],
    )
    .await;
    retained.expect(StatusCode::OK);
    assert_eq!(
        retained.json()["body"]["content"][0]["attrs"]["id"],
        item["id"]
    );
    // Hash failure never becomes ready; retryable transport status is distinct from bad bytes.
    let reserved = json_call(
        r,
        Some(&c),
        "POST",
        &files,
        json!({"team_id":team,"original_name":"bad.txt","size_bytes":4,"sha256":digest(b"good")}),
        &[("idempotency-key", "http-file-bad")],
    )
    .await;
    reserved.expect(StatusCode::CREATED);
    let path = format!("{files}/{}", reserved.json()["id"].as_str().unwrap());
    call(
        r,
        Some(&c),
        "PUT",
        &format!("{path}/content"),
        b"evil".to_vec(),
        &[("if-match", reserved.etag()), ("content-length", "4")],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        call(r, Some(&c), "GET", &path, vec![], &[]).await.json()["state"],
        "rejected"
    );
    json_call(
        r,
        Some(&c),
        "POST",
        &files,
        json!({"team_id":team,"original_name":"../escape","size_bytes":4,"sha256":digest(b"good")}),
        &[("idempotency-key", "http-file-path")],
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
#[cfg(unix)]
async fn required_scanner_unavailable_keeps_bytes_quarantined() {
    let root = Directory::new();
    let store = LocalBlobStore::open(LocalBlobConfig {
        root: root.0.clone(),
        forbidden_roots: vec![std::env::current_dir().unwrap()],
        max_upload_bytes: 25 * 1024 * 1024,
        upload_idle_timeout: Duration::from_secs(5),
        scanner: ScannerConfig {
            socket: Some(root.0.join("absent-scanner.socket")),
            required: true,
            timeout: Duration::from_secs(1),
        },
    })
    .await
    .unwrap();
    let limits = AttachmentLimits {
        antivirus_required: true,
        ..AttachmentLimits::default()
    };
    let h = Harness::new()
        .await
        .with_attachments(AttachmentService::new(store, limits));
    let test = h.clone();
    let outcome=tokio::spawn(async move{
        let r=&test.router;
        let setup=json_call(r,None,"POST","/api/v1/setup",json!({"setup_token":test.setup_token,"email":"owner@scan.invalid","display_name":"Scan owner","password":"Required scanner HTTP passphrase 2026!","workspace_name":"Scanner HTTP","team_name":"Engineering","team_key":"ENG"}),&[]).await;
        setup.expect(StatusCode::CREATED);let session=setup.json();
        let client=Client{cookie:setup.headers["set-cookie"].to_str().unwrap().split(';').next().unwrap().into(),csrf:session["csrf_token"].as_str().unwrap().into()};
        let base=format!("/api/v1/workspaces/{}",session["workspaces"][0]["id"].as_str().unwrap());
        let boot=call(r,Some(&client),"GET",&format!("{base}/bootstrap"),vec![],&[]).await.json();
        let team=boot["teams"][0]["id"].as_str().unwrap();
        let reservation=json_call(r,Some(&client),"POST",&format!("{base}/attachments"),json!({"team_id":team,"original_name":"scan.txt","size_bytes":4,"sha256":digest(b"test")}),&[("idempotency-key","scan-unavailable")]).await;
        reservation.expect(StatusCode::CREATED);let id=reservation.json()["id"].as_str().unwrap().to_owned();let path=format!("{base}/attachments/{id}");
        let transferred=call(r,Some(&client),"PUT",&format!("{path}/content"),b"test".to_vec(),&[("if-match",reservation.etag()),("content-length","4")]).await;
        transferred.expect(StatusCode::ACCEPTED);assert_eq!(transferred.json()["state"],"quarantined");assert_eq!(transferred.json()["scan_status"],"unavailable");
        call(r,Some(&client),"GET",&format!("{path}/content"),vec![],&[]).await.expect(StatusCode::NOT_FOUND);
        let retried=call(r,Some(&client),"POST",&format!("{path}/finalize"),vec![],&[]).await;
        retried.expect(StatusCode::ACCEPTED);assert_eq!(retried.json()["state"],"quarantined");
        let doc=json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"attachment","attrs":{"id":id}}]}});
        json_call(r,Some(&client),"POST",&format!("{base}/issues"),json!({"team_id":team,"title":"Must not link unscanned","document":doc}),&[("idempotency-key","reject-quarantine-link")]).await.expect(StatusCode::UNPROCESSABLE_ENTITY);
    }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
