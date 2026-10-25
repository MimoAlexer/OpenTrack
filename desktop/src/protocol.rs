//! Validation occurs before any credential lookup or network request.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct NativeError {
    pub code: &'static str,
    pub message: &'static str,
    pub retryable: bool,
}
impl NativeError {
    pub const fn new(code: &'static str, message: &'static str, retryable: bool) -> Self {
        Self {
            code,
            message,
            retryable,
        }
    }
    pub const fn network() -> Self {
        Self::new(
            "network_error",
            "The server could not be reached. Check the connection before trying again.",
            true,
        )
    }
    pub const fn storage() -> Self {
        Self::new(
            "credential_storage",
            "The system credential store is unavailable. Unlock it and try again.",
            true,
        )
    }
    pub const fn server() -> Self {
        Self::new(
            "invalid_server_response",
            "The server returned an unsupported response.",
            false,
        )
    }
    pub const fn invalid() -> Self {
        Self {
            code: "invalid_request",
            message: "This desktop request is not supported.",
            retryable: false,
        }
    }
    pub const fn stale() -> Self {
        Self {
            code: "stale_generation",
            message: "The connection changed. Refresh before trying again.",
            retryable: false,
        }
    }
}
#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Patch,
    Put,
    Delete,
}
impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiRequest {
    pub expected_generation: String,
    pub method: Method,
    pub path: String,
    pub query: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
}
pub struct CheckedRequest {
    pub generation: u64,
    pub method: Method,
    pub path: String,
    pub query: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
}
pub fn generation(input: &str) -> Result<u64, NativeError> {
    let value = input.parse::<u64>().map_err(|_| NativeError::invalid())?;
    if value.to_string() != input {
        return Err(NativeError::invalid());
    }
    Ok(value)
}
impl ApiRequest {
    pub fn check(self) -> Result<CheckedRequest, NativeError> {
        let generation = generation(&self.expected_generation)?;
        if self.path.len() > 2048
            || !self.path.is_ascii()
            || self.path.contains(['%', '?', '#', '\\'])
            || self.path.bytes().any(|b| b.is_ascii_control())
            || !route_allowed(self.method, &self.path)
        {
            return Err(NativeError::invalid());
        }
        if let Some(query) = &self.query
            && (query.len() > 8192
                || query.starts_with('?')
                || query.contains('#')
                || query.chars().any(char::is_control))
        {
            return Err(NativeError::invalid());
        }
        let mut headers = BTreeMap::new();
        let mut names = BTreeSet::new();
        for (name, value) in self.headers {
            let name = name.to_ascii_lowercase();
            if !matches!(
                name.as_str(),
                "if-match" | "if-none-match" | "idempotency-key"
            ) || !names.insert(name.clone())
                || !value.is_ascii()
                || value.len() > 512
                || value.bytes().any(|b| b.is_ascii_control())
            {
                return Err(NativeError::invalid());
            }
            if name == "idempotency-key" && !(8..=128).contains(&value.len()) {
                return Err(NativeError::invalid());
            }
            headers.insert(name, value);
        }
        let body = self
            .body
            .map(|value| serde_json::to_vec(&value))
            .transpose()
            .map_err(|_| NativeError::invalid())?;
        let body_limit = if self.path.contains("/documents/")
            || self.path.contains("/comments")
            || self.path.ends_with("/issues")
            || self.path.split('/').any(|segment| segment == "templates")
            || project_narrative_request(self.method, &self.path)
        {
            1024 * 1024
        } else {
            65536
        };
        if body.as_ref().is_some_and(|bytes| bytes.len() > body_limit)
            || (self.method == Method::Get && body.is_some())
        {
            return Err(NativeError::invalid());
        }
        Ok(CheckedRequest {
            generation,
            method: self.method,
            path: self.path,
            query: self.query,
            headers,
            body,
        })
    }
}
fn id(input: &str) -> bool {
    Uuid::parse_str(input).is_ok_and(|id| id.to_string() == input)
}
fn key(input: &str) -> bool {
    let Some((prefix, number)) = input.rsplit_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.len() <= 16
        && prefix
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && !number.is_empty()
        && number.len() <= 20
        && number.bytes().all(|b| b.is_ascii_digit())
}
fn project_narrative_request(method: Method, path: &str) -> bool {
    let segments: Vec<_> = path.split('/').collect();
    match segments.as_slice() {
        ["", "api", "v1", "workspaces", _, "teams", _, "projects"] => method == Method::Post,
        ["", "api", "v1", "workspaces", _, "teams", _, "projects", _] => method == Method::Patch,
        _ => false,
    }
}
fn route_allowed(method: Method, path: &str) -> bool {
    use Method::*;
    let segments: Vec<_> = path.split('/').collect();
    match segments.as_slice() {
        ["", "api", "v1", "auth", "session" | "sessions"] => method == Get,
        ["", "api", "v1", "auth", "sessions", "revoke-others"] => method == Post,
        ["", "api", "v1", "auth", "sessions", session, "revoke"] => id(session) && method == Post,
        ["", "api", "v1", "capabilities" | "organizations"] => method == Get,
        ["", "api", "v1", "workspaces"] => method == Post,
        ["", "api", "v1", "workspaces", w, rest @ ..] if id(w) => workspace_route(method, rest),
        _ => false,
    }
}
fn workspace_route(method: Method, path: &[&str]) -> bool {
    use Method::*;
    match path {
        [] => matches!(method, Get | Patch),
        ["access" | "bootstrap"] => method == Get,
        ["invitations" | "issues" | "labels" | "teams"] => matches!(method, Get | Post),
        ["members" | "mention-candidates" | "issue-reference-candidates"] => method == Get,
        ["attachments"] => matches!(method, Get | Post),
        ["attachments", attachment] => id(attachment) && matches!(method, Get | Delete),
        ["attachments", attachment, "finalize"] => id(attachment) && method == Post,
        ["invitations", invitation] => id(invitation) && method == Get,
        ["invitations", invitation, "revoke"] => id(invitation) && method == Post,
        ["labels", label] => id(label) && matches!(method, Get | Patch),
        ["documents", document] => id(document) && matches!(method, Get | Patch),
        ["documents", document, "revisions"] => id(document) && method == Get,
        ["documents", document, "revisions", revision] => {
            id(document)
                && revision
                    .parse::<i64>()
                    .is_ok_and(|n| n > 0 && n.to_string() == *revision)
                && method == Get
        }
        ["comments", comment] => id(comment) && matches!(method, Get | Patch | Delete),
        ["members", member] => id(member) && matches!(method, Get | Patch),
        ["issues", "lookup" | "deleted"] => method == Get,
        ["issues", "by-key", issue_key] => key(issue_key) && method == Get,
        ["issues", issue] => id(issue) && matches!(method, Get | Patch | Delete),
        [
            "issues",
            issue,
            "activity" | "activity-page" | "document" | "children" | "move-destinations"
            | "move-preview" | "deletion" | "deleted",
        ] => id(issue) && method == Get,
        ["issues", issue, "parent"] => id(issue) && matches!(method, Get | Patch),
        ["issues", issue, "relations"] => id(issue) && matches!(method, Get | Post),
        ["issues", issue, "comments"] => id(issue) && matches!(method, Get | Post),
        ["preferences"] => matches!(method, Get | Patch),
        ["issues", issue, "subscription"] => id(issue) && matches!(method, Get | Patch),
        ["issues", issue, "relations", relation] => {
            id(issue) && id(relation) && matches!(method, Get | Delete)
        }
        ["issues", issue, "move" | "restore" | "position"] => id(issue) && method == Post,
        ["teams", team, rest @ ..] if id(team) => team_route(method, rest),
        _ => false,
    }
}
fn team_route(method: Method, path: &[&str]) -> bool {
    use Method::*;
    match path {
        [] => matches!(method, Get | Patch),
        ["assignees" | "members" | "workflow"] => method == Get,
        ["cycles"] => matches!(method, Get | Post),
        ["cycles", cycle, rest @ ..] if id(cycle) => cycle_route(method, rest),
        ["members", member] => id(member) && matches!(method, Get | Put | Delete),
        ["statuses"] => matches!(method, Get | Post),
        ["templates"] => matches!(method, Get | Post),
        ["templates", template] => id(template) && matches!(method, Get | Patch),
        ["templates", template, "preview"] => id(template) && method == Get,
        ["projects"] => matches!(method, Get | Post),
        ["projects", project, rest @ ..] if id(project) => project_route(method, rest),
        ["statuses", status] => id(status) && matches!(method, Get | Patch),
        ["statuses", status, "replacement-preview"] => id(status) && method == Get,
        ["statuses", status, "archive" | "replace" | "restore"] => id(status) && method == Post,
        ["workflow-replacements" | "issue-order-maintenance", job] => id(job) && method == Get,
        [
            "workflow-replacements" | "issue-order-maintenance",
            job,
            "resume",
        ] => id(job) && method == Post,
        ["workflow", "default" | "order"] => method == Put,
        _ => false,
    }
}
pub(crate) fn large_cycle_response(method: Method, path: &str) -> bool {
    if !route_allowed(method, path) {
        return false;
    }
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        [
            "",
            "api",
            "v1",
            "workspaces",
            _,
            "teams",
            _,
            "cycles",
            _,
            "scope" | "close-preview",
        ] => method == Method::Get,
        [
            "",
            "api",
            "v1",
            "workspaces",
            _,
            "teams",
            _,
            "cycles",
            _,
            "reports",
            _,
        ] => method == Method::Get,
        [
            "",
            "api",
            "v1",
            "workspaces",
            _,
            "teams",
            _,
            "cycles",
            _,
            "close" | "snapshots",
        ] => method == Method::Post,
        _ => false,
    }
}
fn cycle_route(method: Method, path: &[&str]) -> bool {
    use Method::*;
    match path {
        [] => matches!(method, Get | Patch | Delete),
        ["scope" | "close-preview" | "issues"] => method == Get,
        ["snapshots"] => matches!(method, Get | Post),
        ["open" | "reopen" | "close" | "carry-forward"] => method == Post,
        ["issues", issue] => id(issue) && method == Put,
        ["reports", report] => id(report) && method == Get,
        ["carry-forward", operation] => id(operation) && method == Get,
        ["carry-forward", operation, "resume"] => id(operation) && method == Post,
        _ => false,
    }
}
fn project_route(method: Method, path: &[&str]) -> bool {
    use Method::*;
    match path {
        [] => matches!(method, Get | Patch),
        ["metadata"] => matches!(method, Get | Patch),
        ["progress" | "issues" | "document"] => method == Get,
        ["issues", issue] => id(issue) && method == Put,
        ["issues", issue, "milestone"] => id(issue) && method == Put,
        ["milestones"] => matches!(method, Get | Post),
        ["milestones", "order"] => method == Put,
        ["milestones", milestone] => id(milestone) && matches!(method, Get | Patch | Delete),
        ["milestones", milestone, "progress"] => id(milestone) && method == Get,
        ["updates"] => matches!(method, Get | Post),
        ["updates", update] => id(update) && method == Get,
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const PROJECT_WORKSPACE: &str = "00000000-0000-0000-0000-000000000001";
    const PROJECT_TEAM: &str = "00000000-0000-0000-0000-000000000002";
    const PROJECT: &str = "aaaaaaaa-0000-0000-0000-000000000003";
    const PROJECT_ISSUE: &str = "00000000-0000-0000-0000-000000000004";

    fn projects_path() -> String {
        format!("/api/v1/workspaces/{PROJECT_WORKSPACE}/teams/{PROJECT_TEAM}/projects")
    }

    #[test]
    fn project_routes_allow_only_scoped_supported_methods() {
        let base = projects_path();
        for (suffix, allowed) in [
            (String::new(), vec![Method::Get, Method::Post]),
            (format!("/{PROJECT}"), vec![Method::Get, Method::Patch]),
            (
                format!("/{PROJECT}/metadata"),
                vec![Method::Get, Method::Patch],
            ),
            (format!("/{PROJECT}/document"), vec![Method::Get]),
            (format!("/{PROJECT}/progress"), vec![Method::Get]),
            (format!("/{PROJECT}/issues"), vec![Method::Get]),
            (
                format!("/{PROJECT}/milestones"),
                vec![Method::Get, Method::Post],
            ),
            (format!("/{PROJECT}/milestones/order"), vec![Method::Put]),
            (
                format!("/{PROJECT}/milestones/{PROJECT_ISSUE}"),
                vec![Method::Get, Method::Patch, Method::Delete],
            ),
            (
                format!("/{PROJECT}/milestones/{PROJECT_ISSUE}/progress"),
                vec![Method::Get],
            ),
            (
                format!("/{PROJECT}/issues/{PROJECT_ISSUE}/milestone"),
                vec![Method::Put],
            ),
            (
                format!("/{PROJECT}/issues/{PROJECT_ISSUE}"),
                vec![Method::Put],
            ),
            (
                format!("/{PROJECT}/updates"),
                vec![Method::Get, Method::Post],
            ),
            (
                format!("/{PROJECT}/updates/{PROJECT_ISSUE}"),
                vec![Method::Get],
            ),
        ] {
            for method in [
                Method::Get,
                Method::Post,
                Method::Patch,
                Method::Put,
                Method::Delete,
            ] {
                assert_eq!(
                    request(method, &format!("{base}{suffix}")).check().is_ok(),
                    allowed.contains(&method),
                    "Unexpected method {} for {suffix}",
                    method.as_str()
                );
            }
        }
    }

    #[test]
    fn project_routes_reject_noncanonical_identifiers_and_unpublished_actions() {
        let base = projects_path();
        let mut denied = vec![
            "/api/v1/projects".into(),
            format!("/api/v1/workspaces/{PROJECT_WORKSPACE}/projects"),
            format!("/api/v1/workspaces/{PROJECT_WORKSPACE}/projects/{PROJECT}"),
            format!("{base}/"),
            format!("{base}/{PROJECT}/"),
            format!("{base}/{PROJECT}/archive"),
            format!("{base}/{PROJECT}/restore"),
            format!("{base}/{PROJECT}/delete"),
            format!("{base}/{PROJECT}/metadata/"),
            format!("{base}/{PROJECT}/metadata/extra"),
            format!("{base}/{PROJECT}/metadata/document"),
            format!("{base}/{PROJECT}/document/"),
            format!("{base}/{PROJECT}/document/revisions"),
            format!("{base}/{PROJECT}/document/attachments"),
            format!("{base}/{PROJECT}/documents"),
            format!("{base}/{PROJECT}/milestones/order/extra"),
            format!("{base}/{PROJECT}/progress/refresh"),
            format!("{base}/{PROJECT}/issues/{PROJECT_ISSUE}/"),
            format!("{base}/{PROJECT}/issues/{PROJECT_ISSUE}/move"),
            format!("{base}/{PROJECT}/updates/{PROJECT_ISSUE}/"),
            format!("{base}/{PROJECT}/updates/{PROJECT_ISSUE}/edit"),
            format!("{base}/{PROJECT}/updates/"),
        ];
        for invalid in [
            "not-a-uuid".to_owned(),
            PROJECT.to_ascii_uppercase(),
            PROJECT.replace('-', ""),
            format!("{{{PROJECT}}}"),
            "..".to_owned(),
            "%2e%2e".to_owned(),
        ] {
            denied.extend([
                format!("/api/v1/workspaces/{invalid}/teams/{PROJECT_TEAM}/projects"),
                format!("/api/v1/workspaces/{PROJECT_WORKSPACE}/teams/{invalid}/projects"),
                format!("{base}/{invalid}"),
                format!("{base}/{invalid}/metadata"),
                format!("{base}/{invalid}/document"),
                format!("{base}/{invalid}/progress"),
                format!("{base}/{invalid}/updates"),
                format!("{base}/{PROJECT}/issues/{invalid}"),
                format!("{base}/{PROJECT}/updates/{invalid}"),
                format!("{base}/{PROJECT}/milestones/{invalid}"),
                format!("{base}/{PROJECT}/milestones/{invalid}/progress"),
                format!("{base}/{PROJECT}/issues/{invalid}/milestone"),
            ]);
        }
        for path in denied {
            for method in [
                Method::Get,
                Method::Post,
                Method::Patch,
                Method::Put,
                Method::Delete,
            ] {
                assert!(
                    request(method, &path).check().is_err(),
                    "Accepted {} {path}",
                    method.as_str()
                );
            }
        }
    }

    #[test]
    fn project_requests_preserve_preconditions_idempotency_and_query() {
        let base = projects_path();
        let mut attach = request(
            Method::Put,
            &format!("{base}/{PROJECT}/issues/{PROJECT_ISSUE}"),
        );
        attach
            .headers
            .insert("If-Match".into(), "\"project:1\"".into());
        attach.body = Some(serde_json::json!({"included":true}));
        let checked = attach.check().unwrap();
        assert_eq!(checked.headers["if-match"], "\"project:1\"");
        assert_eq!(
            serde_json::from_slice::<Value>(&checked.body.unwrap()).unwrap(),
            serde_json::json!({"included":true})
        );
        let mut metadata = request(Method::Patch, &format!("{base}/{PROJECT}/metadata"));
        metadata
            .headers
            .insert("If-Match".into(), "\"project-metadata:1\"".into());
        metadata.body = Some(serde_json::json!({"summary":"Reviewed metadata"}));
        let checked = metadata.check().unwrap();
        assert_eq!(checked.headers["if-match"], "\"project-metadata:1\"");
        assert_eq!(
            serde_json::from_slice::<Value>(&checked.body.unwrap()).unwrap(),
            serde_json::json!({"summary":"Reviewed metadata"})
        );
        let mut update = request(Method::Post, &format!("{base}/{PROJECT}/updates"));
        update
            .headers
            .insert("Idempotency-Key".into(), "project-update-1".into());
        update.body = Some(serde_json::json!({"body":"Milestone review completed."}));
        let checked = update.check().unwrap();
        assert_eq!(checked.headers["idempotency-key"], "project-update-1");
        assert_eq!(
            serde_json::from_slice::<Value>(&checked.body.unwrap()).unwrap(),
            serde_json::json!({"body":"Milestone review completed."})
        );
        let mut listing = request(Method::Get, &base);
        listing.query = Some("state=active&limit=25&cursor=next%2Bpage".into());
        assert_eq!(
            listing.check().unwrap().query.as_deref(),
            Some("state=active&limit=25&cursor=next%2Bpage")
        );
    }

    #[test]
    fn project_narrative_envelopes_are_bounded_without_expanding_other_writes() {
        let base = projects_path();
        for (method, path, limit) in [
            (Method::Post, base.clone(), 1024 * 1024),
            (Method::Patch, format!("{base}/{PROJECT}"), 1024 * 1024),
            (Method::Patch, format!("{base}/{PROJECT}/metadata"), 65536),
            (Method::Post, format!("{base}/{PROJECT}/updates"), 65536),
            (
                Method::Put,
                format!("{base}/{PROJECT}/issues/{PROJECT_ISSUE}"),
                65536,
            ),
        ] {
            let overhead = serde_json::to_vec(&serde_json::json!({"body":""}))
                .unwrap()
                .len();
            let mut at_limit = request(method, &path);
            at_limit.body = Some(serde_json::json!({"body":"x".repeat(limit - overhead)}));
            assert_eq!(
                at_limit.check().unwrap().body.unwrap().len(),
                limit,
                "Envelope limit for {} {path}",
                method.as_str()
            );
            let mut over_limit = request(method, &path);
            over_limit.body = Some(serde_json::json!({"body":"x".repeat(limit - overhead + 1)}));
            assert!(
                over_limit.check().is_err(),
                "Accepted oversized body for {} {path}",
                method.as_str()
            );
        }
        for path in [
            base.clone(),
            format!("{base}/{PROJECT}"),
            format!("{base}/{PROJECT}/metadata"),
            format!("{base}/{PROJECT}/document"),
            format!("{base}/{PROJECT}/progress"),
            format!("{base}/{PROJECT}/issues"),
            format!("{base}/{PROJECT}/updates"),
            format!("{base}/{PROJECT}/updates/{PROJECT_ISSUE}"),
        ] {
            let mut get = request(Method::Get, &path);
            get.body = Some(serde_json::json!({}));
            assert!(get.check().is_err(), "Accepted GET body for {path}");
        }
    }

    #[test]
    fn personal_preferences_allow_only_scoped_get_and_patch() {
        let path = "/api/v1/workspaces/00000000-0000-0000-0000-000000000001/preferences";
        assert!(request(Method::Get, path).check().is_ok());
        let mut patch = request(Method::Patch, path);
        patch
            .headers
            .insert("If-Match".into(), "\"preferences:1\"".into());
        patch.body = Some(serde_json::json!({"shortcut_mode":"disabled"}));
        assert!(patch.check().is_ok());
        for method in [Method::Post, Method::Put, Method::Delete] {
            assert!(request(method, path).check().is_err());
        }
        for suffix in ["/someone", "/reset", "/shortcut_mode"] {
            assert!(
                request(Method::Patch, &format!("{path}{suffix}"))
                    .check()
                    .is_err()
            );
        }
        assert!(
            request(Method::Get, "/api/v1/workspaces/not-an-id/preferences")
                .check()
                .is_err()
        );
    }
    #[test]
    fn candidate_lookup_and_subscription_routes_do_not_authorize_recipient_writes() {
        let w = "00000000-0000-0000-0000-000000000001";
        let issue = "00000000-0000-0000-0000-000000000002";
        let user = "00000000-0000-0000-0000-000000000003";
        let candidates = format!("/api/v1/workspaces/{w}/mention-candidates");
        let subscription = format!("/api/v1/workspaces/{w}/issues/{issue}/subscription");
        let mut lookup = request(Method::Get, &candidates);
        lookup.query = Some(format!("issue_id={issue}&q=Example&limit=20"));
        assert!(lookup.check().is_ok());
        let issue_candidates = format!("/api/v1/workspaces/{w}/issue-reference-candidates");
        let mut lookup = request(Method::Get, &issue_candidates);
        lookup.query = Some(format!("issue_id={issue}&q=TEST&limit=20"));
        assert!(lookup.check().is_ok());
        for method in [Method::Post, Method::Put, Method::Patch, Method::Delete] {
            assert!(
                request(method, &issue_candidates).check().is_err(),
                "Issue-reference lookup must reject {}",
                method.as_str()
            );
        }
        for path in [
            "/api/v1/issue-reference-candidates".into(),
            "/api/v1/workspaces/not-an-id/issue-reference-candidates".into(),
            format!("{issue_candidates}/{issue}"),
            format!("{issue_candidates}/"),
        ] {
            assert!(request(Method::Get, &path).check().is_err(), "{path}");
        }
        let mut lookup_with_body = request(Method::Get, &issue_candidates);
        lookup_with_body.body = Some(serde_json::json!({"q":"TEST"}));
        assert!(lookup_with_body.check().is_err());
        assert!(request(Method::Get, &subscription).check().is_ok());
        let mut change = request(Method::Patch, &subscription);
        change.body = Some(serde_json::json!({"preference":"unsubscribed"}));
        change
            .headers
            .insert("If-Match".into(), "\"subscription:1\"".into());
        assert!(change.check().is_ok());
        for (method, path) in [
            (Method::Post, candidates.clone()),
            (Method::Patch, candidates),
            (Method::Put, subscription.clone()),
            (Method::Post, subscription.clone()),
            (Method::Delete, subscription.clone()),
            (Method::Patch, format!("{subscription}/{user}")),
            (Method::Post, format!("{subscription}/notify")),
            (
                Method::Get,
                format!("/api/v1/workspaces/{w}/issues/not-an-id/subscription"),
            ),
        ] {
            assert!(request(method, &path).check().is_err(), "{path}");
        }
    }
    #[test]
    fn template_routes_are_scoped_and_allow_bounded_document_requests_only() {
        let w = "00000000-0000-0000-0000-000000000001";
        let team = "00000000-0000-0000-0000-000000000002";
        let template = "00000000-0000-0000-0000-000000000003";
        let base = format!("/api/v1/workspaces/{w}/teams/{team}/templates");
        for (path, method) in [
            (base.clone(), Method::Get),
            (base.clone(), Method::Post),
            (format!("{base}/{template}"), Method::Get),
            (format!("{base}/{template}"), Method::Patch),
            (format!("{base}/{template}/preview"), Method::Get),
        ] {
            assert!(request(method, &path).check().is_ok(), "{path}");
        }
        for (path, method) in [
            (format!("/api/v1/workspaces/{w}/templates"), Method::Get),
            (format!("{base}/not-a-uuid"), Method::Patch),
            (format!("{base}/{template}"), Method::Delete),
            (format!("{base}/{template}/preview"), Method::Post),
            (format!("{base}/{template}/instantiate"), Method::Post),
            (format!("{base}/{template}/purge"), Method::Delete),
        ] {
            assert!(request(method, &path).check().is_err(), "{path}");
        }
        let mut create = request(Method::Post, &base);
        create.body = Some(serde_json::json!({"document":{"body":"x".repeat(256*1024)}}));
        assert!(create.check().is_ok());
        let mut oversized = request(Method::Patch, &format!("{base}/{template}"));
        oversized.body = Some(serde_json::json!({"document":{"body":"x".repeat(1024*1024)}}));
        assert!(oversized.check().is_err());
        let mut read = request(Method::Get, &format!("{base}/{template}/preview"));
        read.body = Some(serde_json::json!({}));
        assert!(read.check().is_err());
        let mut unrelated = request(
            Method::Patch,
            &format!("/api/v1/workspaces/{w}/teams/{team}"),
        );
        unrelated.body = Some(serde_json::json!({"name":"x".repeat(65536)}));
        assert!(unrelated.check().is_err());
    }
    #[test]
    fn document_routes_accept_only_scoped_published_operations_and_bounded_bodies() {
        let w = "00000000-0000-0000-0000-000000000001";
        let item = "00000000-0000-0000-0000-000000000002";
        for (suffix, method) in [
            (format!("issues/{item}/document"), Method::Get),
            (format!("issues/{item}/activity-page"), Method::Get),
            (format!("issues/{item}/comments"), Method::Post),
            (format!("documents/{item}"), Method::Patch),
            (format!("documents/{item}/revisions/2"), Method::Get),
            (format!("comments/{item}"), Method::Delete),
        ] {
            assert!(
                request(method, &format!("/api/v1/workspaces/{w}/{suffix}"))
                    .check()
                    .is_ok()
            );
        }
        for (suffix, method) in [
            (format!("issues/{item}/document"), Method::Delete),
            (format!("documents/{item}"), Method::Post),
            (format!("documents/{item}/revisions/0"), Method::Get),
            (format!("documents/{item}/revisions/02"), Method::Get),
            (format!("comments/{item}/purge"), Method::Delete),
        ] {
            assert!(
                request(method, &format!("/api/v1/workspaces/{w}/{suffix}"))
                    .check()
                    .is_err()
            );
        }
        let path = format!("/api/v1/workspaces/{w}/documents/{item}");
        let mut req = request(Method::Patch, &path);
        req.body = Some(serde_json::json!({"body":"x".repeat(256*1024),"schema_version":1}));
        assert!(req.check().is_ok());
        let mut req = request(Method::Patch, &path);
        req.body = Some(serde_json::json!({"body":"x".repeat(1024*1024),"schema_version":1}));
        assert!(req.check().is_err());
    }
    fn request(method: Method, path: &str) -> ApiRequest {
        ApiRequest {
            expected_generation: "1".into(),
            method,
            path: path.into(),
            query: None,
            headers: BTreeMap::new(),
            body: None,
        }
    }
    #[test]
    fn explicit_routes_keep_auth_and_arbitrary_destinations_out() {
        let w = "00000000-0000-0000-0000-000000000001";
        assert!(
            request(Method::Post, &format!("/api/v1/workspaces/{w}/issues"))
                .check()
                .is_ok()
        );
        for path in [
            "https://example.com/api/v1/auth/session",
            "//example.com",
            "/api/v1/auth/login",
            "/api/v1/setup",
            "/api/v1/auth/logout",
            "/api/v1/auth/native/exchange",
            "/api/v1/invitations/accept",
            "/api/v1/auth/recovery/complete",
            "/api/v1/../auth/session",
            "/api/v1/auth%2fsession",
            "/api/v1/auth/session?other=1",
            "/api/v1/auth/session/",
        ] {
            assert!(
                request(Method::Post, path).check().is_err(),
                "Accepted {path}"
            );
        }
        assert!(request(Method::Get, "/api/v1/auth/session").check().is_ok());
    }
    #[test]
    fn deletion_routes_accept_only_the_published_methods_and_scoped_identifiers() {
        let w = "00000000-0000-0000-0000-000000000001";
        let issue = "00000000-0000-0000-0000-000000000002";
        for (suffix, allowed) in [
            ("issues/deleted".to_owned(), vec![Method::Get]),
            (
                format!("issues/{issue}"),
                vec![Method::Get, Method::Patch, Method::Delete],
            ),
            (format!("issues/{issue}/deletion"), vec![Method::Get]),
            (format!("issues/{issue}/deleted"), vec![Method::Get]),
            (format!("issues/{issue}/restore"), vec![Method::Post]),
        ] {
            for method in [
                Method::Get,
                Method::Post,
                Method::Patch,
                Method::Put,
                Method::Delete,
            ] {
                assert_eq!(
                    request(method, &format!("/api/v1/workspaces/{w}/{suffix}"))
                        .check()
                        .is_ok(),
                    allowed.contains(&method),
                    "Unexpected method {} for {suffix}",
                    method.as_str()
                );
            }
        }
        for suffix in [
            "issues/not-a-uuid/restore".to_owned(),
            format!("issues/{issue}/restore/"),
            format!("issues/{issue}/purge"),
            "issues/deleted/restore".to_owned(),
        ] {
            assert!(
                request(Method::Post, &format!("/api/v1/workspaces/{w}/{suffix}"))
                    .check()
                    .is_err()
            );
        }
    }
    #[test]
    fn credentials_and_duplicate_header_case_are_refused() {
        for header in [
            "Authorization",
            "Cookie",
            "Host",
            "Origin",
            "Referer",
            "Content-Type",
            "X-CSRF-Token",
        ] {
            let mut value = request(Method::Get, "/api/v1/auth/session");
            value.headers.insert(header.into(), "secret".into());
            assert!(value.check().is_err());
        }
        let mut value = request(Method::Get, "/api/v1/auth/session");
        value.headers.insert("If-Match".into(), "etag".into());
        value.headers.insert("if-match".into(), "another".into());
        assert!(value.check().is_err());
    }
    #[test]
    fn query_payload_and_generation_are_bounded() {
        let mut value = request(Method::Get, "/api/v1/auth/session");
        value.query = Some("q=literal%26query&cursor=a%2Bb".into());
        assert!(value.check().is_ok());
        let mut value = request(Method::Get, "/api/v1/auth/session");
        value.body = Some(serde_json::json!({"unexpected":"body"}));
        assert!(value.check().is_err());
        for input in ["-1", "01", "1.0", "18446744073709551616"] {
            assert!(generation(input).is_err());
        }
        assert_eq!(generation("18446744073709551615").ok(), Some(u64::MAX));
    }
    #[test]
    fn every_published_workspace_operation_has_an_explicit_native_route() {
        let contract: Value =
            serde_json::from_str(include_str!("../../docs/api/openapi.json")).unwrap();
        for (path, operations) in contract["paths"].as_object().unwrap() {
            if !path.starts_with("/api/v1/workspaces") {
                continue;
            }
            // Raw attachment bytes are intentionally available only through the
            // native picker/download broker, never generic renderer api_request.
            if path == "/api/v1/workspaces/{w}/attachments/{id}/content" {
                let raw = "/api/v1/workspaces/00000000-0000-0000-0000-000000000001/attachments/00000000-0000-0000-0000-000000000002/content";
                assert!(!route_allowed(Method::Get, raw));
                assert!(!route_allowed(Method::Put, raw));
                continue;
            }
            let mut concrete = path.clone();
            while let Some(start) = concrete.find('{') {
                let end = concrete[start..].find('}').unwrap() + start;
                let replacement = if &concrete[start..=end] == "{revision}" {
                    "1"
                } else if concrete[..start].ends_with("/by-key/") {
                    "TEST-1"
                } else {
                    "00000000-0000-0000-0000-000000000001"
                };
                concrete.replace_range(start..=end, replacement);
            }
            for method in operations.as_object().unwrap().keys() {
                let method: Method =
                    serde_json::from_value(Value::String(method.to_ascii_uppercase())).unwrap();
                assert!(
                    route_allowed(method, &concrete),
                    "Missing explicit route: {} {}",
                    method.as_str(),
                    path
                );
            }
        }
    }
}

#[cfg(test)]
mod cycle_route_tests {
    use super::*;
    #[test]
    fn manual_order_routes_keep_methods_and_identifiers_scoped() {
        let w = "00000000-0000-0000-0000-000000000001";
        let t = "00000000-0000-0000-0000-000000000002";
        let id = "00000000-0000-0000-0000-000000000003";
        for (path, allowed) in [
            (
                format!("/api/v1/workspaces/{w}/issues/{id}/position"),
                Method::Post,
            ),
            (
                format!("/api/v1/workspaces/{w}/teams/{t}/issue-order-maintenance/{id}"),
                Method::Get,
            ),
            (
                format!("/api/v1/workspaces/{w}/teams/{t}/issue-order-maintenance/{id}/resume"),
                Method::Post,
            ),
        ] {
            for method in [
                Method::Get,
                Method::Post,
                Method::Patch,
                Method::Put,
                Method::Delete,
            ] {
                assert_eq!(route_allowed(method, &path), method == allowed);
                assert!(!route_allowed(method, &format!("{path}/extra")));
                assert!(!route_allowed(method, &path.replace(id, "invalid")));
            }
        }
    }
    #[test]
    fn native_cycle_routes_are_method_and_uuid_scoped() {
        let w = "00000000-0000-0000-0000-000000000001";
        let t = "00000000-0000-0000-0000-000000000002";
        let c = "00000000-0000-0000-0000-000000000003";
        let item = "00000000-0000-0000-0000-000000000004";
        let base = format!("/api/v1/workspaces/{w}/teams/{t}/cycles");
        let cases = [
            (String::new(), vec![Method::Get, Method::Post]),
            (
                format!("/{c}"),
                vec![Method::Get, Method::Patch, Method::Delete],
            ),
            (format!("/{c}/scope"), vec![Method::Get]),
            (format!("/{c}/issues"), vec![Method::Get]),
            (format!("/{c}/close-preview"), vec![Method::Get]),
            (format!("/{c}/snapshots"), vec![Method::Get, Method::Post]),
            (format!("/{c}/open"), vec![Method::Post]),
            (format!("/{c}/close"), vec![Method::Post]),
            (format!("/{c}/reopen"), vec![Method::Post]),
            (format!("/{c}/carry-forward"), vec![Method::Post]),
            (format!("/{c}/carry-forward/{item}"), vec![Method::Get]),
            (
                format!("/{c}/carry-forward/{item}/resume"),
                vec![Method::Post],
            ),
            (format!("/{c}/reports/{item}"), vec![Method::Get]),
            (format!("/{c}/issues/{item}"), vec![Method::Put]),
        ];
        for (suffix, allowed) in cases {
            for method in [
                Method::Get,
                Method::Post,
                Method::Patch,
                Method::Put,
                Method::Delete,
            ] {
                assert_eq!(
                    route_allowed(method, &format!("{base}{suffix}")),
                    allowed.contains(&method)
                );
            }
        }
        for path in [
            format!("{base}/invalid"),
            format!("{base}/{c}/reports/invalid"),
            format!("{base}/{c}/issues/invalid"),
            format!("{base}/{c}/carry-forward/invalid/resume"),
            format!("{base}/{c}/close/extra"),
            format!("{base}/{c}/snapshots/extra"),
            format!("{base}/{c}/reports/{item}/extra"),
        ] {
            for method in [
                Method::Get,
                Method::Post,
                Method::Patch,
                Method::Put,
                Method::Delete,
            ] {
                assert!(!route_allowed(method, &path));
            }
        }
    }
}
