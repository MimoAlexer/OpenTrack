//! One bound query path for legacy lists and versioned structured filters.
use crate::{ISSUE_SELECT, IssueListCursor, Tx, issue_from};
use chrono::{DateTime, NaiveDate, Utc};
use opentrack_domain::{
    Issue, IssueFilter, IssueFilterNode, IssueQueryContext, IssueQueryField as Field,
    IssueQueryOperator as Operator, IssueQueryValue as Operand,
};
use serde_json::Value;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

const PRIORITY: &str = "CASE WHEN i.priority=0 THEN 5 ELSE i.priority END";
const CATEGORY: &str = "CASE s.category WHEN 'triage' THEN 0 WHEN 'backlog' THEN 1 WHEN 'unstarted' THEN 2 WHEN 'started' THEN 3 WHEN 'completed' THEN 4 ELSE 5 END";

fn invalid() -> sqlx::Error {
    sqlx::Error::Protocol("Invalid structured issue predicate".into())
}

/// The caller retains the ordinary session/workspace authorization transaction.
/// An AST always narrows the same source/visibility selection used by flat GET.
pub async fn list_issue_query(
    tx: &mut Tx,
    workspace: Uuid,
    user: Uuid,
    filter: &IssueFilter,
    cursor: Option<IssueListCursor>,
    structured: Option<(&IssueFilterNode, &IssueQueryContext)>,
) -> Result<Vec<Issue>, sqlx::Error> {
    let mut query = build_issue_query(workspace, user, filter, cursor, structured)?;
    let rows = query
        .build_query_scalar::<Value>()
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter().map(issue_from).collect()
}

fn build_issue_query(
    workspace: Uuid,
    user: Uuid,
    filter: &IssueFilter,
    cursor: Option<IssueListCursor>,
    structured: Option<(&IssueFilterNode, &IssueQueryContext)>,
) -> Result<QueryBuilder<Postgres>, sqlx::Error> {
    filter.validate().map_err(|_| invalid())?;
    if let Some((node, _)) = structured {
        node.validate().map_err(|_| invalid())?;
    }
    let manual = filter.sort.as_deref() == Some("manual");
    if matches!(cursor, Some(IssueListCursor::Manual(..))) && !manual
        || matches!(cursor, Some(IssueListCursor::Standard(..))) && manual
    {
        return Err(sqlx::Error::Protocol(
            "Issue cursor kind does not match its ordering".into(),
        ));
    }
    let mut query = QueryBuilder::<Postgres>::new(ISSUE_SELECT);
    if manual {
        query.push(" JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id ");
    }
    if let Some((_, context)) = structured {
        // Bind the fixed clock once, including at the largest supported AST.
        // Relative operands share its local calendar date; membership never
        // assumes that a local date is one interval between UTC midnights.
        query
            .push(" CROSS JOIN (SELECT ")
            .push_bind(context.as_of)
            .push("::timestamptz AS as_of,")
            .push_bind(context.timezone.clone())
            .push("::text AS timezone) query_clock ");
    }
    query
        .push(" WHERE i.workspace_id=")
        .push_bind(workspace)
        .push(" AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=")
        .push_bind(user)
        .push(")) AND i.deleted_at IS NULL");
    for (column, value) in [
        ("i.team_id", filter.team_id),
        ("i.status_id", filter.status_id),
        ("i.assignee_id", filter.assignee_id),
    ] {
        if let Some(value) = value {
            query.push(" AND ").push(column).push("=").push_bind(value);
        }
    }
    if let Some(priority) = filter.priority {
        query.push(" AND i.priority=").push_bind(priority);
    }
    if let Some(text) = &filter.q {
        let pattern = format!(
            "%{}%",
            text.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        query
            .push(" AND (i.title ILIKE ")
            .push_bind(pattern.clone())
            .push(" OR t.key||'-'||i.number ILIKE ")
            .push_bind(pattern)
            .push(")");
    }
    query
        .push(" AND ((i.archived_at IS NOT NULL) OR (t.archived_at IS NOT NULL))=")
        .push_bind(filter.archived);
    if !filter.label_ids.is_empty() {
        if filter.label_mode.as_deref().unwrap_or("any") == "all" {
            query
                .push(" AND (SELECT count(*) FROM issue_labels il WHERE il.workspace_id=i.workspace_id AND il.issue_id=i.id AND il.label_id=ANY(")
                .push_bind(filter.label_ids.clone())
                .push("))=")
                .push_bind(filter.label_ids.len() as i64);
        } else {
            query
                .push(" AND EXISTS(SELECT 1 FROM issue_labels il WHERE il.workspace_id=i.workspace_id AND il.issue_id=i.id AND il.label_id=ANY(")
                .push_bind(filter.label_ids.clone())
                .push("))");
        }
    }
    if let Some((node, _)) = structured {
        query.push(" AND (");
        push_node(&mut query, node)?;
        query.push(")");
    }
    match cursor {
        Some(IssueListCursor::Standard(priority, date, id)) => {
            if filter.sort.as_deref() == Some("priority") {
                query
                    .push(" AND (")
                    .push(PRIORITY)
                    .push(">")
                    .push_bind(priority)
                    .push(" OR (")
                    .push(PRIORITY)
                    .push("=")
                    .push_bind(priority)
                    .push(" AND (i.created_at,i.id)<(")
                    .push_bind(date)
                    .push(",")
                    .push_bind(id)
                    .push(")))");
            } else {
                query
                    .push(" AND (i.created_at,i.id)<(")
                    .push_bind(date)
                    .push(",")
                    .push_bind(id)
                    .push(")");
            }
        }
        Some(IssueListCursor::Manual(category, position, status, rank, id)) => {
            query
                .push(" AND (")
                .push(CATEGORY)
                .push(",s.position,s.id,i.rank,i.id)>(")
                .push_bind(category)
                .push(",")
                .push_bind(position)
                .push(",")
                .push_bind(status)
                .push(",")
                .push_bind(rank)
                .push("::numeric,")
                .push_bind(id)
                .push(")");
        }
        None => {}
    }
    query.push(" ORDER BY ");
    match filter.sort.as_deref().unwrap_or("created") {
        "manual" => {
            query.push(CATEGORY).push(",s.position,s.id,i.rank,i.id");
        }
        "priority" => {
            query.push(PRIORITY).push(",i.created_at DESC,i.id DESC");
        }
        "created" => {
            query.push("i.created_at DESC,i.id DESC");
        }
        _ => return Err(invalid()),
    }
    query
        .push(" LIMIT ")
        .push_bind(filter.limit.unwrap_or(50) + 1);
    Ok(query)
}

pub(crate) fn push_node(
    query: &mut QueryBuilder<Postgres>,
    node: &IssueFilterNode,
) -> Result<(), sqlx::Error> {
    match node {
        IssueFilterNode::All { children } | IssueFilterNode::Any { children } => {
            let separator = if matches!(node, IssueFilterNode::All { .. }) {
                " AND "
            } else {
                " OR "
            };
            query.push("(");
            for (index, child) in children.iter().enumerate() {
                if index != 0 {
                    query.push(separator);
                }
                push_node(query, child)?;
            }
            query.push(")");
        }
        IssueFilterNode::Predicate {
            field,
            op,
            value,
            values,
        } => push_predicate(query, *field, *op, value.as_ref(), values.as_deref())?,
    }
    Ok(())
}

fn push_predicate(
    query: &mut QueryBuilder<Postgres>,
    field: Field,
    operator: Operator,
    value: Option<&Operand>,
    values: Option<&[Operand]>,
) -> Result<(), sqlx::Error> {
    if operator == Operator::IsNull {
        let Some(Operand::Boolean(is_null)) = value else {
            return Err(invalid());
        };
        if relation(field).is_some() {
            if *is_null {
                query.push("NOT ");
            }
            push_relation(query, field, None)?;
        } else {
            query
                .push(column(field)?)
                .push(if *is_null { " IS NULL" } else { " IS NOT NULL" });
        }
        return Ok(());
    }
    let operands = if matches!(operator, Operator::In | Operator::NotIn) {
        values.ok_or_else(invalid)?
    } else {
        std::slice::from_ref(value.ok_or_else(invalid)?)
    };
    if field.identifier() {
        let ids = operands
            .iter()
            .map(|operand| match operand {
                Operand::Text(text) => Uuid::parse_str(text).map_err(|_| invalid()),
                _ => Err(invalid()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if relation(field).is_some() {
            if operator == Operator::NotIn {
                query.push("(");
                if field != Field::Label {
                    push_relation(query, field, None)?;
                    query.push(" AND ");
                }
                query.push("NOT ");
                push_relation(query, field, Some(ids))?;
                query.push(")");
            } else {
                push_relation(query, field, Some(ids))?;
            }
        } else {
            query
                .push(column(field)?)
                .push(if operator == Operator::NotIn {
                    "<>ALL("
                } else {
                    "=ANY("
                })
                .push_bind(ids)
                .push(")");
        }
    } else if field == Field::Priority {
        let priorities = operands
            .iter()
            .map(|operand| match operand {
                Operand::Integer(value) => i16::try_from(*value).map_err(|_| invalid()),
                _ => Err(invalid()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        query
            .push("i.priority")
            .push(if operator == Operator::NotIn {
                "<>ALL("
            } else {
                "=ANY("
            })
            .push_bind(priorities)
            .push(")");
    } else if matches!(operator, Operator::In | Operator::NotIn) {
        push_date_set(query, field, operator, operands)?;
    } else {
        push_date_comparison(
            query,
            field,
            operator,
            operands.first().ok_or_else(invalid)?,
        )?;
    }
    Ok(())
}

fn column(field: Field) -> Result<&'static str, sqlx::Error> {
    match field {
        Field::Status => Ok("i.status_id"),
        Field::Assignee => Ok("i.assignee_id"),
        Field::Team => Ok("i.team_id"),
        Field::Priority => Ok("i.priority"),
        Field::CreatedAt => Ok("i.created_at"),
        Field::UpdatedAt => Ok("i.updated_at"),
        Field::DueDate => Ok("i.due_date"),
        _ => Err(invalid()),
    }
}

fn relation(field: Field) -> Option<(&'static str, &'static str)> {
    match field {
        Field::Project => Some(("project_issues", "project_id")),
        Field::Cycle => Some(("cycle_issues", "cycle_id")),
        Field::Label => Some(("issue_labels", "label_id")),
        _ => None,
    }
}

fn push_relation(
    query: &mut QueryBuilder<Postgres>,
    field: Field,
    ids: Option<Vec<Uuid>>,
) -> Result<(), sqlx::Error> {
    let (table, reference) = relation(field).ok_or_else(invalid)?;
    query
        .push("EXISTS(SELECT 1 FROM ")
        .push(table)
        .push(" query_ref WHERE query_ref.workspace_id=i.workspace_id AND query_ref.issue_id=i.id");
    if field != Field::Label {
        query.push(" AND query_ref.team_id=i.team_id");
    }
    if let Some(ids) = ids {
        query
            .push(" AND query_ref.")
            .push(reference)
            .push("=ANY(")
            .push_bind(ids)
            .push(")");
    }
    query.push(")");
    Ok(())
}

fn push_date_comparison(
    query: &mut QueryBuilder<Postgres>,
    field: Field,
    operator: Operator,
    operand: &Operand,
) -> Result<(), sqlx::Error> {
    if field != Field::DueDate
        && let Operand::Text(text) = operand
        && let Ok(instant) = DateTime::parse_from_rfc3339(text)
    {
        query
            .push(column(field)?)
            .push(comparison(operator)?)
            .push_bind(instant.with_timezone(&Utc));
        return Ok(());
    }
    push_local_date_column(query, field)?;
    query.push(comparison(operator)?);
    push_calendar_date(query, operand)
}

fn push_date_set(
    query: &mut QueryBuilder<Postgres>,
    field: Field,
    operator: Operator,
    operands: &[Operand],
) -> Result<(), sqlx::Error> {
    let mut instants = Vec::new();
    let mut dates = Vec::new();
    let mut relative_days = Vec::new();
    for operand in operands {
        match operand {
            Operand::Text(text) => {
                if field != Field::DueDate
                    && let Ok(instant) = DateTime::parse_from_rfc3339(text)
                {
                    instants.push(instant.with_timezone(&Utc));
                } else {
                    dates.push(NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| invalid())?);
                }
            }
            Operand::Relative(relative) => relative_days.push(relative.relative_days),
            _ => return Err(invalid()),
        }
    }
    if instants.is_empty() && dates.is_empty() && relative_days.is_empty() {
        return Err(invalid());
    }
    if operator == Operator::NotIn {
        query.push("NOT ");
    }
    query.push("(");
    let has_instants = !instants.is_empty();
    if has_instants {
        query
            .push(column(field)?)
            .push("=ANY(")
            .push_bind(instants)
            .push(")");
    }
    if !dates.is_empty() || !relative_days.is_empty() {
        if has_instants {
            query.push(" OR ");
        }
        push_local_date_column(query, field)?;
        query.push("=ANY(");
        if !dates.is_empty() {
            query.push_bind(dates).push("::date[]");
            if !relative_days.is_empty() {
                query.push("||");
            }
        }
        if !relative_days.is_empty() {
            query.push("ARRAY(SELECT (query_clock.as_of AT TIME ZONE query_clock.timezone)::date+query_day.day_offset FROM unnest(")
                .push_bind(relative_days)
                .push("::integer[]) query_day(day_offset))");
        }
        query.push(")");
    }
    query.push(")");
    Ok(())
}

fn push_local_date_column(
    query: &mut QueryBuilder<Postgres>,
    field: Field,
) -> Result<(), sqlx::Error> {
    if field == Field::DueDate {
        query.push("i.due_date");
    } else {
        // A local date need not be one UTC interval. Midnight can repeat,
        // a rollback can briefly return to the preceding date, and a date can
        // be skipped entirely. Compare the actual calendar date of each instant.
        query
            .push("(")
            .push(column(field)?)
            .push(" AT TIME ZONE query_clock.timezone)::date");
    }
    Ok(())
}

fn comparison(operator: Operator) -> Result<&'static str, sqlx::Error> {
    match operator {
        Operator::Eq => Ok("="),
        Operator::Before => Ok("<"),
        Operator::After => Ok(">"),
        _ => Err(invalid()),
    }
}

fn push_calendar_date(
    query: &mut QueryBuilder<Postgres>,
    operand: &Operand,
) -> Result<(), sqlx::Error> {
    query.push("(");
    match operand {
        Operand::Text(text) => {
            query.push_bind(NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| invalid())?);
            query.push("::date");
        }
        Operand::Relative(relative) => {
            query
                .push("(query_clock.as_of AT TIME ZONE query_clock.timezone)::date+")
                .push_bind(relative.relative_days);
        }
        _ => return Err(invalid()),
    }
    query.push(")");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentrack_domain::{
        ISSUE_FILTER_MAX_PREDICATES, ISSUE_FILTER_MAX_VALUES, IssueQueryRelativeDate,
    };
    use serde_json::json;
    use sqlx::{Arguments, Execute};

    fn node(value: Value) -> IssueFilterNode {
        serde_json::from_value(value).unwrap()
    }

    fn context() -> IssueQueryContext {
        IssueQueryContext {
            timezone: "Europe/Zurich".into(),
            as_of: DateTime::parse_from_rfc3339("2026-03-29T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    #[test]
    fn request_strings_and_uuid_values_never_become_sql_fragments() {
        let target = Uuid::new_v4();
        let filter = IssueFilter {
            q: Some("'; SELECT private_content -- %_\\".into()),
            ..IssueFilter::default()
        };
        let predicate =
            node(json!({"kind":"predicate","field":"project","op":"in","values":[target]}));
        let mut clock = context();
        clock.timezone = "untrusted timezone '; --".into();
        let mut query = build_issue_query(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &filter,
            None,
            Some((&predicate, &clock)),
        )
        .unwrap();
        let sql = query.sql();
        assert!(!sql.as_str().contains("private_content"));
        assert!(!sql.as_str().contains(&target.to_string()));
        assert!(!sql.as_str().contains("untrusted timezone"));
        assert!(sql.as_str().contains("i.deleted_at IS NULL"));
        assert!(query.build().take_arguments().unwrap().unwrap().len() >= 8);
    }

    #[test]
    fn logical_alternatives_remain_inside_the_unconditional_authorized_scope() {
        let predicate = node(json!({"kind":"any","children":[
            {"kind":"predicate","field":"priority","op":"eq","value":1},
            {"kind":"all","children":[
                {"kind":"predicate","field":"due_date","op":"is_null","value":true},
                {"kind":"predicate","field":"assignee","op":"is_null","value":false}
            ]}
        ]}));
        let query = build_issue_query(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &IssueFilter::default(),
            None,
            Some((&predicate, &context())),
        )
        .unwrap();
        let sql = query.into_string();
        assert!(sql.contains("tm.user_id="));
        assert!(sql.contains(" AND ((i.priority=ANY("));
        assert!(sql.contains(" OR (i.due_date IS NULL AND i.assignee_id IS NOT NULL))"));
        assert!(sql.find("i.deleted_at IS NULL").unwrap() < sql.find(" AND ((i.priority").unwrap());
    }

    #[test]
    fn empty_groups_and_invalid_cross_field_operations_are_not_compiled() {
        for predicate in [
            node(json!({"kind":"any","children":[]})),
            node(json!({"kind":"predicate","field":"priority","op":"before","value":1})),
            node(json!({"kind":"predicate","field":"due_date","op":"eq","value":"';SELECT 1"})),
        ] {
            assert!(
                build_issue_query(
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    &IssueFilter::default(),
                    None,
                    Some((&predicate, &context()))
                )
                .is_err()
            );
        }
    }

    #[test]
    fn only_the_matching_cursor_kind_can_continue_each_order() {
        let id = Uuid::new_v4();
        let standard = IssueListCursor::Standard(1, context().as_of, id);
        let manual = IssueFilter {
            team_id: Some(id),
            sort: Some("manual".into()),
            ..IssueFilter::default()
        };
        assert!(build_issue_query(id, id, &manual, Some(standard), None).is_err());
        let rank = IssueListCursor::Manual(1, 0, id, "0;SELECT 1".into(), id);
        assert!(build_issue_query(id, id, &IssueFilter::default(), Some(rank), None).is_err());
        let rank = IssueListCursor::Manual(1, 0, id, "0;SELECT 1".into(), id);
        let sql = build_issue_query(id, id, &manual, Some(rank), None)
            .unwrap()
            .into_string();
        assert!(!sql.contains("0;SELECT 1"));
        assert!(sql.contains("::numeric,"));
    }

    #[test]
    fn largest_relative_day_sets_stay_below_the_postgresql_parameter_limit() {
        let predicate = IssueFilterNode::All {
            children: (0..ISSUE_FILTER_MAX_PREDICATES)
                .map(|_| IssueFilterNode::Predicate {
                    field: Field::CreatedAt,
                    op: Operator::In,
                    value: None,
                    values: Some(
                        (0..ISSUE_FILTER_MAX_VALUES)
                            .map(|day| {
                                Operand::Relative(IssueQueryRelativeDate {
                                    relative_days: day as i32,
                                })
                            })
                            .collect(),
                    ),
                })
                .collect(),
        };
        let mut query = build_issue_query(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &IssueFilter::default(),
            None,
            Some((&predicate, &context())),
        )
        .unwrap();
        assert!(
            query.sql().as_str().len() < 64 * 1024,
            "Supported date sets should not expand into thousands of scalar comparisons"
        );
        let arguments = query.build().take_arguments().unwrap().unwrap().len();
        assert!(
            arguments < 65_535,
            "Supported query exceeds the PostgreSQL parameter budget"
        );
    }

    const HAVANA: &[(&str, &str)] = &[
        ("before", "2026-11-01T03:59:59Z"),
        ("early", "2026-11-01T04:30:00Z"),
        ("late", "2026-11-01T05:30:00Z"),
        ("next", "2026-11-02T05:00:00Z"),
    ];
    const GOOSE_BAY: &[(&str, &str)] = &[
        ("before", "1988-10-30T01:59:59Z"),
        ("first-minute", "1988-10-30T02:00:30Z"),
        ("previous-date-hole", "1988-10-30T03:30:00Z"),
        ("later", "1988-10-30T04:00:30Z"),
        ("next", "1988-10-31T04:00:00Z"),
    ];
    const APIA: &[(&str, &str)] = &[
        ("before", "2011-12-30T09:30:00Z"),
        ("after", "2011-12-30T10:30:00Z"),
        ("next", "2011-12-31T10:30:00Z"),
    ];

    async fn calendar_pool() -> sqlx::PgPool {
        use std::str::FromStr;
        let url = std::env::var("OPENTRACK_TEST_DATABASE_URL")
            .expect("Calendar query contracts require the isolated PostgreSQL test URL");
        let options = sqlx::postgres::PgConnectOptions::from_str(&url)
            .expect("Parse the isolated PostgreSQL test connection");
        assert!(
            options
                .get_database()
                .is_some_and(|name| name.ends_with("_test")),
            "Calendar query contracts require an isolated *_test database"
        );
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("Connect to isolated PostgreSQL");
        sqlx::query("SET statement_timeout='3s'")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    async fn calendar_labels(
        pool: &sqlx::PgPool,
        zone: &str,
        rows: &[(&str, &str)],
        clock: &str,
        predicate: Value,
    ) -> Vec<String> {
        let predicate = node(predicate);
        predicate.validate().unwrap();
        let mut query = QueryBuilder::<Postgres>::new("SELECT i.label FROM (VALUES ");
        for (index, (label, instant)) in rows.iter().enumerate() {
            if index > 0 {
                query.push(",");
            }
            let instant = DateTime::parse_from_rfc3339(instant)
                .unwrap()
                .with_timezone(&Utc);
            query
                .push("(")
                .push_bind((*label).to_owned())
                .push(",")
                .push_bind(instant)
                .push(",")
                .push_bind(instant)
                .push(")");
        }
        query
            .push(") i(label,created_at,updated_at) CROSS JOIN (SELECT ")
            .push_bind(
                DateTime::parse_from_rfc3339(clock)
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .push("::timestamptz AS as_of,")
            .push_bind(zone.to_owned())
            .push("::text AS timezone) query_clock WHERE ");
        push_node(&mut query, &predicate).unwrap();
        query.push(" ORDER BY i.label");
        query
            .build_query_scalar::<String>()
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn local_calendar_havana_includes_both_midnight_folds() {
        let pool = calendar_pool().await;
        for field in ["created_at", "updated_at"] {
            for (op, expected) in [
                ("eq", vec!["early", "late"]),
                ("before", vec!["before"]),
                ("after", vec!["next"]),
            ] {
                let actual = calendar_labels(
                    &pool,
                    "America/Havana",
                    HAVANA,
                    "2026-11-01T18:00:00Z",
                    json!({"kind":"predicate","field":field,"op":op,"value":"2026-11-01"}),
                )
                .await;
                assert_eq!(actual, expected, "Havana {field} {op} local date");
            }
        }
        let relative = calendar_labels(
            &pool,
            "America/Havana",
            HAVANA,
            "2026-11-01T18:00:00Z",
            json!({"kind":"predicate","field":"created_at","op":"eq","value":{"relative_days":0}}),
        )
        .await;
        assert_eq!(relative, ["early", "late"]);
        pool.close().await;
    }

    #[sqlx::test]
    async fn local_calendar_goose_bay_keeps_the_first_minute_and_previous_date_hole_separate() {
        let pool = calendar_pool().await;
        for (date, op, expected) in [
            ("1988-10-30", "eq", vec!["first-minute", "later"]),
            ("1988-10-30", "before", vec!["before", "previous-date-hole"]),
            ("1988-10-30", "after", vec!["next"]),
            ("1988-10-29", "eq", vec!["before", "previous-date-hole"]),
        ] {
            let actual = calendar_labels(
                &pool,
                "America/Goose_Bay",
                GOOSE_BAY,
                "1988-10-30T18:00:00Z",
                json!({"kind":"predicate","field":"created_at","op":op,"value":date}),
            )
            .await;
            assert_eq!(actual, expected, "Goose Bay {op} {date}");
        }
        pool.close().await;
    }

    #[sqlx::test]
    async fn local_calendar_apia_skipped_day_stays_empty_without_broadening() {
        let pool = calendar_pool().await;
        for (op, expected) in [
            ("eq", vec![]),
            ("before", vec!["before"]),
            ("after", vec!["after", "next"]),
        ] {
            let actual = calendar_labels(
                &pool,
                "Pacific/Apia",
                APIA,
                "2011-12-29T18:00:00Z",
                json!({"kind":"predicate","field":"created_at","op":op,"value":"2011-12-30"}),
            )
            .await;
            assert_eq!(actual, expected, "Apia {op} skipped local date");
        }
        pool.close().await;
    }

    #[sqlx::test]
    async fn local_calendar_sets_combine_exact_instants_with_complete_local_dates() {
        let pool = calendar_pool().await;
        for (op, expected) in [
            ("in", vec!["early", "late", "next"]),
            ("not_in", vec!["before"]),
        ] {
            let actual = calendar_labels(
                &pool,
                "America/Havana",
                HAVANA,
                "2026-11-01T18:00:00Z",
                json!({"kind":"predicate","field":"created_at","op":op,
                    "values":["2026-11-01", "2026-11-02T05:00:00Z", {"relative_days":0}]}),
            )
            .await;
            assert_eq!(actual, expected, "Havana mixed date/instant {op}");
        }
        pool.close().await;
    }
}
