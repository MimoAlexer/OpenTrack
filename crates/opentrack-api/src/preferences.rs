use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use opentrack_domain::{Error, UpdateUserPreferences, UserPreferences};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route(
        "/workspaces/{w}/preferences",
        get(get_preferences).patch(update_preferences),
    )
}
fn response(item: UserPreferences) -> Result<Response, ApiError> {
    let tag = HeaderValue::from_str(&item.etag).map_err(|_| Error::Internal)?;
    let mut result = Json(item).into_response();
    result.headers_mut().insert(header::ETAG, tag);
    Ok(result)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/preferences",params(("w"=Uuid,Path)),responses((status=200,body=UserPreferences)))]
pub(crate) async fn get_preferences(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
) -> Result<Response, ApiError> {
    response(s.app.user_preferences(&a, w).await?)
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/preferences",params(("w"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateUserPreferences,responses((status=200,body=UserPreferences)))]
pub(crate) async fn update_preferences(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<UpdateUserPreferences>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let etag = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::PreconditionRequired)?;
    response(
        s.app
            .update_user_preferences(&a, w, etag, payload(body)?)
            .await?,
    )
}
