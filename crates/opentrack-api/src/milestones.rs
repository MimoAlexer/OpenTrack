use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use opentrack_domain::{
    CreateMilestone, Error, Milestone, MilestoneAssignment, MilestoneOrder, MilestonePage, Project,
    ProjectPageQuery, ProjectProgress, UpdateMilestone,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/workspaces/{w}/teams/{t}/projects/{p}/milestones",
            get(list).post(create),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{p}/milestones/order",
            put(order),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{p}/milestones/{id}",
            get(detail).patch(update).delete(delete),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{p}/milestones/{id}/progress",
            get(progress),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{p}/issues/{issue}/milestone",
            put(assign),
        )
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
#[utoipa::path(get,operation_id="list_milestones",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=MilestonePage)))]
pub(crate) async fn list(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<ProjectPageQuery>,
) -> Result<Json<MilestonePage>, ApiError> {
    Ok(Json(s.app.list_milestones(&a, w, t, p, q).await?))
}
#[utoipa::path(post,operation_id="create_milestone",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("If-Match"=String,Header,description="Current project ETag"),("Idempotency-Key"=String,Header)),request_body=CreateMilestone,responses((status=201,body=Milestone)))]
pub(crate) async fn create(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<CreateMilestone>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &h, &a)?;
    let key = h
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))?;
    let item = s
        .app
        .create_milestone(&a, w, t, p, key, etag(&h)?, payload(body)?)
        .await?;
    let location = format!(
        "/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones/{}",
        item.id
    );
    let validator = item.etag.clone();
    let mut out = response(item, &validator, StatusCode::CREATED)?;
    out.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(out)
}
#[utoipa::path(get,operation_id="get_milestone",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Milestone)))]
pub(crate) async fn detail(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p, id)): Path<(Uuid, Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.get_milestone(&a, w, t, p, id).await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(patch,operation_id="update_milestone",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header,description="Current milestone ETag")),request_body=UpdateMilestone,responses((status=200,body=Milestone)))]
pub(crate) async fn update(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p, id)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<UpdateMilestone>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &h, &a)?;
    let item = s
        .app
        .update_milestone(&a, w, t, p, id, etag(&h)?, payload(body)?)
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(delete,operation_id="delete_milestone",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header,description="Current milestone ETag")),responses((status=204,description="Milestone removed; issues and project membership retained")))]
pub(crate) async fn delete(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p, id)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    csrf(&s, &h, &a)?;
    s.app.delete_milestone(&a, w, t, p, id, etag(&h)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(get,operation_id="milestone_progress",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones/{id}/progress",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=ProjectProgress)))]
pub(crate) async fn progress(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p, id)): Path<(Uuid, Uuid, Uuid, Uuid)>,
) -> Result<Json<ProjectProgress>, ApiError> {
    Ok(Json(s.app.milestone_progress(&a, w, t, p, id).await?))
}
#[utoipa::path(put,operation_id="order_milestones",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones/order",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("If-Match"=String,Header,description="Current project ETag")),request_body=MilestoneOrder,responses((status=200,body=Project)))]
pub(crate) async fn order(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<MilestoneOrder>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &h, &a)?;
    let item = s
        .app
        .order_milestones(&a, w, t, p, etag(&h)?, payload(body)?)
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
#[utoipa::path(put,operation_id="assign_milestone",path="/api/v1/workspaces/{w}/teams/{t}/projects/{p}/issues/{issue}/milestone",params(("w"=Uuid,Path),("t"=Uuid,Path),("p"=Uuid,Path),("issue"=Uuid,Path),("If-Match"=String,Header,description="Current project ETag")),request_body=MilestoneAssignment,responses((status=200,body=Project)))]
pub(crate) async fn assign(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, p, issue)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<MilestoneAssignment>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &h, &a)?;
    let item = s
        .app
        .assign_milestone(&a, w, t, p, issue, etag(&h)?, payload(body)?)
        .await?;
    let validator = item.etag.clone();
    response(item, &validator, StatusCode::OK)
}
