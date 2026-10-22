//! Explicitly opt-in, read-only diagnostic for the selected local development server.
//! No response data, credential material, or saved account references are printed.
use opentrack_desktop::{
    credentials::{CredentialRef, SystemVault, Vault},
    persistence::{Configuration, Persistence},
    protocol::{ApiRequest, Method, NativeError},
    state::Runtime,
    transport::ApiReply,
};
use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, OpenOptions},
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use uuid::Uuid;
use zeroize::Zeroizing;

const ORIGIN: &str = "http://127.0.0.1:18970";
const WORKSPACE: &str = "ff20c62a-90ea-4a74-8073-859ecabe1cc1";
const ISSUE: &str = "5d9b1241-4692-46ec-9ade-72c5681aee27";

struct ReadOnlyVault {
    reference: CredentialRef,
    reads: AtomicUsize,
    refused_writes: AtomicUsize,
}
impl Vault for ReadOnlyVault {
    fn get(
        &self,
        origin: &str,
        reference: &CredentialRef,
    ) -> Result<Option<Zeroizing<String>>, NativeError> {
        if origin != ORIGIN || reference != &self.reference {
            return Err(NativeError::invalid());
        }
        self.reads.fetch_add(1, Ordering::SeqCst);
        SystemVault.get(origin, reference)
    }
    fn set(&self, _: &str, _: &CredentialRef, _: &str) -> Result<(), NativeError> {
        self.refused_writes.fetch_add(1, Ordering::SeqCst);
        Err(NativeError::storage())
    }
    fn delete(&self, _: &str, _: &CredentialRef) -> Result<(), NativeError> {
        self.refused_writes.fetch_add(1, Ordering::SeqCst);
        Err(NativeError::storage())
    }
}

struct TemporaryDirectory(PathBuf);
impl TemporaryDirectory {
    fn new() -> Result<Self, &'static str> {
        let path = std::env::temp_dir().join(format!("opentrack-saved-probe-{}", Uuid::new_v4()));
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| "temporary_directory")?;
        Ok(Self(path))
    }
}
impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Do not construct Persistence over the real directory: its constructor sets
// directory permissions. Validate and read the original without modifying it.
fn read_saved(path: &Path) -> Result<Zeroizing<String>, &'static str> {
    let uid = fs::metadata("/proc/self")
        .map_err(|_| "current_user")?
        .uid();
    let directory = fs::symlink_metadata(path.parent().ok_or("configuration_path")?)
        .map_err(|_| "configuration_directory")?;
    if !directory.is_dir()
        || directory.file_type().is_symlink()
        || directory.uid() != uid
        || directory.mode() & 0o077 != 0
    {
        return Err("configuration_directory");
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(0x20000)
        .open(path)
        .map_err(|_| "configuration_read")?;
    let metadata = file.metadata().map_err(|_| "configuration_metadata")?;
    if !metadata.is_file() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err("configuration_metadata");
    }
    let mut value = Zeroizing::new(String::new());
    file.take(65537)
        .read_to_string(&mut value)
        .map_err(|_| "configuration_read")?;
    if value.len() > 65536 {
        return Err("configuration_size");
    }
    Ok(value)
}

fn request(generation: &str, path: String) -> ApiRequest {
    ApiRequest {
        expected_generation: generation.into(),
        method: Method::Get,
        path,
        query: None,
        headers: BTreeMap::new(),
        body: None,
    }
}

fn report(label: &str, response: &Result<ApiReply, NativeError>, generation: &str) -> bool {
    match response {
        Ok(reply) => {
            let shape = reply.body.as_ref().is_some_and(|body| match label {
                "session" => {
                    body["audience"] == "desktop"
                        && body["csrf_token"] == ""
                        && body["user"].is_object()
                        && body["workspaces"].is_array()
                }
                "bootstrap" => {
                    body["auth_epoch"].is_string()
                        && body["teams"].is_array()
                        && body["statuses"].is_array()
                        && body["issues"].is_array()
                }
                "access" => {
                    body["auth_epoch"].is_string()
                        && body["role"].is_string()
                        && body["archived"].is_boolean()
                }
                "issue" => {
                    body["id"] == ISSUE
                        && body["workspace_id"] == WORKSPACE
                        && body["etag"].as_str() == reply.headers.get("etag").map(String::as_str)
                }
                "document" => {
                    body["owner_id"] == ISSUE
                        && body["owner_type"] == "issue"
                        && body["schema_supported"].is_boolean()
                        && body["body"].is_object()
                        && body["etag"].as_str() == reply.headers.get("etag").map(String::as_str)
                }
                "comments" | "activity" => body["data"].is_array() && body["has_more"].is_boolean(),
                _ => false,
            });
            let valid = reply.status == 200 && reply.generation == generation && shape;
            println!(
                "{label}: HTTP {}; expected shape and generation: {}",
                reply.status,
                if valid { "yes" } else { "no" }
            );
            valid
        }
        Err(error) => {
            // Native errors use reviewed static codes; never print arbitrary server bodies.
            println!("{label}: native error {}", error.code);
            false
        }
    }
}

async fn run() -> Result<(), &'static str> {
    let home = std::env::var_os("HOME").ok_or("home_directory")?;
    let path = PathBuf::from(home).join(".config/app.opentrack.desktop/connections.toml");
    let original = read_saved(&path)?;
    let parsed: Configuration = toml::from_str(&original).map_err(|_| "configuration_parse")?;
    let temporary = TemporaryDirectory::new()?;
    let persistence = Persistence::new(temporary.0.clone()).map_err(|_| "temporary_persistence")?;
    persistence
        .save(&parsed)
        .map_err(|_| "temporary_configuration")?;
    // The production loader validates canonical origins, duplicates and selected membership.
    let validated = persistence.load().map_err(|_| "configuration_validation")?;
    if validated.selected.as_deref() != Some(ORIGIN) {
        return Err("unexpected_selected_server");
    }
    let selected = validated
        .servers
        .into_iter()
        .find(|server| server.origin == ORIGIN)
        .ok_or("selected_server_missing")?;
    if !selected.development_http {
        return Err("development_consent_missing");
    }
    let reference = selected
        .credential
        .clone()
        .ok_or("credential_reference_missing")?;
    persistence
        .save(&Configuration {
            selected: Some(ORIGIN.into()),
            servers: vec![selected],
        })
        .map_err(|_| "temporary_configuration")?;
    let vault = Arc::new(ReadOnlyVault {
        reference,
        reads: AtomicUsize::new(0),
        refused_writes: AtomicUsize::new(0),
    });
    let runtime = Runtime::new(
        persistence,
        vault.clone(),
        Arc::new(|_| {}),
        Arc::new(|_| Err(NativeError::invalid())),
    )
    .map_err(|_| "runtime_initialization")?;
    let generation = runtime.read().await.generation;
    let started = Instant::now();
    let (session, bootstrap, access, issue, document, comments, activity) = tokio::join!(
        runtime.api(request(&generation, "/api/v1/auth/session".into())),
        runtime.api(request(
            &generation,
            format!("/api/v1/workspaces/{WORKSPACE}/bootstrap")
        )),
        runtime.api(request(
            &generation,
            format!("/api/v1/workspaces/{WORKSPACE}/access")
        )),
        runtime.api(request(
            &generation,
            format!("/api/v1/workspaces/{WORKSPACE}/issues/{ISSUE}")
        )),
        runtime.api(request(
            &generation,
            format!("/api/v1/workspaces/{WORKSPACE}/issues/{ISSUE}/document")
        )),
        runtime.api(request(
            &generation,
            format!("/api/v1/workspaces/{WORKSPACE}/issues/{ISSUE}/comments")
        )),
        runtime.api(request(
            &generation,
            format!("/api/v1/workspaces/{WORKSPACE}/issues/{ISSUE}/activity-page")
        )),
    );
    let mut valid = true;
    for (label, reply) in [
        ("session", &session),
        ("bootstrap", &bootstrap),
        ("access", &access),
        ("issue", &issue),
        ("document", &document),
        ("comments", &comments),
        ("activity", &activity),
    ] {
        valid &= report(label, reply, &generation);
    }
    let unchanged = read_saved(&path)?.as_str() == original.as_str();
    let reads = vault.reads.load(Ordering::SeqCst);
    let writes = vault.refused_writes.load(Ordering::SeqCst);
    println!(
        "Concurrent batch: {} ms; credential reads: {reads}; refused writes: {writes}; original configuration unchanged: {unchanged}",
        started.elapsed().as_millis()
    );
    if valid && unchanged && reads == 7 && writes == 0 {
        Ok(())
    } else {
        Err("probe_checks_failed")
    }
}

#[tokio::main]
async fn main() {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--read-saved-loopback-connection"] {
        eprintln!(
            "Opt in with --read-saved-loopback-connection to read the saved loopback connection."
        );
        std::process::exit(2);
    }
    if let Err(code) = run().await {
        eprintln!("Saved connection probe failed: {code}");
        std::process::exit(1);
    }
}
