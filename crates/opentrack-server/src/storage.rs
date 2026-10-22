//! Shared local storage configuration for API transfers and restricted-worker GC.
use crate::config::Settings;
use opentrack_adapters::local_blob::{LocalBlobConfig, LocalBlobStore, ScannerConfig};
use opentrack_domain::AttachmentLimits;
use std::{path::PathBuf, time::Duration};

pub fn limits(cfg: &Settings) -> AttachmentLimits {
    AttachmentLimits {
        max_upload_bytes: cfg.storage.max_upload_bytes,
        workspace_quota_bytes: cfg.storage.workspace_quota_bytes,
        unlinked_ttl_seconds: cfg.storage.quarantine_ttl,
        upload_lease_seconds: 300,
        antivirus_required: cfg.storage.antivirus_required,
    }
}
pub async fn open(cfg: &Settings) -> Result<LocalBlobStore, String> {
    if cfg.storage.backend != "local" {
        return Err("Only local blob storage is implemented".into());
    }
    let root = cfg
        .storage
        .local_root
        .clone()
        .ok_or("Configure storage.local_root before serving files")?;
    // The separately installed worker need not have frontend assets or a checkout.
    // Its executable directory is still an existing boundary the adapter can pin.
    let executable = std::env::current_exe().map_err(|_| "Cannot inspect the storage boundary")?;
    let mut forbidden = vec![PathBuf::from(
        executable
            .parent()
            .ok_or("Cannot inspect the executable directory")?,
    )];
    let current = std::env::current_dir().map_err(|_| "Cannot inspect the storage boundary")?;
    if cfg.server.static_dir.exists() {
        forbidden.push(current.join(&cfg.server.static_dir));
    }
    if let Some(checkout) = current.ancestors().find(|p| p.join(".git").exists()) {
        forbidden.push(checkout.to_owned());
    }
    LocalBlobStore::open(LocalBlobConfig {root,forbidden_roots:forbidden,max_upload_bytes:cfg.storage.max_upload_bytes as u64,upload_idle_timeout:Duration::from_secs(30),scanner:ScannerConfig{socket:cfg.storage.scanner_socket.clone(),required:cfg.storage.antivirus_required,timeout:Duration::from_secs(60)}})
        .await.map_err(|error| match error {
            opentrack_adapters::local_blob::BlobError::Configuration(reason) => format!("Local blob storage configuration failed: {reason}"),
            _ => "Local blob storage is unavailable or unsafe; use an existing private service-owned mode-0700 directory outside the webroot and Git checkout".into(),
        })
}
