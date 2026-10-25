//! OpenAPI is derived from Rust wire DTOs and the handlers registered by this API.
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::OpenApi;
use uuid::Uuid;

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct SetupStatus {
    pub setup_required: bool,
}
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Capabilities {
    pub api_version: String,
    pub version: String,
    pub features: Features,
    pub limits: Limits,
}
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Features {
    pub issues: bool,
    pub projects: bool,
    pub attachments: bool,
    pub ai: bool,
    pub agents: bool,
    pub workspace_settings: bool,
}
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Limits {
    pub title_characters: u32,
    pub page_size: u32,
    pub description_bytes: u32,
    pub max_upload_bytes: i64,
    pub workspace_quota_bytes: i64,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityEntry {
    pub id: Uuid,
    pub event_kind: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub changes: ActivityChanges,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_ids: Option<LabelSetChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<AssigneeChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_id: Option<StatusChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_names: Option<ActivityTextChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<ActivityNumberChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<ActivityNullableNumberChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<ActivityDateChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<ActivityBooleanChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<Uuid>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct LabelSetChange {
    pub before: Vec<Uuid>,
    pub after: Vec<Uuid>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct AssigneeChange {
    #[schema(required = true)]
    pub before: Option<Uuid>,
    #[schema(required = true)]
    pub after: Option<Uuid>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct StatusChange {
    pub before: Uuid,
    pub after: Uuid,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityTextChange {
    pub before: String,
    pub after: String,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityNumberChange {
    pub before: i16,
    pub after: i16,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityNullableNumberChange {
    #[schema(required = true)]
    pub before: Option<i32>,
    #[schema(required = true)]
    pub after: Option<i32>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityDateChange {
    #[schema(required = true)]
    pub before: Option<NaiveDate>,
    #[schema(required = true)]
    pub after: Option<NaiveDate>,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityBooleanChange {
    pub before: bool,
    pub after: bool,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityPage {
    pub data: Vec<ActivityEntry>,
}
/// Omitted fields are unchanged; null clears only assignee, estimate and due date.
#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[schema(min_properties = 1)]
pub(crate) struct IssuePatch {
    #[schema(nullable = false, min_length = 1, max_length = 256)]
    pub title: Option<String>,
    #[schema(nullable = false)]
    pub description: Option<String>,
    #[schema(nullable = false)]
    pub status_id: Option<Uuid>,
    #[schema(nullable = false, minimum = 0, maximum = 4)]
    pub priority: Option<i16>,
    pub assignee_id: Option<Uuid>,
    #[schema(nullable = false, max_items = 50)]
    pub label_ids: Option<Vec<Uuid>>,
    #[schema(minimum = 0, maximum = 1000)]
    pub estimate: Option<i32>,
    pub due_date: Option<NaiveDate>,
    #[schema(nullable = false)]
    pub archived: Option<bool>,
}
#[derive(OpenApi)]
#[openapi(
    info(
        title = "OpenTrack API",
        version = "0.1.0",
        description = "Implemented OpenTrack endpoints. Features absent from this contract are not advertised as available. Browser-cookie writes require allowed Origin and CSRF; native bearer sessions have a separate audience and do not use browser CSRF. Data returned by a conflict is authorized for the current caller."
    ),
    paths(
        crate::preferences::get_preferences,
        crate::preferences::update_preferences,
        crate::mentions::candidates,
        crate::mentions::issue_reference_candidates,
        crate::mentions::subscription,
        crate::mentions::update_subscription,
        crate::cycles::list,
        crate::cycles::create,
        crate::cycles::detail,
        crate::cycles::delete,
        crate::cycles::update,
        crate::cycles::membership,
        crate::cycles::scope,
        crate::cycles::issues,
        crate::cycles::open,
        crate::cycles::reopen,
        crate::cycles::preview,
        crate::cycles::close,
        crate::cycles::report,
        crate::cycles::snapshots,
        crate::cycles::capture,
        crate::cycles::carry,
        crate::cycles::carry_detail,
        crate::cycles::resume,
        crate::projects::list,
        crate::milestones::list,
        crate::milestones::create,
        crate::milestones::detail,
        crate::milestones::update,
        crate::milestones::delete,
        crate::milestones::progress,
        crate::milestones::order,
        crate::milestones::assign,
        crate::projects::create,
        crate::projects::detail,
        crate::projects::update,
        crate::projects::metadata,
        crate::projects::update_metadata,
        crate::projects::document,
        crate::projects::progress,
        crate::projects::issues,
        crate::projects::membership,
        crate::projects::updates,
        crate::projects::publish,
        crate::projects::update_detail,
        crate::templates::list,
        crate::templates::detail,
        crate::templates::preview,
        crate::templates::create,
        crate::templates::update,
        super::native::capabilities,
        super::native::start,
        super::native::consent,
        super::native::approve,
        super::native::exchange,
        super::setup_status,
        super::setup,
        super::login,
        super::session,
        super::logout,
        super::account::sessions,
        super::account::revoke_session,
        super::account::revoke_others,
        super::account::complete_recovery,
        super::capabilities,
        super::bootstrap,
        super::list_issues,
        super::get_issue,
        super::create_issue,
        super::update_issue,
        super::activity,
        super::attachments::reserve,
        super::attachments::list,
        super::attachments::metadata,
        super::attachments::upload,
        super::attachments::finalize,
        super::attachments::download,
        super::attachments::delete,
        super::documents::issue_document,
        super::documents::document,
        super::documents::update_document,
        super::documents::revisions,
        super::documents::revision,
        super::documents::comments,
        super::documents::comment,
        super::documents::create_comment,
        super::documents::update_comment,
        super::documents::delete_comment,
        super::documents::activity_page,
        super::deletion::preview,
        super::deletion::delete,
        super::deletion::list_deleted,
        super::deletion::deleted,
        super::deletion::restore,
        super::management::organizations,
        super::management::create_workspace,
        super::management::workspace,
        super::management::update_workspace,
        super::management::teams,
        super::management::create_team,
        super::management::update_team,
        super::management::members,
        super::management::update_member,
        super::management::team_members,
        super::management::put_team_member,
        super::management::remove_team_member,
        super::management::invitations,
        super::management::create_invitation,
        super::management::revoke_invitation,
        super::management::invitation_preview,
        super::management::accept_invitation,
        super::management::get_team,
        super::management::get_workspace_member,
        super::management::get_team_member,
        super::management::get_invitation,
        super::management::workspace_access,
        super::workflows::workflow,
        super::workflows::statuses,
        super::workflows::status,
        super::workflows::create_status,
        super::workflows::update_status,
        super::workflows::set_default,
        super::workflows::reorder,
        super::workflows::archive_status,
        super::workflows::restore_status,
        super::workflows::replacement_preview,
        super::workflows::replace_status,
        super::workflows::replacement,
        super::workflows::resume_replacement,
        super::issue_ordering::position_issue,
        super::issue_ordering::issue_order_maintenance,
        super::issue_ordering::resume_issue_order_maintenance,
        super::taxonomy::labels,
        super::taxonomy::label,
        super::taxonomy::create_label,
        super::taxonomy::update_label,
        super::taxonomy::assignees,
        super::graph::lookup,
        super::graph::by_key,
        super::graph::parent,
        super::graph::set_parent,
        super::graph::children,
        super::graph::relations,
        super::graph::relation,
        super::graph::create_relation,
        super::graph::remove_relation,
        super::graph::destinations,
        super::graph::move_preview,
        super::graph::move_issue
    ),
    components(schemas(
        super::problem::Problem,
        opentrack_domain::User,
        opentrack_domain::Workspace,
        opentrack_domain::Team,
        opentrack_domain::Status
    ))
)]
struct ApiDoc;

pub fn openapi() -> Value {
    let mut doc = serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI is serializable");
    // Track package metadata changes in cached builds, including macro-generated schemas.
    let license = env!("CARGO_PKG_LICENSE");
    doc["info"]["license"] = json!({ "name": license, "identifier": license });
    doc["components"]["securitySchemes"] = json!({
        "cookieAuth":{"type":"apiKey","in":"cookie","name":"opentrack_session"},
        "csrfToken":{"type":"apiKey","in":"header","name":"X-CSRF-Token"},
        "nativeBearer":{"type":"http","scheme":"bearer","description":"Opaque session credential with desktop audience; browser session tokens are rejected."}
    });
    doc["security"] = json!([{"cookieAuth":[]},{"nativeBearer":[]}]);
    doc["components"]["schemas"]["IssuePatch"]["additionalProperties"] = json!(false);
    for (path, item) in doc["paths"].as_object_mut().expect("OpenAPI paths") {
        for (method, operation) in item.as_object_mut().expect("Path object") {
            if !["get", "post", "patch", "put", "delete"].contains(&method.as_str()) {
                continue;
            }
            let authenticated = operation.get("security").is_none();
            let native_start = path == "/api/v1/auth/native/requests";
            let native_exchange = path == "/api/v1/auth/native/exchange";
            let consent = path.starts_with("/api/v1/auth/native/requests/");
            if native_start {
                operation["security"] = json!([{}, {"nativeBearer":[]}]);
            } else if consent && method == "get" {
                operation["security"] = json!([{"cookieAuth":[]}]);
            }
            if method != "get" && !native_start && !native_exchange {
                if authenticated {
                    operation["security"] = if consent {
                        json!([{"cookieAuth":[],"csrfToken":[]}])
                    } else {
                        json!([{"cookieAuth":[],"csrfToken":[]},{"nativeBearer":[]}])
                    };
                }
                let parameters = operation
                    .as_object_mut()
                    .expect("Operation")
                    .entry("parameters")
                    .or_insert(json!([]))
                    .as_array_mut()
                    .expect("Parameters");
                parameters.push(json!({"name":"Origin","in":"header","required":consent || !authenticated,"schema":{"type":"string","format":"uri"},"description":"Required with browser cookies and browser-only public flows; must match an allowed origin. Authenticated native bearer requests do not require it."}));
            }
            let responses = operation["responses"].as_object_mut().expect("Responses");
            for (status, code, detail) in [
                (
                    400,
                    "bad_request",
                    "The request path, query or JSON is invalid",
                ),
                (401, "unauthenticated", "Sign in to continue"),
                (
                    403,
                    "forbidden",
                    "You do not have permission for this action",
                ),
                (404, "not_found", "This item could not be found"),
                (
                    405,
                    "method_not_allowed",
                    "This HTTP method is not supported",
                ),
                (
                    408,
                    "request_timeout",
                    "The request exceeded its time limit",
                ),
                (409, "conflict", "The command conflicts with current state"),
                (
                    412,
                    "version_conflict",
                    "This item changed since you opened it",
                ),
                (
                    413,
                    "payload_too_large",
                    "The request body exceeds the allowed size",
                ),
                (415, "unsupported_media_type", "Send application/json"),
                (
                    422,
                    "validation_failed",
                    "Request fields or types are invalid",
                ),
                (
                    428,
                    "precondition_required",
                    "An If-Match version is required",
                ),
                (429, "rate_limited", "Too many attempts. Try again later"),
                (500, "internal_error", "An internal error occurred"),
                (
                    503,
                    "dependency_unavailable",
                    "Service temporarily unavailable",
                ),
            ] {
                let request_id = "00000000-0000-4000-8000-000000000001";
                let (code, detail) = if status == 422 && path == "/api/v1/auth/recovery/complete" {
                    (
                        "recovery_invalid",
                        "Invalid, expired, used or revoked recovery codes, unknown accounts and disabled accounts return the same recovery_invalid response. Invalid password fields use validation_failed.",
                    )
                } else if status == 422 && (native_exchange || consent && method == "post") {
                    (
                        "native_auth_invalid",
                        "The native sign-in request is invalid, expired or already used; start again.",
                    )
                } else {
                    (code, detail)
                };
                let mut response = json!({"description":detail,"content":{"application/problem+json":{"schema":{"$ref":"#/components/schemas/Problem"},"example":{"type":format!("/api/v1/problems/{code}"),"title":axum::http::StatusCode::from_u16(status).unwrap().canonical_reason().unwrap(),"status":status,"detail":detail,"instance":format!("urn:uuid:{request_id}"),"code":code,"request_id":request_id,"retryable":matches!(status,408|429|503),"field_errors":[]}}},"headers":{"X-Request-ID":{"schema":{"type":"string","format":"uuid"}},"Cache-Control":{"schema":{"type":"string","const":"no-store"}}}});
                if status == 429 {
                    response["headers"]["Retry-After"] = json!({"schema":{"type":"string"},"description":"Seconds before retrying; do not automatically replay an ambiguous write."});
                }
                if status == 405 {
                    response["headers"]["Allow"] = json!({"schema":{"type":"string"}});
                }
                responses.entry(status.to_string()).or_insert(response);
            }
            for (status, response) in responses {
                if path.starts_with("/api/v1/auth/native/") {
                    response["headers"]["Referrer-Policy"] =
                        json!({"schema":{"type":"string","const":"no-referrer"}});
                }
                if !status.starts_with('2') && status != "304" {
                    continue;
                }
                response["headers"]["Cache-Control"] =
                    json!({"schema":{"type":"string","const":"no-store"}});
                if (method == "patch" && path == "/api/v1/workspaces/{w}/members/{u}")
                    || (method == "put" && path == "/api/v1/workspaces/{w}/teams/{t}/members/{u}")
                {
                    response["headers"]["X-OpenTrack-Session-Invalidated"] = json!({"schema":{"type":"string","const":"true"},"description":"Present only when this committed access increase revoked the caller's session. The entity change succeeded; clear authenticated state and sign in again, without replaying the mutation."});
                    response["headers"]["Set-Cookie"] = json!({"schema":{"type":"string"},"description":"Expires the session cookie when the committed access increase revoked the caller's session; absent for another account's change."});
                }
                if method == "post" && path == "/api/v1/auth/sessions/{id}/revoke" {
                    response["headers"]["X-OpenTrack-Session-Invalidated"] = json!({"schema":{"type":"string","const":"true"},"description":"Present only when the caller revoked this current session. Clear local authenticated state and any stored native credential."});
                }
                if (status == "201" && path != "/api/v1/setup" && !native_start) || status == "202"
                {
                    response["headers"]["Location"] =
                        json!({"schema":{"type":"string"},"description":"Canonical resource path"});
                }
                if matches!(method.as_str(), "patch" | "put")
                    || (method == "get"
                        && [
                            "/api/v1/workspaces/{w}",
                            "/api/v1/workspaces/{w}/preferences",
                            "/api/v1/workspaces/{w}/issues/{id}",
                            "/api/v1/workspaces/{w}/issues/{id}/subscription",
                            "/api/v1/workspaces/{w}/issues/{issue}/document",
                            "/api/v1/workspaces/{w}/documents/{document}",
                            "/api/v1/workspaces/{w}/documents/{document}/revisions/{revision}",
                            "/api/v1/workspaces/{w}/comments/{comment}",
                            "/api/v1/workspaces/{w}/issues/by-key/{key}",
                            "/api/v1/workspaces/{w}/issues/{id}/parent",
                            "/api/v1/workspaces/{w}/issues/{id}/children",
                            "/api/v1/workspaces/{w}/issues/{id}/relations",
                            "/api/v1/workspaces/{w}/issues/{id}/relations/{relation}",
                            "/api/v1/workspaces/{w}/labels/{label}",
                            "/api/v1/workspaces/{w}/teams/{t}",
                            "/api/v1/workspaces/{w}/teams/{t}/templates/{id}",
                            "/api/v1/workspaces/{w}/teams/{t}/cycles/{id}",
                            "/api/v1/workspaces/{w}/teams/{t}/cycles/{id}/carry-forward/{operation}",
                            "/api/v1/workspaces/{w}/teams/{t}/projects/{id}",
                            "/api/v1/workspaces/{w}/teams/{t}/projects/{id}/metadata",
                            "/api/v1/workspaces/{w}/teams/{t}/projects/{id}/document",
                            "/api/v1/workspaces/{w}/teams/{t}/projects/{id}/updates/{update}",
                            "/api/v1/workspaces/{w}/teams/{t}/projects/{p}/milestones/{id}",
                            "/api/v1/workspaces/{w}/members/{u}",
                            "/api/v1/workspaces/{w}/teams/{t}/members/{u}",
                            "/api/v1/workspaces/{w}/invitations/{id}",
                        ]
                        .contains(&path.as_str()))
                    || (status == "201" && path != "/api/v1/setup" && !native_start)
                    || path == "/api/v1/workspaces/{w}/teams/{t}/workflow"
                    || path.contains("/workflow-replacements/")
                    || (path.contains("/statuses/{status}/") && method == "post")
                    || (path == "/api/v1/workspaces/{w}/issues/{id}/move" && method == "post")
                    || (path == "/api/v1/workspaces/{w}/issues/{id}/relations/{relation}"
                        && method == "delete")
                {
                    let description = if path == "/api/v1/workspaces/{w}/teams/{t}/projects/{id}/metadata" {
                        "Strong validator for the complete currently authorized metadata representation, independent of the project brief. Send as If-Match on metadata changes."
                    } else if path.ends_with("/revisions/{revision}") {
                        "Strong validator for the complete currently authorized revision representation, including reference labels. Revisions are read-only."
                    } else if method == "get"
                        && [
                            "/api/v1/workspaces/{w}/issues/{id}/parent",
                            "/api/v1/workspaces/{w}/issues/{id}/children",
                            "/api/v1/workspaces/{w}/issues/{id}/relations",
                            "/api/v1/workspaces/{w}/issues/{id}/relations/{relation}",
                        ]
                        .contains(&path.as_str())
                    {
                        "Strong validator for the complete authorized graph projection. Use If-None-Match for reads; mutations require the source issue's own etag."
                    } else {
                        "Strong quoted aggregate version; send as If-Match on later changes."
                    };
                    response["headers"]["ETag"] =
                        json!({"schema":{"type":"string"},"description":description});
                }
            }
        }
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_contract_keeps_content_out_and_nullable_facts_explicit() {
        let doc = openapi();
        let schemas = &doc["components"]["schemas"];
        assert!(schemas["ActivityEntry"]["properties"]["actor_id"].is_object());
        assert!(
            schemas["ActivityChanges"]["properties"]
                .get("title")
                .is_none()
        );
        for name in ["ActivityNullableNumberChange", "ActivityDateChange"] {
            let schema = &schemas[name];
            assert_eq!(schema["required"], json!(["before", "after"]));
        }
        let value = json!({"id":Uuid::nil(),"event_kind":"issue.updated","actor":"OpenTrack","actor_id":null,"created_at":"2026-10-20T12:00:00Z","changes":{"fields":["estimate","due_date"],"estimate":{"before":null,"after":5},"due_date":{"before":null,"after":"2026-11-01"}}});
        let entry: ActivityEntry = serde_json::from_value(value).unwrap();
        assert!(entry.actor_id.is_none());
        assert_eq!(entry.changes.estimate.unwrap().after, Some(5));
    }
    #[test]
    fn preferences_are_caller_owned_and_versioned() {
        let doc = openapi();
        let path = &doc["paths"]["/api/v1/workspaces/{w}/preferences"];
        assert!(path["get"]["responses"]["200"]["headers"]["ETag"].is_object());
        assert!(path["patch"]["responses"]["200"]["headers"]["ETag"].is_object());
        assert!(
            path["patch"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "If-Match" && p["required"] == true)
        );
        assert_eq!(
            doc["components"]["schemas"]["ShortcutMode"]["enum"],
            json!(["enabled", "disabled"])
        );
        assert_eq!(
            doc["components"]["schemas"]["UpdateUserPreferences"]["additionalProperties"],
            false
        );
        assert_eq!(
            doc["components"]["schemas"]["UpdateUserPreferences"]["required"],
            json!(["shortcut_mode"])
        );
    }
    #[test]
    fn project_metadata_and_document_have_distinct_versioned_contracts() {
        let doc = openapi();
        let metadata = &doc["paths"]["/api/v1/workspaces/{w}/teams/{t}/projects/{id}/metadata"];
        for method in ["get", "patch"] {
            assert_eq!(
                metadata[method]["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/ProjectSummary"
            );
            assert!(metadata[method]["responses"]["200"]["headers"]["ETag"].is_object());
        }
        assert!(
            metadata["patch"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|parameter| parameter["name"] == "If-Match" && parameter["required"] == true)
        );
        assert_eq!(
            metadata["patch"]["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/UpdateProject"
        );
        assert!(
            doc["components"]["schemas"]["ProjectSummary"]["properties"]
                .get("narrative")
                .is_none()
        );
        let document = &doc["paths"]["/api/v1/workspaces/{w}/teams/{t}/projects/{id}/document"];
        assert_eq!(
            document["get"]["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Document"
        );
        assert!(document["get"]["responses"]["200"]["headers"]["ETag"].is_object());
        assert!(document.get("patch").is_none());
        assert_ne!(
            metadata["get"]["operationId"],
            document["get"]["operationId"]
        );
    }
    #[test]
    fn mentions_and_private_subscription_contracts_are_explicit() {
        let doc = openapi();
        let subscription = &doc["paths"]["/api/v1/workspaces/{w}/issues/{id}/subscription"];
        assert!(subscription["get"]["responses"]["200"]["headers"]["ETag"].is_object());
        assert!(subscription["patch"]["responses"]["200"]["headers"]["ETag"].is_object());
        assert!(
            subscription["patch"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "If-Match" && p["required"] == true)
        );
        for schema in [
            &doc["components"]["schemas"]["Document"],
            &doc["components"]["schemas"]["DocumentRevision"]["allOf"][1],
        ] {
            assert!(
                schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("mentions"))
            );
        }
        assert_eq!(
            doc["components"]["schemas"]["SubscriptionSource"]["enum"],
            json!(["creator", "assignment", "mention"])
        );
    }
    #[test]
    fn issue_references_are_required_current_access_projections() {
        let doc = openapi();
        assert!(doc["paths"]["/api/v1/workspaces/{w}/issue-reference-candidates"]["get"]["responses"]["200"].is_object());
        for schema in [
            &doc["components"]["schemas"]["Document"],
            &doc["components"]["schemas"]["DocumentRevision"]["allOf"][1],
        ] {
            assert!(
                schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("issue_references"))
            );
        }
        for field in ["id", "available", "key", "title"] {
            assert!(
                doc["components"]["schemas"]["IssueReference"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(field))
            );
        }
        assert!(
            doc["paths"]["/api/v1/workspaces/{w}/documents/{document}/revisions/{revision}"]["get"]
                ["responses"]["200"]["headers"]["ETag"]
                .is_object()
        );
    }
    #[test]
    fn native_contract_keeps_bearer_and_browser_consent_separate() {
        let doc = openapi();
        let paths = &doc["paths"];
        assert_eq!(
            doc["security"],
            json!([{"cookieAuth":[]},{"nativeBearer":[]}])
        );
        let start = &paths["/api/v1/auth/native/requests"]["post"];
        assert_eq!(start["security"], json!([{}, {"nativeBearer":[]}]));
        assert!(
            start["responses"]["201"]["headers"]
                .get("Location")
                .is_none()
        );
        assert!(start["responses"]["201"]["headers"].get("ETag").is_none());
        assert_eq!(
            paths["/api/v1/auth/native/requests/{id}"]["get"]["security"],
            json!([{"cookieAuth":[]} ])
        );
        assert_eq!(
            paths["/api/v1/auth/native/requests/{id}/approve"]["post"]["security"],
            json!([{"cookieAuth":[],"csrfToken":[]} ])
        );
        for path in [
            "/api/v1/auth/native/requests",
            "/api/v1/auth/native/exchange",
        ] {
            let operation = &paths[path]["post"];
            assert!(
                !operation["parameters"]
                    .as_array()
                    .is_some_and(|parameters| parameters.iter().any(|p| p["name"] == "Origin"))
            );
            for (_, response) in operation["responses"].as_object().unwrap() {
                assert_eq!(
                    response["headers"]["Referrer-Policy"]["schema"]["const"],
                    "no-referrer"
                );
            }
        }
        for schema in ["NativeAuthStart", "NativeAuthApprove", "NativeAuthExchange"] {
            assert_eq!(
                doc["components"]["schemas"][schema]["additionalProperties"],
                false
            );
        }
    }
    #[test]
    fn graph_contract_distinguishes_projection_validators_and_required_repairs() {
        let doc = openapi();
        let schemas = &doc["components"]["schemas"];
        assert!(schemas["Issue"]["properties"].get("parent_id").is_none());
        for (schema, field) in [
            ("SetIssueParent", "parent_id"),
            ("MoveIssue", "assignee_id"),
            ("MoveIssue", "label_ids"),
            ("ParentLink", "target"),
            ("IssueRelation", "target"),
        ] {
            assert!(
                schemas[schema]["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == field)
            );
        }
        for schema in ["SetIssueParent", "CreateIssueRelation", "MoveIssue"] {
            assert_eq!(schemas[schema]["additionalProperties"], false);
        }
        assert_eq!(
            schemas["MoveIssue"]["properties"]["label_ids"]["maxItems"],
            50
        );
        let paths = &doc["paths"];
        for suffix in ["parent", "children", "relations", "relations/{relation}"] {
            let get = &paths[format!("/api/v1/workspaces/{{w}}/issues/{{id}}/{suffix}")]["get"];
            assert!(get["responses"]["304"]["headers"]["ETag"].is_object());
            assert!(
                get["responses"]["200"]["headers"]["ETag"]["description"]
                    .as_str()
                    .unwrap()
                    .contains("source issue")
            );
        }
        let created = &paths["/api/v1/workspaces/{w}/issues/{id}/relations"]["post"];
        assert!(created["responses"]["201"]["headers"]["Location"].is_object());
        assert_eq!(
            created["security"],
            json!([{"cookieAuth":[],"csrfToken":[]},{"nativeBearer":[]}])
        );
        let moved = &paths["/api/v1/workspaces/{w}/issues/{id}/move"]["post"];
        assert!(moved["responses"]["200"]["headers"]["ETag"].is_object());
        assert!(
            moved["responses"]["200"]["headers"]
                .get("Location")
                .is_none()
        );
        assert!(
            paths["/api/v1/workspaces/{w}/issues/{id}/relations/{relation}"]["delete"]["responses"]
                ["200"]["headers"]["ETag"]
                .is_object()
        );
        assert!(paths["/api/v1/workspaces/{w}/issues/by-key/{key}"]["get"]["responses"]["200"]["headers"].get("Location").is_none());
    }

    #[test]
    fn template_detail_has_a_validator_but_compound_preview_does_not() {
        let doc = openapi();
        let paths = &doc["paths"];
        let detail = &paths["/api/v1/workspaces/{w}/teams/{t}/templates/{id}"]["get"];
        assert!(detail["responses"]["200"]["headers"]["ETag"].is_object());
        let preview = &paths["/api/v1/workspaces/{w}/teams/{t}/templates/{id}/preview"]["get"];
        assert!(preview["responses"]["200"]["headers"].get("ETag").is_none());
        assert!(preview["responses"].get("304").is_none());
        assert!(
            doc["components"]["schemas"]["TemplateDefaults"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "status_id")
        );
    }

    #[test]
    fn taxonomy_contract_describes_wire_sets_and_reference_metadata() {
        let doc = openapi();
        let schemas = &doc["components"]["schemas"];
        for field in ["assignee", "label_ids", "labels", "etag"] {
            assert!(
                schemas["Issue"]["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == field)
            );
        }
        assert_eq!(schemas["Issue"]["properties"]["etag"]["readOnly"], true);
        assert_eq!(
            schemas["IssuePatch"]["properties"]["label_ids"]["type"],
            "array"
        );
        assert_eq!(
            schemas["IssuePatch"]["properties"]["label_ids"]["maxItems"],
            50
        );
        for schema in ["CreateLabel", "UpdateLabel"] {
            assert_eq!(schemas[schema]["additionalProperties"], false);
        }
        let paths = &doc["paths"];
        let query = paths["/api/v1/workspaces/{w}/issues"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["name"] == "label_ids")
            .unwrap();
        assert_eq!(query["style"], "form");
        assert_eq!(query["explode"], false);
        assert_eq!(query["schema"]["maxItems"], 50);
        let label = &paths["/api/v1/workspaces/{w}/labels/{label}"];
        assert!(label["get"]["responses"]["200"]["headers"]["ETag"].is_object());
        assert_eq!(
            label["patch"]["security"],
            json!([{"cookieAuth":[], "csrfToken":[]},{"nativeBearer":[]}])
        );
        assert!(paths["/api/v1/workspaces/{w}/labels"]["post"]["responses"]["201"]["headers"]["Location"].is_object());
        assert_eq!(
            schemas["AssignableUser"]["properties"]
                .as_object()
                .unwrap()
                .len(),
            2
        );
        assert!(
            schemas["AssigneePage"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "self_assignable")
        );
    }

    #[test]
    fn contract_preserves_nullable_counters_and_security_boundaries() {
        let doc = openapi();
        assert!(doc["openapi"].as_str().unwrap().starts_with("3.1."));
        let schemas = &doc["components"]["schemas"];
        assert_eq!(schemas["Issue"]["properties"]["version"]["type"], "string");
        assert!(
            schemas["Issue"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "assignee_id")
        );
        assert_eq!(schemas["IssuePatch"]["additionalProperties"], false);
        let paths = &doc["paths"];
        assert_eq!(paths["/api/v1/auth/login"]["post"]["security"], json!([{}]));
        assert_eq!(
            paths["/api/v1/auth/recovery/complete"]["post"]["security"],
            json!([{}])
        );
        assert_eq!(
            paths["/api/v1/workspaces/{w}/issues"]["post"]["security"],
            json!([{"cookieAuth":[],"csrfToken":[]},{"nativeBearer":[]}])
        );
        assert!(paths["/api/v1/workspaces/{w}/issues/{id}"]["patch"]["responses"]["412"]["content"]["application/problem+json"].is_object());
        assert_eq!(
            schemas["InvitationIssued"]["properties"]["token"]["type"],
            json!(["string", "null"])
        );
        let session_fields = schemas["SessionInfo"]["properties"].as_object().unwrap();
        assert_eq!(session_fields.len(), 9);
        assert_eq!(
            session_fields["device_name"]["type"],
            json!(["string", "null"])
        );
        let required = schemas["SessionInfo"]["required"].as_array().unwrap();
        for field in ["audience", "device_name"] {
            assert!(required.iter().any(|value| value == field));
        }
        assert_eq!(
            schemas["SessionAudience"]["enum"],
            json!(["web", "desktop"])
        );
        for secret in ["token_hash", "csrf_token", "password_hash", "user_id"] {
            assert!(!session_fields.contains_key(secret));
        }
        assert_eq!(
            paths["/api/v1/auth/sessions/{id}/revoke"]["post"]["security"],
            json!([{"cookieAuth":[],"csrfToken":[]},{"nativeBearer":[]}])
        );
        assert_eq!(
            paths["/api/v1/workspaces/{w}/members/{u}"]["patch"]["responses"]["200"]["headers"]["X-OpenTrack-Session-Invalidated"]
                ["schema"]["const"],
            "true"
        );
        assert!(
            paths["/api/v1/workspaces/{w}/members/{u}"]["get"]["responses"]["200"]["headers"]
                .get("X-OpenTrack-Session-Invalidated")
                .is_none()
        );
    }
}

#[cfg(test)]
mod cycle_contract_tests {
    use super::openapi;
    #[test]
    fn cycle_operations_have_distinct_client_names_and_required_review_fields() {
        let doc = openapi();
        let mut ids = std::collections::HashSet::new();
        let mut count = 0;
        for (path, methods) in doc["paths"].as_object().unwrap() {
            if !path.contains("/cycles") {
                continue;
            }
            for (_, op) in methods.as_object().unwrap() {
                if let Some(id) = op["operationId"].as_str() {
                    assert!(id.starts_with("cycles_"));
                    assert!(ids.insert(id));
                    count += 1;
                }
            }
        }
        assert_eq!(count, 18);
        for schema in [
            "CloseCycle",
            "CycleMembership",
            "CarryForwardCycle",
            "ResumeCycleCarry",
        ] {
            assert_eq!(
                doc["components"]["schemas"][schema]["additionalProperties"],
                false
            );
        }
        assert!(
            doc["components"]["schemas"]["CycleSnapshot"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "redacted")
        );
    }
}
