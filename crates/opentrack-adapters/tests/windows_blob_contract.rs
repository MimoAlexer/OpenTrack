#![cfg(windows)]

use opentrack_adapters::local_blob::*;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, SystemTime},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf};
use uuid::Uuid;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    web: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("opentrack-windows-blobs-{}", Uuid::new_v4()));
        let root = base.join("blobs");
        let web = base.join("web");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(&web).unwrap();
        Self { base, root, web }
    }
    fn config(&self) -> LocalBlobConfig {
        LocalBlobConfig {
            root: self.root.clone(),
            forbidden_roots: vec![self.web.clone()],
            max_upload_bytes: 1024 * 1024,
            upload_idle_timeout: Duration::from_millis(100),
            scanner: ScannerConfig::default(),
        }
    }
    async fn store(&self) -> LocalBlobStore {
        LocalBlobStore::open(self.config()).await.unwrap()
    }
    fn object(&self, key: BlobKey) -> PathBuf {
        self.root.join("objects").join(key.to_string())
    }
    fn stage(&self, key: BlobKey) -> PathBuf {
        self.root.join("staging").join(key.to_string())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn expected(bytes: &[u8]) -> ExpectedBlob {
    ExpectedBlob {
        key: BlobKey::new(),
        size: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}

#[test]
fn service_secrets_reject_shared_permissions_hardlinks_and_concurrent_writes() {
    let fixture = Fixture::new();
    let path = fixture.base.join("private-secret");
    fs::write(&path, b"secret canary").unwrap();
    let opened = opentrack_adapters::open_private_file(&path).unwrap();
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::rename(&path, fixture.base.join("substitute")).is_err());
    drop(opened);
    let link = fixture.base.join("linked-secret");
    fs::hard_link(&path, &link).unwrap();
    assert!(opentrack_adapters::open_private_file(&path).is_err());
    fs::remove_file(link).unwrap();
    let output = std::process::Command::new("icacls.exe")
        .arg(&path)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(opentrack_adapters::open_private_file(&path).is_err());
}
struct PanicReader;
impl AsyncRead for PanicReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        panic!("existing receipt must not consume input")
    }
}

#[tokio::test]
async fn durable_immutable_roundtrip_retry_and_gc_tombstone() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let bytes = vec![b'x'; 3 * 64 * 1024 + 17];
    let want = expected(&bytes);
    let receipt = store.put(want, bytes.as_slice()).await.unwrap();
    assert_eq!(receipt.size, bytes.len() as u64);
    assert!(fixture.object(want.key).exists());
    assert!(!fixture.stage(want.key).exists());
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.object(want.key))
            .is_err(),
        "published bytes must deny a new writable handle"
    );
    assert_eq!(store.put(want, PanicReader).await.unwrap(), receipt);
    let mut opened = store.open_verified(want).await.unwrap();
    let mut second = store.open_verified(want).await.unwrap();
    let mut actual = Vec::new();
    opened.read_to_end(&mut actual).await.unwrap();
    assert_eq!(actual, bytes);
    second.read_to_end(&mut Vec::new()).await.unwrap();
    let request = CleanupRequest {
        key: want.key,
        older_than: SystemTime::now() + Duration::from_secs(1),
    };
    assert_eq!(
        store.cleanup(vec![request]).await.unwrap()[0].state,
        CleanupState::Busy
    );
    drop(opened);
    drop(second);
    assert_eq!(store.scan(want).await.unwrap(), ScanStatus::NotScanned);
    assert_eq!(
        store.cleanup(vec![request]).await.unwrap()[0].state,
        CleanupState::Removed
    );
    assert!(!fixture.object(want.key).exists());
    assert!(matches!(
        store.put(want, PanicReader).await,
        Err(BlobError::Retired)
    ));
    assert!(matches!(
        store.open_verified(want).await,
        Err(BlobError::Retired)
    ));
}

#[tokio::test]
async fn failed_uploads_remove_only_their_staging_file() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    for bytes in [
        b"bad".as_slice(),
        b"short".as_slice(),
        b"much-too-long".as_slice(),
    ] {
        let want = expected(b"good");
        assert!(store.put(want, bytes).await.is_err());
        assert!(!fixture.object(want.key).exists());
        assert!(!fixture.stage(want.key).exists());
    }
    let want = expected(b"1234");
    let (_writer, reader) = tokio::io::duplex(10);
    assert!(matches!(
        store.put(want, reader).await,
        Err(BlobError::UploadTimeout)
    ));
    assert!(!fixture.stage(want.key).exists());
    let mut config = fixture.config();
    config.max_upload_bytes = 2;
    assert!(matches!(
        LocalBlobStore::open(config)
            .await
            .unwrap()
            .put(want, PanicReader)
            .await,
        Err(BlobError::TooLarge)
    ));
}

#[tokio::test]
async fn pinned_directories_reject_rename_and_hardlink_substitution() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    assert!(fs::rename(&fixture.root, fixture.base.join("moved")).is_err());
    assert!(
        fs::rename(
            fixture.root.join("objects"),
            fixture.root.join("moved-objects")
        )
        .is_err()
    );
    let want = expected(b"private immutable bytes");
    store
        .put(want, b"private immutable bytes".as_slice())
        .await
        .unwrap();
    assert!(fs::hard_link(fixture.object(want.key), fixture.base.join("second-link")).is_err());
    let injected = expected(b"untrusted linked bytes");
    let original = fixture.base.join("untrusted-file");
    fs::write(&original, b"untrusted linked bytes").unwrap();
    fs::hard_link(&original, fixture.object(injected.key)).unwrap();
    assert!(matches!(
        store.put(injected, PanicReader).await,
        Err(BlobError::UnsafeEntry)
    ));
    fs::remove_file(fixture.object(injected.key)).unwrap();
    assert!(store.open_verified(want).await.is_ok());
}

#[tokio::test]
async fn publication_never_replaces_a_racing_destination() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let want = expected(b"four");
    let (mut input, reader) = tokio::io::duplex(10);
    let upload = tokio::spawn(async move { store.put(want, reader).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.stage(want.key).exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fs::write(fixture.object(want.key), b"existing destination").unwrap();
    input.write_all(b"four").await.unwrap();
    input.shutdown().await.unwrap();
    assert!(upload.await.unwrap().is_err());
    assert_eq!(
        fs::read(fixture.object(want.key)).unwrap(),
        b"existing destination"
    );
    assert!(!fixture.stage(want.key).exists());
}

#[tokio::test]
async fn junctions_cannot_replace_storage_roots_or_children() {
    let fixture = Fixture::new();
    let junction = fixture.base.join("redirected");
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&fixture.root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut config = fixture.config();
    config.root = junction.clone();
    assert!(matches!(
        LocalBlobStore::open(config).await,
        Err(BlobError::UnsafeEntry)
    ));
    fs::remove_dir(&junction).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(fixture.root.join("objects"))
        .arg(&fixture.web)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(matches!(
        LocalBlobStore::open(fixture.config()).await,
        Err(BlobError::UnsafeEntry)
    ));
    fs::remove_dir(fixture.root.join("objects")).unwrap();
}

#[tokio::test]
async fn shared_acl_and_unsupported_scanner_fail_closed() {
    let fixture = Fixture::new();
    let output = std::process::Command::new("icacls.exe")
        .arg(&fixture.root)
        .args(["/grant", "*S-1-1-0:(OI)(CI)(R)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(matches!(
        LocalBlobStore::open(fixture.config()).await,
        Err(BlobError::Configuration(_))
    ));
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.scanner.socket = Some(fixture.base.join("scanner.socket"));
    assert!(matches!(
        LocalBlobStore::open(config).await,
        Err(BlobError::Configuration(_))
    ));
    let mut config = fixture.config();
    config.scanner.required = true;
    assert!(matches!(
        LocalBlobStore::open(config).await,
        Err(BlobError::Configuration(_))
    ));
    let mut config = fixture.config();
    config.forbidden_roots = vec![fixture.base.clone()];
    assert!(matches!(
        LocalBlobStore::open(config).await,
        Err(BlobError::Configuration(_))
    ));
}

#[tokio::test]
async fn exclusive_uploads_fence_another_store_and_retirement() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let other = fixture.store().await;
    let want = expected(b"four");
    let (_writer, reader) = tokio::io::duplex(10);
    let writer = tokio::spawn(async move { store.put(want, reader).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.stage(want.key).exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        other.put(want, PanicReader).await,
        Err(BlobError::Busy)
    ));
    let request = CleanupRequest {
        key: want.key,
        older_than: SystemTime::now() + Duration::from_secs(1),
    };
    assert_eq!(
        other.cleanup(vec![request]).await.unwrap()[0].state,
        CleanupState::Busy
    );
    assert!(matches!(
        writer.await.unwrap(),
        Err(BlobError::UploadTimeout)
    ));
    assert_eq!(
        other.cleanup(vec![request]).await.unwrap()[0].state,
        CleanupState::Missing
    );
    assert!(matches!(
        other.put(want, PanicReader).await,
        Err(BlobError::Retired)
    ));
}

#[tokio::test]
async fn cancelling_after_streamed_bytes_allows_safe_same_key_retry_and_gc() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.upload_idle_timeout = Duration::from_secs(3);
    let store = LocalBlobStore::open(config).await.unwrap();
    let other = fixture.store().await;
    let bytes = vec![b'c'; 4 * 64 * 1024];
    let want = expected(&bytes);
    let (mut input, reader) = tokio::io::duplex(64 * 1024);
    let upload = tokio::spawn(async move { store.put(want, reader).await });
    input.write_all(&bytes[..2 * 64 * 1024]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while fs::metadata(fixture.stage(want.key)).map_or(true, |meta| meta.len() == 0) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    upload.abort();
    assert!(upload.await.unwrap_err().is_cancelled());
    drop(input);
    // Cancelling a future does not synchronously cancel already dispatched OS
    // writes. The no-delete-sharing handles fence retry until they finish.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match other.put(want, bytes.as_slice()).await {
                Ok(_) => break,
                Err(BlobError::Busy | BlobError::Io(_)) => tokio::task::yield_now().await,
                Err(error) => panic!("unexpected retry result: {error}"),
            }
        }
    })
    .await
    .unwrap();
    let mut result = Vec::new();
    other
        .open_verified(want)
        .await
        .unwrap()
        .read_to_end(&mut result)
        .await
        .unwrap();
    assert_eq!(result, bytes);
    assert!(!fixture.stage(want.key).exists());
    assert_eq!(
        other
            .cleanup(vec![CleanupRequest {
                key: want.key,
                older_than: SystemTime::now() + Duration::from_secs(1)
            }])
            .await
            .unwrap()[0]
            .state,
        CleanupState::Removed
    );
    assert!(matches!(
        other.put(want, PanicReader).await,
        Err(BlobError::Retired)
    ));
}
