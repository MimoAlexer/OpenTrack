use crate::{ApiError, ApiState, Auth, csrf, issue_response, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use opentrack_domain::{
    DeletedIssue, DeletedIssueQuery, DeletionPreview, Error, Issue, Page, RestoreIssue,
};
use uuid::Uuid;
pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/issues/deleted", get(list_deleted))
        .route("/workspaces/{w}/issues/{id}/deletion", get(preview))
        .route("/workspaces/{w}/issues/{id}/deleted", get(deleted))
        .route("/workspaces/{w}/issues/{id}/restore", post(restore))
}
fn expected(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::PreconditionRequired.into())
}
fn response<T: serde::Serialize>(data: T, etag: &str) -> Result<Response, ApiError> {
    let mut response = Json(data).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(etag).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/deletion",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=DeletionPreview,description="Authorized deletion permission and all direct graph effects; Issue ETag is the delete precondition")))]
pub(crate) async fn preview(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let data = s.app.issue_deletion_preview(&a, w, id).await?;
    let tag = data.issue.etag();
    response(data, &tag)
}
#[utoipa::path(delete,path="/api/v1/workspaces/{w}/issues/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),responses((status=204,description="Issue soft-deleted; parent/children detached and all incident relationships removed atomically")))]
pub(crate) async fn delete(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    csrf(&s, &headers, &a)?;
    s.app.delete_issue(&a, w, id, expected(&headers)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/deleted",params(("w"=Uuid,Path),("team_id"=Option<Uuid>,Query),("q"=Option<String>,Query,max_length=256),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<DeletedIssue>,description="Current-team-authorized trash, newest deletion first; expired entries remain visible with can_restore=false")))]
pub(crate) async fn list_deleted(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<DeletedIssueQuery>,
) -> Result<Json<Page<DeletedIssue>>, ApiError> {
    Ok(Json(s.app.list_deleted_issues(&a, w, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/deleted",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=DeletedIssue)))]
pub(crate) async fn deleted(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let data = s.app.get_deleted_issue(&a, w, id).await?;
    let tag = data.issue.etag();
    response(data, &tag)
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues/{id}/restore",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=RestoreIssue,responses((status=200,body=Issue,description="Restored within 30 days using current references; graph connections are never recreated")))]
pub(crate) async fn restore(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<RestoreIssue>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    issue_response(
        s.app
            .restore_issue(&a, w, id, expected(&headers)?, payload(body)?)
            .await?,
        StatusCode::OK,
    )
}
