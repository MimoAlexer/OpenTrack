//! Immutable local blob storage with platform-specific pinned filesystem access.
//!
//! Callers reserve keys and authorize every phase in PostgreSQL, outside this
//! adapter. Never hold a database transaction while calling these methods.
//! A stored receipt means durable quarantine bytes, NOT permission or scan-ready.
#[cfg(target_os = "linux")]
mod filesystem;
#[cfg(windows)]
#[path = "filesystem_windows.rs"]
mod filesystem;
#[cfg(unix)]
mod scanner;

#[cfg(windows)]
pub(crate) use filesystem::open_private_file;
use filesystem::{Directories, KeyLease, Stage};
use sha2::{Digest, Sha256};
use std::{
    fmt, io,
    path::PathBuf,
    pin::Pin,
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, ReadBuf};
use tokio::sync::Semaphore;
use uuid::Uuid;

const CHUNK: usize = 64 * 1024;
const SNIFF: usize = 8192;
pub const MAX_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("invalid local blob configuration: {0}")]
    Configuration(&'static str),
    #[error("invalid opaque blob key")]
    InvalidKey,
    #[error("blob exceeds the configured size limit")]
    TooLarge,
    #[error("stream length does not match the reservation")]
    LengthMismatch,
    #[error("stream checksum does not match the reservation")]
    HashMismatch,
    #[error("blob operation is already in progress")]
    Busy,
    #[error("blob key has been permanently retired")]
    Retired,
    #[error("stored blob is missing")]
    IntegrityMissing,
    #[error("stored blob does not match its immutable metadata")]
    IntegrityCorrupt,
    #[error("unsafe filesystem entry")]
    UnsafeEntry,
    #[error("upload stream timed out")]
    UploadTimeout,
    #[error("configured scanner is unavailable")]
    ScannerUnavailable,
    #[error("configured scanner timed out")]
    ScannerTimeout,
    #[error("configured scanner returned an invalid or unsuccessful response")]
    ScannerProtocol,
    #[error("scanner detected unsafe content")]
    MalwareDetected,
    #[error("local blob I/O failed")]
    Io(#[from] io::Error),
    #[error("local blob operation could not be confirmed")]
    OperationInterrupted,
}
pub type Result<T> = std::result::Result<T, BlobError>;

/// Only canonical generated UUIDv4 keys can enter filesystem operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobKey(Uuid);
impl BlobKey {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
    pub fn from_uuid(value: Uuid) -> Result<Self> {
        if value.get_version() != Some(uuid::Version::Random)
            || value.get_variant() != uuid::Variant::RFC4122
        {
            return Err(BlobError::InvalidKey);
        }
        Ok(Self(value))
    }
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}
impl Default for BlobKey {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for BlobKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for BlobKey {
    type Err = BlobError;
    fn from_str(value: &str) -> Result<Self> {
        let uuid = Uuid::parse_str(value).map_err(|_| BlobError::InvalidKey)?;
        if uuid.to_string() != value {
            return Err(BlobError::InvalidKey);
        }
        Self::from_uuid(uuid)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedBlob {
    pub key: BlobKey,
    pub size: u64,
    pub sha256: [u8; 32],
}
impl ExpectedBlob {
    pub fn from_hex(key: BlobKey, size: u64, sha256: &str) -> Result<Self> {
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(BlobError::Configuration(
                "expected SHA-256 must be lowercase hexadecimal",
            ));
        }
        let mut hash = [0; 32];
        for (index, byte) in hash.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&sha256[index * 2..index * 2 + 2], 16)
                .map_err(|_| BlobError::Configuration("invalid SHA-256"))?;
        }
        Ok(Self {
            key,
            size,
            sha256: hash,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ScannerConfig {
    pub socket: Option<PathBuf>,
    pub required: bool,
    /// Bounds connect, transfer and response together, not each byte separately.
    pub timeout: Duration,
}
impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            socket: None,
            required: false,
            timeout: Duration::from_secs(60),
        }
    }
}
#[derive(Debug, Clone)]
pub struct LocalBlobConfig {
    /// Existing private directory, owned by the serving UID, mode 0700.
    pub root: PathBuf,
    /// Existing checkout and webroot paths; the storage root must be outside all.
    pub forbidden_roots: Vec<PathBuf>,
    pub max_upload_bytes: u64,
    pub upload_idle_timeout: Duration,
    pub scanner: ScannerConfig,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Clean,
    NotScanned,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    pub key: BlobKey,
    pub size: u64,
    pub sha256: [u8; 32],
    /// Conservative signature detection, not sanitization or preview permission.
    pub mime: &'static str,
}

#[derive(Clone)]
pub struct LocalBlobStore {
    directories: Arc<Directories>,
    config: Arc<LocalBlobConfig>,
    cleanup_lane: Arc<Semaphore>,
}
impl LocalBlobStore {
    pub async fn open(config: LocalBlobConfig) -> Result<Self> {
        if config.max_upload_bytes == 0 || config.max_upload_bytes > MAX_UPLOAD_BYTES {
            return Err(BlobError::Configuration(
                "upload bound must be positive and at most 100 MiB",
            ));
        }
        if config.upload_idle_timeout.is_zero()
            || config.upload_idle_timeout > Duration::from_secs(300)
            || config.scanner.timeout.is_zero()
            || config.scanner.timeout > Duration::from_secs(300)
        {
            return Err(BlobError::Configuration(
                "timeouts must be positive and at most 300 seconds",
            ));
        }
        if config.scanner.required && config.scanner.socket.is_none() {
            return Err(BlobError::Configuration(
                "required scanner has no Unix socket",
            ));
        }
        #[cfg(windows)]
        if config.scanner.socket.is_some() {
            return Err(BlobError::Configuration(
                "Unix socket antivirus scanning is unavailable on Windows",
            ));
        }
        if config
            .scanner
            .socket
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(BlobError::Configuration("scanner socket must be absolute"));
        }
        let config = Arc::new(config);
        let copy = config.clone();
        let directories = blocking(move || Directories::open(&copy)).await?;
        Ok(Self {
            directories: Arc::new(directories),
            config,
            cleanup_lane: Arc::new(Semaphore::new(1)),
        })
    }

    /// Stream to durable quarantine. Existing matching bytes recover a prior
    /// uncertain success without consuming `reader` or replacing the object.
    pub async fn put<R: AsyncRead + Unpin>(
        &self,
        expected: ExpectedBlob,
        mut reader: R,
    ) -> Result<StoredBlob> {
        if expected.size > self.config.max_upload_bytes {
            return Err(BlobError::TooLarge);
        }
        let directories = self.directories.clone();
        let lease = blocking(move || directories.lease(expected.key, true, true)).await?;
        let copy = self.directories.clone();
        let existing = blocking(move || copy.open_object(expected.key)).await?;
        if let Some(file) = existing {
            let mut file = tokio::fs::File::from_std(file);
            let receipt = verify(&mut file, expected).await?;
            // A previous rename may have succeeded before directory fsync
            // failed. Retry must complete durability as well as verify bytes.
            let file = file.into_std().await;
            let directories = self.directories.clone();
            blocking(move || {
                directories.confirm_durable(expected.key, &file)?;
                drop(lease);
                Ok(())
            })
            .await?;
            return Ok(receipt);
        }
        let directories = self.directories.clone();
        let (stage, file) = blocking(move || Stage::create(directories, lease)).await?;
        let mut file = tokio::fs::File::from_std(file);
        let mut hash = Sha256::new();
        let mut size = 0_u64;
        let mut prefix = Vec::with_capacity(SNIFF);
        let mut buffer = vec![0_u8; CHUNK];
        let streamed: Result<()> = async {
            loop {
                let count =
                    tokio::time::timeout(self.config.upload_idle_timeout, reader.read(&mut buffer))
                        .await
                        .map_err(|_| BlobError::UploadTimeout)??;
                if count == 0 {
                    break;
                }
                size = size.checked_add(count as u64).ok_or(BlobError::TooLarge)?;
                if size > expected.size {
                    return Err(BlobError::LengthMismatch);
                }
                hash.update(&buffer[..count]);
                let sniff = count.min(SNIFF - prefix.len());
                prefix.extend_from_slice(&buffer[..sniff]);
                file.write_all(&buffer[..count]).await?;
            }
            if size != expected.size {
                return Err(BlobError::LengthMismatch);
            }
            if <[u8; 32]>::from(hash.finalize()) != expected.sha256 {
                return Err(BlobError::HashMismatch);
            }
            file.flush().await?;
            Ok(())
        }
        .await;
        // Finish outstanding blocking file I/O before reporting a failed
        // upload. Windows cannot reclaim a staging name while such a handle
        // is still completing a Tokio write.
        let file = file.into_std().await;
        if let Err(error) = streamed {
            drop(file);
            drop(stage);
            return Err(error);
        }
        // Publication owns the lease in this non-cancellable blocking task. A
        // cancelled caller may leave a valid orphan, never an acknowledged ready
        // blob. Authoritative GC cannot pass this lease or republish a retired key.
        blocking(move || stage.publish(file)).await?;
        Ok(StoredBlob {
            key: expected.key,
            size,
            sha256: expected.sha256,
            mime: sniff_mime(&prefix),
        })
    }

    /// Verify the exact opened descriptor before any download bytes are exposed.
    pub async fn open_verified(&self, expected: ExpectedBlob) -> Result<VerifiedBlob> {
        if expected.size > MAX_UPLOAD_BYTES {
            return Err(BlobError::IntegrityCorrupt);
        }
        let directories = self.directories.clone();
        let (lease, file) = blocking(move || {
            let lease = directories.lease(expected.key, false, false)?;
            let file = directories
                .open_object(expected.key)?
                .ok_or(BlobError::IntegrityMissing)?;
            Ok((lease, file))
        })
        .await?;
        let mut file = tokio::fs::File::from_std(file);
        let receipt = verify(&mut file, expected).await?;
        file.rewind().await?;
        Ok(VerifiedBlob {
            file,
            _lease: lease,
            receipt,
        })
    }

    /// Reopens and verifies immutable quarantine bytes. Only Clean or an explicit
    /// optional NotScanned result may be proposed for database readiness.
    pub async fn scan(&self, expected: ExpectedBlob) -> Result<ScanStatus> {
        let blob = self.open_verified(expected).await?;
        match &self.config.scanner.socket {
            None if !self.config.scanner.required => Ok(ScanStatus::NotScanned),
            None => Err(BlobError::ScannerUnavailable),
            Some(socket) => {
                #[cfg(unix)]
                {
                    let mut blob = blob;
                    scanner::scan(
                        socket,
                        self.config.scanner.timeout,
                        &mut blob,
                        expected.size,
                    )
                    .await
                }
                #[cfg(windows)]
                {
                    let _ = (socket, blob);
                    Err(BlobError::ScannerUnavailable)
                }
            }
        }
    }

    /// Metadata must already have committed a terminal cleanup claim that fences
    /// uploads/finalization/new references. A read-only candidate query is NOT
    /// enough. Quota is released only after Removed/Missing; Busy is retried.
    /// At most 100 keys are processed; no age-based directory sweep is exposed.
    pub async fn cleanup(&self, requests: Vec<CleanupRequest>) -> Result<Vec<CleanupOutcome>> {
        if requests.len() > 100 {
            return Err(BlobError::Configuration("cleanup batch exceeds 100 keys"));
        }
        let directories = self.directories.clone();
        cleanup_blocking(self.cleanup_lane.clone(), move || {
            requests
                .into_iter()
                .map(|request| directories.cleanup(request))
                .collect()
        })
        .await
    }
}

/// Dropping this stream closes its verified descriptor and releases its GC lock.
pub struct VerifiedBlob {
    file: tokio::fs::File,
    _lease: KeyLease,
    pub receipt: StoredBlob,
}
impl AsyncRead for VerifiedBlob {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_read(cx, buffer)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CleanupRequest {
    pub key: BlobKey,
    pub older_than: SystemTime,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupState {
    Removed,
    Missing,
    Busy,
    TooRecent,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupOutcome {
    pub key: BlobKey,
    pub state: CleanupState,
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| BlobError::OperationInterrupted)?
}
async fn cleanup_blocking<T: Send + 'static>(
    lane: Arc<Semaphore>,
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let permit = lane.try_acquire_owned().map_err(|_| BlobError::Busy)?;
    // Kernel I/O cannot be cancelled by dropping this future. Admission belongs
    // to the blocking operation, so a caller's deadline cannot accumulate GC
    // tasks behind a stalled disk. Clones of the store share this lane.
    blocking(move || {
        let _permit = permit;
        operation()
    })
    .await
}
async fn verify(file: &mut tokio::fs::File, expected: ExpectedBlob) -> Result<StoredBlob> {
    if file.metadata().await?.len() != expected.size {
        return Err(BlobError::IntegrityCorrupt);
    }
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut prefix = Vec::with_capacity(SNIFF);
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > expected.size {
            return Err(BlobError::IntegrityCorrupt);
        }
        hash.update(&buffer[..count]);
        prefix.extend_from_slice(&buffer[..count.min(SNIFF - prefix.len())]);
    }
    if size != expected.size || <[u8; 32]>::from(hash.finalize()) != expected.sha256 {
        return Err(BlobError::IntegrityCorrupt);
    }
    Ok(StoredBlob {
        key: expected.key,
        size,
        sha256: expected.sha256,
        mime: sniff_mime(&prefix),
    })
}
fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        "application/zip"
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        "application/gzip"
    } else if !bytes.is_empty()
        && std::str::from_utf8(bytes).is_ok()
        && !bytes
            .iter()
            .any(|byte| *byte < 0x20 && !b"\n\r\t".contains(byte))
    {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_cleanup_keeps_lane_until_blocking_operation_finishes() {
        let lane = Arc::new(Semaphore::new(1));
        let cloned = lane.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            cleanup_blocking(cloned, move || {
                let _ = started.send(());
                gate.recv().unwrap();
                Ok(())
            })
            .await
        });
        ready.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(matches!(
            cleanup_blocking::<()>(lane.clone(), || panic!("busy lane must not run cleanup")).await,
            Err(BlobError::Busy)
        ));
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while lane.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        cleanup_blocking(lane, || Ok(())).await.unwrap();
    }
}
