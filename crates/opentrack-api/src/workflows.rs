use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use opentrack_domain::{
    CreateStatus, DirectoryQuery, Error, Page, ReorderStatuses, ReplaceStatus, SetDefaultStatus,
    Status, UpdateStatus, Workflow, WorkflowReplacement, WorkflowReplacementPreview,
};
use serde::Deserialize;
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/teams/{t}/workflow", get(workflow))
        .route(
            "/workspaces/{w}/teams/{t}/workflow/default",
            put(set_default),
        )
        .route("/workspaces/{w}/teams/{t}/workflow/order", put(reorder))
        .route(
            "/workspaces/{w}/teams/{t}/statuses",
            get(statuses).post(create_status),
        )
        .route(
            "/workspaces/{w}/teams/{t}/statuses/{status}",
            get(status).patch(update_status),
        )
        .route(
            "/workspaces/{w}/teams/{t}/statuses/{status}/archive",
            post(archive_status),
        )
        .route(
            "/workspaces/{w}/teams/{t}/statuses/{status}/restore",
            post(restore_status),
        )
        .route(
            "/workspaces/{w}/teams/{t}/statuses/{status}/replacement-preview",
            get(replacement_preview),
        )
        .route(
            "/workspaces/{w}/teams/{t}/statuses/{status}/replace",
            post(replace_status),
        )
        .route(
            "/workspaces/{w}/teams/{t}/workflow-replacements/{job}",
            get(replacement),
        )
        .route(
            "/workspaces/{w}/teams/{t}/workflow-replacements/{job}/resume",
            post(resume_replacement),
        )
}

fn expected(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::PreconditionRequired.into())
}
fn key(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()).into())
}
fn job_tag(job: &WorkflowReplacement) -> String {
    format!("\"workflow-replacement:{}:{}\"", job.id, job.version)
}
fn versioned<T: serde::Serialize>(
    data: T,
    status: StatusCode,
    tag: String,
    location: Option<String>,
) -> Result<Response, ApiError> {
    let mut response = (status, Json(data)).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&tag).map_err(|_| Error::Internal)?,
    );
    if let Some(location) = location {
        response.headers_mut().insert(
            header::LOCATION,
            HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
        );
    }
    Ok(response)
}
fn workflow_response(item: Workflow) -> Result<Response, ApiError> {
    let tag = item.etag();
    versioned(item, StatusCode::OK, tag, None)
}
// A conditional read is evaluated only after the application has checked current
// access and loaded the aggregate. A remembered ETag never grants access.
fn not_modified(headers: &HeaderMap, tag: &str) -> Result<Option<Response>, ApiError> {
    let matches = headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|value| value == "*" || value.strip_prefix("W/").unwrap_or(value) == tag);
    if !matches {
        return Ok(None);
    }
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(tag).map_err(|_| Error::Internal)?,
    );
    Ok(Some(response))
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/workflow",params(("w"=Uuid,Path),("t"=Uuid,Path),("If-None-Match"=Option<String>,Header)),responses((status=200,body=Workflow),(status=304,description="The authorized workflow has not changed")))]
pub(crate) async fn workflow(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let item = s.app.workflow(&a, w, t).await?;
    let tag = item.etag();
    if let Some(response) = not_modified(&headers, &tag)? {
        return Ok(response);
    }
    versioned(item, StatusCode::OK, tag, None)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/statuses",params(("w"=Uuid,Path),("t"=Uuid,Path),("state"=Option<String>,Query),("limit"=Option<u32>,Query),("cursor"=Option<String>,Query)),responses((status=200,body=Page<Status>)))]
pub(crate) async fn statuses(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    Query(q): Query<DirectoryQuery>,
) -> Result<Json<Page<Status>>, ApiError> {
    Ok(Json(s.app.list_statuses(&a, w, t, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/statuses/{status}",params(("w"=Uuid,Path),("t"=Uuid,Path),("status"=Uuid,Path)),responses((status=200,body=Status)))]
pub(crate) async fn status(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Status>, ApiError> {
    Ok(Json(s.app.get_status(&a, w, t, id).await?))
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/statuses",params(("w"=Uuid,Path),("t"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=CreateStatus,responses((status=201,body=Workflow)))]
pub(crate) async fn create_status(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CreateStatus>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let (id, item) = s
        .app
        .create_status(
            &a,
            w,
            t,
            key(&headers)?,
            expected(&headers)?,
            payload(body)?,
        )
        .await?;
    let tag = item.etag();
    versioned(
        item,
        StatusCode::CREATED,
        tag,
        Some(format!("/api/v1/workspaces/{w}/teams/{t}/statuses/{id}")),
    )
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/teams/{t}/statuses/{status}",params(("w"=Uuid,Path),("t"=Uuid,Path),("status"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateStatus,responses((status=200,body=Workflow)))]
pub(crate) async fn update_status(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateStatus>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    workflow_response(
        s.app
            .update_status(&a, w, t, id, expected(&headers)?, payload(body)?)
            .await?,
    )
}
#[utoipa::path(put,path="/api/v1/workspaces/{w}/teams/{t}/workflow/default",params(("w"=Uuid,Path),("t"=Uuid,Path),("If-Match"=String,Header)),request_body=SetDefaultStatus,responses((status=200,body=Workflow)))]
pub(crate) async fn set_default(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<SetDefaultStatus>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    workflow_response(
        s.app
            .set_default_status(&a, w, t, expected(&headers)?, payload(body)?)
            .await?,
    )
}
#[utoipa::path(put,path="/api/v1/workspaces/{w}/teams/{t}/workflow/order",params(("w"=Uuid,Path),("t"=Uuid,Path),("If-Match"=String,Header)),request_body=ReorderStatuses,responses((status=200,body=Workflow)))]
pub(crate) async fn reorder(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<ReorderStatuses>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    workflow_response(
        s.app
            .reorder_statuses(&a, w, t, expected(&headers)?, payload(body)?)
            .await?,
    )
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/statuses/{status}/archive",params(("w"=Uuid,Path),("t"=Uuid,Path),("status"=Uuid,Path),("If-Match"=String,Header)),responses((status=200,body=Workflow)))]
pub(crate) async fn archive_status(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    workflow_response(
        s.app
            .archive_status(&a, w, t, id, expected(&headers)?)
            .await?,
    )
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/statuses/{status}/restore",params(("w"=Uuid,Path),("t"=Uuid,Path),("status"=Uuid,Path),("If-Match"=String,Header)),responses((status=200,body=Workflow)))]
pub(crate) async fn restore_status(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    workflow_response(
        s.app
            .restore_status(&a, w, t, id, expected(&headers)?)
            .await?,
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewQuery {
    replacement_status_id: Uuid,
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/statuses/{status}/replacement-preview",params(("w"=Uuid,Path),("t"=Uuid,Path),("status"=Uuid,Path),("replacement_status_id"=Uuid,Query)),responses((status=200,body=WorkflowReplacementPreview)))]
pub(crate) async fn replacement_preview(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<PreviewQuery>,
) -> Result<Json<WorkflowReplacementPreview>, ApiError> {
    let item = s
        .app
        .preview_status_replacement(&a, w, t, id, q.replacement_status_id)
        .await?;
    Ok(Json(item))
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/statuses/{status}/replace",params(("w"=Uuid,Path),("t"=Uuid,Path),("status"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=ReplaceStatus,responses((status=202,body=WorkflowReplacement)))]
pub(crate) async fn replace_status(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<ReplaceStatus>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .replace_status(
            &a,
            w,
            t,
            id,
            key(&headers)?,
            expected(&headers)?,
            payload(body)?,
        )
        .await?;
    let tag = job_tag(&item);
    let location = format!(
        "/api/v1/workspaces/{w}/teams/{t}/workflow-replacements/{}",
        item.id
    );
    versioned(item, StatusCode::ACCEPTED, tag, Some(location))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/workflow-replacements/{job}",params(("w"=Uuid,Path),("t"=Uuid,Path),("job"=Uuid,Path),("If-None-Match"=Option<String>,Header)),responses((status=200,body=WorkflowReplacement),(status=304,description="The authorized replacement has not changed")))]
pub(crate) async fn replacement(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, job)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let item = s.app.get_workflow_replacement(&a, w, t, job).await?;
    let tag = job_tag(&item);
    if let Some(response) = not_modified(&headers, &tag)? {
        return Ok(response);
    }
    versioned(item, StatusCode::OK, tag, None)
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/workflow-replacements/{job}/resume",params(("w"=Uuid,Path),("t"=Uuid,Path),("job"=Uuid,Path),("If-Match"=String,Header)),responses((status=200,body=WorkflowReplacement)))]
pub(crate) async fn resume_replacement(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, job)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .resume_workflow_replacement(&a, w, t, job, expected(&headers)?)
        .await?;
    let tag = job_tag(&item);
    versioned(item, StatusCode::OK, tag, None)
}
