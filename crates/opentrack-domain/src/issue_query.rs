use crate::{Error, IssueFilter, IssuePage, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

pub const ISSUE_QUERY_VERSION: u16 = 1;
pub const ISSUE_FILTER_MAX_DEPTH: usize = 4;
pub const ISSUE_FILTER_MAX_PREDICATES: usize = 50;
pub const ISSUE_FILTER_MAX_NODES: usize = 201;
pub const ISSUE_FILTER_MAX_VALUES: usize = 100;
pub const ISSUE_FILTER_MAX_RELATIVE_DAYS: i32 = 36_500;

fn query_version() -> u16 {
    ISSUE_QUERY_VERSION
}
fn page_limit() -> i64 {
    50
}
fn timezone() -> String {
    "UTC".into()
}

/// A read-only query. The timezone is specific to this query, not a saved
/// workspace setting. Its server clock is returned and bound into page cursors.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueQuery {
    #[serde(default = "query_version")]
    pub version: u16,
    #[serde(default)]
    pub filter: Option<IssueFilterNode>,
    #[serde(default)]
    pub sort: IssueQuerySort,
    pub team_id: Option<Uuid>,
    pub q: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default = "page_limit")]
    pub limit: i64,
    pub cursor: Option<String>,
    #[serde(default = "timezone")]
    pub timezone: String,
}

impl Default for IssueQuery {
    fn default() -> Self {
        Self {
            version: query_version(),
            filter: None,
            sort: IssueQuerySort::Created,
            team_id: None,
            q: None,
            archived: false,
            limit: page_limit(),
            cursor: None,
            timezone: timezone(),
        }
    }
}

impl IssueQuery {
    pub fn validate(&self) -> Result<()> {
        if self.version != ISSUE_QUERY_VERSION {
            return Err(Error::Invalid(
                "This issue query version is unsupported; repair or clear the filter".into(),
            ));
        }
        if !(1..=100).contains(&self.limit)
            || self
                .q
                .as_ref()
                .is_some_and(|q| q.chars().count() > 256 || q.contains('\0'))
            || self
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.len() > 2048)
            || self.timezone.is_empty()
            || self.timezone.len() > 100
            || self.timezone.chars().any(char::is_control)
        {
            return Err(Error::Invalid(
                "Invalid issue query, timezone or page size".into(),
            ));
        }
        if self.sort == IssueQuerySort::Manual && (self.team_id.is_none() || self.archived) {
            return Err(Error::Invalid(
                "Manual sorting requires one explicit active team and excludes archived work"
                    .into(),
            ));
        }
        if let Some(filter) = &self.filter {
            filter.validate()?;
        }
        Ok(())
    }

    pub fn flat_filter(&self) -> IssueFilter {
        IssueFilter {
            team_id: self.team_id,
            q: self.q.clone(),
            archived: self.archived,
            limit: Some(self.limit),
            sort: Some(self.sort.as_str().into()),
            ..IssueFilter::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueQuerySort {
    #[default]
    Created,
    Priority,
    Manual,
}
impl IssueQuerySort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Priority => "priority",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IssueFilterNode {
    All {
        #[schema(no_recursion)]
        children: Vec<IssueFilterNode>,
    },
    Any {
        #[schema(no_recursion)]
        children: Vec<IssueFilterNode>,
    },
    Predicate {
        field: IssueQueryField,
        op: IssueQueryOperator,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional_value"
        )]
        value: Option<IssueQueryValue>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional_values"
        )]
        values: Option<Vec<IssueQueryValue>>,
    },
}

fn optional_value<'de, D: Deserializer<'de>>(
    input: D,
) -> std::result::Result<Option<IssueQueryValue>, D::Error> {
    IssueQueryValue::deserialize(input).map(Some)
}
fn optional_values<'de, D: Deserializer<'de>>(
    input: D,
) -> std::result::Result<Option<Vec<IssueQueryValue>>, D::Error> {
    Vec::<IssueQueryValue>::deserialize(input).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueQueryField {
    Status,
    Priority,
    Assignee,
    Project,
    Cycle,
    Label,
    Team,
    CreatedAt,
    UpdatedAt,
    DueDate,
}
impl IssueQueryField {
    pub fn identifier(self) -> bool {
        matches!(
            self,
            Self::Status | Self::Assignee | Self::Project | Self::Cycle | Self::Label | Self::Team
        )
    }
    pub fn nullable(self) -> bool {
        matches!(
            self,
            Self::Assignee | Self::Project | Self::Cycle | Self::Label | Self::DueDate
        )
    }
    pub fn date(self) -> bool {
        matches!(self, Self::CreatedAt | Self::UpdatedAt | Self::DueDate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueQueryOperator {
    Eq,
    In,
    NotIn,
    Before,
    After,
    IsNull,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum IssueQueryValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
    Relative(IssueQueryRelativeDate),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueQueryRelativeDate {
    pub relative_days: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueQueryContext {
    pub timezone: String,
    pub as_of: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IssueQueryPage {
    #[serde(flatten)]
    pub page: IssuePage,
    pub query_context: IssueQueryContext,
}

impl IssueFilterNode {
    pub fn validate(&self) -> Result<()> {
        self.validate_node(1, &mut 0, &mut 0)
    }

    fn validate_node(&self, depth: usize, nodes: &mut usize, predicates: &mut usize) -> Result<()> {
        *nodes += 1;
        if depth > ISSUE_FILTER_MAX_DEPTH || *nodes > ISSUE_FILTER_MAX_NODES {
            return Err(Error::Invalid(
                "Issue filters exceed the supported depth or node budget".into(),
            ));
        }
        match self {
            Self::All { children } | Self::Any { children } => {
                if children.is_empty() {
                    return Err(Error::Invalid("Filter groups must contain at least one condition; clear the filter explicitly instead".into()));
                }
                for child in children {
                    child.validate_node(depth + 1, nodes, predicates)?;
                }
            }
            Self::Predicate {
                field,
                op,
                value,
                values,
            } => {
                *predicates += 1;
                if *predicates > ISSUE_FILTER_MAX_PREDICATES {
                    return Err(Error::Invalid(
                        "Issue filters support at most 50 predicates".into(),
                    ));
                }
                match op {
                    IssueQueryOperator::In | IssueQueryOperator::NotIn => {
                        let values = values.as_ref().ok_or_else(invalid_predicate)?;
                        if value.is_some()
                            || values.is_empty()
                            || values.len() > ISSUE_FILTER_MAX_VALUES
                        {
                            return Err(invalid_predicate());
                        }
                        for value in values {
                            validate_value(*field, value)?;
                        }
                    }
                    IssueQueryOperator::IsNull => {
                        if !field.nullable()
                            || values.is_some()
                            || !matches!(value, Some(IssueQueryValue::Boolean(_)))
                        {
                            return Err(invalid_predicate());
                        }
                    }
                    IssueQueryOperator::Eq
                    | IssueQueryOperator::Before
                    | IssueQueryOperator::After => {
                        if values.is_some()
                            || matches!(op, IssueQueryOperator::Before | IssueQueryOperator::After)
                                && !field.date()
                        {
                            return Err(invalid_predicate());
                        }
                        validate_value(*field, value.as_ref().ok_or_else(invalid_predicate)?)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn invalid_predicate() -> Error {
    Error::Invalid("Invalid filter field, operator or value; repair or clear this condition".into())
}

fn validate_value(field: IssueQueryField, value: &IssueQueryValue) -> Result<()> {
    if field.identifier() {
        return match value {
            IssueQueryValue::Text(value) if Uuid::parse_str(value).is_ok() => Ok(()),
            _ => Err(invalid_predicate()),
        };
    }
    if field == IssueQueryField::Priority {
        return match value {
            IssueQueryValue::Integer(0..=4) => Ok(()),
            _ => Err(invalid_predicate()),
        };
    }
    if field.date() {
        return match value {
            IssueQueryValue::Relative(relative)
                if relative.relative_days.abs_diff(0) <= ISSUE_FILTER_MAX_RELATIVE_DAYS as u32 =>
            {
                Ok(())
            }
            IssueQueryValue::Text(value)
                if value.len() <= 64
                    && (value.len() == 10
                        && NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
                        || field != IssueQueryField::DueDate
                            && DateTime::parse_from_rfc3339(value).is_ok_and(|time| {
                                time.timestamp_subsec_nanos().is_multiple_of(1000)
                            })) =>
            {
                Ok(())
            }
            _ => Err(invalid_predicate()),
        };
    }
    Err(invalid_predicate())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn predicate() -> IssueFilterNode {
        serde_json::from_value(json!({"kind":"predicate","field":"priority","op":"eq","value":1}))
            .unwrap()
    }

    #[test]
    fn canonical_query_roundtrips_and_preserves_explicit_context() {
        let value = include_str!("../tests/fixtures/issue-query-v1.json");
        let query: IssueQuery = serde_json::from_str(value).unwrap();
        query.validate().unwrap();
        assert_eq!(query.timezone, "UTC");
        let encoded = serde_json::to_value(&query).unwrap();
        let restored: IssueQuery = serde_json::from_value(encoded).unwrap();
        restored.validate().unwrap();
    }

    #[test]
    fn invalid_operators_unknown_fields_null_operands_and_empty_groups_fail_closed() {
        for value in [
            json!({"kind":"all","children":[]}),
            json!({"kind":"any","children":[]}),
            json!({"kind":"predicate","field":"priority","op":"eq","value":5}),
            json!({"kind":"predicate","field":"priority","op":"before","value":1}),
            json!({"kind":"predicate","field":"priority","op":"is_null","value":true}),
            json!({"kind":"predicate","field":"due_date","op":"eq","value":"2026-13-01"}),
            json!({"kind":"predicate","field":"due_date","op":"eq","value":"2026-01-01T12:00:00Z"}),
            json!({"kind":"predicate","field":"created_at","op":"eq","value":"2026-01-01T12:00:00.000000001Z"}),
            json!({"kind":"predicate","field":"assignee","op":"eq","value":null}),
            json!({"kind":"predicate","field":"priority","op":"in","values":[]}),
            json!({"kind":"predicate","field":"priority","op":"eq","value":1,"values":[]}),
            json!({"kind":"predicate","field":"title);DROP TABLE issues;--","op":"eq","value":1}),
            json!({"kind":"predicate","field":"priority","op":"contains","value":1}),
            json!({"kind":"predicate","field":"priority","op":"eq","value":1,"sql":"true"}),
            json!({"kind":"all","children":[{"kind":"predicate","field":"priority","op":"eq","value":1}],"any":[]}),
        ] {
            match serde_json::from_value::<IssueFilterNode>(value.clone()) {
                Ok(node) => assert!(node.validate().is_err(), "accepted invalid filter {value}"),
                Err(_) => {}
            }
        }
    }

    #[test]
    fn maximum_filter_remains_usable_and_each_budget_is_enforced() {
        let maximum = IssueFilterNode::All {
            children: (0..50)
                .map(|_| IssueFilterNode::Any {
                    children: vec![IssueFilterNode::All {
                        children: vec![predicate()],
                    }],
                })
                .collect(),
        };
        maximum.validate().unwrap();
        let too_many_predicates = IssueFilterNode::All {
            children: (0..51).map(|_| predicate()).collect(),
        };
        assert!(too_many_predicates.validate().is_err());
        let extra = IssueFilterNode::All {
            children: vec![maximum.clone(), predicate()],
        };
        assert!(extra.validate().is_err());
        let too_deep = IssueFilterNode::All {
            children: vec![maximum],
        };
        assert!(too_deep.validate().is_err());
        let ids = (0..100)
            .map(|_| IssueQueryValue::Text(Uuid::new_v4().to_string()))
            .collect::<Vec<_>>();
        let mut node = IssueFilterNode::Predicate {
            field: IssueQueryField::Team,
            op: IssueQueryOperator::In,
            value: None,
            values: Some(ids),
        };
        node.validate().unwrap();
        if let IssueFilterNode::Predicate {
            values: Some(values),
            ..
        } = &mut node
        {
            values.push(IssueQueryValue::Text(Uuid::new_v4().to_string()));
        }
        assert!(node.validate().is_err());
        let many_empty = IssueFilterNode::All {
            children: (0..1000)
                .map(|_| IssueFilterNode::All { children: vec![] })
                .collect(),
        };
        assert!(many_empty.validate().is_err());
    }

    #[test]
    fn nullable_fields_require_explicit_null_operator_and_relative_dates_are_bounded() {
        for field in ["assignee", "project", "cycle", "label", "due_date"] {
            let node: IssueFilterNode = serde_json::from_value(
                json!({"kind":"predicate","field":field,"op":"is_null","value":true}),
            )
            .unwrap();
            node.validate().unwrap();
        }
        for days in [-36_500, 0, 36_500] {
            let node: IssueFilterNode = serde_json::from_value(json!({"kind":"predicate","field":"created_at","op":"before","value":{"relative_days":days}})).unwrap();
            node.validate().unwrap();
        }
        let node: IssueFilterNode = serde_json::from_value(json!({"kind":"predicate","field":"created_at","op":"before","value":{"relative_days":i32::MIN}})).unwrap();
        assert!(node.validate().is_err());
        assert!(
            IssueQuery {
                version: 2,
                ..IssueQuery::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            IssueQuery {
                sort: IssueQuerySort::Manual,
                ..IssueQuery::default()
            }
            .validate()
            .is_err()
        );
    }
}
