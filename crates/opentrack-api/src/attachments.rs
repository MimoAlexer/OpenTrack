//! HTTP streams cross the blob boundary only between short authorized DB phases.
use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::TryStreamExt;
use opentrack_adapters::local_blob::{
    BlobError, BlobKey, ExpectedBlob, LocalBlobStore, ScanStatus,
};
use opentrack_app::{ScanOutcome, UploadFailure, UploadPermit};
use opentrack_domain::{
    Actor, Attachment, AttachmentLimits, AttachmentQuery, AttachmentState, CommentDelete,
    CreateAttachment, Error, Page,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
};
use tokio_util::io::StreamReader;
use uuid::Uuid;

pub struct AttachmentService {
    pub store: LocalBlobStore,
    pub limits: AttachmentLimits,
    slots: Arc<Semaphore>,
}
impl AttachmentService {
    pub fn new(store: LocalBlobStore, limits: AttachmentLimits) -> Self {
        Self {
            store,
            limits,
            slots: Arc::new(Semaphore::new(8)),
        }
    }
    fn slot(&self) -> Result<OwnedSemaphorePermit, ApiError> {
        self.slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::RateLimited.into())
    }
}
fn service(s: &ApiState) -> Result<&AttachmentService, ApiError> {
    s.attachments
        .as_deref()
        .ok_or_else(|| Error::Unavailable.into())
}
pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/attachments", get(list).post(reserve))
        .route(
            "/workspaces/{w}/attachments/{id}",
            get(metadata).delete(delete),
        )
        .route(
            "/workspaces/{w}/attachments/{id}/content",
            get(download).put(upload),
        )
        .route("/workspaces/{w}/attachments/{id}/finalize", post(finalize))
}
pub(crate) fn is_transfer(method: &Method, path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 8
        && parts[0..4] == ["", "api", "v1", "workspaces"]
        && Uuid::parse_str(parts[4]).is_ok()
        && parts[5] == "attachments"
        && Uuid::parse_str(parts[6]).is_ok()
        && (matches!(*method, Method::PUT | Method::GET) && parts[7] == "content"
            || method == Method::POST && parts[7] == "finalize")
}
fn expected(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::PreconditionRequired.into())
}
fn response(value: Attachment, status: StatusCode) -> Result<Response, ApiError> {
    let etag = HeaderValue::from_str(&value.etag).map_err(|_| Error::Internal)?;
    let mut reply = (status, Json(value)).into_response();
    reply.headers_mut().insert(header::ETAG, etag);
    Ok(reply)
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/attachments",params(("w"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateAttachment,responses((status=201,body=Attachment)))]
pub(crate) async fn reserve(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<CreateAttachment>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))?;
    let value = s
        .app
        .reserve_attachment(&a, w, key, payload(body)?, &service(&s)?.limits)
        .await?;
    let location =
        HeaderValue::from_str(&format!("/api/v1/workspaces/{w}/attachments/{}", value.id))
            .map_err(|_| Error::Internal)?;
    let mut reply = response(value, StatusCode::CREATED)?;
    reply.headers_mut().insert(header::LOCATION, location);
    Ok(reply)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/attachments",params(("w"=Uuid,Path),("document_id"=Option<Uuid>,Query),("team_id"=Option<Uuid>,Query),("limit"=Option<u32>,Query),("cursor"=Option<String>,Query)),responses((status=200,body=Page<Attachment>)))]
pub(crate) async fn list(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<AttachmentQuery>,
) -> Result<Json<Page<Attachment>>, ApiError> {
    Ok(Json(s.app.attachments(&a, w, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/attachments/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Attachment)))]
pub(crate) async fn metadata(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    response(s.app.attachment(&a, w, id).await?, StatusCode::OK)
}
fn blob(permit: &UploadPermit) -> Result<ExpectedBlob, ApiError> {
    ExpectedBlob::from_hex(
        permit
            .storage_key
            .parse::<BlobKey>()
            .map_err(|_| Error::Internal)?,
        permit.size_bytes,
        &permit.sha256,
    )
    .map_err(|_| Error::Internal.into())
}
#[utoipa::path(put,path="/api/v1/workspaces/{w}/attachments/{id}/content",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Content-Length"=u64,Header)),request_body(content=String,content_type="application/octet-stream"),responses((status=200,body=Attachment),(status=202,body=Attachment)))]
pub(crate) async fn upload(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let service = service(&s)?;
    let _slot = service.slot()?;
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or_else(|| Error::Invalid("Content-Length is required for file uploads".into()))?;
    if length == 0 || length > service.limits.max_upload_bytes as u64 {
        return Err(
            Error::Invalid("This file exceeds the configured upload size limit".into()).into(),
        );
    }
    if headers.contains_key(header::CONTENT_ENCODING) {
        return Err(Error::Invalid("Encoded upload bodies are unsupported".into()).into());
    }
    let current = s.app.attachment(&a, w, id).await?;
    if current.size_bytes as u64 != length {
        return Err(
            Error::Invalid("Content-Length differs from the reserved file size".into()).into(),
        );
    }
    // An ambiguous successful transfer is reconciled against current permission.
    if current.state == AttachmentState::Ready {
        return response(current, StatusCode::OK);
    }
    let permit = s
        .app
        .begin_upload(&a, w, id, expected(&headers)?, &service.limits)
        .await?;
    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let receipt = match service
        .store
        .put(blob(&permit)?, StreamReader::new(stream))
        .await
    {
        Ok(value) => value,
        Err(error) => {
            let failure = match error {
                BlobError::HashMismatch => UploadFailure::ChecksumMismatch,
                BlobError::LengthMismatch | BlobError::TooLarge => UploadFailure::SizeMismatch,
                BlobError::UploadTimeout | BlobError::OperationInterrupted => {
                    UploadFailure::Interrupted
                }
                _ => UploadFailure::StorageUnavailable,
            };
            s.app.fail_attachment_upload(&a, &permit, failure).await?;
            return Err(blob_error(error));
        }
    };
    s.app
        .quarantine_attachment(
            &a,
            &permit,
            receipt.size,
            &format_hash(&receipt.sha256),
            receipt.mime,
        )
        .await?;
    finish(&s, &a, &permit).await
}
async fn finish(s: &ApiState, a: &Actor, permit: &UploadPermit) -> Result<Response, ApiError> {
    let outcome = match service(s)?.store.scan(blob(permit)?).await {
        Ok(ScanStatus::Clean) => ScanOutcome::Clean,
        Ok(ScanStatus::NotScanned) => ScanOutcome::NotScanned,
        Err(BlobError::MalwareDetected) => ScanOutcome::Infected,
        Err(error) => {
            tracing::warn!(kind = error_kind(&error), "Attachment scan deferred");
            ScanOutcome::Unavailable
        }
    };
    let value = s.app.finalize_attachment(a, permit, outcome).await?;
    let status = if value.state == AttachmentState::Quarantined {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    response(value, status)
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/attachments/{id}/finalize",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Attachment),(status=202,body=Attachment)))]
pub(crate) async fn finalize(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let service = service(&s)?;
    let _slot = service.slot()?;
    let current = s.app.attachment(&a, w, id).await?;
    if current.state == AttachmentState::Ready {
        return response(current, StatusCode::OK);
    }
    let permit = s.app.begin_finalize(&a, w, id, &service.limits).await?;
    finish(&s, &a, &permit).await
}
#[utoipa::path(delete,path="/api/v1/workspaces/{w}/attachments/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body(content=Option<CommentDelete>),responses((status=204)))]
pub(crate) async fn delete(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Option<Json<CommentDelete>>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    csrf(&s, &headers, &a)?;
    let input = match body {
        Ok(Some(v)) => v.0,
        Ok(None) => CommentDelete::default(),
        Err(e) => payload(Err(e))?,
    };
    s.app
        .delete_attachment(&a, w, id, expected(&headers)?, input)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/attachments/{id}/content",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,description="Authorized immutable bytes; attachment disposition, no-store and nosniff",content_type="application/octet-stream",body=String)))]
pub(crate) async fn download(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let service = service(&s)?;
    let slot = service.slot()?;
    let value = s.app.attachment_download(&a, w, id).await?;
    let expected = ExpectedBlob::from_hex(
        value.storage_key.parse().map_err(|_| Error::Internal)?,
        value.size_bytes,
        &value.sha256,
    )
    .map_err(|_| Error::Internal)?;
    let file = service
        .store
        .open_verified(expected)
        .await
        .map_err(blob_error)?;
    // Recheck after disk verification; no stale pre-signed capability escapes.
    s.app.attachment_download(&a, w, id).await?;
    let mut reply = bounded_download_body(file, slot, Duration::from_secs(180)).into_response();
    reply.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    reply.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&value.size_bytes.to_string()).map_err(|_| Error::Internal)?,
    );
    reply.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition(&value.original_name)).map_err(|_| Error::Internal)?,
    );
    reply.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    Ok(reply)
}
// Admission happens before this task is spawned (at most eight transfers). The
// deadline belongs to the producer, so an unpolled response cannot retain the
// verified file lock or transfer permit indefinitely. The channel holds 128KiB,
// plus one 64KiB chunk in the producer.
fn bounded_download_body<R>(mut reader: R, permit: OwnedSemaphorePermit, deadline: Duration) -> Body
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    tokio::spawn(async move {
        let _permit = permit;
        let transfer = async {
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let length = tokio::select! {
                    _ = sender.closed() => return Ok(()),
                    length = reader.read(&mut buffer) => length?,
                };
                if length == 0 {
                    return Ok(());
                }
                if sender
                    .send(Ok(Bytes::copy_from_slice(&buffer[..length])))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
        };
        let failure = match tokio::time::timeout(deadline, transfer).await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(_) => Some(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Download timed out",
            )),
        };
        if let Some(error) = failure {
            // Do not wait for a stalled reader to accept the error. Closing the
            // stream before Content-Length bytes also makes truncation explicit.
            let _ = sender.try_send(Err(error));
        }
    });
    Body::from_stream(futures_util::stream::unfold(
        receiver,
        |mut receiver| async { receiver.recv().await.map(|item| (item, receiver)) },
    ))
}
fn format_hash(hash: &[u8; 32]) -> String {
    hash.iter().map(|v| format!("{v:02x}")).collect()
}
fn disposition(name: &str) -> String {
    let encoded: String = name
        .as_bytes()
        .iter()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(b) {
                (*b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("attachment; filename=\"download\"; filename*=UTF-8''{encoded}")
}
fn error_kind(error: &BlobError) -> &'static str {
    match error {
        BlobError::IntegrityMissing => "missing_blob",
        BlobError::IntegrityCorrupt => "corrupt_blob",
        BlobError::ScannerUnavailable | BlobError::ScannerTimeout | BlobError::ScannerProtocol => {
            "scanner_unavailable"
        }
        _ => "blob_unavailable",
    }
}
fn blob_error(error: BlobError) -> ApiError {
    match error {
        BlobError::HashMismatch | BlobError::LengthMismatch | BlobError::TooLarge => {
            Error::Invalid("Uploaded bytes do not match the reserved file".into()).into()
        }
        BlobError::Busy => Error::Conflict(
            "This file is already being processed. Refresh its status before retrying".into(),
        )
        .into(),
        _ => {
            tracing::error!(
                kind = error_kind(&error),
                "Attachment storage operation failed"
            );
            Error::Unavailable.into()
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unpolled_download_releases_admission_at_deadline() {
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().try_acquire_owned().unwrap();
        let body = bounded_download_body(
            std::io::Cursor::new(vec![42_u8; 512 * 1024]),
            permit,
            Duration::from_millis(20),
        );
        assert_eq!(slots.available_permits(), 0);
        let released = tokio::time::timeout(Duration::from_secs(1), slots.acquire()).await;
        assert!(released.is_ok(), "unpolled body kept its transfer permit");
        if let Ok(bytes) = axum::body::to_bytes(body, 512 * 1024).await {
            assert!(bytes.len() < 512 * 1024);
        }
    }
    #[tokio::test]
    async fn disconnected_download_releases_admission_without_waiting_for_deadline() {
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().try_acquire_owned().unwrap();
        let (reader, _writer) = tokio::io::duplex(1);
        let body = bounded_download_body(reader, permit, Duration::from_secs(180));
        drop(body);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), slots.acquire())
                .await
                .is_ok()
        );
    }
    #[test]
    fn filenames_are_header_safe() {
        assert_eq!(
            disposition("a\r\n\"é.txt"),
            "attachment; filename=\"download\"; filename*=UTF-8''a%0D%0A%22%C3%A9.txt"
        );
    }
    #[test]
    fn only_exact_transfer_routes_receive_long_deadline() {
        let path = format!(
            "/api/v1/workspaces/{}/attachments/{}/content",
            Uuid::new_v4(),
            Uuid::new_v4()
        );
        assert!(is_transfer(&Method::PUT, &path));
        assert!(!is_transfer(&Method::POST, &path));
        assert!(!is_transfer(
            &Method::PUT,
            "/api/v1/workspaces/x/attachments/y/content"
        ));
    }
}
