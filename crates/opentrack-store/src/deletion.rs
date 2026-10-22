use super::*;
use opentrack_domain::DeletedIssueQuery;

pub struct IssueDeletionState {
    pub issue: Issue,
    pub deleted_at: Option<DateTime<Utc>>,
    pub restore_until: Option<DateTime<Utc>>,
    pub within_restore: bool,
    pub authorized: bool,
    pub active: bool,
}
const LIFECYCLE_SELECT: &str = "SELECT p.body,p.deleted_at,
 p.deleted_at + interval '30 days' AS restore_until,
 COALESCE(clock_timestamp() < p.deleted_at + interval '30 days',false) AS within_restore,
 wm.role<>'viewer' AND (p.created_by_user_id=$2 OR wm.role IN ('owner','admin') OR EXISTS(
 SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=p.team_id AND tm.user_id=$2 AND tm.role='maintainer')) AS authorized,
 p.archived_at IS NULL AND t.archived_at IS NULL AND w.archived_at IS NULL AS active
 FROM (";
const LIFECYCLE_SUFFIX: &str = ") p JOIN teams t ON t.workspace_id=$1 AND t.id=p.team_id
 JOIN workspaces w ON w.id=$1 JOIN workspace_members wm ON wm.workspace_id=$1 AND wm.user_id=$2 AND wm.active ";
fn row(r: sqlx::postgres::PgRow) -> Result<IssueDeletionState, sqlx::Error> {
    Ok(IssueDeletionState {
        issue: issue_from(r.try_get("body")?)?,
        deleted_at: r.try_get("deleted_at")?,
        restore_until: r.try_get("restore_until")?,
        within_restore: r.try_get("within_restore")?,
        authorized: r.try_get("authorized")?,
        active: r.try_get("active")?,
    })
}
pub async fn issue_deletion_state(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    id: Uuid,
) -> Result<Option<IssueDeletionState>, sqlx::Error> {
    let mut q = sqlx::QueryBuilder::<Postgres>::new(LIFECYCLE_SELECT);
    q.push(ISSUE_SELECT)
        .push(" WHERE i.workspace_id=$1 AND i.id=$3 ")
        .push(VISIBLE)
        .push(LIFECYCLE_SUFFIX);
    q.build()
        .bind(w)
        .bind(user)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .map(row)
        .transpose()
}
pub async fn deleted_issues(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    filter: &DeletedIssueQuery,
    after: Option<(DateTime<Utc>, Uuid)>,
) -> Result<Vec<IssueDeletionState>, sqlx::Error> {
    let pattern = filter.q.as_ref().map(|s| {
        format!(
            "%{}%",
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let mut q = sqlx::QueryBuilder::<Postgres>::new(LIFECYCLE_SELECT);
    q.push(ISSUE_SELECT).push(" WHERE i.workspace_id=$1 AND i.deleted_at IS NOT NULL ").push(VISIBLE)
        .push(" AND ($3::uuid IS NULL OR i.team_id=$3) AND ($4::text IS NULL OR i.title ILIKE $4 OR t.key||'-'||i.number ILIKE $4) AND ($5::timestamptz IS NULL OR (i.deleted_at,i.id)<($5,$6)) ")
        .push(LIFECYCLE_SUFFIX).push(" ORDER BY p.deleted_at DESC,p.issue_id DESC LIMIT $7");
    q.build()
        .bind(w)
        .bind(user)
        .bind(filter.team_id)
        .bind(pattern)
        .bind(after.map(|x| x.0))
        .bind(after.map(|x| x.1))
        .bind(i64::from(filter.limit.unwrap_or(50)) + 1)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(row)
        .collect()
}
