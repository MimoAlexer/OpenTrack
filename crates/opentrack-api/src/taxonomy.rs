use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use opentrack_domain::{
    AssigneePage, AssigneeQuery, CreateLabel, Error, Label, LabelQuery, Page, UpdateLabel,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/labels", get(labels).post(create_label))
        .route(
            "/workspaces/{w}/labels/{label}",
            get(label).patch(update_label),
        )
        .route("/workspaces/{w}/teams/{t}/assignees", get(assignees))
}

fn label_response(label: Label, status: StatusCode) -> Result<Response, ApiError> {
    let tag = label.etag();
    let mut response = (status, Json(label)).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&tag).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/labels",params(
    ("w"=Uuid,Path),
    ("scope"=Option<String>,Query,description="visible (default), workspace, team, or applicable; team and applicable require team_id"),
    ("team_id"=Option<Uuid>,Query),
    ("state"=Option<String>,Query,description="active (default), archived, or all"),
    ("q"=Option<String>,Query,max_length=100,description="Literal case-insensitive name substring"),
    ("limit"=Option<u32>,Query,minimum=1,maximum=100),
    ("cursor"=Option<String>,Query)
),responses((status=200,body=Page<Label>)))]
pub(crate) async fn labels(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<LabelQuery>,
) -> Result<Json<Page<Label>>, ApiError> {
    Ok(Json(s.app.list_labels(&a, w, q).await?))
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/labels/{label}",params(("w"=Uuid,Path),("label"=Uuid,Path)),responses((status=200,body=Label)))]
pub(crate) async fn label(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    label_response(s.app.get_label(&a, w, id).await?, StatusCode::OK)
}

#[utoipa::path(post,path="/api/v1/workspaces/{w}/labels",params(("w"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateLabel,responses((status=201,body=Label)))]
pub(crate) async fn create_label(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<CreateLabel>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| Error::Invalid("An Idempotency-Key is required".into()))?;
    let label = s.app.create_label(&a, w, key, payload(body)?).await?;
    let location = format!("/api/v1/workspaces/{w}/labels/{}", label.id);
    let mut response = label_response(label, StatusCode::CREATED)?;
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}

#[utoipa::path(patch,path="/api/v1/workspaces/{w}/labels/{label}",params(("w"=Uuid,Path),("label"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateLabel,responses((status=200,body=Label)))]
pub(crate) async fn update_label(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateLabel>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let expected = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .ok_or(Error::PreconditionRequired)?;
    label_response(
        s.app
            .update_label(&a, w, id, expected, payload(body)?)
            .await?,
        StatusCode::OK,
    )
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/assignees",params(
    ("w"=Uuid,Path),("t"=Uuid,Path),
    ("q"=Option<String>,Query,max_length=100,description="Literal case-insensitive display-name substring"),
    ("limit"=Option<u32>,Query,minimum=1,maximum=20),
    ("cursor"=Option<String>,Query)
),responses((status=200,body=AssigneePage)))]
pub(crate) async fn assignees(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    Query(q): Query<AssigneeQuery>,
) -> Result<Json<AssigneePage>, ApiError> {
    Ok(Json(s.app.eligible_assignees(&a, w, t, q).await?))
}
