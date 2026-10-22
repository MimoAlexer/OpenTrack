use super::*;
use opentrack_domain::{IssueAssignee, Label, LabelQuery};

const LABEL_SELECT: &str = "SELECT jsonb_build_object('id',l.id,'team_id',l.team_id,'name',l.name,'color',l.color,'archived_at',l.archived_at,'version',l.version::text,'can_manage',w.archived_at IS NULL AND (l.team_id IS NULL OR t.archived_at IS NULL) AND wm.role<>'viewer' AND (wm.role IN('owner','admin') OR (l.team_id IS NOT NULL AND COALESCE(tm.role='maintainer',false)))) AS body,lower(l.name) AS sort_name FROM labels l JOIN workspaces w ON w.id=l.workspace_id JOIN workspace_members wm ON wm.workspace_id=l.workspace_id AND wm.user_id=$2 AND wm.active LEFT JOIN teams t ON t.workspace_id=l.workspace_id AND t.id=l.team_id LEFT JOIN team_members tm ON tm.workspace_id=l.workspace_id AND tm.team_id=l.team_id AND tm.user_id=$2";
const LABEL_VISIBLE: &str = " AND (l.team_id IS NULL OR NOT t.private OR tm.user_id IS NOT NULL)";
pub async fn label(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    id: Uuid,
) -> Result<Option<Label>, sqlx::Error> {
    let mut query = sqlx::QueryBuilder::<Postgres>::new(LABEL_SELECT);
    query
        .push(" WHERE l.workspace_id=$1 AND l.id=$3")
        .push(LABEL_VISIBLE);
    let body = query
        .build_query_scalar::<Value>()
        .bind(w)
        .bind(user)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    body.map(decode).transpose()
}
/// Selection, ordering key, authorization and display metadata share one MVCC
/// snapshot, including while a team label is renamed or archived concurrently.
pub async fn label_page(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    q: &LabelQuery,
    after: Option<&(String, Uuid)>,
) -> Result<Vec<(Label, String)>, sqlx::Error> {
    let term = q.q.as_deref().unwrap_or("");
    let pattern = format!(
        "%{}%",
        term.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut query = sqlx::QueryBuilder::<Postgres>::new(LABEL_SELECT);
    query.push(" WHERE l.workspace_id=$1").push(LABEL_VISIBLE).push(" AND ($3='visible' OR ($3='workspace' AND l.team_id IS NULL) OR ($3='team' AND l.team_id=$4) OR ($3='applicable' AND (l.team_id IS NULL OR l.team_id=$4))) AND ($5='all' OR (l.archived_at IS NOT NULL)=($5='archived')) AND l.name ILIKE $6 AND ($7::text IS NULL OR (lower(l.name),l.id)>($7,$8)) ORDER BY lower(l.name),l.id LIMIT $9");
    let rows = query
        .build()
        .bind(w)
        .bind(user)
        .bind(q.scope.as_deref().unwrap_or("visible"))
        .bind(q.team_id)
        .bind(q.state.as_deref().unwrap_or("active"))
        .bind(pattern)
        .bind(after.map(|a| &a.0))
        .bind(after.map(|a| a.1))
        .bind(i64::from(q.limit.unwrap_or(20)) + 1)
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|row| Ok((decode(row.try_get("body")?)?, row.try_get("sort_name")?)))
        .collect()
}
pub async fn assignable(tx: &mut Tx, w: Uuid, team: Uuid, user: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id JOIN workspaces w ON w.id=m.workspace_id JOIN teams t ON t.workspace_id=m.workspace_id AND t.id=$2 WHERE m.workspace_id=$1 AND m.user_id=$3 AND m.active AND m.role<>'viewer' AND u.disabled_at IS NULL AND w.archived_at IS NULL AND t.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=$2 AND tm.user_id=$3)))").bind(w).bind(team).bind(user).fetch_one(&mut **tx).await
}
pub async fn issue_assignee(
    tx: &mut Tx,
    w: Uuid,
    team: Uuid,
    user: Option<Uuid>,
) -> Result<Option<IssueAssignee>, sqlx::Error> {
    let Some(user) = user else {
        return Ok(None);
    };
    let display_name:String=sqlx::query_scalar("SELECT u.display_name FROM users u JOIN workspace_members m ON m.user_id=u.id WHERE m.workspace_id=$1 AND u.id=$2").bind(w).bind(user).fetch_one(&mut **tx).await?;
    Ok(Some(IssueAssignee {
        id: user,
        display_name,
        assignable: assignable(tx, w, team, user).await?,
    }))
}
/// Upgrade only additive fields of an older stored command outcome, after the
/// caller has reauthorized its recorded team. Do not replace its business result.
pub async fn hydrate_legacy_issue(
    tx: &mut Tx,
    issue: &mut Issue,
    missing_assignee: bool,
) -> Result<(), sqlx::Error> {
    if missing_assignee {
        issue.assignee =
            issue_assignee(tx, issue.workspace_id, issue.team_id, issue.assignee_id).await?;
    }
    if issue.body_version == "0" {
        let migrated: bool = sqlx::query_scalar("SELECT to_regclass('documents') IS NOT NULL")
            .fetch_one(&mut **tx)
            .await?;
        if migrated {
            issue.body_version = "1".into();
        }
    }
    issue.refresh_etag();
    Ok(())
}
pub async fn validate_issue_labels(
    tx: &mut Tx,
    w: Uuid,
    team: Uuid,
    selected: &[Uuid],
    retained: &[Uuid],
) -> Result<bool, sqlx::Error> {
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM labels WHERE workspace_id=$1 AND id=ANY($3) AND (team_id IS NULL OR team_id=$2) AND (archived_at IS NULL OR id=ANY($4))").bind(w).bind(team).bind(selected).bind(retained).fetch_one(&mut **tx).await?;
    Ok(count == selected.len() as i64)
}
pub async fn set_issue_labels(
    tx: &mut Tx,
    w: Uuid,
    issue: Uuid,
    selected: &[Uuid],
) -> Result<(), sqlx::Error> {
    let old:Vec<Uuid>=sqlx::query_scalar("SELECT label_id FROM issue_labels WHERE workspace_id=$1 AND issue_id=$2 ORDER BY label_id FOR UPDATE").bind(w).bind(issue).fetch_all(&mut **tx).await?;
    let remove: Vec<Uuid> = old
        .iter()
        .filter(|id| !selected.contains(id))
        .copied()
        .collect();
    let deleted = sqlx::query(
        "DELETE FROM issue_labels WHERE workspace_id=$1 AND issue_id=$2 AND label_id=ANY($3)",
    )
    .bind(w)
    .bind(issue)
    .bind(&remove)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if deleted != remove.len() as u64 {
        return Err(sqlx::Error::Protocol(
            "Required issue label removal was suppressed".into(),
        ));
    }
    for id in selected.iter().filter(|id| !old.contains(id)) {
        if sqlx::query("INSERT INTO issue_labels(workspace_id,issue_id,label_id) VALUES($1,$2,$3)")
            .bind(w)
            .bind(issue)
            .bind(id)
            .execute(&mut **tx)
            .await?
            .rows_affected()
            != 1
        {
            return Err(sqlx::Error::Protocol(
                "Required issue label link was suppressed".into(),
            ));
        }
    }
    let actual: Vec<Uuid> = sqlx::query_scalar(
        "SELECT label_id FROM issue_labels WHERE workspace_id=$1 AND issue_id=$2 ORDER BY label_id",
    )
    .bind(w)
    .bind(issue)
    .fetch_all(&mut **tx)
    .await?;
    if actual != selected {
        return Err(sqlx::Error::Protocol(
            "Issue label set differs from command".into(),
        ));
    }
    Ok(())
}
