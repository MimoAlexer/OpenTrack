use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::HeaderMap,
    routing::post,
};
use opentrack_domain::{IssueQuery, IssueQueryPage};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/workspaces/{w}/issues/query", post(query))
}

#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues/query",params(("w"=Uuid,Path)),request_body=IssueQuery,
    description="Read-only structured issue query. No idempotency receipt is created. Query timezone defaults to UTC; relative dates and continuation cursors retain the server-recorded query clock.",
    responses((status=200,body=IssueQueryPage)))]
pub(crate) async fn query(
    State(state): State<ApiState>,
    Auth(actor): Auth,
    Path(workspace): Path<Uuid>,
    headers: HeaderMap,
    body: std::result::Result<Json<IssueQuery>, JsonRejection>,
) -> Result<Json<IssueQueryPage>, ApiError> {
    csrf(&state, &headers, &actor)?;
    Ok(Json(
        state
            .app
            .query_issues(&actor, workspace, payload(body)?)
            .await?,
    ))
}
