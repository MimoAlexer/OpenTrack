use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use opentrack_domain::{Error, IssueOrderMaintenance, IssuePosition, PositionIssue};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/issues/{id}/position", post(position_issue))
        .route(
            "/workspaces/{w}/teams/{t}/issue-order-maintenance/{job}",
            get(issue_order_maintenance),
        )
        .route(
            "/workspaces/{w}/teams/{t}/issue-order-maintenance/{job}/resume",
            post(resume_issue_order_maintenance),
        )
}

fn response<T: serde::Serialize>(
    item: T,
    tag: String,
    unchanged: bool,
) -> Result<Response, ApiError> {
    let etag = HeaderValue::from_str(&tag).map_err(|_| Error::Internal)?;
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Json(item).into_response()
    };
    response.headers_mut().insert(header::ETAG, etag);
    Ok(response)
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/issue-order-maintenance/{job}",params(("w"=Uuid,Path),("t"=Uuid,Path),("job"=Uuid,Path),("If-None-Match"=Option<String>,Header)),responses((status=200,body=IssueOrderMaintenance),(status=304)))]
pub(crate) async fn issue_order_maintenance(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, job)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let item = s.app.issue_order_maintenance(&a, w, t, job).await?;
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        == Some(item.etag.as_str());
    response(item.clone(), item.etag, unchanged)
}

#[utoipa::path(post,path="/api/v1/workspaces/{w}/teams/{t}/issue-order-maintenance/{job}/resume",params(("w"=Uuid,Path),("t"=Uuid,Path),("job"=Uuid,Path),("If-Match"=String,Header)),responses((status=200,body=IssueOrderMaintenance)))]
pub(crate) async fn resume_issue_order_maintenance(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, job)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let etag = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .ok_or(Error::PreconditionRequired)?;
    let item = s
        .app
        .resume_issue_order_maintenance(&a, w, t, job, etag)
        .await?;
    response(item.clone(), item.etag, false)
}

#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues/{id}/position",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=PositionIssue,responses((status=200,body=IssuePosition)))]
pub(crate) async fn position_issue(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<PositionIssue>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let etag = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::PreconditionRequired)?;
    let key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("Provide an Idempotency-Key".into()))?;
    let result = s
        .app
        .position_issue(&a, w, id, key, etag, payload(body)?)
        .await?;
    let tag = result.issue.etag();
    response(result, tag, false)
}
