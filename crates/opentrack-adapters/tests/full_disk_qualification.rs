#![cfg(target_os = "linux")]

use opentrack_adapters::local_blob::{
    BlobError, BlobKey, ExpectedBlob, LocalBlobConfig, LocalBlobStore, ScannerConfig,
};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, os::unix::fs::DirBuilderExt, path::PathBuf, time::Duration};
use tokio::io::AsyncReadExt;

#[tokio::test]
#[ignore = "requires scripts/qualify-attachment-storage.sh and its private bounded tmpfs"]
async fn real_enospc_never_publishes_and_same_key_recovers() {
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
    let bytes = vec![b'x'; 2 * 1024 * 1024];
    let expected = ExpectedBlob {
        key: BlobKey::new(),
        size: bytes.len() as u64,
        sha256: Sha256::digest(&bytes).into(),
    };
    let filler = mount.join("filler");
    let mut file = fs::File::create_new(&filler).unwrap();
    file.write_all(&vec![b'f'; 3 * 1024 * 1024]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert!(matches!(store.put(expected, bytes.as_slice()).await,
        Err(BlobError::Io(error)) if error.raw_os_error() == Some(libc::ENOSPC)));
    assert_eq!(fs::read_dir(root.join("objects")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(root.join("staging")).unwrap().count(), 0);
    fs::remove_file(filler).unwrap();
    let receipt = store.put(expected, bytes.as_slice()).await.unwrap();
    assert_eq!(receipt.size, expected.size);
    assert_eq!(receipt.sha256, expected.sha256);
    let mut downloaded = Vec::new();
    store
        .open_verified(expected)
        .await
        .unwrap()
        .read_to_end(&mut downloaded)
        .await
        .unwrap();
    assert_eq!(downloaded.len(), bytes.len());
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(&downloaded)),
        expected.sha256
    );
    assert_eq!(fs::read_dir(root.join("objects")).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root.join("staging")).unwrap().count(), 0);
}
