use crate::{App, db_error, issue_ordering};
use chrono::{DateTime, Utc};
use opentrack_domain::{
    Actor, Error, IssueFilterNode, IssuePage, IssueQuery, IssueQueryContext, IssueQueryField,
    IssueQueryOperator, IssueQueryPage, IssueQuerySort, IssueQueryValue, Result,
};
use opentrack_store as store;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
enum QueryPosition {
    Standard(i16, DateTime<Utc>, Uuid),
    Manual(i32, i32, Uuid, String, Uuid),
}
#[derive(Serialize, Deserialize)]
struct QueryCursor {
    context: IssueQueryContext,
    position: QueryPosition,
}

fn targets(node: &IssueFilterNode, selected: &mut BTreeMap<IssueQueryField, BTreeSet<Uuid>>) {
    match node {
        IssueFilterNode::All { children } | IssueFilterNode::Any { children } => {
            for child in children {
                targets(child, selected);
            }
        }
        IssueFilterNode::Predicate {
            field,
            op,
            value,
            values,
        } if field.identifier() && *op != IssueQueryOperator::IsNull => {
            for value in value.iter().chain(values.iter().flatten()) {
                if let IssueQueryValue::Text(text) = value
                    && let Ok(id) = Uuid::parse_str(text)
                {
                    selected.entry(*field).or_default().insert(id);
                }
            }
        }
        _ => {}
    }
}

pub(crate) async fn validate_targets(
    tx: &mut store::Tx,
    actor: &Actor,
    workspace: Uuid,
    query: &IssueQuery,
) -> Result<()> {
    let mut selected = BTreeMap::new();
    if let Some(filter) = &query.filter {
        targets(filter, &mut selected);
    }
    if let Some(team) = query.team_id {
        selected
            .entry(IssueQueryField::Team)
            .or_default()
            .insert(team);
    }
    for (field, ids) in selected {
        // Each source query applies current workspace/team visibility before a
        // target is accepted, including targets used by a negative predicate.
        let sql = match field {
            IssueQueryField::Team => {
                "SELECT count(*) FROM teams t WHERE t.workspace_id=$1 AND t.id=ANY($2) AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))"
            }
            IssueQueryField::Status => {
                "SELECT count(*) FROM workflow_statuses s JOIN teams t ON t.workspace_id=s.workspace_id AND t.id=s.team_id WHERE s.workspace_id=$1 AND s.id=ANY($2) AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))"
            }
            IssueQueryField::Assignee => {
                "SELECT count(*) FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND m.user_id=ANY($2) AND m.active AND u.disabled_at IS NULL AND $3::uuid IS NOT NULL"
            }
            IssueQueryField::Project => {
                "SELECT count(*) FROM projects p JOIN teams t ON t.workspace_id=p.workspace_id AND t.id=p.team_id WHERE p.workspace_id=$1 AND p.id=ANY($2) AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))"
            }
            IssueQueryField::Cycle => {
                "SELECT count(*) FROM cycles c JOIN teams t ON t.workspace_id=c.workspace_id AND t.id=c.team_id WHERE c.workspace_id=$1 AND c.id=ANY($2) AND c.deleted_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))"
            }
            IssueQueryField::Label => {
                "SELECT count(*) FROM labels l LEFT JOIN teams t ON t.workspace_id=l.workspace_id AND t.id=l.team_id WHERE l.workspace_id=$1 AND l.id=ANY($2) AND (l.team_id IS NULL OR NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))"
            }
            _ => return Err(Error::Internal),
        };
        let ids = ids.into_iter().collect::<Vec<_>>();
        let visible: i64 = sqlx::query_scalar(sql)
            .bind(workspace)
            .bind(&ids)
            .bind(actor.user.id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
        if visible != ids.len() as i64 {
            return Err(Error::Invalid(
                "A selected filter value is unavailable; repair or clear that condition".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) async fn query_clock(tx: &mut store::Tx, timezone: &str) -> Result<DateTime<Utc>> {
    let (as_of, valid_zone): (DateTime<Utc>, bool) = sqlx::query_as(
        "SELECT statement_timestamp(),EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=$1)",
    )
    .bind(timezone)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if !valid_zone || !(timezone == "UTC" || timezone.contains('/')) {
        return Err(Error::Invalid(
            "Choose a valid IANA query timezone, such as UTC or Europe/Zurich".into(),
        ));
    }
    Ok(as_of)
}

impl App {
    pub async fn query_issues(
        &self,
        actor: &Actor,
        workspace: Uuid,
        query: IssueQuery,
    ) -> Result<IssueQueryPage> {
        query.validate()?;
        let mut tx = self.scoped(actor, workspace, false).await?;
        let result = self
            .query_issues_in(&mut tx, actor, workspace, query)
            .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub(crate) async fn query_issues_in(
        &self,
        tx: &mut store::Tx,
        actor: &Actor,
        workspace: Uuid,
        mut query: IssueQuery,
    ) -> Result<IssueQueryPage> {
        query.validate()?;
        if query.sort == IssueQuerySort::Manual {
            query.q = query
                .q
                .as_deref()
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .map(str::to_owned);
        }
        let epoch = store::auth_epoch(tx, workspace).await.map_err(db_error)?;
        validate_targets(tx, actor, workspace, &query).await?;
        let as_of = query_clock(tx, &query.timezone).await?;
        let manual = query.sort == IssueQuerySort::Manual;
        let order_version = if manual {
            let team = query.team_id.ok_or(Error::Internal)?;
            let active: bool = sqlx::query_scalar(
                "SELECT archived_at IS NULL FROM teams WHERE workspace_id=$1 AND id=$2 FOR SHARE",
            )
            .bind(workspace)
            .bind(team)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
            if !active {
                return Err(Error::Conflict(
                    "Restore this team before using manual order".into(),
                ));
            }
            if let Some(progress) = issue_ordering::active(tx, actor, workspace, team, None).await?
            {
                return Err(Error::IssueOrderMaintenance(vec![progress]));
            }
            Some(issue_ordering::order_version(tx, workspace, team).await?)
        } else {
            None
        };
        let mut fingerprint = query.clone();
        fingerprint.cursor = None;
        let binding = format!(
            "{workspace}:{}:{epoch}:{}:{}",
            actor.user.id,
            order_version.as_deref().unwrap_or("standard"),
            serde_json::to_string(&fingerprint).map_err(|_| Error::Internal)?
        );
        let saved: Option<QueryCursor> = query
            .cursor
            .as_deref()
            .map(|cursor| self.taxonomy_cursor_decode("issue-query-v1", &binding, cursor))
            .transpose()?;
        let (context, after) = match saved {
            Some(saved) => {
                if saved.context.timezone != query.timezone {
                    return Err(Error::Invalid(
                        "This cursor belongs to another query timezone".into(),
                    ));
                }
                let after = match saved.position {
                    QueryPosition::Standard(priority, time, id) if !manual => {
                        store::IssueListCursor::Standard(priority, time, id)
                    }
                    QueryPosition::Manual(category, position, status, rank, id) if manual => {
                        store::IssueListCursor::Manual(category, position, status, rank, id)
                    }
                    _ => {
                        return Err(Error::Invalid(
                            "This cursor belongs to another query ordering".into(),
                        ));
                    }
                };
                (saved.context, Some(after))
            }
            None => (
                IssueQueryContext {
                    timezone: query.timezone.clone(),
                    as_of,
                },
                None,
            ),
        };
        let mut data = store::list_issue_query(
            tx,
            workspace,
            actor.user.id,
            &query.flat_filter(),
            after,
            query.filter.as_ref().map(|filter| (filter, &context)),
        )
        .await
        .map_err(db_error)?;
        let has_more = data.len() > query.limit as usize;
        data.truncate(query.limit as usize);
        let next_cursor = if has_more {
            let last = data.last().ok_or(Error::Internal)?;
            let position = if manual {
                let (category, position, status, rank, id): (i32,i32,Uuid,String,Uuid) = sqlx::query_as("SELECT CASE s.category WHEN 'triage' THEN 0 WHEN 'backlog' THEN 1 WHEN 'unstarted' THEN 2 WHEN 'started' THEN 3 WHEN 'completed' THEN 4 ELSE 5 END,s.position,s.id,i.rank::text,i.id FROM issues i JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.id=$3")
                    .bind(workspace).bind(query.team_id).bind(last.id).fetch_one(&mut **tx).await.map_err(db_error)?;
                QueryPosition::Manual(category, position, status, rank, id)
            } else {
                QueryPosition::Standard(
                    if last.priority == 0 { 5 } else { last.priority },
                    last.created_at,
                    last.id,
                )
            };
            Some(self.taxonomy_cursor_encode(
                "issue-query-v1",
                &binding,
                &QueryCursor {
                    context: context.clone(),
                    position,
                },
            )?)
        } else {
            None
        };
        let mut order_maintenance = Vec::new();
        if !manual
            && let Some(team) = query.team_id
            && let Some(progress) = issue_ordering::active(tx, actor, workspace, team, None).await?
        {
            order_maintenance.push(progress);
        }
        Ok(IssueQueryPage {
            page: IssuePage {
                data,
                next_cursor,
                has_more,
                order_version,
                order_maintenance,
            },
            query_context: context,
        })
    }
}
