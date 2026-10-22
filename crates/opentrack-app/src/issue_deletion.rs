use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::{
    DeletedIssue, DeletedIssueQuery, DeletionPreview, Issue, Page, RestoreIssue,
};
use serde_json::json;
use sqlx::Row;
use std::collections::BTreeSet;

const MAX_ATTACHMENTS: i64 = 10_000;
fn deleted(state: store::IssueDeletionState) -> Result<DeletedIssue> {
    Ok(DeletedIssue {
        deleted_at: state.deleted_at.ok_or(Error::NotFound)?,
        restore_until: state.restore_until.ok_or(Error::Internal)?,
        can_restore: state.authorized && state.active && state.within_restore,
        issue: state.issue,
    })
}
async fn state(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
) -> Result<store::IssueDeletionState> {
    store::issue_deletion_state(tx, w, a.user.id, id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)
}
async fn locked_state(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    write: bool,
) -> Result<store::IssueDeletionState> {
    let before = state(tx, a, w, id).await?;
    if !store::lock_graph_teams(tx, w, a.user.id, &[before.issue.team_id], write)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    let current = state(tx, a, w, id).await?;
    if current.issue.team_id != before.issue.team_id {
        return Err(Error::Conflict(
            "This issue moved. Reload before continuing".into(),
        ));
    }
    Ok(current)
}
async fn attachments(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    id: Uuid,
) -> Result<(Option<Uuid>, i64, i64)> {
    let row=sqlx::query("SELECT i.parent_id,(SELECT count(*) FROM issues c WHERE c.workspace_id=$1 AND c.team_id=$2 AND c.parent_id=i.id) AS children,(SELECT count(*) FROM issue_relations r WHERE r.workspace_id=$1 AND r.team_id=$2 AND (r.source_id=i.id OR r.target_id=i.id)) AS relationships FROM issues i WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.id=$3")
        .bind(w).bind(t).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
    Ok((
        row.try_get("parent_id").map_err(db_error)?,
        row.try_get("children").map_err(db_error)?,
        row.try_get("relationships").map_err(db_error)?,
    ))
}
fn writable(state: &store::IssueDeletionState) -> Result<()> {
    if !state.authorized {
        return Err(Error::Forbidden);
    }
    if !state.active {
        return Err(Error::Conflict(
            "Restore the issue, team and workspace from archive before making this change".into(),
        ));
    }
    Ok(())
}
async fn audit(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    action: &str,
    mutation: Uuid,
) -> Result<()> {
    graph::one(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id) VALUES($1,$2,$3,$4,$5)")
        .bind(mutation).bind(w).bind(a.user.id).bind(action).bind(id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}
impl App {
    pub async fn issue_deletion_preview(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
    ) -> Result<DeletionPreview> {
        let mut tx = self.scoped(a, w, false).await?;
        let old = locked_state(&mut tx, a, w, id, false).await?;
        if old.deleted_at.is_some() {
            return Err(Error::NotFound);
        }
        let (parent, children, relations) = attachments(&mut tx, w, old.issue.team_id, id).await?;
        let result = DeletionPreview {
            can_delete: old.authorized && old.active,
            issue: old.issue,
            child_count: children as u64,
            relationship_count: relations as u64,
            has_parent: parent.is_some(),
        };
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn get_deleted_issue(&self, a: &Actor, w: Uuid, id: Uuid) -> Result<DeletedIssue> {
        let mut tx = self.scoped(a, w, false).await?;
        let result = deleted(locked_state(&mut tx, a, w, id, false).await?)?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn list_deleted_issues(
        &self,
        a: &Actor,
        w: Uuid,
        mut q: DeletedIssueQuery,
    ) -> Result<Page<DeletedIssue>> {
        let limit = q.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) || q.q.as_ref().is_some_and(|s| s.chars().count() > 256) {
            return Err(Error::Invalid("Invalid trash search or page size".into()));
        }
        let cursor = q.cursor.take();
        q.limit = Some(limit);
        q.q = q.q.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let mut tx = self.scoped(a, w, false).await?;
        if let Some(t) = q.team_id
            && !store::visible_team(&mut tx, w, t, a.user.id, false)
                .await
                .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{}:{epoch}:{}",
            a.user.id,
            serde_json::to_string(&q).map_err(|_| Error::Internal)?
        );
        let after: Option<(DateTime<Utc>, Uuid)> = cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("deleted-issues", &binding, c))
            .transpose()?;
        let mut rows = store::deleted_issues(&mut tx, w, a.user.id, &q, after)
            .await
            .map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_cursor = if has_more {
            rows.last()
                .map(|r| {
                    self.taxonomy_cursor_encode(
                        "deleted-issues",
                        &binding,
                        &(r.deleted_at.ok_or(Error::Internal)?, r.issue.id),
                    )
                })
                .transpose()?
        } else {
            None
        };
        let data = rows.into_iter().map(deleted).collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
    pub async fn delete_issue(&self, a: &Actor, w: Uuid, id: Uuid, etag: &str) -> Result<()> {
        // Resolve current team visibility before mutation permission so hidden IDs stay 404.
        let mut tx = self.scoped(a, w, false).await?;
        let old = locked_state(&mut tx, a, w, id, true).await?;
        if old.deleted_at.is_some() {
            return Err(Error::NotFound);
        }
        writable(&old)?;
        graph::source_version(&old.issue, etag)?;
        let t = old.issue.team_id;
        let (parent, child_count, relation_count) = attachments(&mut tx, w, t, id).await?;
        if child_count + relation_count + i64::from(parent.is_some()) > MAX_ATTACHMENTS {
            return Err(Error::Conflict("Deleting this issue exceeds the 10,000-connection limit. Detach some connections first".into()));
        }
        let children:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND parent_id=$3 ORDER BY id LIMIT 10001").bind(w).bind(t).bind(id).fetch_all(&mut *tx).await.map_err(db_error)?;
        let relations=sqlx::query("SELECT source_id,target_id FROM issue_relations WHERE workspace_id=$1 AND team_id=$2 AND (source_id=$3 OR target_id=$3) LIMIT 10001").bind(w).bind(t).bind(id).fetch_all(&mut *tx).await.map_err(db_error)?;
        if children.len() as i64 != child_count || relations.len() as i64 != relation_count {
            return Err(Error::Internal);
        }
        let mut ids = BTreeSet::from([id]);
        ids.extend(parent);
        ids.extend(children.iter().copied());
        let mut relation_endpoints = BTreeSet::new();
        for r in relations {
            relation_endpoints.insert(r.try_get::<Uuid, _>("source_id").map_err(db_error)?);
            relation_endpoints.insert(r.try_get::<Uuid, _>("target_id").map_err(db_error)?);
        }
        ids.extend(&relation_endpoints);
        let ids = ids.into_iter().collect::<Vec<_>>();
        graph::lock_issues(&mut tx, w, t, &ids).await?;
        graph::source_version(&state(&mut tx, a, w, id).await?.issue, etag)?;
        let detached=sqlx::query("UPDATE issues SET parent_id=NULL WHERE workspace_id=$1 AND team_id=$2 AND parent_id=$3").bind(w).bind(t).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
        if detached != child_count as u64 {
            return Err(Error::Internal);
        }
        let removed=sqlx::query("DELETE FROM issue_relations WHERE workspace_id=$1 AND team_id=$2 AND (source_id=$3 OR target_id=$3)").bind(w).bind(t).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
        if removed != relation_count as u64 {
            return Err(Error::Internal);
        }
        graph::one(sqlx::query("UPDATE issues SET deleted_at=clock_timestamp(),parent_id=NULL WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let mut events = graph::bump(
            &mut tx,
            a,
            w,
            t,
            &ids,
            "issue.deleted",
            json!({"source":"manual","action":"deleted"}),
        )
        .await?;
        let mutation = events.first().ok_or(Error::Internal)?.mutation;
        let children = children.into_iter().collect::<BTreeSet<_>>();
        for event in &mut events {
            if event.entity != id {
                let hierarchy = children.contains(&event.entity) || parent == Some(event.entity);
                event.kind = if hierarchy {
                    "issue.parent_changed"
                } else {
                    "issue.relation_removed"
                };
                let mut fields = Vec::new();
                if hierarchy {
                    fields.push("parent");
                }
                if relation_endpoints.contains(&event.entity) {
                    fields.push("relations");
                }
                event.changes = json!({"source":"manual","action":if hierarchy {"parent_detached_after_deletion"}else{"relation_removed_after_deletion"},"fields":fields});
            }
            store::append_activity(&mut tx, event)
                .await
                .map_err(db_error)?;
        }
        audit(&mut tx, a, w, id, "issue.deleted", mutation).await?;
        // All entity/activity/audit effects precede the first stream lock. Chunks
        // remain contiguous because that lock is retained until this single commit.
        for chunk in events.chunks(101) {
            store::append_stream_events(&mut tx, chunk)
                .await
                .map_err(db_error)?;
        }
        tx.commit().await.map_err(db_error)
    }
    pub async fn restore_issue(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        mut req: RestoreIssue,
    ) -> Result<Issue> {
        opentrack_domain::normalize_label_ids(&mut req.label_ids)?;
        // Resolve current team visibility before mutation permission so hidden IDs stay 404.
        let mut tx = self.scoped(a, w, false).await?;
        let old = locked_state(&mut tx, a, w, id, true).await?;
        if old.deleted_at.is_none() {
            return Err(Error::NotFound);
        }
        writable(&old)?;
        graph::lock_issues(&mut tx, w, old.issue.team_id, &[id]).await?;
        let old = state(&mut tx, a, w, id).await?;
        graph::source_version(&old.issue, etag)?;
        if !old.within_restore {
            return Err(Error::Conflict(
                "The 30-day restore period has ended".into(),
            ));
        }
        if !store::valid_references(
            &mut tx,
            w,
            old.issue.team_id,
            req.status_id,
            req.assignee_id,
        )
        .await
        .map_err(db_error)?
        {
            return Err(Error::Invalid(
                "Choose an active status and an eligible assignee for this team".into(),
            ));
        }
        if !store::validate_issue_labels(
            &mut tx,
            w,
            old.issue.team_id,
            &req.label_ids,
            &old.issue.label_ids,
        )
        .await
        .map_err(db_error)?
        {
            return Err(Error::Invalid(
                "A selected label is unavailable for this team".into(),
            ));
        }
        store::set_issue_labels(&mut tx, w, id, &req.label_ids)
            .await
            .map_err(db_error)?;
        let restored=sqlx::query("UPDATE issues SET deleted_at=NULL,status_id=$3,assignee_id=$4,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NOT NULL AND clock_timestamp()<deleted_at+interval '30 days'")
            .bind(w).bind(id).bind(req.status_id).bind(req.assignee_id).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
        if restored != 1 {
            let current = state(&mut tx, a, w, id).await?;
            return Err(if !current.within_restore {
                Error::Conflict("The 30-day restore period has ended".into())
            } else {
                Error::Internal
            });
        }
        graph::one(
            sqlx::query(
                "UPDATE teams SET graph_version=graph_version+1 WHERE workspace_id=$1 AND id=$2",
            )
            .bind(w)
            .bind(old.issue.team_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?
            .rows_affected(),
        )?;
        let item = graph::current(&mut tx, a, w, id).await?;
        let event = store::Event {
            workspace: w,
            team: item.team_id,
            entity: id,
            version: item.version.parse().map_err(|_| Error::Internal)?,
            actor: a.user.id,
            kind: "issue.restored",
            mutation: Uuid::new_v4(),
            changes: json!({"source":"manual","action":"restored","status_id":{"before":old.issue.status_id,"after":item.status_id},"assignee_id":{"before":old.issue.assignee_id,"after":item.assignee_id},"label_ids":{"before":old.issue.label_ids,"after":item.label_ids}}),
        };
        store::append_activity(&mut tx, &event)
            .await
            .map_err(db_error)?;
        audit(&mut tx, a, w, id, "issue.restored", event.mutation).await?;
        store::append_stream_events(&mut tx, &[event])
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
}
