use crate::{
    DocumentWrite, Error, Label, Result, Status, bounded_text, deserialize_present,
    normalize_label_ids,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemplateTitleMode {
    #[default]
    Literal,
    Pattern,
}
impl TemplateTitleMode {
    pub fn is_literal(&self) -> bool {
        *self == Self::Literal
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Pattern => "pattern",
        }
    }
}

pub struct TemplateTitleContext<'a> {
    pub team_name: &'a str,
    pub team_key: &'a str,
    pub now: DateTime<Utc>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateTitleError {
    MalformedPattern,
    UnknownPlaceholder,
    TooLong,
    InvalidTitle,
}
impl TemplateTitleError {
    pub fn code(self) -> &'static str {
        match self {
            Self::MalformedPattern => "malformed_pattern",
            Self::UnknownPlaceholder => "unknown_placeholder",
            Self::TooLong => "title_too_long",
            Self::InvalidTitle => "invalid_title",
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::MalformedPattern => {
                "Title patterns require paired double braces, such as {{date}}"
            }
            Self::UnknownPlaceholder => {
                "Title patterns support only {{team.name}}, {{team.key}} and {{date}}"
            }
            Self::TooLong => {
                "The resolved title exceeds 256 characters; shorten the pattern or edit the issue title"
            }
            Self::InvalidTitle => "The resolved title contains unsupported control characters",
        }
    }
}

fn title_parts(title: &str) -> std::result::Result<Vec<(&str, bool)>, TemplateTitleError> {
    let mut rest = title;
    let mut parts = Vec::new();
    while let Some(index) = rest.find(['{', '}']) {
        parts.push((&rest[..index], false));
        rest = &rest[index..];
        let after = rest
            .strip_prefix("{{")
            .ok_or(TemplateTitleError::MalformedPattern)?;
        let end = after
            .find("}}")
            .ok_or(TemplateTitleError::MalformedPattern)?;
        let token = &after[..end];
        if token.contains(['{', '}']) {
            return Err(TemplateTitleError::MalformedPattern);
        }
        if !matches!(token, "team.name" | "team.key" | "date") {
            return Err(TemplateTitleError::UnknownPlaceholder);
        }
        parts.push((token, true));
        rest = &after[end + 2..];
    }
    parts.push((rest, false));
    Ok(parts)
}

pub fn validate_template_title(title: &str, mode: TemplateTitleMode) -> Result<String> {
    let title = title.trim();
    let title = if title.is_empty() {
        String::new()
    } else {
        bounded_text(title, "Template title", 256)?
    };
    if mode == TemplateTitleMode::Pattern {
        title_parts(&title).map_err(|error| Error::Invalid(error.message().into()))?;
    }
    Ok(title)
}

/// Only allowlisted plain strings are substituted, once, with a Unicode character bound.
pub fn materialize_template_title(
    title: &str,
    mode: TemplateTitleMode,
    context: &TemplateTitleContext<'_>,
) -> std::result::Result<String, TemplateTitleError> {
    if title.chars().count() > 256 {
        return Err(TemplateTitleError::TooLong);
    }
    let parts = if mode == TemplateTitleMode::Literal {
        vec![(title, false)]
    } else {
        title_parts(title)?
    };
    let date = context.now.format("%Y-%m-%d").to_string();
    let mut resolved = String::new();
    let mut length = 0;
    for (part, token) in parts {
        let value = if !token {
            part
        } else {
            match part {
                "team.name" => context.team_name,
                "team.key" => context.team_key,
                "date" => &date,
                _ => unreachable!("title_parts validates the allowlist"),
            }
        };
        length += value.chars().count();
        if length > 256 {
            return Err(TemplateTitleError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(TemplateTitleError::InvalidTitle);
        }
        resolved.push_str(value);
    }
    Ok(resolved.trim().to_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateSelection {
    pub id: Uuid,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct TemplateDefaults {
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true, nullable = true)]
    pub status_id: Option<Uuid>,
    pub priority: i16,
    pub label_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
}
fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(deserializer)
}
impl TemplateDefaults {
    pub fn validate(&mut self) -> Result<()> {
        if !(0..=4).contains(&self.priority) {
            return Err(Error::Invalid(
                "Template priority must be between 0 and 4".into(),
            ));
        }
        normalize_label_ids(&mut self.label_ids)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueTemplateSummary {
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub version: String,
    pub etag: String,
    #[schema(required = true, nullable = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub can_manage: bool,
    pub can_use: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TemplateDocument {
    pub schema_version: i32,
    pub body: Value,
    pub plain_text: String,
    pub schema_supported: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueTemplate {
    #[serde(flatten)]
    pub summary: IssueTemplateSummary,
    pub title: String,
    #[serde(default)]
    #[schema(required = true)]
    pub title_mode: TemplateTitleMode,
    pub defaults: TemplateDefaults,
    pub document: TemplateDocument,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TemplateDiagnostic {
    pub field: String,
    pub code: String,
    #[schema(required = true, nullable = true)]
    pub reference_id: Option<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TemplatePreview {
    pub template: IssueTemplate,
    #[schema(required = true, nullable = true)]
    pub materialized_title: Option<String>,
    pub title_context_at: DateTime<Utc>,
    #[schema(required = true, nullable = true)]
    pub resolved_status: Option<Status>,
    #[schema(required = true, nullable = true)]
    pub resolved_project: Option<crate::ProjectSummary>,
    pub labels: Vec<Label>,
    pub diagnostics: Vec<TemplateDiagnostic>,
    pub can_create: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct TemplateQuery {
    pub archived: Option<bool>,
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIssueTemplate {
    pub name: String,
    pub title: String,
    // Omitted/literal retains the request hash used by pre-pattern retries.
    #[serde(default, skip_serializing_if = "TemplateTitleMode::is_literal")]
    pub title_mode: TemplateTitleMode,
    pub defaults: TemplateDefaults,
    pub document: DocumentWrite,
}
impl CreateIssueTemplate {
    pub fn validate(&mut self) -> Result<String> {
        self.name = bounded_text(&self.name, "Template name", 100)?;
        // Empty is useful for a checklist-only template; issue creation still
        // requires an explicit nonempty title after the draft is filled in.
        self.title = validate_template_title(&self.title, self.title_mode)?;
        self.defaults.validate()?;
        validate_template_document(&self.document)
    }
}
pub fn validate_template_document(document: &DocumentWrite) -> Result<String> {
    let plain = document.validate()?;
    if document.moderation_reason.is_some()
        || document.has_attachments()
        || !document.mention_ids().is_empty()
        || !document.issue_reference_ids().is_empty()
    {
        return Err(Error::Invalid(
            "Templates cannot contain attachment references, user mentions, issue references or moderation reasons"
                .into(),
        ));
    }
    Ok(plain)
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct UpdateIssueTemplate {
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub title_mode: Option<TemplateTitleMode>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub defaults: Option<TemplateDefaults>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub document: Option<DocumentWrite>,
    #[serde(default, deserialize_with = "deserialize_present")]
    #[schema(nullable = false)]
    pub archived: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn optional_project_preserves_existing_template_defaults_serialization() {
        let old = json!({"status_id":null,"priority":0,"label_ids":[]});
        let defaults: TemplateDefaults = serde_json::from_value(old.clone()).unwrap();
        assert!(defaults.project_id.is_none());
        assert_eq!(serde_json::to_value(&defaults).unwrap(), old);
        assert_eq!(
            serde_json::to_vec(&defaults).unwrap().as_slice(),
            br#"{"status_id":null,"priority":0,"label_ids":[]}"#
        );
        let project = Uuid::new_v4();
        let mut with_project = old;
        with_project["project_id"] = json!(project);
        let defaults: TemplateDefaults = serde_json::from_value(with_project.clone()).unwrap();
        assert_eq!(defaults.project_id, Some(project));
        assert_eq!(serde_json::to_value(defaults).unwrap(), with_project);
    }
    fn context(at: &str) -> TemplateTitleContext<'static> {
        TemplateTitleContext {
            team_name: "Example Team",
            team_key: "ENG",
            now: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }
    #[test]
    fn title_patterns_are_opt_in_and_preserve_legacy_request_serialization() {
        let raw = json!({"name":"Braces","title":"{{unknown}} {literal}","defaults":{"status_id":null,"priority":0,"label_ids":[]},"document":{"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph"}]}}});
        let mut legacy: CreateIssueTemplate = serde_json::from_value(raw.clone()).unwrap();
        legacy.validate().unwrap();
        assert_eq!(legacy.title_mode, TemplateTitleMode::Literal);
        assert_eq!(serde_json::to_value(&legacy).unwrap(), raw);
        #[derive(Serialize)]
        struct LegacyRequest<'a> {
            name: &'a str,
            title: &'a str,
            defaults: &'a TemplateDefaults,
            document: &'a DocumentWrite,
        }
        assert_eq!(
            serde_json::to_vec(&legacy).unwrap(),
            serde_json::to_vec(&LegacyRequest {
                name: &legacy.name,
                title: &legacy.title,
                defaults: &legacy.defaults,
                document: &legacy.document,
            })
            .unwrap(),
        );
        assert_eq!(
            materialize_template_title(
                &legacy.title,
                legacy.title_mode,
                &context("2026-01-01T00:00:00Z")
            )
            .unwrap(),
            "{{unknown}} {literal}"
        );
        assert!(serde_json::from_value::<UpdateIssueTemplate>(json!({"title_mode":null})).is_err());
        assert!(
            serde_json::from_value::<UpdateIssueTemplate>(json!({"title_mode":"expression"}))
                .is_err()
        );
    }
    #[test]
    fn title_patterns_reject_malformed_and_unknown_expressions() {
        let ctx = context("2026-01-01T00:00:00Z");
        for text in [
            "{{date}",
            "{date}",
            "date}}",
            "{{{date}}}",
            "{{date}}{{",
            "{{date}}}",
        ] {
            assert_eq!(
                materialize_template_title(text, TemplateTitleMode::Pattern, &ctx),
                Err(TemplateTitleError::MalformedPattern),
                "{text}"
            );
            assert!(validate_template_title(text, TemplateTitleMode::Pattern).is_err());
        }
        for text in [
            "{{}}",
            "{{user.name}}",
            "{{ date }}",
            "{{date+1}}",
            "{{team.name.toUpperCase()}}",
        ] {
            assert_eq!(
                materialize_template_title(text, TemplateTitleMode::Pattern, &ctx),
                Err(TemplateTitleError::UnknownPlaceholder),
                "{text}"
            );
        }
    }
    #[test]
    fn title_patterns_use_utc_calendar_date_at_midnight_and_leap_boundaries() {
        for (time, expected) in [
            ("2028-02-28T23:59:59Z", "2028-02-28"),
            ("2028-02-29T00:00:00Z", "2028-02-29"),
            ("2028-03-01T00:30:00+01:00", "2028-02-29"),
            ("2026-12-31T23:59:59Z", "2026-12-31"),
            ("2027-01-01T00:00:00Z", "2027-01-01"),
        ] {
            assert_eq!(
                materialize_template_title(
                    "{{team.key}} {{team.name}} {{date}}",
                    TemplateTitleMode::Pattern,
                    &context(time)
                )
                .unwrap(),
                format!("ENG Example Team {expected}")
            );
        }
    }
    #[test]
    fn materialized_titles_are_bounded_without_truncating_or_recursive_expansion() {
        let name = "界".repeat(128);
        let ctx = TemplateTitleContext {
            team_name: &name,
            ..context("2026-01-01T00:00:00Z")
        };
        assert_eq!(
            materialize_template_title(
                "{{team.name}}{{team.name}}",
                TemplateTitleMode::Pattern,
                &ctx
            )
            .unwrap()
            .chars()
            .count(),
            256
        );
        assert_eq!(
            materialize_template_title(
                "{{team.name}}{{team.name}}!",
                TemplateTitleMode::Pattern,
                &ctx
            ),
            Err(TemplateTitleError::TooLong)
        );
        let ctx = TemplateTitleContext {
            team_name: "{{date}}",
            ..ctx
        };
        assert_eq!(
            materialize_template_title("{{team.name}}", TemplateTitleMode::Pattern, &ctx).unwrap(),
            "{{date}}"
        );
        let ctx = TemplateTitleContext {
            team_name: "unsafe\nname",
            ..ctx
        };
        assert_eq!(
            materialize_template_title("{{team.name}}", TemplateTitleMode::Pattern, &ctx),
            Err(TemplateTitleError::InvalidTitle)
        );
        assert!(validate_template_title(&"界".repeat(257), TemplateTitleMode::Pattern).is_err());
        assert_eq!(
            materialize_template_title("", TemplateTitleMode::Pattern, &ctx).unwrap(),
            ""
        );
    }
    #[test]
    fn replacement_defaults_require_explicit_status_semantics() {
        assert!(
            serde_json::from_value::<TemplateDefaults>(json!({"priority":0,"label_ids":[]}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<TemplateDefaults>(
                json!({"status_id":null,"priority":0,"label_ids":[]})
            )
            .is_ok()
        );
    }
    #[test]
    fn reusable_body_rejects_bound_attachments_and_moderation_context() {
        let document:DocumentWrite=serde_json::from_value(json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"attachment","attrs":{"id":Uuid::new_v4()}}]}})).unwrap();
        assert!(validate_template_document(&document).is_err());
        let mut ordinary = serde_json::from_value::<DocumentWrite>(
            json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph"}]}}),
        )
        .unwrap();
        ordinary.moderation_reason = Some("Not a comment".into());
        assert!(validate_template_document(&ordinary).is_err());
    }
}
