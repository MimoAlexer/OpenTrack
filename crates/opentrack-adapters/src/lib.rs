//! External adapters. Local blobs expose bytes and receipts, never public paths.
#[cfg(any(target_os = "linux", windows))]
pub mod local_blob;

/// Open a Windows service secret without following reparse points or accepting
/// shared ACLs, hardlinks, or a concurrent writable/replacing handle.
#[cfg(windows)]
pub fn open_private_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    local_blob::open_private_file(path).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "secret file is unavailable or is not private to the service user",
        )
    })
}
