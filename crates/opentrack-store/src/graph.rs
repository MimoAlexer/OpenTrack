use super::*;
use opentrack_domain::{GraphStatus, IssueGraphTarget};

#[derive(Debug)]
pub struct GraphIssue {
    pub id: Uuid,
    pub team_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub version: i64,
    pub archived_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}
pub async fn graph_issue(
    tx: &mut Tx,
    w: Uuid,
    id: Uuid,
) -> Result<Option<GraphIssue>, sqlx::Error> {
    sqlx::query("SELECT id,team_id,parent_id,version,archived_at,deleted_at FROM issues WHERE workspace_id=$1 AND id=$2")
        .bind(w).bind(id).fetch_optional(&mut **tx).await?.map(|r| Ok(GraphIssue {
            id:r.try_get("id")?, team_id:r.try_get("team_id")?, parent_id:r.try_get("parent_id")?, version:r.try_get("version")?, archived_at:r.try_get("archived_at")?, deleted_at:r.try_get("deleted_at")?
        })).transpose()
}
pub async fn graph_target(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    id: Uuid,
) -> Result<Option<IssueGraphTarget>, sqlx::Error> {
    let Some(issue) = issue(tx, w, user, id, false).await? else {
        return Ok(None);
    };
    let status=sqlx::query("SELECT id,name,color,category FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND id=$3")
        .bind(w).bind(issue.team_id).bind(issue.status_id).fetch_one(&mut **tx).await?;
    Ok(Some(IssueGraphTarget {
        id: issue.id,
        key: issue.key,
        title: issue.title,
        status: GraphStatus {
            id: status.try_get("id")?,
            name: status.try_get("name")?,
            color: status.try_get("color")?,
            category: status.try_get("category")?,
        },
        archived_at: issue.archived_at,
        etag: issue.etag,
    }))
}
pub async fn lock_graph_teams(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    ids: &[Uuid],
    write: bool,
) -> Result<bool, sqlx::Error> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let suffix = if write {
        " FOR UPDATE OF t"
    } else {
        " FOR SHARE OF t"
    };
    for id in ids {
        let mut q = sqlx::QueryBuilder::<Postgres>::new(
            "SELECT t.id FROM teams t WHERE t.workspace_id=$1 AND t.id=$2 AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))",
        );
        q.push(suffix);
        if q.build_query_scalar::<Uuid>()
            .bind(w)
            .bind(id)
            .bind(user)
            .fetch_optional(&mut **tx)
            .await?
            .is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}
pub async fn lock_graph_issues(
    tx: &mut Tx,
    w: Uuid,
    team: Uuid,
    ids: &[Uuid],
) -> Result<bool, sqlx::Error> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        let actual: Option<Uuid> = sqlx::query_scalar(
            "SELECT team_id FROM issues WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(w)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
        if actual != Some(team) {
            return Ok(false);
        }
    }
    Ok(true)
}
#[derive(Debug)]
pub struct GraphRelation {
    pub id: Uuid,
    pub team_id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub kind: String,
}
pub async fn graph_relation(
    tx: &mut Tx,
    w: Uuid,
    id: Uuid,
) -> Result<Option<GraphRelation>, sqlx::Error> {
    sqlx::query("SELECT id,team_id,source_id,target_id,kind FROM issue_relations WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).fetch_optional(&mut **tx).await?.map(|r|Ok(GraphRelation{id:r.try_get("id")?,team_id:r.try_get("team_id")?,source_id:r.try_get("source_id")?,target_id:r.try_get("target_id")?,kind:r.try_get("kind")?})).transpose()
}
