use super::{BlobError, CHUNK, Result, ScanStatus};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

/// Protocol verified against the official ClamD manual/protocol guide:
/// https://docs.clamav.net/manual/Usage/ClamdProtocol.html
/// https://github.com/Cisco-Talos/clamav/blob/main/docs/man/clamd.8.in
/// NUL-framed zINSTREAM; big-endian u32 chunks; zero chunk ends the stream.
pub(super) async fn scan<R: AsyncRead + Unpin>(
    socket: &Path,
    timeout: Duration,
    reader: &mut R,
    expected: u64,
) -> Result<ScanStatus> {
    tokio::time::timeout(timeout, async {
        let mut socket = UnixStream::connect(socket)
            .await
            .map_err(|_| BlobError::ScannerUnavailable)?;
        socket
            .write_all(b"zINSTREAM\0")
            .await
            .map_err(|_| BlobError::ScannerUnavailable)?;
        let mut buffer = vec![0_u8; CHUNK];
        let mut size = 0_u64;
        loop {
            let count = reader.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > expected {
                return Err(BlobError::IntegrityCorrupt);
            }
            socket
                .write_all(&(count as u32).to_be_bytes())
                .await
                .map_err(|_| BlobError::ScannerUnavailable)?;
            socket
                .write_all(&buffer[..count])
                .await
                .map_err(|_| BlobError::ScannerUnavailable)?;
        }
        if size != expected {
            return Err(BlobError::IntegrityCorrupt);
        }
        socket
            .write_all(&0_u32.to_be_bytes())
            .await
            .map_err(|_| BlobError::ScannerUnavailable)?;
        // The daemon's untrusted diagnostic is neither logged nor forwarded.
        // A fixed reply cap + whole-operation deadline bound broken scanners.
        let mut reply = Vec::with_capacity(128);
        loop {
            let byte = socket
                .read_u8()
                .await
                .map_err(|_| BlobError::ScannerProtocol)?;
            if byte == 0 {
                break;
            }
            if reply.len() == 1024 {
                return Err(BlobError::ScannerProtocol);
            }
            reply.push(byte);
        }
        if reply.starts_with(b"stream: ") && reply.ends_with(b" FOUND") {
            Err(BlobError::MalwareDetected)
        } else if reply == b"stream: OK" {
            // Non-session commands finish when clamd closes the connection.
            // Never treat a clean first record plus extra diagnostics as clean.
            let mut trailing = [0_u8; 1];
            if socket
                .read(&mut trailing)
                .await
                .map_err(|_| BlobError::ScannerProtocol)?
                != 0
            {
                return Err(BlobError::ScannerProtocol);
            }
            Ok(ScanStatus::Clean)
        } else {
            Err(BlobError::ScannerProtocol)
        }
    })
    .await
    .map_err(|_| BlobError::ScannerTimeout)?
}
