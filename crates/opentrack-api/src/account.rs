//! Account-scoped operations remain available without any workspace membership.
use super::{ApiError, ApiState, Auth, clear_cookie_response, csrf, origin, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use opentrack_domain::{Page, RecoveryComplete, SessionInfo, SessionQuery};
use uuid::Uuid;

pub(super) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/auth/recovery/complete", post(complete_recovery))
        .route("/auth/sessions", get(sessions))
        .route("/auth/sessions/{id}/revoke", post(revoke_session))
        .route("/auth/sessions/revoke-others", post(revoke_others))
}

#[utoipa::path(post,path="/api/v1/auth/recovery/complete",security(()),
    request_body=RecoveryComplete,
    responses((status=204,description="Password changed, recovery code consumed and all account sessions revoked; browser cookie cleared. Sign in again.")))]
pub(super) async fn complete_recovery(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Result<Json<RecoveryComplete>, JsonRejection>,
) -> Result<Response, ApiError> {
    origin(&state, &headers)?;
    super::credentials::browser_cookie(&headers)?;
    state.app.complete_recovery(payload(body)?).await?;
    Ok(clear_cookie_response(&state))
}

#[utoipa::path(get,path="/api/v1/auth/sessions",
    params(("limit"=Option<u32>,Query,description="Page size, from 1 through 100; default 50"),
           ("cursor"=Option<String>,Query,description="Opaque cursor bound to the current account and session")),
    responses((status=200,body=Page<SessionInfo>)))]
pub(super) async fn sessions(
    State(state): State<ApiState>,
    Auth(actor): Auth,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Page<SessionInfo>>, ApiError> {
    Ok(Json(state.app.sessions(&actor, query).await?))
}

#[utoipa::path(post,path="/api/v1/auth/sessions/{id}/revoke",
    params(("id"=Uuid,Path)),
    responses((status=204,description="Own session revoked; revoking the current session signals invalidation and clears its browser cookie when applicable. Other sessions require recent authentication.")))]
pub(super) async fn revoke_session(
    State(state): State<ApiState>,
    Auth(actor): Auth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&state, &headers, &actor)?;
    if state.app.revoke_session(&actor, id).await? {
        let mut response = if actor.audience == opentrack_domain::SessionAudience::Web {
            clear_cookie_response(&state)
        } else {
            StatusCode::NO_CONTENT.into_response()
        };
        response.headers_mut().insert(
            "x-opentrack-session-invalidated",
            HeaderValue::from_static("true"),
        );
        Ok(response)
    } else {
        Ok(StatusCode::NO_CONTENT.into_response())
    }
}

#[utoipa::path(post,path="/api/v1/auth/sessions/revoke-others",
    responses((status=204,description="All other sessions revoked; current session retained. Requires recent authentication.")))]
pub(super) async fn revoke_others(
    State(state): State<ApiState>,
    Auth(actor): Auth,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    csrf(&state, &headers, &actor)?;
    state.app.revoke_other_sessions(&actor).await?;
    Ok(StatusCode::NO_CONTENT)
}
