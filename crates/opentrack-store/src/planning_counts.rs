use crate::Tx;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::postgres::PgRow;
use uuid::Uuid;

pub async fn apply_planning_events(tx: &mut Tx, workspace: Uuid) -> Result<PgRow, sqlx::Error> {
    sqlx::query("SELECT * FROM worker_apply_planning_events($1,100)")
        .bind(workspace)
        .fetch_one(&mut **tx)
        .await
}

pub async fn planning_backfill_batch(
    tx: &mut Tx,
    workspace: Uuid,
    job: Uuid,
    owner: Uuid,
    fence: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT worker_planning_backfill_batch($1,$2,$3,$4,100)")
        .bind(workspace)
        .bind(job)
        .bind(owner)
        .bind(fence)
        .fetch_one(&mut **tx)
        .await
}

pub async fn project_planning_counts(
    tx: &mut Tx,
    workspace: Uuid,
    team: Uuid,
    project: Uuid,
    milestone: Option<Uuid>,
) -> Result<PgRow, sqlx::Error> {
    sqlx::query("WITH source AS (SELECT s.category,false AS excluded,f.category AS fact_category,f.excluded AS fact_excluded,f.calculated_at,coalesce(f.issue_version=i.version,false) AS fact_current FROM project_issues pi JOIN issues i ON i.workspace_id=pi.workspace_id AND i.team_id=pi.team_id AND i.id=pi.issue_id JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id LEFT JOIN planning_issue_facts f ON f.workspace_id=i.workspace_id AND f.consumer_version=1 AND f.issue_id=i.id WHERE pi.workspace_id=$1 AND pi.team_id=$2 AND pi.project_id=$3 AND ($4::uuid IS NULL OR pi.milestone_id=$4) AND i.archived_at IS NULL AND i.deleted_at IS NULL), validation AS (SELECT coalesce(bool_and(fact_current),false) AS ready,min(calculated_at) AS facts_as_of FROM source), buckets AS (SELECT CASE WHEN v.ready THEN s.fact_category ELSE s.category END AS category,CASE WHEN v.ready THEN s.fact_excluded ELSE s.excluded END AS excluded,count(*) AS count FROM source s CROSS JOIN validation v GROUP BY 1,2) SELECT coalesce(jsonb_agg(jsonb_build_array(b.category,b.excluded,b.count)) FILTER(WHERE b.count IS NOT NULL),'[]'::jsonb) AS buckets,v.ready AS facts_ready,v.facts_as_of,statement_timestamp() AS as_of FROM validation v LEFT JOIN buckets b ON true GROUP BY v.ready,v.facts_as_of")
        .bind(workspace).bind(team).bind(project).bind(milestone).fetch_one(&mut **tx).await
}

pub async fn milestone_planning_page(
    tx: &mut Tx,
    workspace: Uuid,
    project: Uuid,
    after: Option<(i64, Uuid)>,
    limit: i64,
) -> Result<Vec<PgRow>, sqlx::Error> {
    sqlx::query("WITH page AS (SELECT * FROM milestones WHERE workspace_id=$1 AND project_id=$2 AND ($3::bigint IS NULL OR (rank,id)>($3,$4)) ORDER BY rank,id LIMIT $5), source AS (SELECT m.id,s.category,false AS excluded,f.category AS fact_category,f.excluded AS fact_excluded,coalesce(bool_and(coalesce(f.issue_version=i.version,false)) OVER(PARTITION BY m.id),false) AS ready,min(f.calculated_at) OVER(PARTITION BY m.id) AS facts_as_of FROM page m JOIN project_issues pi ON pi.workspace_id=m.workspace_id AND pi.project_id=m.project_id AND pi.milestone_id=m.id JOIN issues i ON i.workspace_id=pi.workspace_id AND i.id=pi.issue_id JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id LEFT JOIN planning_issue_facts f ON f.workspace_id=i.workspace_id AND f.consumer_version=1 AND f.issue_id=i.id WHERE i.archived_at IS NULL AND i.deleted_at IS NULL), buckets AS (SELECT id,ready,facts_as_of,CASE WHEN ready THEN fact_category ELSE category END AS category,CASE WHEN ready THEN fact_excluded ELSE excluded END AS excluded,count(*) AS count FROM source GROUP BY 1,2,3,4,5), counts AS (SELECT id,bool_and(ready) AS facts_ready,min(facts_as_of) AS facts_as_of,jsonb_agg(jsonb_build_array(category,excluded,count)) AS buckets FROM buckets GROUP BY id) SELECT m.*,coalesce(c.buckets,'[]'::jsonb) AS buckets,coalesce(c.facts_ready,false) AS facts_ready,c.facts_as_of,statement_timestamp() AS as_of FROM page m LEFT JOIN counts c ON c.id=m.id ORDER BY m.rank,m.id")
        .bind(workspace).bind(project).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(limit).fetch_all(&mut **tx).await
}

pub async fn current_cycle_planning_rows(
    tx: &mut Tx,
    workspace: Uuid,
    team: Uuid,
    cycle: Uuid,
    timezone: &str,
) -> Result<(Vec<PgRow>, DateTime<Utc>, NaiveDate), sqlx::Error> {
    let mut rows=sqlx::query("SELECT source.*,statement_timestamp() AS as_of,(statement_timestamp() AT TIME ZONE $4)::date AS local_date FROM (VALUES(1)) anchor(n) LEFT JOIN LATERAL (SELECT i.id,i.version,i.estimate,s.category,(i.deleted_at IS NOT NULL OR i.archived_at IS NOT NULL) AS excluded,i.deleted_at IS NOT NULL AS deleted,f.category AS fact_category,f.estimate AS fact_estimate,f.excluded AS fact_excluded,f.calculated_at,coalesce(f.issue_version=i.version,false) AS fact_current FROM issues i JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id LEFT JOIN planning_issue_facts f ON f.workspace_id=i.workspace_id AND f.consumer_version=1 AND f.issue_id=i.id WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.id=ANY(ARRAY(SELECT issue_id FROM cycle_issues WHERE workspace_id=$1 AND team_id=$2 AND cycle_id=$3 ORDER BY issue_id LIMIT 10001)) ORDER BY i.id) source ON true ORDER BY source.id")
        .bind(workspace).bind(team).bind(cycle).bind(timezone).fetch_all(&mut **tx).await?;
    use sqlx::Row;
    let first = rows.first().ok_or(sqlx::Error::RowNotFound)?;
    let clock = (first.try_get("as_of")?, first.try_get("local_date")?);
    if first.try_get::<Option<Uuid>, _>("id")?.is_none() {
        rows.clear();
    }
    Ok((rows, clock.0, clock.1))
}
