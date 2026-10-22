use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use sqlx::Row;

pub(crate) fn one(count: u64) -> Result<()> {
    if count == 1 {
        Ok(())
    } else {
        Err(Error::Internal)
    }
}
pub(crate) fn source_version(issue: &Issue, etag: &str) -> Result<()> {
    if etag.is_empty() {
        return Err(Error::PreconditionRequired);
    }
    if issue.etag != etag {
        return Err(Error::Stale(Box::new(issue.clone())));
    }
    Ok(())
}
pub(crate) async fn current(tx: &mut store::Tx, a: &Actor, w: Uuid, id: Uuid) -> Result<Issue> {
    store::issue(tx, w, a.user.id, id, false)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)
}
pub(crate) async fn scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    write: bool,
) -> Result<Issue> {
    let before = current(tx, a, w, id).await?;
    if !store::lock_graph_teams(tx, w, a.user.id, &[before.team_id], write)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    let issue = current(tx, a, w, id).await?;
    if issue.team_id != before.team_id {
        return Err(Error::Conflict(
            "The issue moved. Reload before continuing".into(),
        ));
    }
    if write {
        active(tx, w, &issue).await?;
    }
    Ok(issue)
}
pub(crate) async fn active(tx: &mut store::Tx, w: Uuid, issue: &Issue) -> Result<()> {
    lifecycle::require_active_team(tx, w, issue.team_id).await?;
    if issue.archived_at.is_some() {
        return Err(Error::Conflict(
            "Restore the issue before changing its relationships or team".into(),
        ));
    }
    Ok(())
}
pub(crate) async fn graph_version(tx: &mut store::Tx, w: Uuid, t: Uuid) -> Result<i64> {
    sqlx::query_scalar("SELECT graph_version FROM teams WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(t)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)
}
async fn can_change(tx: &mut store::Tx, a: &Actor, w: Uuid, issue: &Issue) -> Result<bool> {
    let writable:bool=sqlx::query_scalar("SELECT w.archived_at IS NULL AND t.archived_at IS NULL AND m.role<>'viewer' FROM workspaces w JOIN teams t ON t.workspace_id=w.id AND t.id=$2 JOIN workspace_members m ON m.workspace_id=w.id AND m.user_id=$3 AND m.active WHERE w.id=$1").bind(w).bind(issue.team_id).bind(a.user.id).fetch_one(&mut **tx).await.map_err(db_error)?;
    Ok(writable && issue.archived_at.is_none())
}
pub(crate) async fn target(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    team: Uuid,
    id: Uuid,
    etag: &str,
) -> Result<IssueGraphTarget> {
    let issue = current(tx, a, w, id).await?;
    if issue.team_id != team || issue.archived_at.is_some() {
        return Err(Error::NotFound);
    }
    let target = store::graph_target(tx, w, a.user.id, id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    if etag.is_empty() || etag.len() > 256 {
        return Err(Error::Invalid("Provide the target's current ETag".into()));
    }
    if target.etag != etag {
        return Err(Error::VersionConflict(json!({"target":target})));
    }
    Ok(target)
}
pub(crate) async fn lock_issues(tx: &mut store::Tx, w: Uuid, t: Uuid, ids: &[Uuid]) -> Result<()> {
    if !store::lock_graph_issues(tx, w, t, ids)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    Ok(())
}
pub(crate) async fn bump(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    ids: &[Uuid],
    kind: &'static str,
    changes: serde_json::Value,
) -> Result<Vec<store::Event<'static>>> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mutation = Uuid::new_v4();
    let mut events = vec![];
    for id in ids {
        let version:Option<i64>=sqlx::query_scalar("UPDATE issues SET version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3 RETURNING version").bind(w).bind(t).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?;
        events.push(store::Event {
            workspace: w,
            team: t,
            entity: id,
            version: version.ok_or(Error::Internal)?,
            actor: a.user.id,
            kind,
            mutation,
            changes: changes.clone(),
        });
    }
    one(sqlx::query(
        "UPDATE teams SET graph_version=graph_version+1 WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(t)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?
    .rows_affected())?;
    Ok(events)
}
pub(crate) async fn effects(tx: &mut store::Tx, events: &[store::Event<'_>]) -> Result<()> {
    for event in events {
        store::append_activity(tx, event).await.map_err(db_error)?;
    }
    store::append_stream_events(tx, events)
        .await
        .map_err(db_error)
}
pub(crate) async fn receipt_authority(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    issue: &Issue,
) -> Result<()> {
    current(tx, a, w, issue.id).await?;
    if !store::visible_team(tx, w, issue.team_id, a.user.id, false)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    Ok(())
}
pub(crate) async fn save<T: Serialize>(
    tx: &mut store::Tx,
    w: Uuid,
    u: Uuid,
    route: &str,
    key: &str,
    item: &T,
) -> Result<()> {
    one(sqlx::query("UPDATE idempotency_records SET response_body=$5 WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4").bind(w).bind(u).bind(route).bind(key).bind(serde_json::to_value(item).map_err(|_|Error::Internal)?).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}
async fn projected_relation(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    issue: &Issue,
    r: &store::GraphRelation,
    can_remove: bool,
) -> Result<IssueRelation> {
    if r.team_id != issue.team_id || (r.source_id != issue.id && r.target_id != issue.id) {
        return Err(Error::NotFound);
    }
    let outgoing = r.source_id == issue.id;
    let other = if outgoing { r.target_id } else { r.source_id };
    let kind = match (r.kind.as_str(), outgoing) {
        ("blocks", true) => RelationKind::Blocks,
        ("blocks", false) => RelationKind::BlockedBy,
        ("related", _) => RelationKind::Related,
        ("duplicate", true) => RelationKind::DuplicateOf,
        ("duplicate", false) => RelationKind::HasDuplicate,
        _ => return Err(Error::Internal),
    };
    let target = store::graph_target(tx, w, a.user.id, other)
        .await
        .map_err(db_error)?;
    Ok(IssueRelation {
        id: r.id,
        kind,
        target,
        can_remove,
    })
}
fn page_limit(q: &GraphPageQuery) -> Result<u32> {
    let n = q.limit.unwrap_or(20);
    if !(1..=50).contains(&n) {
        return Err(Error::Invalid("Page size must be 1–50".into()));
    }
    Ok(n)
}
impl App {
    async fn graph_cursor_binding(
        &self,
        tx: &mut store::Tx,
        a: &Actor,
        w: Uuid,
        issue: &Issue,
        version: i64,
        limit: u32,
    ) -> Result<String> {
        let epoch = store::auth_epoch(tx, w).await.map_err(db_error)?;
        Ok(format!(
            "{w}:{}:{}:{}:{epoch}:{version}:{limit}",
            a.user.id, issue.team_id, issue.id
        ))
    }
    pub async fn issue_parent(&self, a: &Actor, w: Uuid, id: Uuid) -> Result<IssueParent> {
        let mut tx = self.scoped(a, w, false).await?;
        let issue = scope(&mut tx, a, w, id, false).await?;
        let can_change = can_change(&mut tx, a, w, &issue).await?;
        let version = graph_version(&mut tx, w, issue.team_id).await?;
        let raw = store::graph_issue(&mut tx, w, id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let parent = if let Some(id) = raw.parent_id {
            Some(ParentLink {
                target: store::graph_target(&mut tx, w, a.user.id, id)
                    .await
                    .map_err(db_error)?,
                can_detach: can_change,
            })
        } else {
            None
        };
        let out = IssueParent {
            issue_id: id,
            issue_etag: issue.etag,
            graph_version: version.to_string(),
            parent,
            can_change,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn issue_children(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        q: GraphPageQuery,
    ) -> Result<GraphPage<IssueGraphTarget>> {
        let limit = page_limit(&q)?;
        let mut tx = self.scoped(a, w, false).await?;
        let issue = scope(&mut tx, a, w, id, false).await?;
        let version = graph_version(&mut tx, w, issue.team_id).await?;
        let binding = self
            .graph_cursor_binding(&mut tx, a, w, &issue, version, limit)
            .await?;
        let after: Option<Uuid> = decode_cursor(self, "children", &binding, q.cursor.as_deref())?;
        let mut ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND parent_id=$3 AND deleted_at IS NULL AND ($4::uuid IS NULL OR id>$4) ORDER BY id LIMIT $5").bind(w).bind(issue.team_id).bind(id).bind(after).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let next_cursor = if has_more {
            ids.last()
                .map(|id| self.taxonomy_cursor_encode("children", &binding, id))
                .transpose()?
        } else {
            None
        };
        let mut data = vec![];
        for id in ids {
            if let Some(target) = store::graph_target(&mut tx, w, a.user.id, id)
                .await
                .map_err(db_error)?
            {
                data.push(target)
            }
        }
        let out = GraphPage {
            data,
            next_cursor,
            has_more,
            issue_etag: issue.etag,
            graph_version: version.to_string(),
        };
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn issue_relations(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        q: GraphPageQuery,
    ) -> Result<GraphPage<IssueRelation>> {
        let limit = page_limit(&q)?;
        let mut tx = self.scoped(a, w, false).await?;
        let issue = scope(&mut tx, a, w, id, false).await?;
        let version = graph_version(&mut tx, w, issue.team_id).await?;
        let binding = self
            .graph_cursor_binding(&mut tx, a, w, &issue, version, limit)
            .await?;
        let after: Option<Uuid> = decode_cursor(self, "relations", &binding, q.cursor.as_deref())?;
        let mut ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM issue_relations WHERE workspace_id=$1 AND team_id=$2 AND (source_id=$3 OR target_id=$3) AND ($4::uuid IS NULL OR id>$4) ORDER BY id LIMIT $5").bind(w).bind(issue.team_id).bind(id).bind(after).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let next_cursor = if has_more {
            ids.last()
                .map(|id| self.taxonomy_cursor_encode("relations", &binding, id))
                .transpose()?
        } else {
            None
        };
        let can_remove = can_change(&mut tx, a, w, &issue).await?;
        let mut data = vec![];
        for id in ids {
            let r = store::graph_relation(&mut tx, w, id)
                .await
                .map_err(db_error)?
                .ok_or(Error::Internal)?;
            data.push(projected_relation(&mut tx, a, w, &issue, &r, can_remove).await?);
        }
        let out = GraphPage {
            data,
            next_cursor,
            has_more,
            issue_etag: issue.etag,
            graph_version: version.to_string(),
        };
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn get_issue_relation(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        relation: Uuid,
    ) -> Result<IssueRelation> {
        let mut tx = self.scoped(a, w, false).await?;
        let issue = scope(&mut tx, a, w, id, false).await?;
        let can_remove = can_change(&mut tx, a, w, &issue).await?;
        let r = store::graph_relation(&mut tx, w, relation)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let out = projected_relation(&mut tx, a, w, &issue, &r, can_remove).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn set_issue_parent(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        req: SetIssueParent,
    ) -> Result<Issue> {
        validate_parent_pair(req.parent_id, req.parent_etag.as_deref())?;
        let mut tx = self.scoped(a, w, true).await?;
        let issue = scope(&mut tx, a, w, id, true).await?;
        source_version(&issue, etag)?;
        let old = store::graph_issue(&mut tx, w, id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if req.parent_id == Some(id) {
            return Err(Error::Conflict("An issue cannot be its own parent".into()));
        }
        if let Some(parent) = req.parent_id {
            target(
                &mut tx,
                a,
                w,
                issue.team_id,
                parent,
                req.parent_etag.as_deref().ok_or(Error::Internal)?,
            )
            .await?;
        }
        let mut ids = vec![id];
        ids.extend(old.parent_id);
        ids.extend(req.parent_id);
        lock_issues(&mut tx, w, issue.team_id, &ids).await?;
        if old.parent_id == req.parent_id {
            tx.commit().await.map_err(db_error)?;
            return Ok(issue);
        }
        one(
            sqlx::query("UPDATE issues SET parent_id=$3 WHERE workspace_id=$1 AND id=$2")
                .bind(w)
                .bind(id)
                .bind(req.parent_id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?
                .rows_affected(),
        )?;
        let events=bump(&mut tx,a,w,issue.team_id,&ids,"issue.parent_changed",json!({"source":"manual","action":if req.parent_id.is_some(){"parent_set"}else{"parent_detached"}})).await?;
        let out = current(&mut tx, a, w, id).await?;
        effects(&mut tx, &events).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn create_issue_relation(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        key: &str,
        etag: &str,
        req: CreateIssueRelation,
    ) -> Result<RelationMutation> {
        let (kh, rh) = lifecycle::request_hash(key, &(id, etag, &req))?;
        let mut tx = self.scoped(a, w, true).await?;
        if let Some(receipt) = lifecycle::retry::<RelationMutation>(
            &mut tx,
            w,
            a.user.id,
            "issues.relation.create",
            &kh,
            &rh,
        )
        .await?
        {
            receipt_authority(&mut tx, a, w, &receipt.issue).await?;
            if let Some(target) = &receipt.relation.target {
                current(&mut tx, a, w, target.id).await?;
            }
            tx.commit().await.map_err(db_error)?;
            return Ok(receipt);
        }
        let issue = scope(&mut tx, a, w, id, true).await?;
        source_version(&issue, etag)?;
        if req.target_id == id {
            return Err(Error::Conflict("An issue cannot relate to itself".into()));
        }
        target(
            &mut tx,
            a,
            w,
            issue.team_id,
            req.target_id,
            &req.target_etag,
        )
        .await?;
        lock_issues(&mut tx, w, issue.team_id, &[id, req.target_id]).await?;
        let (kind, source, target) = match req.kind {
            CreateRelationKind::Blocks => ("blocks", id, req.target_id),
            CreateRelationKind::BlockedBy => ("blocks", req.target_id, id),
            CreateRelationKind::Related => {
                ("related", id.min(req.target_id), id.max(req.target_id))
            }
            CreateRelationKind::DuplicateOf => ("duplicate", id, req.target_id),
        };
        let relation = Uuid::new_v4();
        one(sqlx::query("INSERT INTO issue_relations(id,workspace_id,team_id,source_id,target_id,kind,created_by) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(relation).bind(w).bind(issue.team_id).bind(source).bind(target).bind(kind).bind(a.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let events = bump(
            &mut tx,
            a,
            w,
            issue.team_id,
            &[id, req.target_id],
            "issue.relation_added",
            json!({"source":"manual","action":"relation_added","relation_id":relation}),
        )
        .await?;
        let issue = current(&mut tx, a, w, id).await?;
        let row = store::graph_relation(&mut tx, w, relation)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        let relation = projected_relation(&mut tx, a, w, &issue, &row, true).await?;
        let out = RelationMutation { issue, relation };
        save(&mut tx, w, a.user.id, "issues.relation.create", &kh, &out).await?;
        effects(&mut tx, &events).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn remove_issue_relation(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        relation: Uuid,
        etag: &str,
    ) -> Result<Issue> {
        let mut tx = self.scoped(a, w, true).await?;
        let issue = scope(&mut tx, a, w, id, true).await?;
        source_version(&issue, etag)?;
        let r = store::graph_relation(&mut tx, w, relation)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if r.team_id != issue.team_id || (r.source_id != id && r.target_id != id) {
            return Err(Error::NotFound);
        }
        lock_issues(&mut tx, w, issue.team_id, &[r.source_id, r.target_id]).await?;
        one(
            sqlx::query("DELETE FROM issue_relations WHERE workspace_id=$1 AND id=$2")
                .bind(w)
                .bind(relation)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?
                .rows_affected(),
        )?;
        let events = bump(
            &mut tx,
            a,
            w,
            issue.team_id,
            &[r.source_id, r.target_id],
            "issue.relation_removed",
            json!({"source":"manual","action":"relation_removed","relation_id":relation}),
        )
        .await?;
        let out = current(&mut tx, a, w, id).await?;
        effects(&mut tx, &events).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn lookup_issues(
        &self,
        a: &Actor,
        w: Uuid,
        mut q: IssueLookupQuery,
    ) -> Result<Page<IssueGraphTarget>> {
        let limit = q.limit.unwrap_or(20);
        if !(1..=20).contains(&limit) || q.q.as_ref().is_some_and(|s| s.chars().count() > 100) {
            return Err(Error::Invalid("Invalid lookup query".into()));
        }
        let cursor = q.cursor.take();
        q.limit = Some(limit);
        q.q = Some(q.q.as_deref().unwrap_or("").trim().into());
        let mut tx = self.scoped(a, w, false).await?;
        if !store::lock_graph_teams(&mut tx, w, a.user.id, &[q.team_id], false)
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
        let after: Option<(DateTime<Utc>, Uuid)> =
            decode_cursor(self, "issue-lookup", &binding, cursor.as_deref())?;
        let text = q.q.as_deref().unwrap_or("");
        let pattern = format!(
            "%{}%",
            text.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut rows=sqlx::query("SELECT i.id,i.created_at FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.archived_at IS NULL AND i.deleted_at IS NULL AND t.archived_at IS NULL AND ($3::uuid IS NULL OR i.id<>$3) AND (i.title ILIKE $4 OR t.key||'-'||i.number ILIKE $4 OR EXISTS(SELECT 1 FROM issue_key_aliases a WHERE a.workspace_id=i.workspace_id AND a.issue_id=i.id AND a.team_prefix||'-'||a.number=upper($5))) AND ($6::timestamptz IS NULL OR (i.created_at,i.id)<($6,$7)) ORDER BY i.created_at DESC,i.id DESC LIMIT $8").bind(w).bind(q.team_id).bind(q.exclude_issue_id).bind(pattern).bind(text).bind(after.map(|x|x.0)).bind(after.map(|x|x.1)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_cursor = if has_more {
            rows.last()
                .map(|r| {
                    self.taxonomy_cursor_encode(
                        "issue-lookup",
                        &binding,
                        &(
                            r.try_get::<DateTime<Utc>, _>("created_at")
                                .map_err(db_error)?,
                            r.try_get::<Uuid, _>("id").map_err(db_error)?,
                        ),
                    )
                })
                .transpose()?
        } else {
            None
        };
        let mut data = vec![];
        for r in rows {
            if let Some(v) =
                store::graph_target(&mut tx, w, a.user.id, r.try_get("id").map_err(db_error)?)
                    .await
                    .map_err(db_error)?
            {
                data.push(v)
            }
        }
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
    pub async fn resolve_issue_key(&self, a: &Actor, w: Uuid, key: &str) -> Result<Issue> {
        if key.len() > 32 || !key.is_ascii() {
            return Err(Error::NotFound);
        }
        let key = key.to_ascii_uppercase();
        let mut tx = self.scoped(a, w, false).await?;
        let id:Option<Uuid>=sqlx::query_scalar("SELECT i.id FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE i.workspace_id=$1 AND (t.key||'-'||i.number=$2 OR EXISTS(SELECT 1 FROM issue_key_aliases a WHERE a.workspace_id=i.workspace_id AND a.issue_id=i.id AND a.team_prefix||'-'||a.number=$2)) AND i.deleted_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=i.workspace_id AND tm.team_id=i.team_id AND tm.user_id=$3)) LIMIT 1").bind(w).bind(key).bind(a.user.id).fetch_optional(&mut *tx).await.map_err(db_error)?;
        let issue = current(&mut tx, a, w, id.ok_or(Error::NotFound)?).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(issue)
    }
}
fn decode_cursor<T: DeserializeOwned>(
    app: &App,
    domain: &str,
    binding: &str,
    cursor: Option<&str>,
) -> Result<Option<T>> {
    cursor
        .map(|c| app.taxonomy_cursor_decode(domain, binding, c))
        .transpose()
}
