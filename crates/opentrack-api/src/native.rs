//! Native session handoff. Session credentials only leave through code exchange.
use crate::{
    ApiError, ApiState,
    credentials::{self, Credential},
    csrf, payload,
};
use axum::{
    Json, Router,
    extract::{Path, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
    routing::{get, post},
};
use opentrack_domain::{
    ClientCapabilities, Error, NativeAuthApproval, NativeAuthApprove, NativeAuthConsent,
    NativeAuthExchange, NativeAuthStart, NativeAuthStartReceipt, NativeSessionGrant,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/client-capabilities", get(capabilities))
        .route("/auth/native/requests", post(start))
        .route("/auth/native/requests/{id}", get(consent))
        .route("/auth/native/requests/{id}/approve", post(approve))
        .route("/auth/native/exchange", post(exchange))
}

// Applied outside the shared response-header layers, including extractor errors.
pub(crate) async fn privacy_headers(request: Request, next: Next) -> Response {
    let private = request.uri().path().starts_with("/api/v1/auth/native/")
        || request.uri().path() == "/auth/native/approve";
    let mut response = next.run(request).await;
    if private {
        response.headers_mut().insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

#[utoipa::path(get,path="/api/v1/client-capabilities",security(()),responses((status=200,body=ClientCapabilities)))]
pub(crate) async fn capabilities() -> Json<ClientCapabilities> {
    Json(ClientCapabilities {
        api_version: "v1".into(),
        native_auth_version: 1,
        desktop_min_protocol: 1,
    })
}

#[utoipa::path(post,path="/api/v1/auth/native/requests",security(()),request_body=NativeAuthStart,responses((status=201,body=NativeAuthStartReceipt)))]
pub(crate) async fn start(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Result<Json<NativeAuthStart>, JsonRejection>,
) -> Result<(StatusCode, Json<NativeAuthStartReceipt>), ApiError> {
    let previous = match credentials::extract(&headers)? {
        Credential::None => None,
        Credential::Desktop(token) => Some(state.app.authenticate_native(token).await?),
        Credential::Web(_) => return Err(Error::Unauthorized.into()),
    };
    let result = state
        .app
        .start_native_auth(previous.as_ref(), payload(body)?)
        .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

#[utoipa::path(get,path="/api/v1/auth/native/requests/{id}",params(("id"=Uuid,Path)),responses((status=200,body=NativeAuthConsent)))]
pub(crate) async fn consent(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<NativeAuthConsent>, ApiError> {
    let token = credentials::browser_cookie(&headers)?.ok_or(Error::Unauthorized)?;
    let actor = state.app.authenticate(token).await?;
    Ok(Json(state.app.native_auth_consent(&actor, id).await?))
}

#[utoipa::path(post,path="/api/v1/auth/native/requests/{id}/approve",params(("id"=Uuid,Path)),request_body=NativeAuthApprove,responses((status=200,body=NativeAuthApproval)))]
pub(crate) async fn approve(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<NativeAuthApprove>, JsonRejection>,
) -> Result<Json<NativeAuthApproval>, ApiError> {
    let token = credentials::browser_cookie(&headers)?.ok_or(Error::Unauthorized)?;
    let actor = state.app.authenticate(token).await?;
    csrf(&state, &headers, &actor)?;
    Ok(Json(
        state
            .app
            .approve_native_auth(&actor, id, payload(body)?)
            .await?,
    ))
}

#[utoipa::path(post,path="/api/v1/auth/native/exchange",security(()),request_body=NativeAuthExchange,responses((status=200,body=NativeSessionGrant)))]
pub(crate) async fn exchange(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Result<Json<NativeAuthExchange>, JsonRejection>,
) -> Result<Json<NativeSessionGrant>, ApiError> {
    if !matches!(credentials::extract(&headers)?, Credential::None) {
        return Err(Error::Unauthorized.into());
    }
    Ok(Json(state.app.exchange_native_auth(payload(body)?).await?))
}
