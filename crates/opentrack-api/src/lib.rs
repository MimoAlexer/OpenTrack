mod account;
mod attachments;
mod preferences;
mod projects;
pub use attachments::AttachmentService;
mod admission;
mod contract;
mod credentials;
mod deletion;
mod documents;
mod graph;
mod management;
mod mentions;
mod native;
mod problem;
mod taxonomy;
mod templates;
mod workflows;
use axum::{
    Json, Router,
    body::Body,
    extract::Request,
    extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
pub use contract::openapi;
use opentrack_app::App;
use opentrack_domain::{Actor, CreateIssue, Error, Issue, IssueFilter, Login, Setup};
use problem::{ApiError, payload};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiState {
    pub app: App,
    pub attachments: Option<Arc<AttachmentService>>,
    pub origins: Arc<Vec<String>>,
    pub secure_cookie: bool,
    pub static_dir: Arc<PathBuf>,
}
pub fn router(state: ApiState) -> Router {
    let api = Router::new()
        .route("/openapi.json", get(|| async { Json(openapi()) }))
        .route("/problems/{code}", get(problem_documentation))
        .route("/setup/status", get(setup_status))
        .route("/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/session", get(session))
        .route("/auth/logout", post(logout))
        .merge(account::routes())
        .merge(native::routes())
        .route("/capabilities", get(capabilities))
        .route("/workspaces/{w}/bootstrap", get(bootstrap))
        .route(
            "/workspaces/{w}/issues",
            get(list_issues).post(create_issue),
        )
        .route(
            "/workspaces/{w}/issues/{id}",
            get(get_issue).patch(update_issue).delete(deletion::delete),
        )
        .route("/workspaces/{w}/issues/{id}/activity", get(activity))
        .merge(management::routes())
        .merge(taxonomy::routes())
        .merge(templates::routes())
        .merge(projects::routes())
        .merge(mentions::routes())
        .merge(preferences::routes())
        .merge(graph::routes())
        .merge(deletion::routes())
        .merge(documents::routes())
        .merge(attachments::routes())
        .merge(workflows::routes())
        .fallback(not_found)
        .layer(middleware::from_fn(problem::normalize))
        .layer(middleware::from_fn_with_state(
            Arc::new(admission::Admission::default()),
            admission::limit_auth,
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ));
    let assets = Router::new()
        .fallback_service(ServeDir::new(state.static_dir.join("assets")))
        .layer(middleware::from_fn(asset_cache));
    // This prepaint script is deliberately unhashed and must revalidate after
    // deployment. Keep its exact route separate from immutable bundled assets.
    let theme = Router::new()
        .route_service(
            "/theme.js",
            ServeFile::new(state.static_dir.join("theme.js")),
        )
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ));
    Router::new().nest("/api/v1",api)
        .route("/health/live",get(live))
        .route("/health/ready",get(ready))
        .nest("/assets",assets)
        .merge(theme)
        .fallback(spa)
        .layer(DefaultBodyLimit::max(1024*1024))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS,HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY,HeaderValue::from_static("same-origin")))
        .layer(SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY,HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'")))
        .layer(middleware::from_fn(request_deadline))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn(problem::request_context))
        .layer(middleware::from_fn(native::privacy_headers))
        .with_state(state)
}
struct Auth(Actor);
impl FromRequestParts<ApiState> for Auth {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Auth(
            credentials::authenticate(state, &parts.headers).await?,
        ))
    }
}
fn origin(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiError> {
    let value = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::Forbidden)?;
    if !state.origins.iter().any(|o| o == value) {
        return Err(Error::Forbidden.into());
    }
    Ok(())
}
fn csrf(state: &ApiState, headers: &HeaderMap, actor: &Actor) -> Result<(), ApiError> {
    if actor.audience == opentrack_domain::SessionAudience::Desktop {
        return Ok(());
    }
    origin(state, headers)?;
    let token = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::Forbidden)?;
    if !opentrack_app::secret_matches(token, &actor.csrf_token) {
        return Err(Error::Forbidden.into());
    }
    Ok(())
}
fn cookie_response<T: serde::Serialize>(
    state: &ApiState,
    data: T,
    token: &str,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let mut response = (status, Json(data)).into_response();
    let suffix = if state.secure_cookie { "; Secure" } else { "" };
    let value = format!(
        "opentrack_session={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age=604800{suffix}"
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).map_err(|_| Error::Internal)?,
    );
    Ok(response)
}
fn issue_response(issue: Issue, status: StatusCode) -> Result<Response, ApiError> {
    let tag = issue.etag();
    let location = format!(
        "/api/v1/workspaces/{}/issues/{}",
        issue.workspace_id, issue.id
    );
    let mut response = (status, Json(issue)).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&tag).map_err(|_| Error::Internal)?,
    );
    if status == StatusCode::CREATED {
        response.headers_mut().insert(
            header::LOCATION,
            HeaderValue::from_str(&location).map_err(|_| Error::Internal)?,
        );
    }
    Ok(response)
}
#[utoipa::path(get,path="/api/v1/setup/status",security(()),responses((status=200,body=contract::SetupStatus)))]
async fn setup_status(State(s): State<ApiState>) -> Result<Json<contract::SetupStatus>, ApiError> {
    Ok(Json(contract::SetupStatus {
        setup_required: s.app.setup_required().await?,
    }))
}
#[utoipa::path(post,path="/api/v1/setup",security(()),request_body=Setup,responses((status=201,body=opentrack_domain::Session)))]
async fn setup(
    State(s): State<ApiState>,
    headers: HeaderMap,
    body: Result<Json<Setup>, JsonRejection>,
) -> Result<Response, ApiError> {
    origin(&s, &headers)?;
    credentials::browser_cookie(&headers)?;
    let (data, token) = s.app.setup(payload(body)?).await?;
    cookie_response(&s, data, &token, StatusCode::CREATED)
}
#[utoipa::path(post,path="/api/v1/auth/login",security(()),request_body=Login,responses((status=200,body=opentrack_domain::Session)))]
async fn login(
    State(s): State<ApiState>,
    headers: HeaderMap,
    body: Result<Json<Login>, JsonRejection>,
) -> Result<Response, ApiError> {
    origin(&s, &headers)?;
    let previous = credentials::browser_cookie(&headers)?;
    let (data, token) = s.app.login_with_previous(payload(body)?, previous).await?;
    cookie_response(&s, data, &token, StatusCode::OK)
}
#[utoipa::path(get,path="/api/v1/auth/session",responses((status=200,body=opentrack_domain::Session)))]
async fn session(
    State(s): State<ApiState>,
    Auth(a): Auth,
) -> Result<Json<opentrack_domain::Session>, ApiError> {
    Ok(Json(s.app.session(&a).await?))
}
#[utoipa::path(post,path="/api/v1/auth/logout",responses((status=204,description="Session revoked and cookie cleared")))]
async fn logout(
    State(s): State<ApiState>,
    Auth(a): Auth,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    s.app.logout(&a).await?;
    Ok(if a.audience == opentrack_domain::SessionAudience::Web {
        clear_cookie_response(&s)
    } else {
        StatusCode::NO_CONTENT.into_response()
    })
}
fn clear_cookie_response(state: &ApiState) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    clear_session_cookie(state, &mut response);
    response
}
fn clear_session_cookie(state: &ApiState, response: &mut Response) {
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(if state.secure_cookie {
            "opentrack_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0; Secure"
        } else {
            "opentrack_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
        }),
    );
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/bootstrap",params(("w"=Uuid,Path)),responses((status=200,body=opentrack_domain::Bootstrap)))]
async fn bootstrap(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
) -> Result<Json<opentrack_domain::Bootstrap>, ApiError> {
    Ok(Json(s.app.bootstrap(&a, w).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues",params(("w"=Uuid,Path),("team_id"=Option<Uuid>,Query),("status_id"=Option<Uuid>,Query),("assignee_id"=Option<Uuid>,Query),("priority"=Option<i16>,Query),("q"=Option<String>,Query),("archived"=Option<bool>,Query),("label_ids"=Option<Vec<Uuid>>,Query,style=Form,explode=false,max_items=50,description="Comma-separated visible label IDs; archived labels are allowed"),("label_mode"=Option<String>,Query,description="any (default) or all"),("sort"=Option<String>,Query,description="created (default) or priority; priority orders 1,2,3,4,0 then newest first"),("limit"=Option<i64>,Query),("cursor"=Option<String>,Query)),responses((status=200,body=opentrack_domain::Page<Issue>)))]
async fn list_issues(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    Query(f): Query<IssueFilter>,
) -> Result<Json<opentrack_domain::Page<Issue>>, ApiError> {
    Ok(Json(s.app.list_issues(&a, w, f).await?))
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=Issue)))]
async fn get_issue(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    issue_response(s.app.get_issue(&a, w, id).await?, StatusCode::OK)
}
#[utoipa::path(get,path="/api/v1/workspaces/{w}/issues/{id}/activity",params(("w"=Uuid,Path),("id"=Uuid,Path)),responses((status=200,body=contract::ActivityPage)))]
async fn activity(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({"data":s.app.issue_activity(&a,w,id).await?})))
}
#[utoipa::path(post,path="/api/v1/workspaces/{w}/issues",params(("w"=Uuid,Path),("Idempotency-Key"=String,Header)),request_body=CreateIssue,responses((status=201,body=Issue)))]
async fn create_issue(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path(w): Path<Uuid>,
    headers: HeaderMap,
    body: Result<Json<CreateIssue>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Invalid("Idempotency-Key is required".into()))?;
    issue_response(
        s.app.create_issue(&a, w, key, payload(body)?).await?,
        StatusCode::CREATED,
    )
}
#[utoipa::path(patch,path="/api/v1/workspaces/{w}/issues/{id}",params(("w"=Uuid,Path),("id"=Uuid,Path),("If-Match"=String,Header)),request_body=contract::IssuePatch,responses((status=200,body=Issue)))]
async fn update_issue(
    State(s): State<ApiState>,
    Auth(a): Auth,
    Path((w, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    csrf(&s, &headers, &a)?;
    let etag = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::PreconditionRequired)?;
    issue_response(
        s.app.update_issue(&a, w, id, etag, payload(body)?).await?,
        StatusCode::OK,
    )
}
async fn not_found() -> ApiError {
    ApiError::from(Error::NotFound)
}
#[utoipa::path(get,path="/api/v1/capabilities",security(()),responses((status=200,body=contract::Capabilities)))]
async fn capabilities(State(s): State<ApiState>) -> Json<contract::Capabilities> {
    Json(contract::Capabilities {
        api_version: "v1".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        features: contract::Features {
            issues: true,
            projects: true,
            attachments: s.attachments.is_some(),
            ai: false,
            agents: false,
            workspace_settings: true,
        },
        limits: contract::Limits {
            title_characters: 256,
            page_size: 100,
            description_bytes: 262144,
            max_upload_bytes: s
                .attachments
                .as_ref()
                .map_or(0, |v| v.limits.max_upload_bytes),
            workspace_quota_bytes: s
                .attachments
                .as_ref()
                .map_or(0, |v| v.limits.workspace_quota_bytes),
        },
    })
}
async fn problem_documentation(Path(code): Path<String>) -> Result<Json<Value>, ApiError> {
    if ![
        "bad_request",
        "unauthenticated",
        "forbidden",
        "not_found",
        "method_not_allowed",
        "request_timeout",
        "conflict",
        "version_conflict",
        "payload_too_large",
        "unsupported_media_type",
        "validation_failed",
        "unsupported_document_schema",
        "cursor_expired",
        "precondition_required",
        "rate_limited",
        "internal_error",
        "dependency_unavailable",
        "request_failed",
        "invite_sign_in_required",
        "reauthentication_required",
        "recovery_invalid",
        "native_auth_invalid",
    ]
    .contains(&code.as_str())
    {
        return Err(Error::NotFound.into());
    }
    Ok(Json(
        json!({"code":code,"format":"RFC 9457 application/problem+json","contract":"/api/v1/openapi.json","guidance":"Read detail for the safe explanation. Preserve drafts on version_conflict; sign in on unauthenticated; reconcile ambiguous mutations before retrying."}),
    ))
}
async fn live() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"status":"live"})),
    )
}
async fn ready(State(s): State<ApiState>) -> Response {
    let (status, value) = if s.app.ready().await {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
    };
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"status":value})),
    )
        .into_response()
}
fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| {
            value.split(',').any(|range| {
                let mut parts = range.trim().split(';');
                if !parts
                    .next()
                    .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/html"))
                {
                    return false;
                }
                let mut quality = None;
                for parameter in parts {
                    let Some((name, value)) = parameter.trim().split_once('=') else {
                        return false;
                    };
                    if name.trim().is_empty() || value.trim().is_empty() {
                        return false;
                    }
                    if !name.trim().eq_ignore_ascii_case("q") {
                        continue;
                    }
                    let value = value.trim();
                    let valid = matches!(value, "0" | "1")
                        || value.split_once('.').is_some_and(|(whole, fraction)| {
                            matches!(whole, "0" | "1")
                                && fraction.len() <= 3
                                && fraction.bytes().all(|digit| {
                                    digit.is_ascii_digit() && (whole == "0" || digit == b'0')
                                })
                        });
                    if quality.is_some() || !valid {
                        return false;
                    }
                    quality = Some(value.bytes().any(|digit| matches!(digit, b'1'..=b'9')));
                }
                quality.unwrap_or(true)
            })
        })
}
async fn spa(State(s): State<ApiState>, uri: Uri, method: Method, headers: HeaderMap) -> Response {
    if !matches!(method, Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if !accepts_html(&headers)
        || ["/api", "/assets", "/health"].iter().any(|prefix| {
            uri.path() == *prefix
                || uri
                    .path()
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
        || uri
            .path()
            .rsplit('/')
            .next()
            .is_some_and(|p| p.contains('.'))
    {
        return ApiError::from(Error::NotFound).into_response();
    }
    match tokio::fs::read(s.static_dir.join("index.html")).await {
        Ok(content) => {
            let length = content.len();
            let mut response = (
                [
                    (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                    (header::CACHE_CONTROL, "no-cache"),
                    (header::VARY, "Accept"),
                ],
                if method == Method::HEAD {
                    Body::empty()
                } else {
                    Body::from(content)
                },
            )
                .into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_LENGTH, HeaderValue::from(length));
            response
        }
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CACHE_CONTROL, "no-store")],
            "The web client is not available. Build web/ before starting the server.",
        )
            .into_response(),
    }
}
pub async fn serve(
    listener: tokio::net::TcpListener,
    state: ApiState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    use std::future::IntoFuture;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let server = axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        let _ = sender.send(());
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result=&mut server=>result,
        _=receiver=>tokio::time::timeout(std::time::Duration::from_secs(30),&mut server).await.unwrap_or_else(|_|Err(std::io::Error::new(std::io::ErrorKind::TimedOut,"Shutdown grace elapsed"))),
    }
}
async fn asset_cache(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let value = if response.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    response
}
async fn request_deadline(request: Request, next: Next) -> Response {
    let duration = if attachments::is_transfer(request.method(), request.uri().path()) {
        180
    } else {
        15
    };
    match tokio::time::timeout(std::time::Duration::from_secs(duration), next.run(request)).await {
        Ok(response) => response,
        Err(_) => problem::problem_response(
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            "The request exceeded its time limit".into(),
            None,
        ),
    }
}
