//! Native-only file handles and bounded transfers. IPC never accepts a path,
//! bearer, raw body or arbitrary destination. Dialog paths stay inside Rust.
mod files;

use crate::{
    links::ServerOrigin,
    persistence::SavedServer,
    protocol::{self, ApiRequest, Method, NativeError},
    state::{Expected, Runtime},
    transport::ApiReply,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    future::Future,
    io,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncSeekExt, AsyncWriteExt, ReadBuf},
    sync::{OwnedSemaphorePermit, Semaphore, watch},
};
use tokio_util::io::ReaderStream;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
const HANDLE_TTL: Duration = Duration::from_secs(30 * 60);
const CANCEL_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Serialize, Clone)]
pub struct PickedFile {
    pub handle: Uuid,
    pub original_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub mime_hint: Option<String>,
}
struct Handle {
    file: Arc<File>,
    value: PickedFile,
    expires: Instant,
    origin: String,
}
struct Operation {
    handle: Option<Uuid>,
    cancel: watch::Sender<bool>,
    active: Arc<AtomicBool>,
}
#[derive(Default)]
pub(crate) struct AttachmentState {
    handles: HashMap<Uuid, Handle>,
    operations: HashMap<Uuid, Operation>,
    cancelled: HashMap<Uuid, Instant>,
    cancellation_barrier: Option<Instant>,
}
impl AttachmentState {
    pub fn reset(&mut self) {
        self.handles.clear();
        self.cancelled.clear();
        self.cancellation_barrier = None;
        for (_, operation) in self.operations.drain() {
            let _ = operation.cancel.send(true);
        }
    }
    fn prune(&mut self) {
        self.cancelled
            .retain(|_, expires| *expires > Instant::now());
        if self
            .cancellation_barrier
            .is_some_and(|until| until <= Instant::now())
        {
            self.cancellation_barrier = None;
        }
        self.handles
            .retain(|_, handle| handle.expires > Instant::now());
        self.operations
            .retain(|_, operation| operation.active.load(Ordering::Acquire));
    }
    fn operation(
        &mut self,
        id: Uuid,
        handle: Option<Uuid>,
    ) -> Result<(OperationGuard, watch::Receiver<bool>), NativeError> {
        self.prune();
        if self.cancelled.contains_key(&id) {
            return Err(cancelled_upload());
        }
        if self.cancellation_barrier.is_some() {
            return Err(cancel_queue_full());
        }
        if self.operations.len() >= 4
            || self.operations.contains_key(&id)
            || handle.is_some_and(|handle| {
                self.operations
                    .values()
                    .any(|operation| operation.handle == Some(handle))
            })
        {
            return Err(NativeError::new(
                "attachment_busy",
                "Another file transfer is still running. Wait or cancel it first.",
                true,
            ));
        }
        let (cancel, receiver) = watch::channel(false);
        let active = Arc::new(AtomicBool::new(true));
        self.operations.insert(
            id,
            Operation {
                handle,
                cancel,
                active: active.clone(),
            },
        );
        Ok((OperationGuard(active), receiver))
    }
}
struct OperationGuard(Arc<AtomicBool>);
impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    pub expected_generation: String,
    pub handle: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadRequest {
    pub expected_generation: String,
    pub workspace_id: Uuid,
    pub attachment_id: Uuid,
    pub handle: Uuid,
    pub etag: String,
    pub operation_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    pub expected_generation: String,
    pub operation_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadRequest {
    pub expected_generation: String,
    pub workspace_id: Uuid,
    pub attachment_id: Uuid,
    pub original_name: String,
}
#[derive(Serialize)]
pub struct DownloadReply {
    pub saved: bool,
    pub generation: String,
}
#[derive(Serialize, Clone)]
pub struct AttachmentProgress {
    pub operation_id: Uuid,
    pub sent_bytes: u64,
    pub total_bytes: u64,
}
pub type ProgressEmitter = Arc<dyn Fn(AttachmentProgress) + Send + Sync>;

impl Runtime {
    async fn attachment_file_task<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> Result<T, NativeError> + Send + 'static,
    ) -> Result<T, NativeError> {
        file_task(self.attachment_io.clone(), operation).await
    }
    /// One user dialog at a time. The shell must hold this until its callback and
    /// handle registration complete; stale dialogs never install fresh handles.
    pub async fn attachment_dialog_permit(
        &self,
        expected: &str,
    ) -> Result<OwnedSemaphorePermit, NativeError> {
        let inner = self.inner.lock().await;
        inner.check(protocol::generation(expected)?)?;
        inner.selected()?;
        self.attachment_dialogs
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                NativeError::new(
                    "file_dialog_busy",
                    "Finish the open file dialog first.",
                    true,
                )
            })
    }

    /// Trusted shell path, deliberately absent from the IPC argument schema.
    pub async fn accept_attachment_selection(
        &self,
        request: Expected,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<PickedFile>, NativeError> {
        let generation = protocol::generation(&request.expected_generation)?;
        let origin = {
            let mut inner = self.inner.lock().await;
            inner.check(generation)?;
            inner.attachments.prune();
            if paths.len() > 16 || inner.attachments.handles.len() + paths.len() > 16 {
                return Err(handle_limit());
            }
            inner.selected()?.origin
        };
        let picked = self
            .attachment_file_task(move || {
                paths
                    .into_iter()
                    .map(files::pick)
                    .collect::<Result<Vec<_>, _>>()
            })
            .await?;
        let mut inner = self.inner.lock().await;
        inner.check(generation)?;
        inner.attachments.prune();
        if inner.attachments.handles.len() + picked.len() > 16 || inner.selected()?.origin != origin
        {
            return Err(handle_limit());
        }
        let mut result = Vec::with_capacity(picked.len());
        for (file, original_name, size_bytes, sha256) in picked {
            let value = PickedFile {
                handle: Uuid::new_v4(),
                original_name,
                size_bytes,
                sha256,
                mime_hint: None,
            };
            inner.attachments.handles.insert(
                value.handle,
                Handle {
                    file: Arc::new(file),
                    value: value.clone(),
                    expires: Instant::now() + HANDLE_TTL,
                    origin: origin.clone(),
                },
            );
            result.push(value);
        }
        Ok(result)
    }
    pub async fn attachment_release(&self, request: ReleaseRequest) -> Result<(), NativeError> {
        let mut inner = self.inner.lock().await;
        inner.check(protocol::generation(&request.expected_generation)?)?;
        inner.attachments.handles.remove(&request.handle);
        for operation in inner.attachments.operations.values() {
            if operation.handle == Some(request.handle) {
                let _ = operation.cancel.send(true);
            }
        }
        Ok(())
    }
    pub async fn attachment_cancel(&self, request: CancelRequest) -> Result<(), NativeError> {
        let mut inner = self.inner.lock().await;
        inner.check(protocol::generation(&request.expected_generation)?)?;
        inner.attachments.prune();
        // IPC tasks may register out of order. Remember cancellation before an
        // upload is registered, without allowing unbounded renderer input.
        if let Some(operation) = inner.attachments.operations.get(&request.operation_id) {
            let _ = operation.cancel.send(true);
        }
        if inner.attachments.cancelled.len() >= 64
            && !inner
                .attachments
                .cancelled
                .contains_key(&request.operation_id)
        {
            // Pause new registrations instead of evicting a live cancellation
            // and admitting that delayed upload after the table fills.
            inner.attachments.cancellation_barrier = Some(Instant::now() + CANCEL_TTL);
            return Err(cancel_queue_full());
        }
        inner
            .attachments
            .cancelled
            .insert(request.operation_id, Instant::now() + CANCEL_TTL);
        Ok(())
    }
    pub async fn attachment_upload(
        &self,
        request: UploadRequest,
        emit: ProgressEmitter,
    ) -> Result<ApiReply, NativeError> {
        let generation = protocol::generation(&request.expected_generation)?;
        if request.etag.is_empty()
            || request.etag.len() > 512
            || !request.etag.is_ascii()
            || request.etag.bytes().any(|b| b.is_ascii_control())
        {
            return Err(NativeError::invalid());
        }
        let (server, file, picked, _operation, mut cancel) = {
            let mut inner = self.inner.lock().await;
            inner.check(generation)?;
            inner.attachments.prune();
            let server = inner.selected()?;
            let handle = inner
                .attachments
                .handles
                .get(&request.handle)
                .ok_or_else(expired)?;
            if handle.origin != server.origin {
                return Err(expired());
            }
            let file = handle.file.clone();
            let picked = handle.value.clone();
            let (guard, cancel) = inner
                .attachments
                .operation(request.operation_id, Some(request.handle))?;
            (server, file, picked, guard, cancel)
        };
        let operation = async {
            let token = self.attachment_token(generation, &server).await?;
            let origin = ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
            let file = self
                .attachment_file_task(move || {
                    files::clone_unchanged(&file, picked.size_bytes, &picked.sha256)
                })
                .await?;
            let mut file = tokio::fs::File::from_std(file);
            file.rewind().await.map_err(|_| file_error())?;
            self.inner.lock().await.check(generation)?;
            let reader = UploadReader {
                file,
                sent: 0,
                total: picked.size_bytes,
                operation_id: request.operation_id,
                emit,
            };
            let body = reqwest::Body::wrap_stream(ReaderStream::with_capacity(reader, 64 * 1024));
            let reply = self
                .transport
                .upload_attachment(
                    &origin,
                    request.workspace_id,
                    request.attachment_id,
                    &request.etag,
                    picked.size_bytes,
                    body,
                    &token,
                )
                .await?;
            if self.inner.lock().await.check(generation).is_err() {
                return Err(uncertain());
            }
            if reply.status == 401 {
                self.clear_credential(
                    generation,
                    &server,
                    Some(NativeError::new(
                        "session_expired",
                        "Your session ended. Sign in again to continue.",
                        false,
                    )),
                    None,
                )
                .await?;
            }
            reply.api(generation)
        };
        tokio::select! { biased;
            _ = cancelled(&mut cancel) => Err(cancelled_upload()),
            result = operation => result,
        }
    }
    async fn attachment_token(
        &self,
        generation: u64,
        server: &SavedServer,
    ) -> Result<Zeroizing<String>, NativeError> {
        let token = self.token(server).await?;
        self.inner.lock().await.check(generation)?;
        match token {
            Some(token) => Ok(token),
            None => {
                if server.credential.is_some() {
                    self.clear_credential(
                        generation,
                        server,
                        Some(NativeError::new(
                            "session_expired",
                            "Sign in again to continue.",
                            false,
                        )),
                        None,
                    )
                    .await?;
                }
                Err(NativeError::new(
                    "sign_in_required",
                    "Sign in to this server first.",
                    false,
                ))
            }
        }
    }

    /// The chooser closure is provided by native Rust, never by the renderer.
    pub async fn attachment_download<F, Fut>(
        &self,
        request: DownloadRequest,
        choose: F,
    ) -> Result<DownloadReply, NativeError>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Option<PathBuf>>,
    {
        let generation = protocol::generation(&request.expected_generation)?;
        if request.original_name.len() > 255 || request.original_name.chars().any(char::is_control)
        {
            return Err(NativeError::invalid());
        }
        let _dialog = self
            .attachment_dialog_permit(&request.expected_generation)
            .await?;
        let path = format!(
            "/api/v1/workspaces/{}/attachments/{}",
            request.workspace_id, request.attachment_id
        );
        let metadata = self
            .api(ApiRequest {
                expected_generation: request.expected_generation.clone(),
                method: Method::Get,
                path,
                query: None,
                headers: BTreeMap::new(),
                body: None,
            })
            .await?;
        if metadata.status != 200 {
            return Err(NativeError::new(
                "attachment_unavailable",
                "This attachment is not available for download.",
                false,
            ));
        }
        let metadata = metadata.body.ok_or_else(NativeError::server)?;
        let size = metadata["size_bytes"]
            .as_u64()
            .filter(|size| *size > 0 && *size <= MAX_ATTACHMENT_BYTES)
            .ok_or_else(NativeError::server)?;
        let digest = metadata["sha256"]
            .as_str()
            .filter(|hash| valid_hash(hash))
            .ok_or_else(NativeError::server)?
            .to_string();
        if metadata["id"] != request.attachment_id.to_string() || metadata["state"] != "ready" {
            return Err(NativeError::server());
        }
        let filename = metadata["original_name"]
            .as_str()
            .filter(|name| valid_name(name))
            .ok_or_else(NativeError::server)?
            .to_string();
        let (server, _operation, cancel) = {
            let mut inner = self.inner.lock().await;
            inner.check(generation)?;
            let server = inner.selected()?;
            let (guard, cancel) = inner.attachments.operation(Uuid::new_v4(), None)?;
            (server, guard, cancel)
        };
        let Some(destination) = choose(filename).await else {
            return Ok(DownloadReply {
                saved: false,
                generation: generation.to_string(),
            });
        };
        self.inner.lock().await.check(generation)?;
        let mut cancellation = cancel.clone();
        let operation = async {
            let token = self.attachment_token(generation, &server).await?;
            let origin = ServerOrigin::parse(&server.origin).map_err(|_| NativeError::invalid())?;
            let mut response = self
                .transport
                .download_attachment(&origin, request.workspace_id, request.attachment_id, &token)
                .await?;
            if response.status().as_u16() == 401 {
                self.clear_credential(
                    generation,
                    &server,
                    Some(NativeError::new(
                        "session_expired",
                        "Your session ended. Sign in again to continue.",
                        false,
                    )),
                    None,
                )
                .await?;
                return Err(NativeError::new(
                    "attachment_unavailable",
                    "Sign in again before downloading this attachment.",
                    false,
                ));
            }
            if response.status().as_u16() != 200
                || response.content_length() != Some(size)
                || response
                    .headers()
                    .get("content-type")
                    .and_then(|value| value.to_str().ok())
                    != Some("application/octet-stream")
            {
                return Err(NativeError::server());
            }
            self.inner.lock().await.check(generation)?;
            let (target, file) = self
                .attachment_file_task(move || files::DownloadTarget::create(destination))
                .await?;
            let mut file = tokio::fs::File::from_std(file);
            let mut hash = Sha256::new();
            let mut received = 0_u64;
            while let Some(bytes) = response.chunk().await.map_err(|_| NativeError::network())? {
                received += bytes.len() as u64;
                if received > size {
                    return Err(integrity());
                }
                file.write_all(&bytes).await.map_err(|_| file_error())?;
                hash.update(&bytes);
            }
            if received != size || hex(&hash.finalize()) != digest {
                return Err(integrity());
            }
            file.flush().await.map_err(|_| file_error())?;
            let file = file.into_std().await;
            let target = self
                .attachment_file_task(move || target.prepare(file))
                .await?;
            // Serialize the final short publish with connection changes. Nothing
            // is published after a completed generation switch/cancellation.
            let inner = self.inner.lock().await;
            inner.check(generation)?;
            if *cancel.borrow() {
                return Err(cancelled_download());
            }
            self.attachment_file_task(move || target.publish()).await?;
            drop(inner);
            Ok(DownloadReply {
                saved: true,
                generation: generation.to_string(),
            })
        };
        tokio::select! { biased;
            _ = cancelled(&mut cancellation) => Err(cancelled_download()),
            result = operation => result,
        }
    }
}
struct UploadReader {
    file: tokio::fs::File,
    sent: u64,
    total: u64,
    operation_id: Uuid,
    emit: ProgressEmitter,
}
impl AsyncRead for UploadReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        match Pin::new(&mut self.file).poll_read(cx, buffer) {
            Poll::Ready(Ok(())) => {
                self.sent += (buffer.filled().len() - before) as u64;
                if self.sent > self.total {
                    return Poll::Ready(Err(io::Error::other("selected file changed")));
                }
                (self.emit)(AttachmentProgress {
                    operation_id: self.operation_id,
                    sent_bytes: self.sent,
                    total_bytes: self.total,
                });
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}
async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    while !*receiver.borrow_and_update() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
        && name != "."
        && name != ".."
}
fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn hex(hash: &[u8]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn handle_limit() -> NativeError {
    NativeError::new(
        "file_handle_limit",
        "Select at most 16 files, or remove unused selections first.",
        false,
    )
}
fn expired() -> NativeError {
    NativeError::new(
        "file_handle_expired",
        "Select the file again; its private selection has expired or the connection changed.",
        false,
    )
}
fn file_error() -> NativeError {
    NativeError::new(
        "file_unavailable",
        "The selected file or destination is unavailable. Choose it again.",
        false,
    )
}
fn integrity() -> NativeError {
    NativeError::new(
        "attachment_integrity",
        "The downloaded bytes do not match the attachment. No file was saved.",
        true,
    )
}
fn uncertain() -> NativeError {
    NativeError::new(
        "stale_response",
        "The connection changed during upload. Check the attachment status before retrying.",
        false,
    )
}
fn cancelled_upload() -> NativeError {
    NativeError::new(
        "attachment_cancelled",
        "Upload cancelled. Check its status before retrying; the server may have received it.",
        false,
    )
}
fn cancelled_download() -> NativeError {
    NativeError::new(
        "attachment_cancelled",
        "Download cancelled because the connection changed. No file was saved.",
        false,
    )
}

fn cancel_queue_full() -> NativeError {
    NativeError::new(
        "attachment_cancel_queue_full",
        "Too many recent cancellations. Wait before starting another transfer.",
        true,
    )
}

async fn file_task<T: Send + 'static>(
    lane: Arc<Semaphore>,
    operation: impl FnOnce() -> Result<T, NativeError> + Send + 'static,
) -> Result<T, NativeError> {
    let permit = lane.try_acquire_owned().map_err(|_| {
        NativeError::new(
            "file_busy",
            "File operations are still finishing. Try again shortly.",
            true,
        )
    })?;
    // Cancellation cannot interrupt a kernel call: retain admission inside the
    // blocking task, not the async command that may be dropped by its caller.
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation()
    })
    .await
    .map_err(|_| file_error())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_file_command_keeps_admission_until_actual_io_finishes() {
        let lane = Arc::new(Semaphore::new(1));
        let cloned = lane.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            file_task(cloned, move || {
                let _ = started.send(());
                gate.recv().unwrap();
                Ok(())
            })
            .await
        });
        ready.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            file_task(lane.clone(), || Ok(())).await.unwrap_err().code,
            "file_busy"
        );
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while lane.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        file_task(lane, || Ok(())).await.unwrap();
    }
}
