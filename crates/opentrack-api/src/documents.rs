use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use opentrack_domain::{
    Comment, CommentDelete, DiscussionQuery, Document, DocumentRevision, DocumentRevisionSummary,
    DocumentWrite, Error, Page,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/workspaces/{w}/issues/{issue}/document",
            get(issue_document),
        )
        .route(
            "/workspaces/{w}/documents/{document}",
            get(document).patch(update_document),
        )
        .route(
            "/workspaces/{w}/documents/{document}/revisions",
            get(revisions),
        )
        .route(
            "/workspaces/{w}/documents/{document}/revisions/{revision}",
            get(revision),
        )
        .route(
            "/workspaces/{w}/issues/{issue}/comments",
            get(comments).post(create_comment),
        )
        .route(
            "/workspaces/{w}/comments/{comment}",
            get(comment).patch(update_comment).delete(delete_comment),
        )
        .route(
            "/workspaces/{w}/issues/{issue}/activity-page",
            get(activity_page),
        )
}

fn response<T: serde::Serialize>(
    value: T,
    etag: &str,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let mut response = (status, Json(value)).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(etag).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}
fn expected(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::PreconditionRequired.into())
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{issue}/document",params(("w"=Uuid,Path),("issue"=Uuid,Path)),responses((status=200,body=Document)))]
pub(crate) async fn issue_document(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let value = s.app.issue_document(&a, w, id).await?;
    let tag = value.etag.clone();
    response(value, &tag, StatusCode::OK)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/documents/{document}",params(("w"=Uuid,Path),("document"=Uuid,Path)),responses((status=200,body=Document)))]
pub(crate) async fn document(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let value = s.app.get_document(&a, w, id).await?;
    let tag = value.etag.clone();
    response(value, &tag, StatusCode::OK)
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/documents/{document}",params(("w"=Uuid,Path),("document"=Uuid,Path),("If-Match"=String,Header)),request_body=DocumentWrite,responses((status=200,body=Document)))]
pub(crate) async fn update_document(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<DocumentWrite>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let value = s
        .app
        .update_document(&a, w, id, expected(&headers)?, payload(body)?)
        .await?;
    let tag = value.etag.clone();
    response(value, &tag, StatusCode::OK)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/documents/{document}/revisions",params(("w"=Uuid,Path),("document"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<DocumentRevisionSummary>)))]
pub(crate) async fn revisions(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<DiscussionQuery>,
) -> Result<Json<Page<DocumentRevisionSummary>>, ApiError> {
    Ok(Json(s.app.document_revisions(&a, w, id, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/documents/{document}/revisions/{revision}",params(("w"=Uuid,Path),("document"=Uuid,Path),("revision"=i64,Path,minimum=1)),responses((status=200,body=DocumentRevision)))]
pub(crate) async fn revision(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id, version)): Path<(Uuid, Uuid, i64)>,
) -> Result<Response, ApiError> {
    if version < 1 {
        return Err(Error::Invalid("Revision must be positive".into()).into());
    }
    let value = s.app.document_revision(&a, w, id, version).await?;
    let tag = opentrack_domain::graph_etag(&value)?;
    response(value, &tag, StatusCode::OK)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{issue}/comments",params(("w"=Uuid,Path),("issue"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<Comment>)))]
pub(crate) async fn comments(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<DiscussionQuery>,
) -> Result<Json<Page<Comment>>, ApiError> {
    Ok(Json(s.app.list_comments(&a, w, id, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/comments/{comment}",params(("w"=Uuid,Path),("comment"=Uuid,Path)),responses((status=200,body=Comment)))]
pub(crate) async fn comment(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let value = s.app.get_comment(&a, w, id).await?;
    let tag = value.etag.clone();
    response(value, &tag, StatusCode::OK)
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues/{issue}/comments",params(("w"=Uuid,Path),("issue"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=DocumentWrite,responses((status=201,body=Comment)))]
pub(crate) async fn create_comment(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<DocumentWrite>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))?;
    let value = s.app.create_comment(&a, w, id, key, payload(body)?).await?;
    let tag = value.etag.clone();
    let location = format!("/api/v1/workspaces/{w}/comments/{}", value.id);
    let mut response = response(value, &tag, StatusCode::CREATED)?;
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/comments/{comment}",params(("w"=Uuid,Path),("comment"=Uuid,Path),("If-Match"=String,Header)),request_body=DocumentWrite,responses((status=200,body=Comment)))]
pub(crate) async fn update_comment(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<DocumentWrite>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let value = s
        .app
        .update_comment(&a, w, id, expected(&headers)?, payload(body)?)
        .await?;
    let tag = value.etag.clone();
    response(value, &tag, StatusCode::OK)
}
#[utoipa::path(delete,path="/api/v1/workspaces/{w}/comments/{comment}",params(("w"=Uuid,Path),("comment"=Uuid,Path),("If-Match"=String,Header)),request_body(content=Option<CommentDelete>),responses((status=204)))]
pub(crate) async fn delete_comment(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Option<Json<CommentDelete>>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    csrf(&s, &headers, &a)?;
    let body = match body {
        Ok(Some(value)) => value.0,
        Ok(None) => CommentDelete::default(),
        Err(error) => payload::<CommentDelete>(Err(error))?,
    };
    s.app
        .delete_comment(&a, w, id, expected(&headers)?, body)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{issue}/activity-page",params(("w"=Uuid,Path),("issue"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<crate::contract::ActivityEntry>)))]
pub(crate) async fn activity_page(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<DiscussionQuery>,
) -> Result<Json<Page<serde_json::Value>>, ApiError> {
    Ok(Json(s.app.issue_activity_page(&a, w, id, q).await?))
}
