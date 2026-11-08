use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use opentrack_domain::{
    Error, MarkNotificationsRead, NotificationFilter, NotificationItem, NotificationPage,
    NotificationPreferences, NotificationQuery, NotificationsMarkedRead, UpdateNotification,
    UpdateNotificationPreferences,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/notifications", get(list))
        .route("/workspaces/{w}/notifications/mark-read", post(mark_read))
        .route("/workspaces/{w}/notifications/{id}", patch(update))
        .route(
            "/workspaces/{w}/notification-preferences",
            get(get_preferences).patch(update_preferences),
        )
}

fn versioned<T: serde::Serialize>(item: T, etag: &str) -> Result<Response, ApiError> {
    let value = HeaderValue::from_str(etag).map_err(|_| Error::Internal)?;
    let mut response = Json(item).into_response();
    response.headers_mut().insert(header::ETAG, value);
    Ok(response)
}
fn etag(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| Error::PreconditionRequired.into())
}

#[utoipa::path(get,operation_id="notifications_list",path="/api/v1/workspaces/{w}/notifications",params(("w"=Uuid,Path),("filter"=Option<NotificationFilter>,Query,description="Defaults to unread. All excludes archived and future snoozed items."),("limit"=Option<u32>,Query,minimum=1,maximum=50),("cursor"=Option<String>,Query)),responses((status=200,body=NotificationPage)))]
pub(crate) async fn list(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(query): Query<NotificationQuery>,
) -> Result<Json<NotificationPage>, ApiError> {
    Ok(Json(s.app.notifications(&a, w, query).await?))
}

#[utoipa::path(patch,operation_id="notifications_update",path="/api/v1/workspaces/{w}/notifications/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateNotification,responses((status=200,body=NotificationItem)))]
pub(crate) async fn update(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateNotification>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .update_notification(&a, w, id, etag(&headers)?, payload(body)?)
        .await?;
    let tag = item.etag.clone();
    versioned(item, &tag)
}

#[utoipa::path(post,operation_id="notifications_mark_read",path="/api/v1/workspaces/{w}/notifications/mark-read",params(("w"=Uuid,Path)),request_body=MarkNotificationsRead,responses((status=200,body=NotificationsMarkedRead,description="Marks at most 100 delivered items through the signed snapshot. Repeat the same snapshot while has_more is true.")))]
pub(crate) async fn mark_read(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<MarkNotificationsRead>, JsonRejection>,
) -> Result<Json<NotificationsMarkedRead>, ApiError> {
    csrf(&s, &headers, &a)?;
    Ok(Json(
        s.app.mark_notifications_read(&a, w, payload(body)?).await?,
    ))
}

#[utoipa::path(get,operation_id="notification_preferences_get",path="/api/v1/workspaces/{w}/notification-preferences",params(("w"=Uuid,Path)),responses((status=200,body=NotificationPreferences)))]
pub(crate) async fn get_preferences(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
) -> Result<Response, ApiError> {
    let item = s.app.notification_preferences(&a, w).await?;
    let tag = item.etag.clone();
    versioned(item, &tag)
}

#[utoipa::path(patch,operation_id="notification_preferences_update",path="/api/v1/workspaces/{w}/notification-preferences",params(("w"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateNotificationPreferences,responses((status=200,body=NotificationPreferences)))]
pub(crate) async fn update_preferences(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<UpdateNotificationPreferences>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .update_notification_preferences(&a, w, etag(&headers)?, payload(body)?)
        .await?;
    let tag = item.etag.clone();
    versioned(item, &tag)
}
