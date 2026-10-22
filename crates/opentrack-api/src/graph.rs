use crate::{ApiError, ApiState, Auth, csrf, issue_response, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use opentrack_domain::{
    CreateIssueRelation, Error, GraphPage, GraphPageQuery, Issue, IssueGraphTarget,
    IssueLookupQuery, IssueMovePreview, IssueParent, IssueRelation, MoveDestination,
    MoveDestinationQuery, MoveIssue, Page, RelationMutation, SetIssueParent,
};
use serde::Deserialize;
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/issues/lookup", get(lookup))
        .route("/workspaces/{w}/issues/by-key/{key}", get(by_key))
        .route(
            "/workspaces/{w}/issues/{id}/parent",
            get(parent).patch(set_parent),
        )
        .route("/workspaces/{w}/issues/{id}/children", get(children))
        .route(
            "/workspaces/{w}/issues/{id}/relations",
            get(relations).post(create_relation),
        )
        .route(
            "/workspaces/{w}/issues/{id}/relations/{relation}",
            get(relation).delete(remove_relation),
        )
        .route(
            "/workspaces/{w}/issues/{id}/move-destinations",
            get(destinations),
        )
        .route(
            "/workspaces/{w}/issues/{id}/move-preview",
            get(move_preview),
        )
        .route("/workspaces/{w}/issues/{id}/move", post(move_issue))
}

fn expected(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::PreconditionRequired.into())
}
fn key(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()).into())
}

// Hash the complete authorized projection. A target rename, permission change or
// historical endpoint disappearing must invalidate this resource's validator.
fn graph_response<T: serde::Serialize>(data: T, headers: &HeaderMap) -> Result<Response, ApiError> {
    let tag = opentrack_domain::graph_etag(&data)?;
    let matches = headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|v| v == "*" || v.strip_prefix("W/").unwrap_or(v) == tag);
    // Every caller loads data through App authorization before entering here.
    let mut response = if matches {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Json(data).into_response()
    };
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&tag).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/lookup",params(
    ("w"=Uuid,Path),("team_id"=Uuid,Query),
    ("q"=Option<String>,Query,max_length=100,description="Literal key/title search, including authorized historical keys"),
    ("exclude_issue_id"=Option<Uuid>,Query),
    ("limit"=Option<u32>,Query,minimum=1,maximum=20),("cursor"=Option<String>,Query)
),responses((status=200,body=Page<IssueGraphTarget>)))]
pub(crate) async fn lookup(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<IssueLookupQuery>,
) -> Result<Json<Page<IssueGraphTarget>>, ApiError> {
    Ok(Json(s.app.lookup_issues(&a, w, q).await?))
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/by-key/{key}",params(("w"=Uuid,Path),("key"=String,Path)),responses((status=200,body=Issue,description="Canonical issue after current-team authorization; no redirect is emitted")))]
pub(crate) async fn by_key(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, key)): Path<(Uuid, String)>,
) -> Result<Response, ApiError> {
    issue_response(s.app.resolve_issue_key(&a, w, &key).await?, StatusCode::OK)
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/parent",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-None-Match"=Option<String>,Header)),responses((status=200,body=IssueParent),(status=304,description="Authorized parent projection is unchanged")))]
pub(crate) async fn parent(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    graph_response(s.app.issue_parent(&a, w, id).await?, &headers)
}

#[utoipa::path(patch,path="/api/v1/workspaces/{w}/issues/{id}/parent",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header,description="Source issue ETag, not the parent projection ETag")),request_body=SetIssueParent,responses((status=200,body=Issue)))]
pub(crate) async fn set_parent(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<SetIssueParent>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    issue_response(
        s.app
            .set_issue_parent(&a, w, id, expected(&headers)?, payload(body)?)
            .await?,
        StatusCode::OK,
    )
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/children",params(
    ("w"=Uuid,Path),("id"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=50),
    ("cursor"=Option<String>,Query),("If-None-Match"=Option<String>,Header)
),responses((status=200,body=GraphPage<IssueGraphTarget>),(status=304,description="Authorized children page is unchanged")))]
pub(crate) async fn children(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<GraphPageQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    graph_response(s.app.issue_children(&a, w, id, q).await?, &headers)
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/relations",params(
    ("w"=Uuid,Path),("id"=Uuid,Path),("limit"=Option<u32>,Query,minimum=1,maximum=50),
    ("cursor"=Option<String>,Query),("If-None-Match"=Option<String>,Header)
),responses((status=200,body=GraphPage<IssueRelation>),(status=304,description="Authorized relationships page is unchanged")))]
pub(crate) async fn relations(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<GraphPageQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    graph_response(s.app.issue_relations(&a, w, id, q).await?, &headers)
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/relations/{relation}",params(("w"=Uuid,Path),("id"=Uuid,Path),("relation"=Uuid,Path),("If-None-Match"=Option<String>,Header)),responses((status=200,body=IssueRelation),(status=304,description="Authorized relationship is unchanged")))]
pub(crate) async fn relation(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id, relation)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    graph_response(
        s.app.get_issue_relation(&a, w, id, relation).await?,
        &headers,
    )
}

#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues/{id}/relations",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header,description="Source issue ETag"),("Idempotency-Key"=String,Header)),request_body=CreateIssueRelation,responses((status=201,body=RelationMutation)))]
pub(crate) async fn create_relation(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<CreateIssueRelation>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let result = s
        .app
        .create_issue_relation(
            &a,
            w,
            id,
            key(&headers)?,
            expected(&headers)?,
            payload(body)?,
        )
        .await?;
    let tag = result.issue.etag();
    let location = format!(
        "/api/v1/workspaces/{w}/issues/{id}/relations/{}",
        result.relation.id
    );
    let mut response = (StatusCode::CREATED, Json(result)).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&tag).map_err(|_| Error::Internal)?,
    );
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}

#[utoipa::path(delete,path="/api/v1/workspaces/{w}/issues/{id}/relations/{relation}",params(("w"=Uuid,Path),("id"=Uuid,Path),("relation"=Uuid,Path),("If-Match"=String,Header,description="Source issue ETag; target ETag is not required")),responses((status=200,body=Issue)))]
pub(crate) async fn remove_relation(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id, relation)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    issue_response(
        s.app
            .remove_issue_relation(&a, w, id, relation, expected(&headers)?)
            .await?,
        StatusCode::OK,
    )
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/move-destinations",params(
    ("w"=Uuid,Path),("id"=Uuid,Path),("q"=Option<String>,Query,max_length=100),
    ("limit"=Option<u32>,Query,minimum=1,maximum=50),("cursor"=Option<String>,Query)
),responses((status=200,body=Page<MoveDestination>)))]
pub(crate) async fn destinations(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<MoveDestinationQuery>,
) -> Result<Json<Page<MoveDestination>>, ApiError> {
    Ok(Json(s.app.issue_move_destinations(&a, w, id, q).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MovePreviewQuery {
    destination_team_id: Uuid,
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/move-preview",params(("w"=Uuid,Path),("id"=Uuid,Path),("destination_team_id"=Uuid,Query)),responses((status=200,body=IssueMovePreview)))]
pub(crate) async fn move_preview(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<MovePreviewQuery>,
) -> Result<Json<IssueMovePreview>, ApiError> {
    Ok(Json(
        s.app
            .preview_issue_move(&a, w, id, q.destination_team_id)
            .await?,
    ))
}

#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues/{id}/move",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=MoveIssue,responses((status=200,body=Issue,description="Moved issue with stable UUID and new canonical key")))]
pub(crate) async fn move_issue(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<MoveIssue>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    issue_response(
        s.app
            .move_issue(
                &a,
                w,
                id,
                key(&headers)?,
                expected(&headers)?,
                payload(body)?,
            )
            .await?,
        StatusCode::OK,
    )
}
