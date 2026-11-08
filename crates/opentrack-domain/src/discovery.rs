//! Typed persistent discovery definitions; execution context is always ephemeral.
use crate::{Error, IssueQuery, Result};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryKind {
    Search,
    View,
}
impl DiscoveryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::View => "view",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryVisibility {
    Personal,
    Team,
    Workspace,
}
impl DiscoveryVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Team => "team",
            Self::Workspace => "workspace",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryLayout {
    List,
    Board,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryGroup {
    None,
    Status,
    Priority,
    Assignee,
    Project,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryColumn {
    Key,
    Title,
    Status,
    Priority,
    Assignee,
    Project,
    DueDate,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryDefinition {
    pub query: IssueQuery,
    pub layout: DiscoveryLayout,
    pub group_by: DiscoveryGroup,
    #[schema(min_items = 1, max_items = 7)]
    pub columns: Vec<DiscoveryColumn>,
}
impl DiscoveryDefinition {
    pub fn validate(&self) -> Result<()> {
        self.query.validate()?;
        if self.query.cursor.is_some()
            || self.columns.is_empty()
            || self.columns.len() > 7
            || !self.columns.contains(&DiscoveryColumn::Title)
            || self
                .columns
                .iter()
                .enumerate()
                .any(|(i, c)| self.columns[..i].contains(c))
        {
            return Err(Error::Invalid("Save a definition without a continuation cursor and choose unique display columns including title".into()));
        }
        if self.layout == DiscoveryLayout::Board && self.query.limit > 50 {
            return Err(Error::Invalid(
                "Board views support at most 50 issues per page".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveDiscovery {
    #[schema(min_length = 1, max_length = 100)]
    pub name: String,
    pub kind: DiscoveryKind,
    pub visibility: DiscoveryVisibility,
    pub team_id: Option<Uuid>,
    pub definition: DiscoveryDefinition,
}
impl SaveDiscovery {
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().into();
        if self.name.is_empty()
            || self.name.chars().count() > 100
            || self.name.chars().any(char::is_control)
        {
            return Err(Error::Invalid("Choose a name of 1–100 characters".into()));
        }
        if (self.visibility == DiscoveryVisibility::Team) != self.team_id.is_some()
            || self.kind == DiscoveryKind::Search
                && self.visibility != DiscoveryVisibility::Personal
        {
            return Err(Error::Invalid(
                "Saved searches are personal; team views require exactly one audience team".into(),
            ));
        }
        self.definition.validate()?;
        if self.kind == DiscoveryKind::Search {
            SearchRequest {
                query: self.definition.query.clone(),
                types: vec![SearchEntity::Issue],
            }
            .validate()?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SavedDiscovery {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub kind: DiscoveryKind,
    pub visibility: DiscoveryVisibility,
    pub team_id: Option<Uuid>,
    pub definition: Option<DiscoveryDefinition>,
    pub repair_required: Option<String>,
    pub favorite: bool,
    pub can_edit: bool,
    pub version: String,
    pub etag: String,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryListQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    pub kind: Option<DiscoveryKind>,
    #[serde(default)]
    pub favorites: bool,
}
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecuteDiscovery {
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FavoriteDiscovery {
    pub favorite: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchEntity {
    Issue,
    Project,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: IssueQuery,
    pub types: Vec<SearchEntity>,
}
impl SearchRequest {
    pub fn validate(&self) -> Result<()> {
        self.query.validate()?;
        let q = self.query.q.as_deref().unwrap_or("").trim();
        if q.is_empty()
            || q.split_whitespace().count() > 32
            || q.chars().any(char::is_control)
            || self.types.is_empty()
            || self.types.len() > 2
            || self.types.len() == 2 && self.types[0] == self.types[1]
        {
            return Err(Error::Invalid(
                "Search needs 1–256 characters, at most 32 words and unique supported types".into(),
            ));
        }
        if self.query.sort != crate::IssueQuerySort::Created || self.query.archived {
            return Err(Error::Invalid(
                "Search uses relevance order and live, nonarchived sources".into(),
            ));
        }
        if self.types.contains(&SearchEntity::Project)
            && (self.query.filter.is_some() || self.query.team_id.is_some())
        {
            return Err(Error::Invalid("Structured issue filters require issue-only search; choose the issue type explicitly".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchHit {
    pub id: Uuid,
    pub team_id: Uuid,
    pub kind: SearchEntity,
    pub key: Option<String>,
    pub title: String,
    pub snippet: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchPage {
    pub data: Vec<SearchHit>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
    pub query_context: crate::IssueQueryContext,
    pub fuzzy_degraded: bool,
    pub notice: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoveryExecution {
    pub issues: Option<crate::IssueQueryPage>,
    pub search: Option<SearchPage>,
}
