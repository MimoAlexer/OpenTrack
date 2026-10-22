use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use opentrack_domain::{
    CreateIssueTemplate, Error, IssueTemplate, IssueTemplateSummary, Page, TemplatePreview,
    TemplateQuery, UpdateIssueTemplate,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/workspaces/{w}/teams/{t}/templates",
            get(list).post(create),
        )
        .route(
            "/workspaces/{w}/teams/{t}/templates/{id}",
            get(detail).patch(update),
        )
        .route(
            "/workspaces/{w}/teams/{t}/templates/{id}/preview",
            get(preview),
        )
}
fn response(item: IssueTemplate, status: StatusCode) -> Result<Response, ApiError> {
    let etag = HeaderValue::from_str(&item.summary.etag).map_err(|_| Error::Internal)?;
    let mut result = (status, Json(item)).into_response();
    result.headers_mut().insert(header::ETAG, etag);
    Ok(result)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/templates",params(("w"=Uuid,Path),("t"=Uuid,Path),("archived"=Option<bool>,Query),("q"=Option<String>,Query),("limit"=Option<u32>,Query,minimum=1,maximum=100),("cursor"=Option<String>,Query)),responses((status=200,body=Page<IssueTemplateSummary>)))]
pub(crate) async fn list(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    Query(q): Query<TemplateQuery>,
) -> Result<Json<Page<IssueTemplateSummary>>, ApiError> {
    Ok(Json(s.app.list_templates(&a, w, t, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/templates/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=IssueTemplate)))]
pub(crate) async fn detail(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    response(s.app.get_template(&a, w, t, id).await?, StatusCode::OK)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/templates/{id}/preview",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=TemplatePreview)))]
pub(crate) async fn preview(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<TemplatePreview>, ApiError> {
    // Reference states are not versioned by the template. Never advertise the
    // template's validator as a strong validator for this compound projection.
    Ok(Json(s.app.preview_template(&a, w, t, id).await?))
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/templates",params(("w"=Uuid,Path),("t"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateIssueTemplate,responses((status=201,body=IssueTemplate)))]
pub(crate) async fn create(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CreateIssueTemplate>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))?;
    let item = s.app.create_template(&a, w, t, key, payload(body)?).await?;
    let location = format!(
        "/api/v1/workspaces/{w}/teams/{t}/templates/{}",
        item.summary.id
    );
    let mut result = response(item, StatusCode::CREATED)?;
    result.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(result)
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/teams/{t}/templates/{id}",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateIssueTemplate,responses((status=200,body=IssueTemplate)))]
pub(crate) async fn update(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateIssueTemplate>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let etag = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::PreconditionRequired)?;
    response(
        s.app
            .update_template(&a, w, t, id, etag, payload(body)?)
            .await?,
        StatusCode::OK,
    )
}
