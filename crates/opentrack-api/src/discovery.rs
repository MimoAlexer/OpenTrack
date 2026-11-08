use crate::{ApiError, ApiState, Auth, csrf, payload};
use axum::{
    Json, Router,
    extract::{OriginalUri, Path, Query, RawQuery, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use opentrack_domain::*;
use serde::Deserialize;
use std::result::Result;
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/workspaces/{w}/views", get(list).post(create))
        .route(
            "/workspaces/{w}/views/{id}",
            get(detail).patch(update).delete(delete),
        )
        .route("/workspaces/{w}/views/{id}/favorite", put(favorite))
        .route("/workspaces/{w}/views/{id}/execute", post(execute))
        .route("/workspaces/{w}/saved-searches", get(list).post(create))
        .route(
            "/workspaces/{w}/saved-searches/{id}",
            get(detail).patch(update).delete(delete),
        )
        .route(
            "/workspaces/{w}/saved-searches/{id}/favorite",
            put(favorite),
        )
        .route("/workspaces/{w}/saved-searches/{id}/execute", post(execute))
        .route("/workspaces/{w}/search", get(search_get))
        .route("/workspaces/{w}/search/query", post(search_query))
        .route("/workspaces/{w}/search/suggestions", get(suggestions))
}
fn route_kind(uri: &axum::http::Uri) -> DiscoveryKind {
    if uri.path().split('/').nth(5) == Some("saved-searches") {
        DiscoveryKind::Search
    } else {
        DiscoveryKind::View
    }
}
fn tag(h: &HeaderMap) -> Result<&str, ApiError> {
    h.get(header::IF_MATCH)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| Error::PreconditionRequired.into())
}
fn key(h: &HeaderMap) -> Result<&str, ApiError> {
    h.get("Idempotency-Key")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| Error::Invalid("Provide an Idempotency-Key".into()).into())
}
fn response(d: SavedDiscovery) -> Result<Response, ApiError> {
    let etag = HeaderValue::from_str(&d.etag).map_err(|_| Error::Internal)?;
    let mut r = Json(d).into_response();
    r.headers_mut().insert(header::ETAG, etag);
    Ok(r)
}
#[utoipa::path(get,operation_id="views_list",path="/api/v1/workspaces/{w}/views",params(("w"=Uuid,Path),("limit"=Option<i64>,Query),("cursor"=Option<String>,Query),("favorites"=Option<bool>,Query)),responses((status=200,body=Page<SavedDiscovery>)))]
pub(crate) async fn list(
    OriginalUri(uri): OriginalUri,
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(mut q): Query<DiscoveryListQuery>,
) -> Result<Json<Page<SavedDiscovery>>, ApiError> {
    let expected = route_kind(&uri);
    if q.kind.is_some_and(|kind| kind != expected) {
        return Err(Error::Invalid("The kind filter must match this resource route".into()).into());
    }
    q.kind = Some(expected);
    Ok(Json(s.app.discoveries(&a, w, q).await?))
}
#[utoipa::path(post,operation_id="views_create",path="/api/v1/workspaces/{w}/views",params(("w"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=SaveDiscovery,responses((status=201,body=SavedDiscovery)))]
pub(crate) async fn create(
    OriginalUri(uri): OriginalUri,
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    h: HeaderMap,
    body: Result<Json<SaveDiscovery>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &h, &a)?;
    let request = payload(body)?;
    if request.kind != route_kind(&uri) {
        return Err(
            Error::Invalid("The definition kind must match this resource route".into()).into(),
        );
    }
    let item = s.app.create_discovery(&a, w, key(&h)?, request).await?;
    let location = HeaderValue::from_str(&format!("{}/{}", uri.path(), item.id))
        .map_err(|_| Error::Internal)?;
    let mut result = response(item)?;
    *result.status_mut() = StatusCode::CREATED;
    result.headers_mut().insert(header::LOCATION, location);
    Ok(result)
}
#[utoipa::path(get,operation_id="views_detail",path="/api/v1/workspaces/{w}/views/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=SavedDiscovery)))]
pub(crate) async fn detail(
    OriginalUri(uri): OriginalUri,
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.discovery(&a, w, id).await?;
    if item.kind != route_kind(&uri) {
        return Err(Error::NotFound.into());
    }
    response(item)
}
#[utoipa::path(patch,operation_id="views_update",path="/api/v1/workspaces/{w}/views/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),request_body=SaveDiscovery,responses((status=200,body=SavedDiscovery)))]
pub(crate) async fn update(
    OriginalUri(uri): OriginalUri,
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<SaveDiscovery>, JsonRejection>,
) -> Result<Response, ApiError> {
    let item = s.app.discovery(&a, w, id).await?;
    if item.kind != route_kind(&uri) {
        return Err(Error::NotFound.into());
    }
    csrf(&s, &h, &a)?;
    response(
        s.app
            .update_discovery(&a, w, id, tag(&h)?, key(&h)?, payload(body)?)
            .await?,
    )
}
#[utoipa::path(delete,operation_id="views_delete",path="/api/v1/workspaces/{w}/views/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header),("Idempotency-Key"=String,Header)),responses((status=204)))]
pub(crate) async fn delete(
    OriginalUri(uri): OriginalUri,
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    match s.app.discovery(&a, w, id).await {
        Ok(item) if item.kind != route_kind(&uri) => return Err(Error::NotFound.into()),
        Ok(_) | Err(Error::NotFound) => {}
        Err(error) => return Err(error.into()),
    }
    csrf(&s, &h, &a)?;
    s.app
        .delete_discovery(&a, w, id, tag(&h)?, key(&h)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(put,operation_id="views_favorite",path="/api/v1/workspaces/{w}/views/{id}/favorite",params(("w"=Uuid,Path),("id"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=FavoriteDiscovery,responses((status=200,body=SavedDiscovery)))]
pub(crate) async fn favorite(
    OriginalUri(uri): OriginalUri,
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<FavoriteDiscovery>, JsonRejection>,
) -> Result<Response, ApiError> {
    let item = s.app.discovery(&a, w, id).await?;
    if item.kind != route_kind(&uri) {
        return Err(Error::NotFound.into());
    }
    csrf(&s, &h, &a)?;
    response(
        s.app
            .favorite_discovery(&a, w, id, key(&h)?, payload(body)?.favorite)
            .await?,
    )
}
#[utoipa::path(post,operation_id="views_execute",path="/api/v1/workspaces/{w}/views/{id}/execute",params(("w"=Uuid,Path),("id"=Uuid,Path)),request_body=ExecuteDiscovery,responses((status=200,body=DiscoveryExecution)))]
pub(crate) async fn execute(
    OriginalUri(uri): OriginalUri,
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    body: Result<Json<ExecuteDiscovery>, JsonRejection>,
) -> Result<Json<DiscoveryExecution>, ApiError> {
    let item = s.app.discovery(&a, w, id).await?;
    if item.kind != route_kind(&uri) {
        return Err(Error::NotFound.into());
    }
    csrf(&s, &h, &a)?;
    Ok(Json(
        s.app
            .execute_discovery(&a, w, id, payload(body)?.cursor)
            .await?,
    ))
}
#[utoipa::path(post,operation_id="search_query",path="/api/v1/workspaces/{w}/search/query",params(("w"=Uuid,Path)),request_body=SearchRequest,responses((status=200,body=SearchPage)))]
pub(crate) async fn search_query(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    RawQuery(url): RawQuery,
    h: HeaderMap,
    body: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchPage>, ApiError> {
    csrf(&s, &h, &a)?;
    if url.as_deref().is_some_and(|q| !q.is_empty()) {
        return Err(Error::Invalid("Send search query fields only in the JSON body".into()).into());
    }
    Ok(Json(s.app.search(&a, w, payload(body)?).await?))
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchGet {
    q: String,
    kind: Option<SearchEntity>,
    team_id: Option<Uuid>,
    limit: Option<i64>,
    cursor: Option<String>,
}
impl SearchGet {
    fn request(self, suggestions: bool) -> Result<SearchRequest, ApiError> {
        let limit = self.limit.unwrap_or(if suggestions { 20 } else { 50 });
        if suggestions && !(1..=20).contains(&limit) {
            return Err(Error::Invalid("Suggestions support at most 20 results".into()).into());
        }
        Ok(SearchRequest {
            query: IssueQuery {
                q: Some(self.q),
                team_id: self.team_id,
                limit,
                cursor: self.cursor,
                ..Default::default()
            },
            types: self.kind.map(|k| vec![k]).unwrap_or_else(|| {
                if self.team_id.is_some() {
                    vec![SearchEntity::Issue]
                } else {
                    vec![SearchEntity::Issue, SearchEntity::Project]
                }
            }),
        })
    }
}
#[utoipa::path(get,operation_id="search_get",path="/api/v1/workspaces/{w}/search",params(("w"=Uuid,Path),("q"=String,Query),("kind"=Option<SearchEntity>,Query),("team_id"=Option<Uuid>,Query),("limit"=Option<i64>,Query),("cursor"=Option<String>,Query)),responses((status=200,body=SearchPage)))]
pub(crate) async fn search_get(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<SearchGet>,
) -> Result<Json<SearchPage>, ApiError> {
    Ok(Json(s.app.search(&a, w, q.request(false)?).await?))
}
#[utoipa::path(get,operation_id="search_suggestions",path="/api/v1/workspaces/{w}/search/suggestions",params(("w"=Uuid,Path),("q"=String,Query),("kind"=Option<SearchEntity>,Query),("limit"=Option<i64>,Query)),responses((status=200,body=SearchPage)))]
pub(crate) async fn suggestions(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<SearchGet>,
) -> Result<Json<SearchPage>, ApiError> {
    Ok(Json(s.app.search(&a, w, q.request(true)?).await?))
}
