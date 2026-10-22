use crate::{
    ApiError, ApiState, Auth, clear_session_cookie, cookie_response, csrf, origin, payload,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use opentrack_domain::{
    AcceptInvitation, CreateInvitation, CreateTeam, CreateWorkspace, DirectoryQuery, Error,
    Invitation, InvitationIssued, InvitationPreview, InvitationToken, OrganizationSummary, Page,
    PutTeamMember, Session, TeamDetail, TeamMember, UpdateMember, UpdateTeam, UpdateWorkspace,
    WorkspaceAccess, WorkspaceDetail, WorkspaceMember,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/organizations", get(organizations))
        .route("/workspaces", post(create_workspace))
        .route("/workspaces/{w}/access", get(workspace_access))
        .route("/workspaces/{w}", get(workspace).patch(update_workspace))
        .route("/workspaces/{w}/teams", get(teams).post(create_team))
        .route(
            "/workspaces/{w}/teams/{t}",
            get(get_team).patch(update_team),
        )
        .route("/workspaces/{w}/members", get(members))
        .route(
            "/workspaces/{w}/members/{u}",
            get(get_workspace_member).patch(update_member),
        )
        .route("/workspaces/{w}/teams/{t}/members", get(team_members))
        .route(
            "/workspaces/{w}/teams/{t}/members/{u}",
            get(get_team_member)
                .put(put_team_member)
                .delete(remove_team_member),
        )
        .route(
            "/workspaces/{w}/invitations",
            get(invitations).post(create_invitation),
        )
        .route("/workspaces/{w}/invitations/{id}", get(get_invitation))
        .route(
            "/workspaces/{w}/invitations/{id}/revoke",
            post(revoke_invitation),
        )
        .route("/invitations/preview", post(invitation_preview))
        .route("/invitations/accept", post(accept_invitation))
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
fn entity<T: serde::Serialize>(
    data: T,
    status: StatusCode,
    etag: String,
    location: String,
) -> Result<Response, ApiError> {
    let mut response = (status, Json(data)).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag).map_err(|_| Error::Internal)?,
    );
    if status == StatusCode::CREATED {
        response.headers_mut().insert(
            header::LOCATION,
            HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
        );
    }
    Ok(response)
}
/// A committed access change is still successful when it ends the caller's session.
fn access_result(
    state: &ApiState,
    actor: &opentrack_domain::Actor,
    mut response: Response,
    revoked: bool,
) -> Response {
    if revoked {
        if actor.audience == opentrack_domain::SessionAudience::Web {
            clear_session_cookie(state, &mut response);
        }
        response.headers_mut().insert(
            "x-opentrack-session-invalidated",
            HeaderValue::from_static("true"),
        );
    }
    response
}
#[utoipa::path(get, path="/api/v1/organizations", responses((status=200, body=Vec<OrganizationSummary>)))]
pub(crate) async fn organizations(
    State(s): State<ApiState>,
    Auth(a): Auth,
) -> Result<Json<Vec<OrganizationSummary>>, ApiError> {
    Ok(Json(s.app.organizations(&a).await?))
}
#[utoipa::path(post, path="/api/v1/workspaces", request_body=CreateWorkspace, params(("Idempotency-Key"=String, Header)), responses((status=201, body=WorkspaceDetail)))]
pub(crate) async fn create_workspace(
    State(s): State<ApiState>,
    Auth(a): Auth,
    headers: HeaderMap,
    body: Result<Json<CreateWorkspace>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .create_workspace(&a, key(&headers)?, payload(body)?)
        .await?;
    let tag = format!("\"workspace:{}:{}\"", item.id, item.version);
    let location = format!("/api/v1/workspaces/{}", item.id);
    entity(item, StatusCode::CREATED, tag, location)
}
#[utoipa::path(get, path="/api/v1/workspaces/{w}", params(("w"=Uuid,Path)), responses((status=200,body=WorkspaceDetail)))]
pub(crate) async fn workspace(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
) -> Result<Response, ApiError> {
    let item = s.app.workspace(&a, w).await?;
    let tag = format!("\"workspace:{w}:{}\"", item.version);
    entity(item, StatusCode::OK, tag, String::new())
}
#[utoipa::path(patch, path="/api/v1/workspaces/{w}", params(("w"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateWorkspace,responses((status=200,body=WorkspaceDetail)))]
pub(crate) async fn update_workspace(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<UpdateWorkspace>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .update_workspace(&a, w, expected(&headers)?, payload(body)?)
        .await?;
    let tag = format!("\"workspace:{w}:{}\"", item.version);
    entity(item, StatusCode::OK, tag, String::new())
}
#[utoipa::path(get, path="/api/v1/workspaces/{w}/teams", params(("w"=Uuid,Path),("limit"=Option<u32>,Query),("cursor"=Option<String>,Query),("state"=Option<String>,Query)),responses((status=200,body=Page<TeamDetail>)))]
pub(crate) async fn teams(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<DirectoryQuery>,
) -> Result<Json<Page<TeamDetail>>, ApiError> {
    Ok(Json(s.app.list_teams(&a, w, q).await?))
}
#[utoipa::path(post, path="/api/v1/workspaces/{w}/teams", params(("w"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateTeam,responses((status=201,body=TeamDetail)))]
pub(crate) async fn create_team(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<CreateTeam>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .create_team(&a, w, key(&headers)?, payload(body)?)
        .await?;
    let tag = format!("\"team:{}:{}\"", item.id, item.version);
    let location = format!("/api/v1/workspaces/{w}/teams/{}", item.id);
    entity(item, StatusCode::CREATED, tag, location)
}
#[utoipa::path(patch, path="/api/v1/workspaces/{w}/teams/{t}", params(("w"=Uuid,Path),("t"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateTeam,responses((status=200,body=TeamDetail)))]
pub(crate) async fn update_team(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateTeam>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .update_team(&a, w, t, expected(&headers)?, payload(body)?)
        .await?;
    let tag = format!("\"team:{t}:{}\"", item.version);
    entity(item, StatusCode::OK, tag, String::new())
}
#[utoipa::path(get, path="/api/v1/workspaces/{w}/members",params(("w"=Uuid,Path),("limit"=Option<u32>,Query),("cursor"=Option<String>,Query),("state"=Option<String>,Query)),responses((status=200,body=Page<WorkspaceMember>)))]
pub(crate) async fn members(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<DirectoryQuery>,
) -> Result<Json<Page<WorkspaceMember>>, ApiError> {
    Ok(Json(s.app.workspace_members(&a, w, q).await?))
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/members/{u}",params(("w"=Uuid,Path),("u"=Uuid,Path),("If-Match"=String,Header)),request_body=UpdateMember,responses((status=200,body=WorkspaceMember)))]
pub(crate) async fn update_member(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, u)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<UpdateMember>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .update_member(&a, w, u, expected(&headers)?, payload(body)?)
        .await?;
    let revoked = item.current_session_revoked;
    let item = item.entity;
    let tag = format!("\"member:{w}:{u}:{}\"", item.version);
    Ok(access_result(
        &s,
        &a,
        entity(item, StatusCode::OK, tag, String::new())?,
        revoked,
    ))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/members",params(("w"=Uuid,Path),("t"=Uuid,Path),("limit"=Option<u32>,Query),("cursor"=Option<String>,Query),("state"=Option<String>,Query)),responses((status=200,body=Page<TeamMember>)))]
pub(crate) async fn team_members(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
    Query(q): Query<DirectoryQuery>,
) -> Result<Json<Page<TeamMember>>, ApiError> {
    Ok(Json(s.app.team_members(&a, w, t, q).await?))
}
#[utoipa::path(put,path="/api/v1/workspaces/{w}/teams/{t}/members/{u}",params(("w"=Uuid,Path),("t"=Uuid,Path),("u"=Uuid,Path),("If-Match"=Option<String>,Header),("If-None-Match"=Option<String>,Header)),request_body=PutTeamMember,responses((status=200,body=TeamMember),(status=201,body=TeamMember)))]
pub(crate) async fn put_team_member(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, u)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<PutTeamMember>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let missing = headers.get(header::IF_NONE_MATCH);
    if missing.is_some() && headers.contains_key(header::IF_MATCH) {
        return Err(Error::Invalid("Use one membership precondition, not both".into()).into());
    }
    let tag = if let Some(value) = missing {
        if value != "*" {
            return Err(Error::Invalid("New membership requires If-None-Match: *".into()).into());
        }
        None
    } else {
        Some(expected(&headers)?)
    };
    let status = if tag.is_none() {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let item = s
        .app
        .put_team_member(&a, w, t, u, tag, payload(body)?)
        .await?;
    let revoked = item.current_session_revoked;
    let item = item.entity;
    let etag = format!("\"team-member:{t}:{u}:{}\"", item.version);
    let location = format!("/api/v1/workspaces/{w}/teams/{t}/members/{u}");
    Ok(access_result(
        &s,
        &a,
        entity(item, status, etag, location)?,
        revoked,
    ))
}
#[utoipa::path(delete,path="/api/v1/workspaces/{w}/teams/{t}/members/{u}",params(("w"=Uuid,Path),("t"=Uuid,Path),("u"=Uuid,Path),("If-Match"=String,Header)),responses((status=204,description="Membership removed")))]
pub(crate) async fn remove_team_member(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, u)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    csrf(&s, &headers, &a)?;
    s.app
        .remove_team_member(&a, w, t, u, expected(&headers)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/invitations",params(("w"=Uuid,Path),("limit"=Option<u32>,Query),("cursor"=Option<String>,Query),("state"=Option<String>,Query)),responses((status=200,body=Page<Invitation>)))]
pub(crate) async fn invitations(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(q): Query<DirectoryQuery>,
) -> Result<Json<Page<Invitation>>, ApiError> {
    Ok(Json(s.app.invitations(&a, w, q).await?))
}
/// The raw token is displayed once. Idempotent replay returns token:null; revoke and recreate if the first response was lost.
#[utoipa::path(post,path="/api/v1/workspaces/{w}/invitations",params(("w"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateInvitation,responses((status=201,body=InvitationIssued)))]
pub(crate) async fn create_invitation(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<CreateInvitation>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .create_invitation(&a, w, key(&headers)?, payload(body)?)
        .await?;
    let tag = format!(
        "\"invitation:{}:{}\"",
        item.invitation.id, item.invitation.version
    );
    let location = format!("/api/v1/workspaces/{w}/invitations/{}", item.invitation.id);
    entity(item, StatusCode::CREATED, tag, location)
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/invitations/{id}/revoke",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),responses((status=200,body=Invitation)))]
pub(crate) async fn revoke_invitation(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let item = s
        .app
        .revoke_invitation(&a, w, id, expected(&headers)?)
        .await?;
    let tag = format!("\"invitation:{id}:{}\"", item.version);
    entity(item, StatusCode::OK, tag, String::new())
}
#[utoipa::path(post,path="/api/v1/invitations/preview",security(()),request_body=InvitationToken,responses((status=200,body=InvitationPreview)))]
pub(crate) async fn invitation_preview(
    State(s): State<ApiState>,
    headers: HeaderMap,
    body: Result<Json<InvitationToken>, JsonRejection>,
) -> Result<Json<InvitationPreview>, ApiError> {
    origin(&s, &headers)?;
    crate::credentials::browser_cookie(&headers)?;
    Ok(Json(s.app.invitation_preview(&payload(body)?.token).await?))
}
#[utoipa::path(post,path="/api/v1/invitations/accept",security(()),request_body=AcceptInvitation,responses((status=200,body=Session)))]
pub(crate) async fn accept_invitation(
    State(s): State<ApiState>,
    headers: HeaderMap,
    body: Result<Json<AcceptInvitation>, JsonRejection>,
) -> Result<Response, ApiError> {
    origin(&s, &headers)?;
    let token = crate::credentials::browser_cookie(&headers)?;
    let actor = if let Some(token) = token {
        Some(s.app.authenticate(token).await?)
    } else {
        None
    };
    if let Some(actor) = &actor {
        csrf(&s, &headers, actor)?;
    }
    let (session, token) = s
        .app
        .accept_invitation(actor.as_ref(), payload(body)?)
        .await?;
    cookie_response(&s, session, &token, StatusCode::OK)
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}",params(("w"=Uuid,Path),("t"=Uuid,Path)),responses((status=200,body=TeamDetail)))]
pub(crate) async fn get_team(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.get_team(&a, w, t).await?;
    let tag = item.etag();
    entity(item, StatusCode::OK, tag, String::new())
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/members/{u}",params(("w"=Uuid,Path),("u"=Uuid,Path)),responses((status=200,body=WorkspaceMember)))]
pub(crate) async fn get_workspace_member(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, u)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.get_workspace_member(&a, w, u).await?;
    let tag = item.etag(w);
    entity(item, StatusCode::OK, tag, String::new())
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/teams/{t}/members/{u}",params(("w"=Uuid,Path),("t"=Uuid,Path),("u"=Uuid,Path)),responses((status=200,body=TeamMember)))]
pub(crate) async fn get_team_member(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, t, u)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.get_team_member(&a, w, t, u).await?;
    let tag = item.etag(t);
    entity(item, StatusCode::OK, tag, String::new())
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/invitations/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Invitation)))]
pub(crate) async fn get_invitation(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let item = s.app.get_invitation(&a, w, id).await?;
    let tag = item.etag();
    entity(item, StatusCode::OK, tag, String::new())
}

#[utoipa::path(get,path="/api/v1/workspaces/{w}/access",params(("w"=Uuid,Path)),responses((status=200,body=WorkspaceAccess)))]
pub(crate) async fn workspace_access(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
) -> Result<Json<WorkspaceAccess>, ApiError> {
    Ok(Json(s.app.workspace_access(&a, w).await?))
}
