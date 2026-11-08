use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, RawQuery, State, rejection::JsonRejection},
    http::HeaderMap,
    routing::post,
};
use opentrack_domain::{Error, IssueQuery, IssueQueryPage};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/workspaces/{w}/issues/query", post(query))
}

#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues/query",params(("w"=Uuid,Path)),request_body=IssueQuery,
    description="Read-only structured issue query. Send query fields only in the JSON body; URL query parameters are rejected. No idempotency receipt is created. Query timezone defaults to UTC; relative dates and continuation cursors retain the server-recorded query clock.",
    responses((status=200,body=IssueQueryPage)))]
pub(crate) async fn query(
    State(state): State<ApiState>,
    Auth(actor): Auth,
    Path(workspace): Path<Uuid>,
    RawQuery(url_query): RawQuery,
    headers: HeaderMap,
    body: std::result::Result<Json<IssueQuery>, JsonRejection>,
) -> Result<Json<IssueQueryPage>, ApiError> {
    csrf(&state, &headers, &actor)?;
    if url_query.as_deref().is_some_and(|query| !query.is_empty()) {
        return Err(Error::Invalid(
            "Send structured issue query fields in the JSON body, without URL query parameters"
                .into(),
        )
        .into());
    }
    Ok(Json(
        state
            .app
            .query_issues(&actor, workspace, payload(body)?)
            .await?,
    ))
}
