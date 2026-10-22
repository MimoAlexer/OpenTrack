use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use opentrack_domain::{
    Error, IssueReferenceCandidate, IssueReferenceQuery, IssueSubscription, MentionCandidate,
    MentionQuery, Page, UpdateIssueSubscription,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/mention-candidates", get(candidates))
        .route(
            "/workspaces/{w}/issue-reference-candidates",
            get(issue_reference_candidates),
        )
        .route(
            "/workspaces/{w}/issues/{id}/subscription",
            get(subscription).patch(update_subscription),
        )
}
fn response(item: IssueSubscription) -> Result<Response, ApiError> {
    let tag = HeaderValue::from_str(&item.etag).map_err(|_| Error::Internal)?;
    let mut result = Json(item).into_response();
    result.headers_mut().insert(header::ETAG, tag);
    Ok(result)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/mention-candidates",params(
    ("w"=Uuid,Path),("team_id"=Option<Uuid>,Query),("issue_id"=Option<Uuid>,Query),("document_id"=Option<Uuid>,Query),
    ("q"=Option<String>,Query,max_length=100),("limit"=Option<u32>,Query,minimum=1,maximum=50),("cursor"=Option<String>,Query)
),responses((status=200,body=Page<MentionCandidate>)))]
pub(crate) async fn candidates(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<MentionQuery>,
) -> Result<Json<Page<MentionCandidate>>, ApiError> {
    Ok(Json(s.app.mention_candidates(&a, w, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issue-reference-candidates",params(
    ("w"=Uuid,Path),("team_id"=Option<Uuid>,Query),("issue_id"=Option<Uuid>,Query),("document_id"=Option<Uuid>,Query),
    ("q"=Option<String>,Query,max_length=100),("limit"=Option<u32>,Query,minimum=1,maximum=20),("cursor"=Option<String>,Query)
),responses((status=200,body=Page<IssueReferenceCandidate>)))]
pub(crate) async fn issue_reference_candidates(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<IssueReferenceQuery>,
) -> Result<Json<Page<IssueReferenceCandidate>>, ApiError> {
    Ok(Json(s.app.issue_reference_candidates(&a, w, q).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/subscription",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=IssueSubscription)))]
pub(crate) async fn subscription(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    response(s.app.issue_subscription(&a, w, id).await?)
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/issues/{id}/subscription",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateIssueSubscription,responses((status=200,body=IssueSubscription)))]
pub(crate) async fn update_subscription(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateIssueSubscription>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let etag = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::PreconditionRequired)?;
    response(
        s.app
            .update_issue_subscription(&a, w, id, etag, payload(body)?)
            .await?,
    )
}
