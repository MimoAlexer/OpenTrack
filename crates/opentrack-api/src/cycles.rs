use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use opentrack_domain::*;
use std::result::Result;
use uuid::Uuid;
pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/teams/{t}/cycles/{id}/issues", get(issues))
        .route("/workspaces/{w}/teams/{t}/cycles", get(list).post(create))
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}",
            get(detail).patch(update).delete(delete),
        )
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}/issues/{issue}",
            put(membership),
        )
        .route("/workspaces/{w}/teams/{t}/cycles/{id}/scope", get(scope))
        .route("/workspaces/{w}/teams/{t}/cycles/{id}/open", post(open))
        .route("/workspaces/{w}/teams/{t}/cycles/{id}/reopen", post(reopen))
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}/close-preview",
            get(preview),
        )
        .route("/workspaces/{w}/teams/{t}/cycles/{id}/close", post(close))
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}/reports/{report}",
            get(report),
        )
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}/snapshots",
            get(snapshots).post(capture),
        )
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}/carry-forward",
            post(carry),
        )
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}/carry-forward/{operation}",
            get(carry_detail),
        )
        .route(
            "/workspaces/{w}/teams/{t}/cycles/{id}/carry-forward/{operation}/resume",
            post(resume),
        )
}
fn key(h: &HeaderMap) -> Result<&str, Error> {
    h.get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))
}
fn etag(h: &HeaderMap) -> Result<&str, Error> {
    h.get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::PreconditionRequired)
}
fn response<T: serde::Serialize>(
    item: T,
    validator: &str,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let mut out = (status, Json(item)).into_response();
    out.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(validator).map_err(|_| Error::Internal)?,
    );
    Ok(out)
}
#[utoipa::path(operation_id="cycles_list",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles",params(("w"=Uuid,Path),("t"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<Cycle>)))]
pub(crate) async fn list(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    Query(q): Query<CycleQuery>,
) -> Result<Response, ApiError> {
    let item = s.app.list_cycles(&a, w, t, q).await?;
    Ok(Json(item).into_response())
}
#[utoipa::path(operation_id="cycles_create",post,path="/api/v1/workspaces/{w}/teams/{t}/cycles",params(("w"=Uuid,Path),("t"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateCycle,responses((status=201,body=Cycle)))]
pub(crate) async fn create(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CreateCycle>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .create_cycle(&a, w, t, key(&headers)?, payload(body)?)
        .await?;
    let validator = item.etag.clone();
    let location = format!("/api/v1/workspaces/{w}/teams/{t}/cycles/{}", item.id);
    let mut out = response(item, &validator, StatusCode::CREATED)?;
    out.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(out)
}
#[utoipa::path(operation_id="cycles_detail",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Cycle)))]
pub(crate) async fn detail(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.get_cycle(&a, w, t, id).await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(operation_id="cycles_update",patch,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateCycle,responses((status=200,body=Cycle)))]
pub(crate) async fn update(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateCycle>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .update_cycle(&a, w, t, id, etag(&headers)?, payload(body)?)
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(operation_id="cycles_membership",put,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/issues/{issue}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("issue"=Uuid,Path),("If-Match"=String,Header)),request_body=CycleMembership,responses((status=200,body=Cycle)))]
pub(crate) async fn membership(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id, issue)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CycleMembership>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .set_cycle_issue(&a, w, t, id, issue, etag(&headers)?, payload(body)?)
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(operation_id="cycles_scope",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/scope",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=CycleSnapshot)))]
pub(crate) async fn scope(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.cycle_scope(&a, w, t, id).await?;
    Ok(Json(item).into_response())
}
#[utoipa::path(operation_id="cycles_open",post,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/open",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),responses((status=200,body=Cycle)))]
pub(crate) async fn open(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s.app.open_cycle(&a, w, t, id, etag(&headers)?).await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(operation_id="cycles_reopen",post,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/reopen",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=ReopenCycle,responses((status=200,body=Cycle)))]
pub(crate) async fn reopen(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<ReopenCycle>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .reopen_cycle(&a, w, t, id, etag(&headers)?, payload(body)?)
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(operation_id="cycles_preview",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/close-preview",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=CycleClosePreview)))]
pub(crate) async fn preview(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.cycle_close_preview(&a, w, t, id).await?;
    Ok(Json(item).into_response())
}
#[utoipa::path(operation_id="cycles_close",post,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/close",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=CloseCycle,responses((status=200,body=CycleCloseReport)))]
pub(crate) async fn close(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CloseCycle>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .close_cycle(
            &a,
            w,
            t,
            id,
            etag(&headers)?,
            key(&headers)?,
            payload(body)?,
        )
        .await?;
    Ok(Json(item).into_response())
}
#[utoipa::path(operation_id="cycles_report",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/reports/{report}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("report"=Uuid,Path)),responses((status=200,body=CycleCloseReport)))]
pub(crate) async fn report(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id, report)): Path<(Uuid, Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.cycle_report(&a, w, t, id, report).await?;
    Ok(Json(item).into_response())
}
#[utoipa::path(operation_id="cycles_snapshots",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/snapshots",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<CycleSnapshotPoint>)))]
pub(crate) async fn snapshots(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<CycleQuery>,
) -> Result<Response, ApiError> {
    let item = s.app.cycle_snapshots(&a, w, t, id, q).await?;
    Ok(Json(item).into_response())
}
#[utoipa::path(operation_id="cycles_capture",post,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/snapshots",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),responses((status=200,body=CycleSnapshot)))]
pub(crate) async fn capture(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .capture_cycle_snapshot(&a, w, t, id, etag(&headers)?)
        .await?;
    Ok(Json(item).into_response())
}
#[utoipa::path(operation_id="cycles_carry",post,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/carry-forward",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=CarryForwardCycle,responses((status=200,body=CycleCarryOperation)))]
pub(crate) async fn carry(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CarryForwardCycle>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .carry_forward_cycle(
            &a,
            w,
            t,
            id,
            etag(&headers)?,
            key(&headers)?,
            payload(body)?,
        )
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(operation_id="cycles_carry_detail",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/carry-forward/{operation}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("operation"=Uuid,Path)),responses((status=200,body=CycleCarryOperation)))]
pub(crate) async fn carry_detail(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id, operation)): Path<(Uuid, Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.cycle_carry_operation(&a, w, t, id, operation).await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(operation_id="cycles_resume",post,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/carry-forward/{operation}/resume",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("operation"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=ResumeCycleCarry,responses((status=200,body=CycleCarryOperation)))]
pub(crate) async fn resume(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id, operation)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<ResumeCycleCarry>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .resume_cycle_carry(
            &a,
            w,
            t,
            id,
            operation,
            etag(&headers)?,
            key(&headers)?,
            payload(body)?,
        )
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}

#[utoipa::path(operation_id="cycles_delete",delete,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),responses((status=204)))]
pub(crate) async fn delete(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    csrf(&s, &headers, &a)?;
    s.app.delete_cycle(&a, w, t, id, etag(&headers)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(operation_id="cycles_issues",get,path="/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/issues",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<CycleIssue>)))]
pub(crate) async fn issues(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<CycleQuery>,
) -> Result<Response, ApiError> {
    Ok(Json(s.app.cycle_issues(&a, w, t, id, q).await?).into_response())
}
