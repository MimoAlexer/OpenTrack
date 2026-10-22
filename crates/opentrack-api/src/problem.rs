use axum::{
    Json,
    extract::{Request, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use opentrack_domain::Error;
use serde::Serialize;
use serde_json::{Value, json};
use tracing::Instrument;
use uuid::Uuid;

tokio::task_local! { static REQUEST_ID: Uuid; }

pub(crate) async fn request_context(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4();
    REQUEST_ID
        .scope(request_id, async move {
            let mut response = next
                .run(request)
                .instrument(tracing::info_span!("request", %request_id))
                .await;
            response.headers_mut().insert(
                "x-request-id",
                HeaderValue::from_str(&request_id.to_string()).expect("UUID is header-safe"),
            );
            response
        })
        .await
}

/// Safe, machine-readable error shared by every JSON API route.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Problem {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    pub instance: String,
    pub code: String,
    pub request_id: Uuid,
    pub retryable: bool,
    pub field_errors: Vec<FieldError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FieldError {
    pub path: String,
    pub code: String,
    pub message: String,
}
#[derive(Debug)]
pub(crate) enum ApiError {
    Domain(Error),
    Http(StatusCode, &'static str, &'static str),
}
impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self::Domain(error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, detail, details) = match self {
            Self::Domain(error) => {
                let (status, code) = match &error {
                    Error::Invalid(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation_failed"),
                    Error::UnsupportedDocumentSchema => (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "unsupported_document_schema",
                    ),
                    Error::CursorExpired => (StatusCode::UNPROCESSABLE_ENTITY, "cursor_expired"),
                    Error::InvalidRecovery => {
                        (StatusCode::UNPROCESSABLE_ENTITY, "recovery_invalid")
                    }
                    Error::InvalidNativeAuth => {
                        (StatusCode::UNPROCESSABLE_ENTITY, "native_auth_invalid")
                    }
                    Error::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthenticated"),
                    Error::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
                    Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
                    Error::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
                    Error::Stale(_) | Error::VersionConflict(_) => {
                        (StatusCode::PRECONDITION_FAILED, "version_conflict")
                    }
                    Error::PreconditionRequired => {
                        (StatusCode::PRECONDITION_REQUIRED, "precondition_required")
                    }
                    Error::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
                    Error::Unavailable => {
                        (StatusCode::SERVICE_UNAVAILABLE, "dependency_unavailable")
                    }
                    Error::InviteSignInRequired => {
                        (StatusCode::CONFLICT, "invite_sign_in_required")
                    }
                    Error::ReauthenticationRequired => {
                        (StatusCode::FORBIDDEN, "reauthentication_required")
                    }
                    Error::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
                };
                let details = if let Error::Stale(current) = &error {
                    Some(json!({"current":current}))
                } else if let Error::VersionConflict(current) = &error {
                    Some(json!({"current":current}))
                } else {
                    None
                };
                (status, code, error.to_string(), details)
            }
            Self::Http(status, code, detail) => (status, code, detail.to_owned(), None),
        };
        problem_response(status, code, detail, details)
    }
}
pub(crate) fn problem_response(
    status: StatusCode,
    code: &str,
    detail: String,
    details: Option<Value>,
) -> Response {
    let request_id = REQUEST_ID
        .try_with(|id| *id)
        .unwrap_or_else(|_| Uuid::new_v4());
    let body = Problem {
        type_uri: format!("/api/v1/problems/{code}"),
        title: status
            .canonical_reason()
            .unwrap_or("Request failed")
            .to_owned(),
        status: status.as_u16(),
        detail,
        instance: format!("urn:uuid:{request_id}"),
        code: code.to_owned(),
        request_id,
        retryable: matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::REQUEST_TIMEOUT
        ),
        field_errors: Vec::new(),
        details,
    };
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // The request ID is generated locally, never copied from an untrusted request header.
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id.to_string()).expect("UUID is header-safe"),
    );
    if status == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("300"));
    }
    response
}
pub(crate) fn payload<T>(value: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    value
        .map(|j| j.0)
        .map_err(|rejection| match rejection.status() {
            StatusCode::PAYLOAD_TOO_LARGE => ApiError::Http(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "The request body exceeds the allowed size",
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE => ApiError::Http(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "Send a JSON request with Content-Type application/json",
            ),
            StatusCode::BAD_REQUEST => ApiError::Http(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "The request body is not valid JSON",
            ),
            _ => Error::Invalid("Request body does not match the expected fields and types".into())
                .into(),
        })
}
/// Normalize framework path/query/method rejections without exposing extractor input.
pub(crate) async fn normalize(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
        return response;
    }
    if response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v == "application/problem+json")
    {
        return response;
    }
    let (code, detail) = match status {
        StatusCode::BAD_REQUEST => ("bad_request", "The request path or query is invalid"),
        StatusCode::METHOD_NOT_ALLOWED => (
            "method_not_allowed",
            "This HTTP method is not supported for this resource",
        ),
        StatusCode::PAYLOAD_TOO_LARGE => (
            "payload_too_large",
            "The request body exceeds the allowed size",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            "unsupported_media_type",
            "Send a JSON request with Content-Type application/json",
        ),
        StatusCode::NOT_FOUND => ("not_found", "This item could not be found"),
        _ => ("request_failed", "The request could not be completed"),
    };
    let mut problem = problem_response(status, code, detail.into(), None);
    if let Some(allow) = response.headers().get(header::ALLOW) {
        problem.headers_mut().insert(header::ALLOW, allow.clone());
    }
    problem
}
