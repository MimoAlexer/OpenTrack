use super::*;
use opentrack_domain::*;
use serde_json::Value;
use sqlx::Row;

pub(crate) async fn projection(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    body: &Value,
) -> Result<Vec<IssueReference>> {
    let ids = document_issue_reference_ids(body);
    if ids.is_empty() {
        return Ok(vec![]);
    }
    // Resolve all labels in one statement. The stored document contains IDs only;
    // deleted, missing, cross-workspace and private targets have the same marker.
    let rows = sqlx::query("SELECT requested.id, visible.key, visible.title FROM unnest($3::uuid[]) requested(id) LEFT JOIN (SELECT i.id,t.key||'-'||i.number AS key,i.title FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE i.workspace_id=$1 AND i.deleted_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=i.workspace_id AND m.team_id=i.team_id AND m.user_id=$2))) visible ON visible.id=requested.id ORDER BY requested.id")
        .bind(w).bind(a.user.id).bind(ids).fetch_all(&mut **tx).await.map_err(db_error)?;
    rows.into_iter()
        .map(|row| {
            let key: Option<String> = row.try_get("key").map_err(db_error)?;
            Ok(IssueReference {
                id: row.try_get("id").map_err(db_error)?,
                available: key.is_some(),
                key,
                title: row.try_get("title").map_err(db_error)?,
            })
        })
        .collect()
}

async fn eligible_targets(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    ids: &[Uuid],
) -> Result<Vec<(Uuid, Uuid)>> {
    sqlx::query_as("SELECT i.id,i.team_id FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE i.workspace_id=$1 AND i.id=ANY($3) AND i.deleted_at IS NULL AND i.archived_at IS NULL AND t.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=i.workspace_id AND m.team_id=i.team_id AND m.user_id=$2)) ORDER BY i.id")
        .bind(w).bind(a.user.id).bind(ids).fetch_all(&mut **tx).await.map_err(db_error)
}

/// Before any team/entity lock, serialize new references with target moves,
/// deletion and archive using the existing canonical sorted team lock order.
pub(crate) async fn lock_new_targets(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    source_team: Uuid,
    body: &Value,
    previous: &Value,
) -> Result<()> {
    let previous = document_issue_reference_ids(previous);
    let ids: Vec<Uuid> = document_issue_reference_ids(body)
        .into_iter()
        .filter(|id| previous.binary_search(id).is_err())
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let targets = eligible_targets(tx, a, w, &ids).await?;
    if targets.len() != ids.len() {
        return Err(Error::Invalid(
            "A newly referenced issue is unavailable".into(),
        ));
    }
    let mut teams = vec![source_team];
    teams.extend(targets.iter().map(|(_, team)| *team));
    if !store::lock_graph_teams(tx, w, a.user.id, &teams, true)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    if eligible_targets(tx, a, w, &ids).await? != targets {
        return Err(Error::Conflict(
            "A referenced issue moved or became unavailable. Reload before saving".into(),
        ));
    }
    Ok(())
}

/// Obtain the owning issue without taking a team lock. Ordinary document scope
/// rechecks author/moderator and historical comment authority after these locks.
pub(crate) async fn lock_document_targets(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    document: Uuid,
    body: &Value,
) -> Result<()> {
    if document_issue_reference_ids(body).is_empty() {
        return Ok(());
    }
    let row = sqlx::query("SELECT coalesce(d.issue_id,c.issue_id) AS issue_id,d.body FROM documents d LEFT JOIN comments c ON c.workspace_id=d.workspace_id AND c.id=d.comment_id WHERE d.workspace_id=$1 AND d.id=$2 AND d.deleted_at IS NULL")
        .bind(w).bind(document).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    let issue_id: Uuid = row.try_get("issue_id").map_err(db_error)?;
    let source = graph::current(tx, a, w, issue_id).await?;
    lock_new_targets(
        tx,
        a,
        w,
        source.team_id,
        body,
        &row.try_get::<Value, _>("body").map_err(db_error)?,
    )
    .await?;
    if graph::current(tx, a, w, issue_id).await?.team_id != source.team_id {
        return Err(Error::Conflict(
            "The source issue moved. Reload before saving".into(),
        ));
    }
    Ok(())
}

impl App {
    pub async fn issue_reference_candidates(
        &self,
        a: &Actor,
        w: Uuid,
        mut q: IssueReferenceQuery,
    ) -> Result<Page<IssueReferenceCandidate>> {
        q.validate()?;
        q.q = Some(q.q.as_deref().unwrap_or("").trim().to_owned());
        q.limit = Some(q.limit.unwrap_or(20));
        let mut tx = self.scoped(a, w, false).await?;
        let (team, origin) = crate::mentions::scope(
            &mut tx,
            a,
            w,
            &MentionQuery {
                team_id: q.team_id,
                issue_id: q.issue_id,
                document_id: q.document_id,
                ..Default::default()
            },
        )
        .await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let cursor = q.cursor.take();
        let binding = format!(
            "{w}:{}:{epoch}:{team}:{origin}:{}",
            a.user.id,
            serde_json::to_string(&q).map_err(|_| Error::Internal)?
        );
        let after: Option<(chrono::DateTime<chrono::Utc>, Uuid)> = cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("issue-references-v1", &binding, c))
            .transpose()?;
        let search = q.q.as_deref().unwrap_or("");
        let mut rows = sqlx::query("SELECT i.id,t.key||'-'||i.number AS key,i.title,i.created_at FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE i.workspace_id=$1 AND i.deleted_at IS NULL AND i.archived_at IS NULL AND t.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=i.workspace_id AND m.team_id=i.team_id AND m.user_id=$2)) AND (strpos(lower(i.title),lower($3))>0 OR strpos(lower(t.key||'-'||i.number),lower($3))>0 OR EXISTS(SELECT 1 FROM issue_key_aliases alias WHERE alias.workspace_id=i.workspace_id AND alias.issue_id=i.id AND alias.team_prefix||'-'||alias.number=upper($3))) AND ($4::timestamptz IS NULL OR (i.created_at,i.id)<($4,$5)) ORDER BY i.created_at DESC,i.id DESC LIMIT $6")
            .bind(w).bind(a.user.id).bind(search).bind(after.map(|v| v.0)).bind(after.map(|v| v.1)).bind(i64::from(q.limit.unwrap_or(20)) + 1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > q.limit.unwrap_or(20) as usize;
        rows.truncate(q.limit.unwrap_or(20) as usize);
        let next_cursor = if has_more {
            rows.last()
                .map(|r| {
                    self.taxonomy_cursor_encode(
                        "issue-references-v1",
                        &binding,
                        &(
                            r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                                .map_err(db_error)?,
                            r.try_get::<Uuid, _>("id").map_err(db_error)?,
                        ),
                    )
                })
                .transpose()?
        } else {
            None
        };
        let data = rows
            .into_iter()
            .map(|r| {
                Ok(IssueReferenceCandidate {
                    id: r.try_get("id").map_err(db_error)?,
                    key: r.try_get("key").map_err(db_error)?,
                    title: r.try_get("title").map_err(db_error)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
}
