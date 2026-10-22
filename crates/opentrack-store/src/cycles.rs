use super::*;
use opentrack_domain::CycleIssue;
#[allow(clippy::too_many_arguments)]
pub async fn cycle_issue_page(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    team: Uuid,
    cycle: Uuid,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<CycleIssue>, sqlx::Error> {
    let mut q = sqlx::QueryBuilder::<Postgres>::new(
        "SELECT scope.body,s.name AS status_name,s.category AS status_category FROM (",
    );
    q.push(ISSUE_SELECT).push(" JOIN cycle_issues ci ON ci.workspace_id=i.workspace_id AND ci.team_id=i.team_id AND ci.issue_id=i.id WHERE i.workspace_id=$1 AND i.team_id=$3 AND ci.cycle_id=$4 AND i.deleted_at IS NULL ").push(VISIBLE).push(" AND ($5::uuid IS NULL OR i.id>$5) ORDER BY i.id LIMIT $6) scope JOIN workflow_statuses s ON s.workspace_id=$1 AND s.team_id=$3 AND s.id=(scope.body->>'status_id')::uuid ORDER BY scope.issue_id");
    let rows = q
        .build()
        .bind(w)
        .bind(user)
        .bind(team)
        .bind(cycle)
        .bind(after)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|r| {
            let i = issue_from(r.try_get("body")?)?;
            Ok(CycleIssue {
                id: i.id,
                key: i.key,
                title: i.title,
                status_id: i.status_id,
                status_name: r.try_get("status_name")?,
                status_category: r.try_get("status_category")?,
                priority: i.priority,
                estimate: i.estimate,
                archived_at: i.archived_at,
                version: i.version,
                etag: i.etag,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub async fn cycle_snapshot_points(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    team: Uuid,
    cycle: Uuid,
    after: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<opentrack_domain::CycleSnapshotPoint>, sqlx::Error> {
    let rows=sqlx::query("SELECT s.id,s.cycle_id,s.local_date,s.captured_at,s.kind,s.definition_version,p.scope_count,p.completed_count,p.canceled_count,p.unfinished_count,p.excluded_count,p.estimate_total,p.visible_count<>jsonb_array_length(s.items) AS redacted FROM (SELECT * FROM cycle_snapshots WHERE workspace_id=$1 AND team_id=$3 AND cycle_id=$4 AND ($5::timestamptz IS NULL OR (captured_at,id)<($5,$6)) ORDER BY captured_at DESC,id DESC LIMIT $7) s CROSS JOIN LATERAL (SELECT count(*) AS visible_count,count(*) FILTER(WHERE NOT (f->>'excluded')::boolean) AS scope_count,count(*) FILTER(WHERE NOT (f->>'excluded')::boolean AND f->>'category'='completed') AS completed_count,count(*) FILTER(WHERE NOT (f->>'excluded')::boolean AND f->>'category'='canceled') AS canceled_count,count(*) FILTER(WHERE NOT (f->>'excluded')::boolean AND f->>'category' NOT IN('completed','canceled')) AS unfinished_count,count(*) FILTER(WHERE (f->>'excluded')::boolean) AS excluded_count,coalesce(sum((f->>'estimate')::bigint) FILTER(WHERE NOT (f->>'excluded')::boolean AND f->>'category'<>'canceled'),0)::bigint AS estimate_total FROM jsonb_array_elements(s.items) f JOIN issues i ON i.workspace_id=$1 AND i.id=(f->>'issue_id')::uuid JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE i.deleted_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$2))) p ORDER BY s.captured_at DESC,s.id DESC").bind(w).bind(user).bind(team).bind(cycle).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(limit).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|r| {
            Ok(opentrack_domain::CycleSnapshotPoint {
                id: r.try_get("id")?,
                cycle_id: r.try_get("cycle_id")?,
                local_date: r.try_get("local_date")?,
                captured_at: r.try_get("captured_at")?,
                kind: r.try_get("kind")?,
                definition_version: r.try_get("definition_version")?,
                redacted: r.try_get("redacted")?,
                scope_count: r.try_get("scope_count")?,
                completed_count: r.try_get("completed_count")?,
                canceled_count: r.try_get("canceled_count")?,
                unfinished_count: r.try_get("unfinished_count")?,
                excluded_count: r.try_get("excluded_count")?,
                estimate_total: r.try_get("estimate_total")?,
            })
        })
        .collect()
}
