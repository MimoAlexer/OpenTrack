use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use opentrack_domain::{
    CreateProject, CreateProjectUpdate, Document, Error, Page, Project, ProjectIssue,
    ProjectMembership, ProjectPageQuery, ProjectProgress, ProjectQuery, ProjectSummary,
    ProjectUpdate, UpdateProject,
};
use uuid::Uuid;
pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/updates/{update}",
            get(update_detail),
        )
        .route("/workspaces/{w}/teams/{t}/projects", get(list).post(create))
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}",
            get(detail).patch(update),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/metadata",
            get(metadata).patch(update_metadata),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/document",
            get(document),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/progress",
            get(progress),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/issues",
            get(issues),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/issues/{issue}",
            put(membership),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/updates",
            get(updates).post(publish),
        )
}
fn response(item: Project, status: StatusCode) -> Result<Response, ApiError> {
    let etag = item.etag.clone();
    tagged_response(item, &etag, status)
}
fn tagged_response<T: serde::Serialize>(
    item: T,
    etag: &str,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let etag = HeaderValue::from_str(etag).map_err(|_| Error::Internal)?;
    let mut out = (status, Json(item)).into_response();
    out.headers_mut().insert(header::ETAG, etag);
    Ok(out)
}
fn key(headers: &HeaderMap) -> Result<&str, Error> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))
}
fn etag(headers: &HeaderMap) -> Result<&str, Error> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::PreconditionRequired)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/projects",params(("w"=Uuid,Path),("t"=Uuid,Path),("q"=Option<String>,Query),("archived"=Option<bool>,Query),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<ProjectSummary>)))]
pub(crate) async fn list(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    Query(q): Query<ProjectQuery>,
) -> Result<Json<Page<ProjectSummary>>, ApiError> {
    Ok(Json(s.app.list_projects(&a, w, t, q).await?))
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/projects",params(("w"=Uuid,Path),("t"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateProject,responses((status=201,body=Project)))]
pub(crate) async fn create(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CreateProject>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .create_project(&a, w, t, key(&headers)?, payload(body)?)
        .await?;
    let location = format!("/api/v1/workspaces/{w}/teams/{t}/projects/{}", item.id);
    let mut out = response(item, StatusCode::CREATED)?;
    out.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(out)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Project)))]
pub(crate) async fn detail(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    response(s.app.get_project(&a, w, t, id).await?, StatusCode::OK)
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateProject,responses((status=200,body=Project)))]
pub(crate) async fn update(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateProject>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    response(
        s.app
            .update_project(&a, w, t, id, etag(&headers)?, payload(body)?)
            .await?,
        StatusCode::OK,
    )
}
#[utoipa::path(get,operation_id="get_project_metadata",path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/metadata",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=ProjectSummary)))]
pub(crate) async fn metadata(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.get_project_metadata(&a, w, t, id).await?;
    let etag = item.etag.clone();
    tagged_response(item, &etag, StatusCode::OK)
}
#[utoipa::path(patch,operation_id="update_project_metadata",path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/metadata",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header,description="Current metadata ETag; independent of the project brief")),request_body=UpdateProject,responses((status=200,body=ProjectSummary)))]
pub(crate) async fn update_metadata(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateProject>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .update_project_metadata(&a, w, t, id, etag(&headers)?, payload(body)?)
        .await?;
    let etag = item.etag.clone();
    tagged_response(item, &etag, StatusCode::OK)
}
#[utoipa::path(get,operation_id="project_document",path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/document",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Document)))]
pub(crate) async fn document(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.project_document(&a, w, t, id).await?;
    let etag = item.etag.clone();
    tagged_response(item, &etag, StatusCode::OK)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/progress",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=ProjectProgress)))]
pub(crate) async fn progress(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<ProjectProgress>, ApiError> {
    Ok(Json(s.app.project_progress(&a, w, t, id).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/issues",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<ProjectIssue>)))]
pub(crate) async fn issues(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<ProjectPageQuery>,
) -> Result<Json<Page<ProjectIssue>>, ApiError> {
    Ok(Json(s.app.project_issues(&a, w, t, id, q).await?))
}
#[utoipa::path(put,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/issues/{issue}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("issue"=Uuid,Path),("If-Match"=String,Header)),request_body=ProjectMembership,responses((status=200,body=Project)))]
pub(crate) async fn membership(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id, issue)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<ProjectMembership>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    response(
        s.app
            .set_project_issue(&a, w, t, id, issue, etag(&headers)?, payload(body)?)
            .await?,
        StatusCode::OK,
    )
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/updates",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=25),("cursor"=Option<String>,Query)),responses((status=200,body=Page<ProjectUpdate>)))]
pub(crate) async fn updates(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<ProjectPageQuery>,
) -> Result<Json<Page<ProjectUpdate>>, ApiError> {
    Ok(Json(s.app.project_updates(&a, w, t, id, q).await?))
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/updates",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateProjectUpdate,responses((status=201,body=ProjectUpdate)))]
pub(crate) async fn publish(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CreateProjectUpdate>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .create_project_update(&a, w, t, id, key(&headers)?, payload(body)?)
        .await?;
    let location = format!(
        "/api/v1/workspaces/{w}/teams/{t}/projects/{id}/updates/{}",
        item.id
    );
    let mut out = update_response(item, StatusCode::CREATED)?;
    out.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(out)
}
fn update_response(item: ProjectUpdate, status: StatusCode) -> Result<Response, ApiError> {
    // Updates are immutable; the validator also binds the currently projected
    // author name rather than claiming an immutable user profile.
    let validator = item.etag()?;
    let mut out = (status, Json(item)).into_response();
    out.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&validator).map_err(|_| Error::Internal)?,
    );
    Ok(out)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/updates/{update}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("update"=Uuid,Path)),responses((status=200,body=ProjectUpdate)))]
pub(crate) async fn update_detail(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id, update)): Path<(Uuid, Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    update_response(
        s.app.get_project_update(&a, w, t, id, update).await?,
        StatusCode::OK,
    )
}
