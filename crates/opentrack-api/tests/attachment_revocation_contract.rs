//! Saved attachment URLs replayed through Axum, real PostgreSQL and local blobs.
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use opentrack_adapters::local_blob::{LocalBlobConfig, LocalBlobStore, ScannerConfig};
use opentrack_api::{ApiState, AttachmentService, router};
use opentrack_app::{Operator, generate_recovery_code};
use opentrack_domain::AttachmentLimits;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc, time::Duration};
use support::Harness;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

const ORIGIN: &str = "https://attachment-revocation.example.test";
const PASSWORD: &str = "Attachment contract initial passphrase 2026!";
const REPLACEMENT: &str = "Attachment contract replacement passphrase 2026!";
const MEMBER_EMAIL: &str = "member@example.test";
const BYTES: &[u8] = b"private-attachment-byte-marker-74019\nSynthetic file content.\n";
const NAME: &str = "private-attachment-name-74019.txt";
const VERIFIER: &str = "eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHg";
const CHALLENGE: &str = "r2OTv5p8bMf1Vxfo6zus0GD5OCMULnxAyfo2HEtvEp0";

#[derive(Clone)]
enum Credential {
    Web { cookie: String, csrf: String },
    Native { bearer: String, session_id: String },
}
#[derive(Clone)]
struct Client {
    credential: Credential,
    user_id: String,
}
impl Client {
    fn web(reply: &Reply) -> Self {
        let session = reply.json();
        Self {
            credential: Credential::Web {
                cookie: reply.headers["set-cookie"]
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .into(),
                csrf: session["csrf_token"].as_str().unwrap().into(),
            },
            user_id: session["user"]["id"].as_str().unwrap().into(),
        }
    }
}
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}
impl Reply {
    #[track_caller]
    fn expect(&self, status: StatusCode) {
        // Authentication replies contain credentials; never dump their bodies.
        assert_eq!(self.status, status);
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap()
    }
    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
}
async fn raw(
    r: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    bytes: Vec<u8>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut request = Request::builder().method(method).uri(path);
    match client.map(|c| &c.credential) {
        Some(Credential::Web { cookie, csrf }) => {
            request = request
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .header("origin", ORIGIN);
        }
        Some(Credential::Native { bearer, .. }) => {
            request = request.header("authorization", bearer);
        }
        None if !path.starts_with("/api/v1/auth/native/") => {
            request = request.header("origin", ORIGIN);
        }
        None => {}
    }
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let response = r
        .clone()
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
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
async fn request(
    r: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut headers = extra.to_vec();
    if body.is_some() {
        headers.push(("content-type", "application/json"));
    }
    raw(
        r,
        client,
        method,
        path,
        body.map(|v| serde_json::to_vec(&v).unwrap())
            .unwrap_or_default(),
        &headers,
    )
    .await
}
async fn get(r: &Router, client: &Client, path: &str) -> Reply {
    let reply = request(r, Some(client), "GET", path, None, &[]).await;
    reply.expect(StatusCode::OK);
    reply
}
async fn login(r: &Router, previous: Option<&Client>, password: &str) -> Client {
    let reply = request(
        r,
        previous,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email":MEMBER_EMAIL,"password":password})),
        &[],
    )
    .await;
    reply.expect(StatusCode::OK);
    Client::web(&reply)
}
async fn native(r: &Router, browser: &Client) -> Client {
    let start = request(r, None, "POST", "/api/v1/auth/native/requests", Some(json!({
        "code_challenge":CHALLENGE,"callback_uri":"http://127.0.0.1:38419/opentrack/native-callback",
        "state":"1111111111111111111111111111111111111111111111111111111111111111",
        "device_name":"Example desktop"
    })), &[]).await;
    start.expect(StatusCode::CREATED);
    let start = start.json();
    let approved = request(
        r,
        Some(browser),
        "POST",
        &format!(
            "/api/v1/auth/native/requests/{}/approve",
            start["id"].as_str().unwrap()
        ),
        Some(json!({"confirmation_code":start["confirmation_code"]})),
        &[],
    )
    .await;
    approved.expect(StatusCode::OK);
    let callback = Url::parse(approved.json()["callback_uri"].as_str().unwrap()).unwrap();
    let code = callback
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .into_owned();
    let exchanged = request(
        r,
        None,
        "POST",
        "/api/v1/auth/native/exchange",
        Some(json!({"request_id":start["id"],"code":code,"code_verifier":VERIFIER})),
        &[],
    )
    .await;
    exchanged.expect(StatusCode::OK);
    let grant = exchanged.json();
    Client {
        credential: Credential::Native {
            bearer: format!("Bearer {}", grant["access_token"].as_str().unwrap()),
            session_id: grant["session_id"].as_str().unwrap().into(),
        },
        user_id: browser.user_id.clone(),
    }
}
async fn session_id(r: &Router, client: &Client) -> String {
    if let Credential::Native { session_id, .. } = &client.credential {
        return session_id.clone();
    }
    get(r, client, "/api/v1/auth/sessions").await.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["current"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("opentrack-revocation-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    router: Router,
    owner: Client,
    member: Client,
    base: String,
    team: String,
}
impl Fixture {
    async fn new(h: &Harness, root: &Directory) -> Self {
        let store = LocalBlobStore::open(LocalBlobConfig {
            root: root.0.clone(),
            forbidden_roots: vec![std::env::current_dir().unwrap()],
            max_upload_bytes: 25 * 1024 * 1024,
            upload_idle_timeout: Duration::from_secs(5),
            scanner: ScannerConfig::default(),
        })
        .await
        .unwrap();
        let router = router(ApiState {
            app: h.app.clone(),
            attachments: Some(Arc::new(AttachmentService::new(
                store,
                AttachmentLimits::default(),
            ))),
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(root.0.join("unused-static")),
        });
        let setup = request(&router, None, "POST", "/api/v1/setup", Some(json!({
            "setup_token":h.setup_token,"email":"owner@example.test","display_name":"Example Owner",
            "password":PASSWORD,"workspace_name":"Attachment qualification","team_name":"Public team","team_key":"PUB"
        })), &[]).await;
        setup.expect(StatusCode::CREATED);
        let owner = Client::web(&setup);
        let base = format!(
            "/api/v1/workspaces/{}",
            setup.json()["workspaces"][0]["id"].as_str().unwrap()
        );
        let team = get(&router, &owner, &format!("{base}/bootstrap"))
            .await
            .json()["teams"][0]["id"]
            .as_str()
            .unwrap()
            .into();
        let invited = request(
            &router,
            Some(&owner),
            "POST",
            &format!("{base}/invitations"),
            Some(json!({"email":MEMBER_EMAIL,"role":"member","team_ids":[]})),
            &[("idempotency-key", "example-member")],
        )
        .await;
        invited.expect(StatusCode::CREATED);
        let accepted = request(&router, None, "POST", "/api/v1/invitations/accept",
            Some(json!({"token":invited.json()["token"],"display_name":"Example Member","password":PASSWORD})), &[]).await;
        accepted.expect(StatusCode::OK);
        Self {
            router,
            owner,
            member: Client::web(&accepted),
            base,
            team,
        }
    }
    async fn private_team(&self, key: &str) -> String {
        let reply = request(
            &self.router,
            Some(&self.owner),
            "POST",
            &format!("{}/teams", self.base),
            Some(json!({"name":format!("Private {key}"),"key":key,"private":true})),
            &[("idempotency-key", &format!("private-team-{key}"))],
        )
        .await;
        reply.expect(StatusCode::CREATED);
        reply.json()["id"].as_str().unwrap().into()
    }
    async fn membership(&self, team: &str, add: bool) {
        let path = format!("{}/teams/{team}/members/{}", self.base, self.member.user_id);
        if add {
            request(
                &self.router,
                Some(&self.owner),
                "PUT",
                &path,
                Some(json!({"role":"contributor"})),
                &[("if-none-match", "*")],
            )
            .await
            .expect(StatusCode::CREATED);
        } else {
            let current = get(&self.router, &self.owner, &path).await;
            request(
                &self.router,
                Some(&self.owner),
                "DELETE",
                &path,
                None,
                &[("if-match", current.etag())],
            )
            .await
            .expect(StatusCode::NO_CONTENT);
        }
    }
    async fn move_issue(&self, issue: &str, destination: &str) {
        let current = get(&self.router, &self.owner, issue).await;
        let preview = get(
            &self.router,
            &self.owner,
            &format!("{issue}/move-preview?destination_team_id={destination}"),
        )
        .await
        .json();
        assert!(preview["blockers"].as_array().unwrap().is_empty());
        let moved = request(&self.router, Some(&self.owner), "POST", &format!("{issue}/move"),
            Some(json!({"destination_team_id":destination,"status_id":preview["destination_default_status_id"],
                "assignee_id":null,"label_ids":[],"preview_token":preview["preview_token"]})),
            &[("if-match", current.etag()), ("idempotency-key", &Uuid::new_v4().to_string())]).await;
        moved.expect(StatusCode::OK);
        assert_eq!(moved.json()["team_id"], destination);
    }
    async fn upload(&self, uploader: &Client, team: &str) -> SavedFile {
        let sha = format!("{:x}", Sha256::digest(BYTES));
        let reservation = request(&self.router, Some(uploader), "POST", &format!("{}/attachments", self.base),
            Some(json!({"team_id":team,"original_name":NAME,"size_bytes":BYTES.len(),"sha256":sha,"mime_hint":"text/plain"})),
            &[("idempotency-key", &Uuid::new_v4().to_string())]).await;
        reservation.expect(StatusCode::CREATED);
        // Preserve the server's returned location; replay never obtains a new URL.
        let metadata_url = reservation.headers["location"].to_str().unwrap().to_owned();
        let content_url = format!("{metadata_url}/content");
        let uploaded = raw(
            &self.router,
            Some(uploader),
            "PUT",
            &content_url,
            BYTES.to_vec(),
            &[
                ("if-match", reservation.etag()),
                ("content-length", &BYTES.len().to_string()),
            ],
        )
        .await;
        uploaded.expect(StatusCode::OK);
        assert_eq!(uploaded.json()["state"], "ready");
        SavedFile {
            metadata_url,
            content_url,
            id: uploaded.json()["id"].as_str().unwrap().into(),
            metadata_etag: uploaded.etag().into(),
            sha,
            document_id: None,
        }
    }
    async fn linked_issue(&self, team: &str) -> (SavedFile, String) {
        let mut file = self.upload(&self.owner, team).await;
        let issue = request(&self.router, Some(&self.owner), "POST", &format!("{}/issues", self.base),
            Some(json!({"team_id":team,"title":"Example attachment issue","document":document(&[&file.id])})),
            &[("idempotency-key", &Uuid::new_v4().to_string())]).await;
        issue.expect(StatusCode::CREATED);
        file.capture(&self.router, &self.owner).await;
        (
            file,
            format!(
                "{}/issues/{}",
                self.base,
                issue.json()["id"].as_str().unwrap()
            ),
        )
    }
}
fn document(ids: &[&str]) -> Value {
    json!({"schema_version":1,"body":{"type":"doc","content":ids.iter()
        .map(|id| json!({"type":"attachment","attrs":{"id":id}})).collect::<Vec<_>>()}})
}
struct SavedFile {
    metadata_url: String,
    content_url: String,
    id: String,
    metadata_etag: String,
    sha: String,
    document_id: Option<String>,
}
impl SavedFile {
    async fn capture(&mut self, r: &Router, reader: &Client) {
        let metadata = get(r, reader, &self.metadata_url).await;
        self.metadata_etag = metadata.etag().into();
        self.document_id = metadata.json()["document_id"].as_str().map(str::to_owned);
        self.readable(r, reader).await;
    }
    async fn readable(&self, r: &Router, reader: &Client) {
        let metadata = get(r, reader, &self.metadata_url).await;
        assert_eq!(metadata.json()["original_name"], NAME);
        assert_eq!(metadata.json()["sha256"], self.sha);
        let download = get(r, reader, &self.content_url).await;
        assert_eq!(download.bytes, BYTES);
        assert_eq!(download.headers["content-type"], "application/octet-stream");
        assert_eq!(download.headers["content-length"], BYTES.len().to_string());
        assert_eq!(download.headers["x-content-type-options"], "nosniff");
        assert!(
            download.headers["content-disposition"]
                .to_str()
                .unwrap()
                .contains(NAME)
        );
        assert_eq!(format!("{:x}", Sha256::digest(&download.bytes)), self.sha);
    }
    async fn denied(&self, r: &Router, reader: Option<&Client>, status: StatusCode) {
        // Match the saved metadata validator too: conditional handling must not
        // disclose existence or bypass current authority on either endpoint.
        let variants = [
            vec![],
            vec![("range", "bytes=0-15")],
            vec![("range", "bytes=0-0,3-9")],
            vec![("if-none-match", self.metadata_etag.as_str())],
            vec![("if-none-match", "*")],
            vec![("if-modified-since", "Wed, 31 Dec 2098 23:59:59 GMT")],
            vec![
                ("range", "bytes=0-15"),
                ("if-range", self.metadata_etag.as_str()),
            ],
            vec![("if-match", self.metadata_etag.as_str())],
        ];
        for path in [&self.content_url, &self.metadata_url] {
            for headers in &variants {
                let reply = request(r, reader, "GET", path, None, headers).await;
                reply.expect(status);
                assert_eq!(reply.headers["content-type"], "application/problem+json");
                for header in [
                    "etag",
                    "last-modified",
                    "content-disposition",
                    "content-range",
                    "accept-ranges",
                    "location",
                ] {
                    assert!(
                        !reply.headers.contains_key(header),
                        "Denied response disclosed {header}"
                    );
                }
                let problem = reply.json();
                let (code, detail) = if status == StatusCode::UNAUTHORIZED {
                    ("unauthenticated", "Sign in to continue")
                } else {
                    ("not_found", "This item could not be found")
                };
                let request_id = reply.headers["x-request-id"].to_str().unwrap();
                Uuid::parse_str(request_id).unwrap();
                assert_eq!(
                    problem,
                    json!({
                        "type":format!("/api/v1/problems/{code}"), "title":status.canonical_reason().unwrap(),
                        "status":status.as_u16(), "detail":detail, "instance":format!("urn:uuid:{request_id}"),
                        "code":code, "request_id":request_id, "retryable":false, "field_errors":[]
                    }),
                    "Only the generic problem envelope may be returned"
                );
                let serialized = format!(
                    "{}{:?}",
                    String::from_utf8_lossy(&reply.bytes),
                    reply.headers
                );
                for forbidden in [
                    NAME,
                    "private-attachment-byte-marker-74019",
                    &self.sha,
                    &self.id,
                    "original_name",
                    "size_bytes",
                    "detected_mime",
                    "scan_status",
                    "storage_key",
                    "lease_token",
                ] {
                    assert!(
                        !serialized.contains(forbidden),
                        "Denied response disclosed private attachment data"
                    );
                }
                if let Some(document) = &self.document_id {
                    assert!(!serialized.contains(document));
                }
            }
        }
    }
}

enum Case {
    Sessions,
    Recovery,
    Account,
    Workspace,
    Team,
    Moves,
    Historical,
}
async fn run(case: Case) {
    let root = Directory::new();
    let h = Harness::new().await;
    let copy = h.clone();
    let outcome = tokio::spawn(async move {
        let fixture = Fixture::new(&copy, &root).await;
        match case {
            Case::Sessions => sessions(&fixture).await,
            Case::Recovery => recovery(&copy, &fixture).await,
            Case::Account => account(&copy, &fixture).await,
            Case::Workspace => workspace(&fixture).await,
            Case::Team => team(&fixture).await,
            Case::Moves => moves(&fixture).await,
            Case::Historical => historical(&fixture).await,
        }
    })
    .await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test]
async fn saved_urls_reject_logged_out_rotated_and_revoked_web_and_native_sessions() {
    run(Case::Sessions).await;
}
async fn sessions(f: &Fixture) {
    let r = &f.router;
    let (mut file, _) = f.linked_issue(&f.team).await;
    file.capture(r, &f.member).await;
    let draft = f.upload(&f.member, &f.team).await;
    let desktop = native(r, &f.member).await;
    file.readable(r, &desktop).await;
    for client in [&f.member, &desktop] {
        draft.readable(r, client).await;
    }
    file.denied(r, None, StatusCode::UNAUTHORIZED).await;
    draft.denied(r, None, StatusCode::UNAUTHORIZED).await;

    for client in [&f.member, &desktop] {
        request(r, Some(client), "POST", "/api/v1/auth/logout", None, &[])
            .await
            .expect(StatusCode::NO_CONTENT);
        file.denied(r, Some(client), StatusCode::UNAUTHORIZED).await;
        draft
            .denied(r, Some(client), StatusCode::UNAUTHORIZED)
            .await;
        file.readable(r, &f.owner).await;
    }
    let browser = login(r, None, PASSWORD).await;
    file.readable(r, &browser).await;
    draft.readable(r, &browser).await;
    let rotated = login(r, Some(&browser), PASSWORD).await;
    file.denied(r, Some(&browser), StatusCode::UNAUTHORIZED)
        .await;
    draft
        .denied(r, Some(&browser), StatusCode::UNAUTHORIZED)
        .await;
    file.readable(r, &rotated).await;
    draft.readable(r, &rotated).await;

    let other_browser = login(r, None, PASSWORD).await;
    let other_native = native(r, &rotated).await;
    for client in [&other_browser, &other_native] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
        let id = session_id(r, client).await;
        request(
            r,
            Some(&rotated),
            "POST",
            &format!("/api/v1/auth/sessions/{id}/revoke"),
            None,
            &[],
        )
        .await
        .expect(StatusCode::NO_CONTENT);
        file.denied(r, Some(client), StatusCode::UNAUTHORIZED).await;
        draft
            .denied(r, Some(client), StatusCode::UNAUTHORIZED)
            .await;
        file.readable(r, &rotated).await;
        draft.readable(r, &rotated).await;
    }
    let other_browser = login(r, None, PASSWORD).await;
    let other_native = native(r, &rotated).await;
    for client in [&other_browser, &other_native] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
    }
    request(
        r,
        Some(&rotated),
        "POST",
        "/api/v1/auth/sessions/revoke-others",
        None,
        &[],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    for client in [&other_browser, &other_native] {
        file.denied(r, Some(client), StatusCode::UNAUTHORIZED).await;
        draft
            .denied(r, Some(client), StatusCode::UNAUTHORIZED)
            .await;
    }
    file.readable(r, &rotated).await;
    draft.readable(r, &rotated).await;
    file.readable(r, &f.owner).await;
}

#[tokio::test]
async fn saved_urls_reject_pre_recovery_web_and_native_sessions() {
    run(Case::Recovery).await;
}
async fn recovery(h: &Harness, f: &Fixture) {
    let r = &f.router;
    let (mut file, _) = f.linked_issue(&f.team).await;
    file.capture(r, &f.member).await;
    let draft = f.upload(&f.member, &f.team).await;
    let browser = login(r, None, PASSWORD).await;
    let desktop = native(r, &f.member).await;
    for client in [&f.member, &browser, &desktop] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
    }
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let user = Uuid::parse_str(&f.member.user_id).unwrap();
    let token = generate_recovery_code(user);
    operator
        .issue_recovery(
            user,
            Uuid::new_v4(),
            &token,
            30,
            "Synthetic attachment recovery qualification",
        )
        .await
        .unwrap();
    request(
        r,
        None,
        "POST",
        "/api/v1/auth/recovery/complete",
        Some(json!({"token":token,"password":REPLACEMENT})),
        &[],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    for client in [&f.member, &browser, &desktop] {
        file.denied(r, Some(client), StatusCode::UNAUTHORIZED).await;
        draft
            .denied(r, Some(client), StatusCode::UNAUTHORIZED)
            .await;
    }
    file.readable(r, &f.owner).await;
    let fresh = login(r, None, REPLACEMENT).await;
    file.readable(r, &fresh).await;
    draft.readable(r, &fresh).await;
    let fresh_native = native(r, &fresh).await;
    file.readable(r, &fresh_native).await;
    draft.readable(r, &fresh_native).await;
}

#[tokio::test]
async fn saved_urls_reject_disabled_accounts_and_do_not_revive_old_sessions_on_enable() {
    run(Case::Account).await;
}
async fn account(h: &Harness, f: &Fixture) {
    let r = &f.router;
    let (mut file, _) = f.linked_issue(&f.team).await;
    file.capture(r, &f.member).await;
    let draft = f.upload(&f.member, &f.team).await;
    let desktop = native(r, &f.member).await;
    for client in [&f.member, &desktop] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
    }
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    let user = Uuid::parse_str(&f.member.user_id).unwrap();
    operator
        .disable_account(user, "Synthetic attachment account qualification")
        .await
        .unwrap();
    for client in [&f.member, &desktop] {
        file.denied(r, Some(client), StatusCode::UNAUTHORIZED).await;
        draft
            .denied(r, Some(client), StatusCode::UNAUTHORIZED)
            .await;
    }
    file.readable(r, &f.owner).await;
    operator
        .enable_account(user, "Restore synthetic attachment account")
        .await
        .unwrap();
    for client in [&f.member, &desktop] {
        file.denied(r, Some(client), StatusCode::UNAUTHORIZED).await;
        draft
            .denied(r, Some(client), StatusCode::UNAUTHORIZED)
            .await;
    }
    let fresh = login(r, None, PASSWORD).await;
    let fresh_native = native(r, &fresh).await;
    for client in [&fresh, &fresh_native] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
    }
}

#[tokio::test]
async fn saved_urls_reject_deactivated_workspace_members_with_live_web_and_native_sessions() {
    run(Case::Workspace).await;
}
async fn workspace(f: &Fixture) {
    let r = &f.router;
    let (mut file, _) = f.linked_issue(&f.team).await;
    file.capture(r, &f.member).await;
    let draft = f.upload(&f.member, &f.team).await;
    let desktop = native(r, &f.member).await;
    for client in [&f.member, &desktop] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
    }
    let path = format!("{}/members/{}", f.base, f.member.user_id);
    let current = get(r, &f.owner, &path).await;
    request(
        r,
        Some(&f.owner),
        "PATCH",
        &path,
        Some(json!({"active":false})),
        &[("if-match", current.etag())],
    )
    .await
    .expect(StatusCode::OK);
    for client in [&f.member, &desktop] {
        get(r, client, "/api/v1/auth/session").await;
        file.denied(r, Some(client), StatusCode::NOT_FOUND).await;
        draft.denied(r, Some(client), StatusCode::NOT_FOUND).await;
    }
    file.readable(r, &f.owner).await;
}

#[tokio::test]
async fn saved_urls_reject_removed_private_team_members_with_live_web_and_native_sessions() {
    run(Case::Team).await;
}
async fn team(f: &Fixture) {
    let r = &f.router;
    let private = f.private_team("PVT").await;
    f.membership(&private, true).await;
    let member = login(r, None, PASSWORD).await;
    let (mut file, _) = f.linked_issue(&private).await;
    file.capture(r, &member).await;
    let draft = f.upload(&member, &private).await;
    let desktop = native(r, &member).await;
    for client in [&member, &desktop] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
    }
    f.membership(&private, false).await;
    for client in [&member, &desktop] {
        get(r, client, "/api/v1/auth/session").await;
        file.denied(r, Some(client), StatusCode::NOT_FOUND).await;
        draft.denied(r, Some(client), StatusCode::NOT_FOUND).await;
    }
    file.readable(r, &f.owner).await;
    // Granting private access revokes existing sessions. Fresh sign-in restores
    // current authority for the same URL, without resurrecting old credentials.
    f.membership(&private, true).await;
    for client in [&member, &desktop] {
        file.denied(r, Some(client), StatusCode::UNAUTHORIZED).await;
        draft
            .denied(r, Some(client), StatusCode::UNAUTHORIZED)
            .await;
    }
    let fresh = login(r, None, PASSWORD).await;
    let fresh_native = native(r, &fresh).await;
    for client in [&fresh, &fresh_native] {
        file.readable(r, client).await;
        draft.readable(r, client).await;
    }
}

#[tokio::test]
async fn saved_urls_follow_public_private_and_private_private_issue_moves() {
    run(Case::Moves).await;
}
async fn moves(f: &Fixture) {
    let r = &f.router;
    let (mut file, issue) = f.linked_issue(&f.team).await;
    file.capture(r, &f.member).await;
    let desktop = native(r, &f.member).await;
    file.readable(r, &desktop).await;
    let first = f.private_team("ONE").await;
    let second = f.private_team("TWO").await;
    f.move_issue(&issue, &first).await;
    for client in [&f.member, &desktop] {
        file.denied(r, Some(client), StatusCode::NOT_FOUND).await;
    }
    file.readable(r, &f.owner).await;
    f.membership(&first, true).await;
    let member = login(r, None, PASSWORD).await;
    let desktop = native(r, &member).await;
    for client in [&member, &desktop] {
        file.readable(r, client).await;
    }
    f.move_issue(&issue, &second).await;
    for client in [&member, &desktop] {
        file.denied(r, Some(client), StatusCode::NOT_FOUND).await;
    }
    file.readable(r, &f.owner).await;
    f.move_issue(&issue, &f.team).await;
    for client in [&member, &desktop] {
        file.readable(r, client).await;
    }
}

#[tokio::test]
async fn saved_private_revision_and_comment_urls_keep_their_audience_after_public_move() {
    run(Case::Historical).await;
}
async fn historical(f: &Fixture) {
    let r = &f.router;
    let private = f.private_team("HIS").await;
    f.membership(&private, true).await;
    let member = login(r, None, PASSWORD).await;
    let (mut retained, issue) = f.linked_issue(&private).await;
    let desktop = native(r, &member).await;
    retained.capture(r, &member).await;
    let mut comment_file = f.upload(&f.owner, &private).await;
    request(
        r,
        Some(&f.owner),
        "POST",
        &format!("{issue}/comments"),
        Some(document(&[&comment_file.id])),
        &[("idempotency-key", "historical-comment")],
    )
    .await
    .expect(StatusCode::CREATED);
    comment_file.capture(r, &member).await;
    let doc = get(r, &f.owner, &format!("{issue}/document")).await;
    request(
        r,
        Some(&f.owner),
        "PATCH",
        &format!(
            "{}/documents/{}",
            f.base,
            doc.json()["id"].as_str().unwrap()
        ),
        Some(document(&[])),
        &[("if-match", doc.etag())],
    )
    .await
    .expect(StatusCode::OK);
    f.move_issue(&issue, &f.team).await;
    for client in [&member, &desktop] {
        retained.readable(r, client).await;
        comment_file.readable(r, client).await;
    }
    f.membership(&private, false).await;
    for client in [&member, &desktop] {
        get(r, client, &issue).await;
        retained
            .denied(r, Some(client), StatusCode::NOT_FOUND)
            .await;
        comment_file
            .denied(r, Some(client), StatusCode::NOT_FOUND)
            .await;
    }
    retained.readable(r, &f.owner).await;
    comment_file.readable(r, &f.owner).await;
}
