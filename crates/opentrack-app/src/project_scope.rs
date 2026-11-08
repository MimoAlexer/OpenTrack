use super::*;
use hmac::{Hmac, Mac};
use opentrack_domain::*;
use serde::Deserialize;
use serde_json::json;
use sqlx::{Row, postgres::PgRow};

const MAX_SCOPE: i64 = 1000;

fn baseline(row: &PgRow) -> Result<ProjectScopeBaseline> {
    Ok(ProjectScopeBaseline {
        id: row.try_get("id").map_err(db_error)?,
        label: row.try_get("label").map_err(db_error)?,
        reason: row.try_get("reason").map_err(db_error)?,
        author_id: row.try_get("author_id").map_err(db_error)?,
        author_name: row.try_get("author_name").map_err(db_error)?,
        created_at: row.try_get("created_at").map_err(db_error)?,
    })
}

#[derive(Deserialize)]
struct EventRow {
    #[serde(flatten)]
    event: ProjectScopeEvent,
}

impl App {
    fn scope_etag(&self, a: &Actor, p: &Project, revision: i64, epoch: i64) -> Result<String> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.setup_token_hash.as_bytes())
            .map_err(|_| Error::Internal)?;
        mac.update(b"opentrack.project-scope.v1\0");
        mac.update(format!("{}:{}:{}:{revision}:{epoch}", a.user.id, p.id, p.version).as_bytes());
        Ok(format!("\"scope:{:x}\"", mac.finalize().into_bytes()))
    }

    async fn scope_read(
        &self,
        tx: &mut store::Tx,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        q: ProjectScopeQuery,
    ) -> Result<ProjectScopeHistory> {
        let p = projects::project(tx, a, w, t, id).await?;
        let head=sqlx::query("SELECT * FROM project_scope_heads WHERE workspace_id=$1 AND team_id=$2 AND project_id=$3 FOR SHARE")
            .bind(w).bind(t).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
        let revision: i64 = head.try_get("revision").map_err(db_error)?;
        let epoch = store::auth_epoch(tx, w).await.map_err(db_error)?;
        let etag = self.scope_etag(a, &p, revision, epoch)?;
        let limit = q.limit.unwrap_or(25);
        if !(1..=25).contains(&limit) {
            return Err(Error::Invalid(
                "Scope history pages are limited to 25 entries".into(),
            ));
        }
        let directory_binding = format!("{w}:{t}:{id}:{}:{epoch}:{etag}:{limit}", a.user.id);
        let baseline_after_id: Option<Uuid> = q
            .baseline_cursor
            .as_deref()
            .map(|c| {
                self.taxonomy_cursor_decode("project-scope-baselines-v1", &directory_binding, c)
            })
            .transpose()?;
        let baseline_after: Option<i64> = if let Some(b) = baseline_after_id {
            Some(sqlx::query_scalar("SELECT ordinal FROM project_scope_baselines WHERE workspace_id=$1 AND project_id=$2 AND id=$3").bind(w).bind(id).bind(b).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::CursorExpired)?)
        } else {
            None
        };
        let mut rows=sqlx::query("SELECT b.*,u.display_name AS author_name FROM project_scope_baselines b LEFT JOIN users u ON u.id=b.author_id WHERE b.workspace_id=$1 AND b.team_id=$2 AND b.project_id=$3 AND ($4::bigint IS NULL OR b.ordinal<$4) ORDER BY b.ordinal DESC LIMIT $5")
            .bind(w).bind(t).bind(id).bind(baseline_after).bind(i64::from(limit)+1).fetch_all(&mut **tx).await.map_err(db_error)?;
        let baselines_has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let baselines_next_cursor = if baselines_has_more {
            rows.last()
                .map(|r| {
                    self.taxonomy_cursor_encode(
                        "project-scope-baselines-v1",
                        &directory_binding,
                        &r.try_get::<Uuid, _>("id").map_err(db_error)?,
                    )
                })
                .transpose()?
        } else {
            None
        };
        let baselines = rows.iter().map(baseline).collect::<Result<Vec<_>>>()?;
        let selected=sqlx::query("SELECT b.*,u.display_name AS author_name FROM project_scope_baselines b LEFT JOIN users u ON u.id=b.author_id WHERE b.workspace_id=$1 AND b.team_id=$2 AND b.project_id=$3 AND ($4::uuid IS NULL OR b.id=$4) ORDER BY b.ordinal DESC LIMIT 1")
            .bind(w).bind(t).bind(id).bind(q.baseline_id).fetch_optional(&mut **tx).await.map_err(db_error)?;
        if q.baseline_id.is_some() && selected.is_none() {
            return Err(Error::NotFound);
        }
        let selected_id = selected
            .as_ref()
            .map(|r| r.try_get::<Uuid, _>("id").map_err(db_error))
            .transpose()?;
        let cut = selected
            .as_ref()
            .map(|r| r.try_get::<i64, _>("ordinal").map_err(db_error))
            .transpose()?
            .unwrap_or(0);
        let binding = format!(
            "{w}:{t}:{id}:{}:{epoch}:{etag}:{selected_id:?}:{limit}",
            a.user.id
        );
        let after_id: Option<Uuid> = q
            .cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("project-scope-history-v1", &binding, c))
            .transpose()?;
        let after: Option<i64> = if let Some(e) = after_id {
            Some(sqlx::query_scalar("SELECT ordinal FROM project_scope_events WHERE workspace_id=$1 AND project_id=$2 AND id=$3").bind(w).bind(id).bind(e).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::CursorExpired)?)
        } else {
            None
        };
        let item_after: Option<Uuid> = q
            .items_cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("project-scope-items-v1", &binding, c))
            .transpose()?;
        // A single SQL statement derives every item and count from the same
        // authorized snapshot. Never resolve retained titles or emit hidden IDs.
        // Project and baseline sizes are highly skewed. An unnamed statement lets
        // PostgreSQL cost the scoped membership indexes for this specific request.
        let row = sqlx::query(include_str!("project_scope_page.sql"))
            .persistent(false)
            .bind(w)
            .bind(t)
            .bind(id)
            .bind(a.user.id)
            .bind(selected_id)
            .bind(cut)
            .bind(after.map_or(i64::MAX, |ordinal| ordinal - 1))
            .bind(i64::from(limit) + 1)
            .bind(item_after)
            .bind(i64::from(limit))
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
        let items_has_more: bool = row.try_get("items_has_more").map_err(db_error)?;
        let items_next_cursor = if items_has_more {
            Some(self.taxonomy_cursor_encode(
                "project-scope-items-v1",
                &binding,
                &row.try_get::<Uuid, _>("last_item").map_err(db_error)?,
            )?)
        } else {
            None
        };
        fn items(row: &PgRow, key: &str) -> Result<Vec<ProjectScopeIssue>> {
            serde_json::from_value(row.try_get(key).map_err(db_error)?).map_err(|_| Error::Internal)
        }
        let mut events: Vec<EventRow> =
            serde_json::from_value(row.try_get("events").map_err(db_error)?)
                .map_err(|_| Error::Internal)?;
        let has_more = events.len() > limit as usize;
        events.truncate(limit as usize);
        let next_cursor = if has_more {
            events
                .last()
                .map(|e| {
                    self.taxonomy_cursor_encode("project-scope-history-v1", &binding, &e.event.id)
                })
                .transpose()?
        } else {
            None
        };
        Ok(ProjectScopeHistory {
            capture_limit: MAX_SCOPE as u32,
            items_has_more,
            items_next_cursor,
            baselines_has_more,
            baselines_next_cursor,
            scope_etag: etag,
            history_started_at: head.try_get("history_started_at").map_err(db_error)?,
            legacy_history_unknown: head.try_get("legacy_history_unknown").map_err(db_error)?,
            can_agree: p.can_edit
                && p.archived_at.is_none()
                && (p.can_manage || p.lead_id == Some(a.user.id)),
            baselines,
            baseline: selected.as_ref().map(baseline).transpose()?,
            comparison: selected_id
                .map(|_| {
                    Ok::<_, Error>(ProjectScopeComparison {
                        unchanged: row.try_get::<i64, _>("unchanged").map_err(db_error)? as usize,
                        added: items(&row, "added")?,
                        removed: items(&row, "removed")?,
                        postponed: items(&row, "postponed")?,
                    })
                })
                .transpose()?,
            current: items(&row, "current")?,
            events: Page {
                data: events.into_iter().map(|e| e.event).collect(),
                has_more,
                next_cursor,
            },
        })
    }

    pub async fn project_scope_history(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        q: ProjectScopeQuery,
    ) -> Result<ProjectScopeHistory> {
        let mut tx = self.scoped(a, w, false).await?;
        let out = self.scope_read(&mut tx, a, w, t, id, q).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn agree_project_scope(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        key: &str,
        mut req: AgreeProjectScope,
    ) -> Result<ProjectScopeHistory> {
        req.label = bounded_text(&req.label, "Baseline label", 160)?;
        req.reason = bounded_text(&req.reason, "Agreement reason", 1000)?;
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let route = format!("project-scope-agree:{t}:{id}");
        let mut tx = self.scoped(a, w, false).await?;
        visible_scope_target(&mut tx, a, w, t, id).await?;
        let receipt = lifecycle::lock_retry_record(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        projects::scope(&mut tx, a, w, t, true).await?;
        projects::lock(&mut tx, w, t, id).await?;
        let p = projects::project(&mut tx, a, w, t, id).await?;
        if !p.can_manage && p.lead_id != Some(a.user.id) {
            return Err(Error::Forbidden);
        }
        let replay = lifecycle::retry_body::<Uuid>(receipt, &rh)?;
        if p.archived_at.is_some() {
            return Err(Error::Conflict(
                "Restore this project before agreeing its scope".into(),
            ));
        }
        let b = if let Some(id) = replay {
            id
        } else {
            let current = self
                .scope_read(&mut tx, a, w, t, id, ProjectScopeQuery::default())
                .await?;
            lifecycle::version(
                etag,
                &current.scope_etag,
                &json!({"scope_etag":current.scope_etag}),
            )?;
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM project_issues pi JOIN issues i ON i.workspace_id=pi.workspace_id AND i.id=pi.issue_id WHERE pi.workspace_id=$1 AND pi.project_id=$2 AND i.deleted_at IS NULL").bind(w).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
            if count > MAX_SCOPE {
                return Err(Error::Invalid(
                    "Agree a baseline of at most 1,000 current issues".into(),
                ));
            }
            let ordinal:i64=sqlx::query_scalar("UPDATE project_scope_heads SET revision=revision+1 WHERE workspace_id=$1 AND project_id=$2 RETURNING revision").bind(w).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
            let b = Uuid::new_v4();
            graph::one(sqlx::query("INSERT INTO project_scope_baselines(id,workspace_id,team_id,project_id,ordinal,label,reason,author_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(b).bind(w).bind(t).bind(id).bind(ordinal).bind(&req.label).bind(&req.reason).bind(a.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            let inserted=sqlx::query("INSERT INTO project_scope_baseline_items(workspace_id,baseline_id,issue_id) SELECT $1,$2,pi.issue_id FROM project_issues pi JOIN issues i ON i.workspace_id=pi.workspace_id AND i.id=pi.issue_id WHERE pi.workspace_id=$1 AND pi.project_id=$3 AND i.deleted_at IS NULL").bind(w).bind(b).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
            if inserted != count as u64 {
                return Err(Error::Internal);
            }
            let version:i64=sqlx::query_scalar("UPDATE projects SET version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 RETURNING version").bind(w).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
            scope_emit(&mut tx, a, w, t, id, version, "project.scope_agreed", b).await?;
            save_scope_retry(&mut tx, w, a.user.id, &route, &kh, b).await?;
            b
        };
        let out = self
            .scope_read(
                &mut tx,
                a,
                w,
                t,
                id,
                ProjectScopeQuery {
                    baseline_id: Some(b),
                    ..Default::default()
                },
            )
            .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn change_project_scope(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        key: &str,
        mut req: ChangeProjectScope,
    ) -> Result<ProjectScopeHistory> {
        req.reason = bounded_text(&req.reason, "Scope change reason", 1000)?;
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let route = format!("project-scope-change:{t}:{id}");
        let mut tx = self.scoped(a, w, false).await?;
        visible_scope_target(&mut tx, a, w, t, id).await?;
        scope_issue(&mut tx, w, t, &req).await?;
        let receipt = lifecycle::lock_retry_record(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        projects::scope(&mut tx, a, w, t, true).await?;
        projects::lock(&mut tx, w, t, id).await?;
        let p = projects::project(&mut tx, a, w, t, id).await?;
        // A move can finish while the command waits on the receipt or team lock.
        scope_issue(&mut tx, w, t, &req).await?;
        let replay = lifecycle::retry_body::<Uuid>(receipt, &rh)?;
        if p.archived_at.is_some() {
            return Err(Error::Conflict(
                "Restore this project before changing its scope".into(),
            ));
        }
        if replay.is_none() {
            let linked: Option<Uuid> = sqlx::query_scalar(
                "SELECT project_id FROM project_issues WHERE workspace_id=$1 AND issue_id=$2",
            )
            .bind(w)
            .bind(req.issue_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
            if (req.action == ProjectScopeAction::Added && linked.is_some())
                || (req.action != ProjectScopeAction::Added && linked != Some(id))
            {
                return Err(Error::Conflict(
                    "This issue's project membership changed. Refresh the scope before continuing"
                        .into(),
                ));
            }
            let current = self
                .scope_read(&mut tx, a, w, t, id, ProjectScopeQuery::default())
                .await?;
            lifecycle::version(
                etag,
                &current.scope_etag,
                &json!({"scope_etag":current.scope_etag}),
            )?;
            sqlx::query("SELECT set_config('opentrack.scope_action',$1,true),set_config('opentrack.scope_reason',$2,true)").bind(req.action.as_str()).bind(&req.reason).execute(&mut *tx).await.map_err(db_error)?;
            match req.action {
                ProjectScopeAction::Added => {
                    graph::one(sqlx::query("INSERT INTO project_issues(workspace_id,team_id,project_id,issue_id) VALUES($1,$2,$3,$4)").bind(w).bind(t).bind(id).bind(req.issue_id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
                }
                ProjectScopeAction::Removed | ProjectScopeAction::Postponed => {
                    graph::one(sqlx::query("DELETE FROM project_issues WHERE workspace_id=$1 AND project_id=$2 AND issue_id=$3").bind(w).bind(id).bind(req.issue_id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
                }
            }
            let version:i64=sqlx::query_scalar("UPDATE projects SET version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 RETURNING version").bind(w).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
            let mutation = Uuid::new_v4();
            scope_emit(
                &mut tx,
                a,
                w,
                t,
                id,
                version,
                "project.scope_changed",
                mutation,
            )
            .await?;
            save_scope_retry(&mut tx, w, a.user.id, &route, &kh, mutation).await?;
        }
        let out = self
            .scope_read(&mut tx, a, w, t, id, ProjectScopeQuery::default())
            .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
}

// Check source visibility without loading project documents or taking write locks.
// The command rechecks authority and source state after receipt admission.
async fn visible_scope_target(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    id: Uuid,
) -> Result<()> {
    if !store::visible_team(tx, w, t, a.user.id, false)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE workspace_id=$1 AND team_id=$2 AND id=$3)",
    )
    .bind(w)
    .bind(t)
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if !exists {
        return Err(Error::NotFound);
    }
    Ok(())
}

async fn scope_issue(tx: &mut store::Tx, w: Uuid, t: Uuid, req: &ChangeProjectScope) -> Result<()> {
    let visible: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM issues WHERE workspace_id=$1 AND team_id=$2 AND id=$3
                       AND deleted_at IS NULL AND (NOT $4 OR archived_at IS NULL))",
    )
    .bind(w)
    .bind(t)
    .bind(req.issue_id)
    .bind(req.action == ProjectScopeAction::Added)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if !visible {
        return Err(Error::NotFound);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn scope_emit(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    id: Uuid,
    version: i64,
    kind: &'static str,
    mutation: Uuid,
) -> Result<()> {
    store::identity_audit(tx, w, a.user.id, id, kind, mutation)
        .await
        .map_err(db_error)?;
    store::append_stream_events(
        tx,
        &[store::Event {
            workspace: w,
            team: t,
            entity: id,
            version,
            actor: a.user.id,
            kind,
            mutation,
            changes: json!({}),
        }],
    )
    .await
    .map_err(db_error)
}

async fn save_scope_retry(
    tx: &mut store::Tx,
    w: Uuid,
    user: Uuid,
    route: &str,
    key: &str,
    receipt: Uuid,
) -> Result<()> {
    graph::one(sqlx::query("UPDATE idempotency_records SET response_body=$5 WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4").bind(w).bind(user).bind(route).bind(key).bind(json!(receipt)).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}
