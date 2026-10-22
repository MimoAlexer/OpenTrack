#![cfg(target_os = "linux")]

use opentrack_adapters::local_blob::*;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
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
        let base = std::env::temp_dir().join(format!("opentrack-blob-test-{}", Uuid::new_v4()));
        mkdir(&base);
        let root = base.join("blobs");
        mkdir(&root);
        let web = base.join("web");
        mkdir(&web);
        Self { base, root, web }
    }
    fn config(&self) -> LocalBlobConfig {
        LocalBlobConfig {
            root: self.root.clone(),
            forbidden_roots: vec![self.web.clone()],
            max_upload_bytes: 25 * 1024 * 1024,
            upload_idle_timeout: Duration::from_millis(200),
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
fn mkdir(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}
fn expected(data: &[u8]) -> ExpectedBlob {
    ExpectedBlob {
        key: BlobKey::new(),
        size: data.len() as u64,
        sha256: Sha256::digest(data).into(),
    }
}
async fn wait_stage(path: &Path) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !path.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn durable_stream_roundtrip_is_immutable_and_retry_recovers_receipt() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let mut data = vec![b'x'; 3 * 64 * 1024 + 39];
    data[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let want = expected(&data);
    let stored = store.put(want, data.as_slice()).await.unwrap();
    assert_eq!(stored.mime, "image/png");
    assert_eq!(stored.sha256, want.sha256);
    let metadata = fs::metadata(fixture.object(want.key)).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o400);
    assert_eq!(metadata.nlink(), 1);
    assert!(!fixture.stage(want.key).exists());
    let mut opened = store.open_verified(want).await.unwrap();
    let mut returned = Vec::new();
    opened.read_to_end(&mut returned).await.unwrap();
    drop(opened);
    assert_eq!(returned, data);
    assert_eq!(store.put(want, PanicReader).await.unwrap(), stored);
    assert_eq!(
        fs::metadata(fixture.object(want.key)).unwrap().ino(),
        metadata.ino()
    );
    assert_eq!(store.scan(want).await.unwrap(), ScanStatus::NotScanned);
    let mut wrong = want;
    wrong.sha256[0] ^= 1;
    assert!(matches!(
        store.put(wrong, PanicReader).await,
        Err(BlobError::IntegrityCorrupt)
    ));
}
struct PanicReader;
impl AsyncRead for PanicReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        panic!("idempotent existing receipt must not consume request bytes")
    }
}

#[tokio::test]
async fn mismatch_limit_and_stream_failure_never_publish_or_leave_staging() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.max_upload_bytes = 4;
    let store = LocalBlobStore::open(config).await.unwrap();
    let too_large = expected(b"12345");
    assert!(matches!(
        store.put(too_large, PanicReader).await,
        Err(BlobError::TooLarge)
    ));
    for (want, bytes, kind) in [
        (expected(b"1234"), b"123".as_slice(), "length"),
        (expected(b"123"), b"1234".as_slice(), "length"),
        (expected(b"1234"), b"4321".as_slice(), "hash"),
    ] {
        let error = store.put(want, bytes).await.unwrap_err();
        assert!(matches!(
            (&error, kind),
            (BlobError::LengthMismatch, "length") | (BlobError::HashMismatch, "hash")
        ));
        assert!(!fixture.object(want.key).exists());
        assert!(!fixture.stage(want.key).exists());
    }
    let want = expected(b"1234");
    assert!(matches!(
        store.put(want, BrokenReader).await,
        Err(BlobError::Io(_))
    ));
    assert!(!fixture.object(want.key).exists());
    assert!(!fixture.stage(want.key).exists());
}
struct BrokenReader;
impl AsyncRead for BrokenReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "fixture interruption",
        )))
    }
}

#[test]
fn filesystem_write_failure_does_not_publish() {
    const CHILD: &str = "OPENTRACK_BLOB_LIMITED_WRITE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "filesystem_write_failure_does_not_publish",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated limited-write test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    // An isolated child process exercises a real kernel file-write refusal;
    // neither the developer process nor the shared test runner is constrained.
    let limit = libc::rlimit {
        rlim_cur: 1024,
        rlim_max: 1024,
    };
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new(); let store = fixture.store().await;
        let bytes = vec![b'x'; 4096]; let want = expected(&bytes);
        assert!(matches!(store.put(want, bytes.as_slice()).await, Err(BlobError::Io(error)) if error.raw_os_error() == Some(libc::EFBIG)));
        assert!(!fixture.object(want.key).exists()); assert!(!fixture.stage(want.key).exists());
    });
}

#[tokio::test]
async fn cancellation_and_idle_timeout_release_exclusive_lease_and_staging() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let want = expected(b"1234");
    let (mut sender, reader) = tokio::io::duplex(8);
    sender.write_all(b"12").await.unwrap();
    let task_store = store.clone();
    let task = tokio::spawn(async move { task_store.put(want, reader).await });
    wait_stage(&fixture.stage(want.key)).await;
    assert!(matches!(
        store.put(want, b"1234".as_slice()).await,
        Err(BlobError::Busy)
    ));
    assert_eq!(
        store
            .cleanup(vec![CleanupRequest {
                key: want.key,
                older_than: SystemTime::now() + Duration::from_secs(1)
            }])
            .await
            .unwrap()[0]
            .state,
        CleanupState::Busy
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(sender);
    assert!(!fixture.stage(want.key).exists());
    assert!(!fixture.object(want.key).exists());
    store.put(want, b"1234".as_slice()).await.unwrap();
    let pending = expected(b"abcd");
    let (_sender, reader) = tokio::io::duplex(8);
    assert!(matches!(
        store.put(pending, reader).await,
        Err(BlobError::UploadTimeout)
    ));
    assert!(!fixture.stage(pending.key).exists());
    assert!(!fixture.object(pending.key).exists());
}

#[tokio::test]
async fn interrupted_readonly_staging_is_recovered_without_replacing_live_bytes() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let want = expected(b"new bytes");
    fs::write(fixture.stage(want.key), b"partial old crash").unwrap();
    fs::set_permissions(fixture.stage(want.key), fs::Permissions::from_mode(0o400)).unwrap();
    store.put(want, b"new bytes".as_slice()).await.unwrap();
    assert_eq!(fs::read(fixture.object(want.key)).unwrap(), b"new bytes");
    assert!(!fixture.stage(want.key).exists());
}

#[tokio::test]
async fn root_exclusions_symlinks_and_unsafe_ancestors_fail_closed() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.root = fixture.web.clone();
    assert!(matches!(
        LocalBlobStore::open(config).await,
        Err(BlobError::Configuration(_))
    ));
    let alias = fixture.base.join("alias");
    symlink(&fixture.root, &alias).unwrap();
    let mut config = fixture.config();
    config.root = alias;
    assert!(LocalBlobStore::open(config).await.is_err());
    let parent = fixture.base.join("unsafe");
    mkdir(&parent);
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();
    let root = parent.join("private");
    mkdir(&root);
    let mut config = fixture.config();
    config.root = root;
    assert!(matches!(
        LocalBlobStore::open(config).await,
        Err(BlobError::Configuration(_))
    ));
    let mut config = fixture.config();
    config.root = fixture.base.join("../blobs");
    assert!(LocalBlobStore::open(config).await.is_err());
    let checkout = fixture.base.join("checkout");
    mkdir(&checkout);
    mkdir(&checkout.join(".git"));
    let root = checkout.join("blobs");
    mkdir(&root);
    let mut config = fixture.config();
    config.root = root;
    assert!(matches!(
        LocalBlobStore::open(config).await,
        Err(BlobError::Configuration(_))
    ));
    for key in [
        "../escape",
        "/etc/passwd",
        "file.png",
        "00000000-0000-0000-0000-000000000000",
        "018f57ba-7d26-7b7c-9d70-a9a05342db99",
    ] {
        assert!(key.parse::<BlobKey>().is_err());
    }
    assert!(
        BlobKey::new()
            .to_string()
            .to_uppercase()
            .parse::<BlobKey>()
            .is_err()
    );
}

#[tokio::test]
async fn substituted_symlinks_hardlinks_and_renamed_root_do_not_escape_descriptors() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let outside = fixture.base.join("outside");
    fs::write(&outside, b"untouched").unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o400)).unwrap();
    let want = expected(b"untouched");
    store.put(want, b"untouched".as_slice()).await.unwrap();
    fs::remove_file(fixture.object(want.key)).unwrap();
    symlink(&outside, fixture.object(want.key)).unwrap();
    assert!(matches!(
        store.open_verified(want).await,
        Err(BlobError::UnsafeEntry)
    ));
    assert!(matches!(
        store
            .cleanup(vec![CleanupRequest {
                key: want.key,
                older_than: SystemTime::now() + Duration::from_secs(1)
            }])
            .await,
        Err(BlobError::UnsafeEntry)
    ));
    fs::remove_file(fixture.object(want.key)).unwrap();
    fs::hard_link(&outside, fixture.object(want.key)).unwrap();
    assert!(matches!(
        store.open_verified(want).await,
        Err(BlobError::UnsafeEntry)
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");
    fs::remove_file(fixture.object(want.key)).unwrap();
    let stage_key = expected(b"more");
    symlink(&outside, fixture.stage(stage_key.key)).unwrap();
    assert!(matches!(
        store.put(stage_key, b"more".as_slice()).await,
        Err(BlobError::UnsafeEntry)
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");
    let pinned = fixture.base.join("pinned");
    fs::rename(&fixture.root, &pinned).unwrap();
    mkdir(&fixture.root);
    let new = expected(b"pinned bytes");
    store.put(new, b"pinned bytes".as_slice()).await.unwrap();
    assert!(pinned.join("objects").join(new.key.to_string()).exists());
    assert!(!fixture.object(new.key).exists());
}

#[tokio::test]
async fn missing_and_corrupt_ready_blobs_are_explicit_integrity_failures() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let want = expected(b"original");
    assert!(matches!(
        store.open_verified(want).await,
        Err(BlobError::IntegrityMissing)
    ));
    store.put(want, b"original".as_slice()).await.unwrap();
    fs::set_permissions(fixture.object(want.key), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(fixture.object(want.key), b"modified").unwrap();
    fs::set_permissions(fixture.object(want.key), fs::Permissions::from_mode(0o400)).unwrap();
    assert!(matches!(
        store.open_verified(want).await,
        Err(BlobError::IntegrityCorrupt)
    ));
    assert!(matches!(
        store.scan(want).await,
        Err(BlobError::IntegrityCorrupt)
    ));
    fs::remove_file(fixture.object(want.key)).unwrap();
    assert!(matches!(
        store.open_verified(want).await,
        Err(BlobError::IntegrityMissing)
    ));
}

#[tokio::test]
async fn terminal_cleanup_respects_age_and_readers_then_prevents_late_republication() {
    let fixture = Fixture::new();
    let store = fixture.store().await;
    let want = expected(b"retire");
    store.put(want, b"retire".as_slice()).await.unwrap();
    let old = CleanupRequest {
        key: want.key,
        older_than: SystemTime::UNIX_EPOCH,
    };
    assert_eq!(
        store.cleanup(vec![old]).await.unwrap()[0].state,
        CleanupState::TooRecent
    );
    let current = CleanupRequest {
        key: want.key,
        older_than: SystemTime::now() + Duration::from_secs(1),
    };
    let reader = store.open_verified(want).await.unwrap();
    assert_eq!(
        store.cleanup(vec![current]).await.unwrap()[0].state,
        CleanupState::Busy
    );
    drop(reader);
    assert_eq!(
        store.cleanup(vec![current]).await.unwrap()[0].state,
        CleanupState::Removed
    );
    assert_eq!(
        store.cleanup(vec![current]).await.unwrap()[0].state,
        CleanupState::Missing
    );
    assert!(matches!(
        store.put(want, b"retire".as_slice()).await,
        Err(BlobError::Retired)
    ));
    let absent = expected(b"late");
    assert_eq!(
        store
            .cleanup(vec![CleanupRequest {
                key: absent.key,
                older_than: current.older_than
            }])
            .await
            .unwrap()[0]
            .state,
        CleanupState::Missing
    );
    assert!(matches!(
        store.put(absent, b"late".as_slice()).await,
        Err(BlobError::Retired)
    ));
    assert!(store.cleanup(vec![current; 101]).await.is_err());
}

async fn scanner_store(
    fixture: &Fixture,
    reply: Vec<u8>,
    expected_data: Vec<u8>,
) -> (LocalBlobStore, tokio::task::JoinHandle<()>) {
    let socket = fixture.base.join("scanner.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut command = [0_u8; 10];
        stream.read_exact(&mut command).await.unwrap();
        assert_eq!(&command, b"zINSTREAM\0");
        let mut received = Vec::new();
        loop {
            let size = stream.read_u32().await.unwrap() as usize;
            if size == 0 {
                break;
            }
            assert!(size <= 65536);
            let mut bytes = vec![0; size];
            stream.read_exact(&mut bytes).await.unwrap();
            received.extend_from_slice(&bytes);
        }
        assert_eq!(received, expected_data);
        stream.write_all(&reply).await.unwrap();
    });
    let mut config = fixture.config();
    config.scanner = ScannerConfig {
        socket: Some(socket),
        required: true,
        timeout: Duration::from_secs(2),
    };
    (LocalBlobStore::open(config).await.unwrap(), task)
}

#[tokio::test]
async fn clamav_instream_handles_clean_detected_protocol_and_bounded_reply() {
    for (reply, result) in [
        (b"stream: OK\0".to_vec(), "clean"),
        (
            b"stream: OK\0stream: unexpected ERROR\0".to_vec(),
            "protocol",
        ),
        (b"stream: Fixture.Signature FOUND\0".to_vec(), "detected"),
        (
            b"INSTREAM size limit exceeded. ERROR\0".to_vec(),
            "protocol",
        ),
        (vec![b'x'; 1026], "protocol"),
    ] {
        let fixture = Fixture::new();
        let data = vec![b'a'; 150_000];
        let want = expected(&data);
        let (store, task) = scanner_store(&fixture, reply, data.clone()).await;
        store.put(want, data.as_slice()).await.unwrap();
        match (result, store.scan(want).await) {
            ("clean", Ok(ScanStatus::Clean))
            | ("detected", Err(BlobError::MalwareDetected))
            | ("protocol", Err(BlobError::ScannerProtocol)) => (),
            other => panic!("unexpected scan outcome: {other:?}"),
        }
        task.await.unwrap();
        assert!(fixture.object(want.key).exists());
    }
}

#[tokio::test]
async fn required_unavailable_and_timed_out_scanner_never_report_clean() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.scanner.required = true;
    assert!(matches!(
        LocalBlobStore::open(config.clone()).await,
        Err(BlobError::Configuration(_))
    ));
    config.scanner.socket = Some(fixture.base.join("missing.sock"));
    let store = LocalBlobStore::open(config).await.unwrap();
    let want = expected(b"scan");
    store.put(want, b"scan".as_slice()).await.unwrap();
    assert!(matches!(
        store.scan(want).await,
        Err(BlobError::ScannerUnavailable)
    ));
    let socket = fixture.base.join("silent.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(stream);
    });
    let mut config = fixture.config();
    config.scanner = ScannerConfig {
        socket: Some(socket),
        required: false,
        timeout: Duration::from_millis(50),
    };
    let store = LocalBlobStore::open(config).await.unwrap();
    assert!(matches!(
        store.scan(want).await,
        Err(BlobError::ScannerTimeout)
    ));
    task.abort();
}
