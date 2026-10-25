use crate::Tx;
use uuid::Uuid;

pub async fn append_issue_rank(
    tx: &mut Tx,
    workspace: Uuid,
    team: Uuid,
    status: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("WITH tail AS(SELECT rank FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 AND archived_at IS NULL AND deleted_at IS NULL ORDER BY rank DESC,id DESC LIMIT 1),next AS(SELECT coalesce((SELECT rank FROM tail),0)+1024 AS rank) SELECT CASE WHEN rank<100000000000000000000 THEN rank::numeric(38,18)::text END FROM next")
        .bind(workspace)
        .bind(team)
        .bind(status)
        .fetch_one(&mut **tx)
        .await
}
