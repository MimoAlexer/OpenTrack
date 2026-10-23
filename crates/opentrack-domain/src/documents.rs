use crate::{Error, MentionReference, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;

pub const DOCUMENT_MAX_BYTES: usize = 256 * 1024;
pub const DOCUMENT_MAX_NODES: usize = 10_000;
pub const DOCUMENT_MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentWrite {
    pub schema_version: i32,
    pub body: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moderation_reason: Option<String>,
}
impl DocumentWrite {
    /// Validation is iterative: untrusted nesting never drives Rust recursion.
    pub fn validate(&self) -> Result<String> {
        if self.schema_version != 1 {
            return Err(Error::UnsupportedDocumentSchema);
        }
        let mut stack = vec![(&self.body, 1_usize, "root")];
        let mut count = 0;
        let mut text = String::new();
        let mut attachments = std::collections::BTreeSet::new();
        let mut mentions = std::collections::BTreeSet::new();
        let mut issue_references = std::collections::BTreeSet::new();
        while let Some((node, depth, parent)) = stack.pop() {
            count += 1;
            if depth > DOCUMENT_MAX_DEPTH || count > DOCUMENT_MAX_NODES {
                return invalid("Document exceeds its depth or node limit");
            }
            let object = node
                .as_object()
                .ok_or_else(|| Error::Invalid("Document nodes must be objects".into()))?;
            if object
                .keys()
                .any(|k| !["type", "attrs", "content", "marks", "text"].contains(&k.as_str()))
            {
                return invalid("Unknown document node field");
            }
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid("Document node type is required".into()))?;
            if !allowed_child(parent, kind) {
                return invalid("Unsupported node or invalid document structure");
            }
            let attrs = object.get("attrs");
            if attrs.is_some_and(|a| !a.is_object()) {
                return invalid("Document attributes must be objects");
            }
            let keys: Vec<&str> = attrs
                .and_then(Value::as_object)
                .map(|a| a.keys().map(String::as_str).collect())
                .unwrap_or_default();
            match kind {
                "issueReference" => {
                    let raw = attrs
                        .and_then(|a| a.get("id"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Invalid("Issue reference requires an ID".into()))?;
                    let id = Uuid::parse_str(raw).map_err(|_| {
                        Error::Invalid("Issue reference requires a valid ID".into())
                    })?;
                    if keys.len() != 1 || keys[0] != "id" || raw != id.to_string() {
                        return invalid(
                            "Issue reference attributes contain only a canonical issue ID",
                        );
                    }
                    issue_references.insert(id);
                    if issue_references.len() > 50 {
                        return invalid("A document supports up to 50 distinct issue references");
                    }
                }
                "userMention" => {
                    let raw = attrs
                        .and_then(|a| a.get("id"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Invalid("Mention requires an ID".into()))?;
                    let id = Uuid::parse_str(raw)
                        .map_err(|_| Error::Invalid("Mention requires a valid ID".into()))?;
                    if keys.len() != 1 || keys[0] != "id" || raw != id.to_string() {
                        return invalid("Mention attributes contain only a canonical user ID");
                    }
                    mentions.insert(id);
                    if mentions.len() > 50 {
                        return invalid("A document supports up to 50 distinct user mentions");
                    }
                }
                "attachment" => {
                    let id = attrs
                        .and_then(|a| a.get("id"))
                        .and_then(Value::as_str)
                        .and_then(|id| Uuid::parse_str(id).ok())
                        .ok_or_else(|| Error::Invalid("Attachment requires a valid ID".into()))?;
                    if keys.len() != 1
                        || keys[0] != "id"
                        || !attachments.insert(id)
                        || attachments.len() > 100
                    {
                        return invalid(
                            "A document supports up to 100 distinct attachment references",
                        );
                    }
                }
                "heading" => {
                    if keys.iter().any(|k| *k != "level")
                        || !attrs
                            .and_then(|a| a.get("level"))
                            .and_then(Value::as_i64)
                            .is_some_and(|v| (1..=3).contains(&v))
                    {
                        return invalid("Heading level must be 1–3");
                    }
                }
                "orderedList" => {
                    if keys.iter().any(|k| *k != "start")
                        || attrs.and_then(|a| a.get("start")).is_some_and(|v| {
                            !v.as_i64()
                                .is_some_and(|n| (1..=i32::MAX as i64).contains(&n))
                        })
                    {
                        return invalid("Invalid ordered list start");
                    }
                }
                "taskItem" => {
                    if keys.iter().any(|k| *k != "checked")
                        || !attrs
                            .and_then(|a| a.get("checked"))
                            .is_some_and(Value::is_boolean)
                    {
                        return invalid("Task items require checked true or false");
                    }
                }
                "codeBlock" => {
                    if keys.iter().any(|k| *k != "language")
                        || attrs.and_then(|a| a.get("language")).is_some_and(|v| {
                            !v.is_null()
                                && !v.as_str().is_some_and(|s| {
                                    s.len() <= 64
                                        && s.bytes().all(|b| {
                                            b.is_ascii_alphanumeric() || b"_+-#".contains(&b)
                                        })
                                })
                        })
                    {
                        return invalid("Invalid code language");
                    }
                }
                _ if !keys.is_empty() => return invalid("Unknown document attribute"),
                _ => {}
            }
            if kind == "text" {
                if object.contains_key("content") {
                    return invalid("Text cannot contain child nodes");
                }
                let value = object
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Invalid("Text nodes require text".into()))?;
                if value.is_empty() || value.contains('\0') {
                    return invalid("Invalid document text");
                }
                text.push_str(value);
            } else {
                if object.contains_key("text") || object.contains_key("marks") {
                    return invalid("Only text nodes may carry text or marks");
                }
                if kind == "userMention" {
                    text.push_str("@someone");
                }
                if kind == "issueReference" {
                    text.push_str("[issue]");
                }
                if kind == "hardBreak"
                    || (![
                        "doc",
                        "listItem",
                        "taskItem",
                        "bulletList",
                        "orderedList",
                        "taskList",
                        "userMention",
                        "issueReference",
                    ]
                    .contains(&kind)
                        && !text.is_empty()
                        && !text.ends_with('\n'))
                {
                    text.push('\n');
                }
            }
            if let Some(marks) = object.get("marks") {
                if parent == "codeBlock" {
                    return invalid("Code blocks cannot contain inline formatting");
                }
                let marks = marks
                    .as_array()
                    .ok_or_else(|| Error::Invalid("Marks must be a list".into()))?;
                if marks.len() > 5 {
                    return invalid("Too many inline marks");
                }
                let mut seen = std::collections::BTreeSet::new();
                for mark in marks {
                    let mark = mark
                        .as_object()
                        .ok_or_else(|| Error::Invalid("Invalid inline mark".into()))?;
                    if mark
                        .keys()
                        .any(|k| !["type", "attrs"].contains(&k.as_str()))
                    {
                        return invalid("Unknown inline mark field");
                    }
                    let name = mark
                        .get("type")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Invalid("Invalid inline mark".into()))?;
                    if !seen.insert(name)
                        || !["bold", "italic", "strike", "code", "link"].contains(&name)
                    {
                        return invalid("Unsupported or duplicate inline mark");
                    }
                    let attrs = mark.get("attrs").and_then(Value::as_object);
                    if mark.get("attrs").is_some() && attrs.is_none() {
                        return invalid("Invalid inline mark attributes");
                    }
                    if name == "link" {
                        let attrs =
                            attrs.ok_or_else(|| Error::Invalid("Link requires href".into()))?;
                        let href = attrs
                            .get("href")
                            .and_then(Value::as_str)
                            .ok_or_else(|| Error::Invalid("Link requires href".into()))?;
                        if attrs.len() != 1 || !safe_document_link(href) {
                            return invalid("Only safe http, https and mailto links are supported");
                        }
                    } else if attrs.is_some_and(|a| !a.is_empty()) {
                        return invalid("Unknown inline mark attribute");
                    }
                }
            }
            let children = object
                .get("content")
                .map(|v| {
                    v.as_array()
                        .ok_or_else(|| Error::Invalid("Node content must be a list".into()))
                })
                .transpose()?;
            if [
                "text",
                "hardBreak",
                "attachment",
                "userMention",
                "issueReference",
            ]
            .contains(&kind)
                && children.is_some()
            {
                return invalid("Leaf nodes cannot contain children");
            }
            if [
                "bulletList",
                "orderedList",
                "taskList",
                "listItem",
                "taskItem",
                "blockquote",
            ]
            .contains(&kind)
                && children.is_none_or(Vec::is_empty)
            {
                return invalid("Container requires content");
            }
            if ["listItem", "taskItem"].contains(&kind)
                && children
                    .and_then(|c| c.first())
                    .and_then(|n| n.get("type"))
                    .and_then(Value::as_str)
                    != Some("paragraph")
            {
                return invalid("List items must start with a paragraph");
            }
            if let Some(children) = children {
                if children.len() + stack.len() + count > DOCUMENT_MAX_NODES {
                    return invalid("Document exceeds its node limit");
                }
                for child in children.iter().rev() {
                    stack.push((child, depth + 1, kind));
                }
            }
        }
        if serde_json::to_vec(&self.body)
            .map_err(|_| Error::Internal)?
            .len()
            > DOCUMENT_MAX_BYTES
        {
            return invalid("Document exceeds 256 KiB");
        }
        Ok(text)
    }
    pub fn from_plain_text(text: &str) -> Self {
        let content = if text.is_empty() {
            vec![]
        } else {
            vec![json!({"type":"text","text":text})]
        };
        Self {
            schema_version: 1,
            body: json!({"type":"doc","content":[{"type":"paragraph","content":content}]}),
            moderation_reason: None,
        }
    }
    pub fn mention_ids(&self) -> Vec<Uuid> {
        document_mention_ids(&self.body)
    }
    pub fn issue_reference_ids(&self) -> Vec<Uuid> {
        document_issue_reference_ids(&self.body)
    }
    /// Call after validate(): nodes are bounded and every attachment ID is valid.
    pub fn has_attachments(&self) -> bool {
        let mut pending = vec![&self.body];
        while let Some(node) = pending.pop() {
            if node.get("type").and_then(Value::as_str) == Some("attachment") {
                return true;
            }
            if let Some(children) = node.get("content").and_then(Value::as_array) {
                pending.extend(children);
            }
        }
        false
    }
}
pub fn document_mention_ids(body: &Value) -> Vec<Uuid> {
    document_reference_ids(body, "userMention")
}
pub fn document_issue_reference_ids(body: &Value) -> Vec<Uuid> {
    document_reference_ids(body, "issueReference")
}
fn document_reference_ids(body: &Value, kind: &str) -> Vec<Uuid> {
    let mut pending = vec![body];
    let mut ids = std::collections::BTreeSet::new();
    while let Some(node) = pending.pop() {
        if node.get("type").and_then(Value::as_str) == Some(kind)
            && let Some(id) = node
                .get("attrs")
                .and_then(|v| v.get("id"))
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        {
            ids.insert(id);
        }
        if let Some(children) = node.get("content").and_then(Value::as_array) {
            pending.extend(children);
        }
    }
    ids.into_iter().collect()
}
fn invalid<T>(message: &str) -> Result<T> {
    Err(Error::Invalid(message.into()))
}
fn allowed_child(parent: &str, kind: &str) -> bool {
    match parent {
        "root" => kind == "doc",
        "paragraph" | "heading" => matches!(
            kind,
            "text" | "hardBreak" | "userMention" | "issueReference"
        ),
        "codeBlock" => kind == "text",
        "bulletList" | "orderedList" => kind == "listItem",
        "taskList" => kind == "taskItem",
        "doc" | "blockquote" | "listItem" | "taskItem" => matches!(
            kind,
            "paragraph"
                | "heading"
                | "bulletList"
                | "orderedList"
                | "taskList"
                | "blockquote"
                | "codeBlock"
                | "attachment"
        ),
        _ => false,
    }
}
pub fn safe_document_link(href: &str) -> bool {
    if href.len() > 2048
        || href.chars().any(|c| c.is_control() || c.is_whitespace())
        || href.contains('\\')
    {
        return false;
    }
    let Ok(url) = url::Url::parse(href) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.scheme() {
        "http" | "https" => url.host_str().is_some(),
        "mailto" => !url.path().is_empty() && url.query().is_none() && url.fragment().is_none(),
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Document {
    pub id: Uuid,
    pub owner_type: String,
    pub owner_id: Uuid,
    pub schema_supported: bool,
    pub schema_version: i32,
    pub body: Value,
    pub plain_text: String,
    #[serde(default)]
    #[schema(required = true)]
    pub mentions: Vec<MentionReference>,
    #[serde(default)]
    #[schema(required = true)]
    pub issue_references: Vec<IssueReference>,
    pub version: String,
    pub etag: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub can_edit: bool,
}
impl Document {
    pub fn refresh_etag(&mut self) {
        self.etag = format!(
            "\"document:{}:{}:{}:{:x}\"",
            self.id,
            self.version,
            self.can_edit,
            Sha256::digest(
                serde_json::to_vec(&(&self.mentions, &self.issue_references))
                    .expect("Reference projections serialize")
            )
        );
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CommentAuthor {
    pub id: Uuid,
    pub display_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Comment {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub author: CommentAuthor,
    #[schema(required = true)]
    pub document: Option<Document>,
    pub version: String,
    pub etag: String,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub edited_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub deleted_at: Option<DateTime<Utc>>,
    pub can_edit: bool,
    pub can_delete: bool,
}
impl Comment {
    pub fn refresh_etag(&mut self) {
        self.etag = format!("\"comment:{}:{}:{:x}\"", self.id, self.version, Sha256::digest(json!({"document":self.document.as_ref().map(|d| &d.etag),"author":self.author,"edit":self.can_edit,"delete":self.can_delete}).to_string().as_bytes()));
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DocumentRevisionSummary {
    pub revision: String,
    pub schema_version: i32,
    #[schema(required = true)]
    pub author: Option<CommentAuthor>,
    pub migrated: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DocumentRevision {
    #[serde(flatten)]
    pub summary: DocumentRevisionSummary,
    pub body: Value,
    pub plain_text: String,
    #[serde(default)]
    #[schema(required = true)]
    pub mentions: Vec<MentionReference>,
    #[serde(default)]
    #[schema(required = true)]
    pub issue_references: Vec<IssueReference>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueReference {
    pub id: Uuid,
    pub available: bool,
    #[schema(required = true, nullable = true)]
    pub key: Option<String>,
    #[schema(required = true, nullable = true)]
    pub title: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueReferenceCandidate {
    pub id: Uuid,
    pub key: String,
    pub title: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueReferenceQuery {
    pub team_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
    pub document_id: Option<Uuid>,
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
impl IssueReferenceQuery {
    pub fn validate(&self) -> Result<()> {
        if [self.team_id, self.issue_id, self.document_id]
            .iter()
            .flatten()
            .count()
            != 1
            || !(1..=20).contains(&self.limit.unwrap_or(20))
            || self.q.as_ref().is_some_and(|s| s.chars().count() > 100)
            || self.cursor.as_ref().is_some_and(|s| s.len() > 2048)
        {
            return invalid(
                "Choose one issue reference scope, a page size of 1–20, and search at most 100 characters",
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscussionQuery {
    #[schema(minimum = 1, maximum = 100)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
impl DiscussionQuery {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.limit.unwrap_or(25))
            || self.cursor.as_ref().is_some_and(|s| s.len() > 2048)
        {
            return invalid("Invalid discussion page size or cursor");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentDelete {
    pub moderation_reason: Option<String>,
}

pub fn plain_document_shape(body: &Value) -> bool {
    body.get("type").and_then(Value::as_str) == Some("doc")
        && body
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|nodes| {
                nodes.iter().all(|n| {
                    n.get("type").and_then(Value::as_str) == Some("paragraph")
                        && n.get("attrs")
                            .is_none_or(|a| a.as_object().is_some_and(|a| a.is_empty()))
                        && n.get("content").is_none_or(|c| {
                            c.as_array().is_some_and(|c| {
                                c.iter().all(|n| {
                                    n.get("type").and_then(Value::as_str) == Some("text")
                                        && n.get("marks")
                                            .is_none_or(|m| m.as_array().is_some_and(Vec::is_empty))
                                })
                            })
                        })
                })
            })
}

#[cfg(test)]
mod reference_tests {
    use super::*;
    fn document(nodes: Vec<Value>) -> DocumentWrite {
        serde_json::from_value(json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":nodes}]}})).unwrap()
    }
    #[test]
    fn references_are_bounded_id_only_inline_leaves_with_redacted_plain_text() {
        let id = Uuid::new_v4();
        let node = json!({"type":"issueReference","attrs":{"id":id}});
        let valid = document(vec![
            json!({"type":"text","text":"See "}),
            node.clone(),
            node.clone(),
        ]);
        assert_eq!(valid.validate().unwrap(), "See [issue][issue]");
        assert_eq!(valid.issue_reference_ids(), vec![id]);
        assert!(!plain_document_shape(&valid.body));
        for bad in [
            json!({"type":"issueReference","attrs":{"id":id,"title":"forged secret"}}),
            json!({"type":"issueReference","attrs":{"id":id,"key":"PRIVATE-1"}}),
            json!({"type":"issueReference","attrs":{"id":id.to_string().to_uppercase()}}),
            json!({"type":"issueReference","attrs":{"id":"../escape"}}),
            json!({"type":"issueReference","attrs":{"id":id},"content":[]}),
            json!({"type":"issueReference","attrs":{"id":id},"text":"forged"}),
            json!({"type":"issueReference","attrs":{"id":id},"marks":[]}),
        ] {
            assert!(document(vec![bad]).validate().is_err());
        }
        let nodes: Vec<Value> = (0..51)
            .map(|_| json!({"type":"issueReference","attrs":{"id":Uuid::new_v4()}}))
            .collect();
        assert!(document(nodes[..50].to_vec()).validate().is_ok());
        assert!(document(nodes).validate().is_err());
        let mut block = valid;
        block.body["content"][0]["type"] = json!("codeBlock");
        assert!(block.validate().is_err());
    }
}
