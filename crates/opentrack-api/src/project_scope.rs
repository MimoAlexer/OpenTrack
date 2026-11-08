use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use opentrack_domain::{
    AgreeProjectScope, ChangeProjectScope, Error, ProjectScopeHistory, ProjectScopeQuery,
};
use uuid::Uuid;
pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/scope-history",
            get(history),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/scope-baselines",
            post(agree),
        )
        .route(
            "/workspaces/{w}/teams/{t}/projects/{id}/scope-changes",
            post(change),
        )
}
fn response(item: ProjectScopeHistory, status: StatusCode) -> Result<Response, ApiError> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&item).map_err(|_| Error::Internal)?;
    // Pages have independent validators; scope_etag in the body is the opaque
    // write precondition, not a validator for every selected/paged representation.
    if bytes.len() > 900 * 1024 {
        return Err(Error::Internal.into());
    }
    let etag = HeaderValue::from_str(&format!("\"scope-page:{:x}\"", Sha256::digest(bytes)))
        .map_err(|_| Error::Internal)?;
    let mut out = (status, Json(item)).into_response();
    out.headers_mut().insert(header::ETAG, etag);
    Ok(out)
}
fn headers(h: &HeaderMap) -> Result<(&str, &str), Error> {
    Ok((
        h.get(header::IF_MATCH)
            .and_then(|v| v.to_str().ok())
            .ok_or(Error::PreconditionRequired)?,
        h.get("idempotency-key")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))?,
    ))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/scope-history",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("baseline_id"=Option<Uuid>,Query),("cursor"=Option<String>,Query),("items_cursor"=Option<String>,Query),("baseline_cursor"=Option<String>,Query),("limit"=Option<u32>,Query,minimum=1,maximum=25)),responses((status=200,body=ProjectScopeHistory)))]
pub(crate) async fn history(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<ProjectScopeQuery>,
) -> Result<Response, ApiError> {
    response(
        s.app.project_scope_history(&a, w, t, id, q).await?,
        StatusCode::OK,
    )
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/scope-baselines",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=AgreeProjectScope,responses((status=201,body=ProjectScopeHistory)))]
pub(crate) async fn agree(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<AgreeProjectScope>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &h, &a)?;
    let (etag, key) = headers(&h)?;
    response(
        s.app
            .agree_project_scope(&a, w, t, id, etag, key, payload(body)?)
            .await?,
        StatusCode::CREATED,
    )
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/projects/{id}/scope-changes",params(("w"=Uuid,Path),("t"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=ChangeProjectScope,responses((status=200,body=ProjectScopeHistory)))]
pub(crate) async fn change(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, id)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<ChangeProjectScope>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &h, &a)?;
    let (etag, key) = headers(&h)?;
    response(
        s.app
            .change_project_scope(&a, w, t, id, etag, key, payload(body)?)
            .await?,
        StatusCode::OK,
    )
}
