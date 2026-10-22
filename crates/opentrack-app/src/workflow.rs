use super::*;
use chrono::Utc;
use hmac::{Hmac, Mac};
use opentrack_domain::*;
use serde_json::json;
use sqlx::Row;
use std::collections::HashSet;

fn exactly_one(n: u64) -> Result<()> {
    if n == 1 { Ok(()) } else { Err(Error::Internal) }
}
fn color(value: &str) -> Result<()> {
    if STATUS_COLORS.contains(&value) {
        Ok(())
    } else {
        Err(Error::Invalid("Choose an available status color".into()))
    }
}
async fn lock_team(tx: &mut store::Tx, w: Uuid, t: Uuid, user: Uuid, write: bool) -> Result<()> {
    if !store::visible_team(tx, w, t, user, write)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    if !write {
        sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR SHARE")
            .bind(w)
            .bind(t)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    }
    Ok(())
}
async fn read_workflow(tx: &mut store::Tx, w: Uuid, t: Uuid, user: Uuid) -> Result<Workflow> {
    store::workflow(tx, w, t, user)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)
}
async fn manage(
    tx: &mut store::Tx,
    actor: &Actor,
    w: Uuid,
    t: Uuid,
    etag: Option<&str>,
) -> Result<Workflow> {
    lock_team(tx, w, t, actor.user.id, true).await?;
    lifecycle::require_active_team(tx, w, t).await?;
    let flow = read_workflow(tx, w, t, actor.user.id).await?;
    if !flow.can_manage {
        return Err(Error::Forbidden);
    }
    if let Some(tag) = etag {
        lifecycle::version(tag, &flow.etag(), &flow)?;
    }
    Ok(flow)
}
async fn status(tx: &mut store::Tx, w: Uuid, t: Uuid, s: Uuid) -> Result<Status> {
    store::workflow_status(tx, w, t, s)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)
}
async fn bump(tx: &mut store::Tx, w: Uuid, t: Uuid, user: Uuid) -> Result<Workflow> {
    sqlx::query_scalar::<_,i64>("UPDATE teams SET workflow_version=workflow_version+1 WHERE workspace_id=$1 AND id=$2 RETURNING workflow_version").bind(w).bind(t).fetch_one(&mut **tx).await.map_err(db_error)?;
    read_workflow(tx, w, t, user).await
}
async fn emit(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    user: Uuid,
    version: &str,
    action: &str,
    entity: Uuid,
) -> Result<()> {
    store::identity_event(
        tx,
        store::IdentityEvent {
            workspace: w,
            actor: user,
            entity,
            version: version.parse().map_err(|_| Error::Internal)?,
            team: Some(t),
            action,
            access_changed: false,
        },
    )
    .await
    .map_err(db_error)
}
async fn used_count(tx: &mut store::Tx, w: Uuid, t: Uuid, s: Uuid) -> Result<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3",
    )
    .bind(w)
    .bind(t)
    .bind(s)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)
}
async fn target_in_use(tx: &mut store::Tx, w: Uuid, t: Uuid, s: Uuid) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_replacements r JOIN jobs j ON j.workspace_id=r.workspace_id AND j.id=r.id WHERE r.workspace_id=$1 AND r.team_id=$2 AND r.replacement_status_id=$3 AND j.state<>'succeeded')").bind(w).bind(t).bind(s).fetch_one(&mut **tx).await.map_err(db_error)
}
async fn replacement_pair(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    s: Uuid,
    target: Uuid,
) -> Result<Status> {
    let source = status(tx, w, t, s).await?;
    let replacement = status(tx, w, t, target).await?;
    if s == target
        || source.archived_at.is_some()
        || source.retiring_at.is_some()
        || replacement.archived_at.is_some()
        || replacement.retiring_at.is_some()
    {
        return Err(Error::Invalid(
            "Choose two different active statuses in this team".into(),
        ));
    }
    if target_in_use(tx, w, t, s).await? {
        return Err(Error::Conflict(
            "This status is the target of an unfinished replacement".into(),
        ));
    }
    Ok(source)
}

impl App {
    pub async fn workflow(&self, actor: &Actor, w: Uuid, t: Uuid) -> Result<Workflow> {
        let mut tx = self.scoped(actor, w, false).await?;
        lock_team(&mut tx, w, t, actor.user.id, false).await?;
        let result = read_workflow(&mut tx, w, t, actor.user.id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn get_status(&self, actor: &Actor, w: Uuid, t: Uuid, s: Uuid) -> Result<Status> {
        let mut tx = self.scoped(actor, w, false).await?;
        lock_team(&mut tx, w, t, actor.user.id, false).await?;
        let result = status(&mut tx, w, t, s).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn list_statuses(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        q: DirectoryQuery,
    ) -> Result<Page<Status>> {
        let limit = q.limit.unwrap_or(50);
        let state = q.state.as_deref().unwrap_or("active");
        if !(1..=100).contains(&limit) || !["active", "archived", "all"].contains(&state) {
            return Err(Error::Invalid("Invalid status directory query".into()));
        }
        let mut tx = self.scoped(actor, w, false).await?;
        lock_team(&mut tx, w, t, actor.user.id, false).await?;
        let version: i64 = sqlx::query_scalar(
            "SELECT workflow_version FROM teams WHERE workspace_id=$1 AND id=$2",
        )
        .bind(w)
        .bind(t)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{t}:{}:{epoch}:{version}:{state}:{limit}",
            actor.user.id
        );
        let after = q
            .cursor
            .as_deref()
            .map(|c| self.status_cursor_decode(&binding, c, Utc::now().timestamp()))
            .transpose()?;
        let mut ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND ($3='all' OR (archived_at IS NOT NULL)=($3='archived')) AND ($4::uuid IS NULL OR id>$4) ORDER BY id LIMIT $5").bind(w).bind(t).bind(state).bind(after).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let mut data = Vec::with_capacity(ids.len());
        for id in &ids {
            data.push(status(&mut tx, w, t, *id).await?);
        }
        let next_cursor = if has_more {
            ids.last()
                .map(|id| self.status_cursor_encode(&binding, *id, Utc::now().timestamp()))
                .transpose()?
        } else {
            None
        };
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
    pub async fn create_status(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        key: &str,
        etag: &str,
        mut req: CreateStatus,
    ) -> Result<(Uuid, Workflow)> {
        req.name = bounded_text(&req.name, "Status name", 100)?;
        color(&req.color)?;
        if !STATUS_CATEGORIES.contains(&req.category.as_str()) {
            return Err(Error::Invalid("Choose an available status category".into()));
        }
        let (kh, rh) = lifecycle::request_hash(key, &(t, &req))?;
        let mut tx = self.scoped(actor, w, true).await?;
        let replay = lifecycle::retry::<(Uuid, Workflow)>(
            &mut tx,
            w,
            actor.user.id,
            "statuses.create",
            &kh,
            &rh,
        )
        .await?;
        let flow = manage(
            &mut tx,
            actor,
            w,
            t,
            if replay.is_none() { Some(etag) } else { None },
        )
        .await?;
        if let Some(result) = replay {
            tx.commit().await.map_err(db_error)?;
            return Ok(result);
        }
        if flow.statuses.len() >= 100 {
            return Err(Error::Conflict(
                "A team can have at most 100 nonarchived statuses".into(),
            ));
        }
        let position = flow
            .statuses
            .iter()
            .filter(|s| s.category == req.category)
            .map(|s| s.position)
            .max()
            .unwrap_or(-1)
            .checked_add(1)
            .ok_or(Error::Internal)?;
        let id = Uuid::new_v4();
        exactly_one(sqlx::query("INSERT INTO workflow_statuses(id,workspace_id,team_id,name,category,color,position) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(w).bind(t).bind(req.name).bind(req.category).bind(req.color).bind(position).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let result = (id, bump(&mut tx, w, t, actor.user.id).await?);
        lifecycle::save_retry(&mut tx, w, actor.user.id, "statuses.create", &kh, &result).await?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &result.1.version,
            "workflow.status_created",
            id,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn update_status(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        s: Uuid,
        etag: &str,
        req: UpdateStatus,
    ) -> Result<Workflow> {
        if req.name.is_none() && req.color.is_none() {
            return Err(Error::Invalid("Provide a field to update".into()));
        }
        let name = req
            .name
            .map(|v| bounded_text(&v, "Status name", 100))
            .transpose()?;
        if let Some(c) = &req.color {
            color(c)?;
        }
        let mut tx = self.scoped(actor, w, true).await?;
        manage(&mut tx, actor, w, t, Some(etag)).await?;
        let old = status(&mut tx, w, t, s).await?;
        if old.archived_at.is_some() || old.retiring_at.is_some() {
            return Err(Error::Conflict(
                "Restore or finish replacing this status before editing it".into(),
            ));
        }
        exactly_one(sqlx::query("UPDATE workflow_statuses SET name=COALESCE($4,name),color=COALESCE($5,color) WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(s).bind(name).bind(req.color).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let result = bump(&mut tx, w, t, actor.user.id).await?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &result.version,
            "workflow.status_updated",
            s,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn set_default_status(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        etag: &str,
        req: SetDefaultStatus,
    ) -> Result<Workflow> {
        let mut tx = self.scoped(actor, w, true).await?;
        manage(&mut tx, actor, w, t, Some(etag)).await?;
        let target = status(&mut tx, w, t, req.status_id).await?;
        if target.archived_at.is_some() || target.retiring_at.is_some() {
            return Err(Error::Invalid("The default must be active".into()));
        }
        set_default(&mut tx, w, t, req.status_id).await?;
        let result = bump(&mut tx, w, t, actor.user.id).await?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &result.version,
            "workflow.default_changed",
            req.status_id,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn reorder_statuses(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        etag: &str,
        req: ReorderStatuses,
    ) -> Result<Workflow> {
        let mut tx = self.scoped(actor, w, true).await?;
        let flow = manage(&mut tx, actor, w, t, Some(etag)).await?;
        let expected: HashSet<_> = flow.statuses.iter().map(|s| s.id).collect();
        let given: HashSet<_> = req.status_ids.iter().copied().collect();
        if given != expected || given.len() != req.status_ids.len() {
            return Err(Error::Invalid(
                "Provide every nonarchived status once".into(),
            ));
        }
        let mut last_category = 0;
        for (position, id) in req.status_ids.iter().enumerate() {
            let item = flow
                .statuses
                .iter()
                .find(|s| s.id == *id)
                .ok_or(Error::Internal)?;
            let category = STATUS_CATEGORIES
                .iter()
                .position(|c| *c == item.category)
                .ok_or(Error::Internal)?;
            if category < last_category {
                return Err(Error::Invalid(
                    "Keep statuses within the ordered category groups".into(),
                ));
            }
            last_category = category;
            exactly_one(sqlx::query("UPDATE workflow_statuses SET position=$4 WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(id).bind(position as i32).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        let result = bump(&mut tx, w, t, actor.user.id).await?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &result.version,
            "workflow.reordered",
            t,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn archive_status(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        s: Uuid,
        etag: &str,
    ) -> Result<Workflow> {
        let mut tx = self.scoped(actor, w, true).await?;
        manage(&mut tx, actor, w, t, Some(etag)).await?;
        let item = status(&mut tx, w, t, s).await?;
        if item.is_default
            || item.retiring_at.is_some()
            || used_count(&mut tx, w, t, s).await? > 0
            || target_in_use(&mut tx, w, t, s).await?
        {
            return Err(Error::Conflict(
                "Choose an explicit replacement for a used or default status".into(),
            ));
        }
        if item.archived_at.is_none() {
            exactly_one(sqlx::query("UPDATE workflow_statuses SET archived_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(s).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        let result = bump(&mut tx, w, t, actor.user.id).await?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &result.version,
            "workflow.status_archived",
            s,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn restore_status(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        s: Uuid,
        etag: &str,
    ) -> Result<Workflow> {
        let mut tx = self.scoped(actor, w, true).await?;
        let flow = manage(&mut tx, actor, w, t, Some(etag)).await?;
        let item = status(&mut tx, w, t, s).await?;
        if item.archived_at.is_none() {
            return Err(Error::Conflict("This status is not archived".into()));
        }
        if flow.statuses.len() >= 100 {
            return Err(Error::Conflict(
                "A team can have at most 100 nonarchived statuses".into(),
            ));
        }
        exactly_one(sqlx::query("UPDATE workflow_statuses SET archived_at=NULL,retiring_at=NULL,replacement_status_id=NULL,replacement_job_id=NULL WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(s).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let result = bump(&mut tx, w, t, actor.user.id).await?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &result.version,
            "workflow.status_restored",
            s,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn preview_status_replacement(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        s: Uuid,
        replacement: Uuid,
    ) -> Result<WorkflowReplacementPreview> {
        let mut tx = self.scoped(actor, w, false).await?;
        let flow = manage(&mut tx, actor, w, t, None).await?;
        let source = replacement_pair(&mut tx, w, t, s, replacement).await?;
        let result = WorkflowReplacementPreview {
            team_id: t,
            source_status_id: s,
            replacement_status_id: replacement,
            issue_count: used_count(&mut tx, w, t, s).await?.to_string(),
            source_is_default: source.is_default,
            workflow_version: flow.version,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    // Scope IDs, retry key and aggregate precondition are separate command inputs.
    #[allow(clippy::too_many_arguments)]
    pub async fn replace_status(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        s: Uuid,
        key: &str,
        etag: &str,
        req: ReplaceStatus,
    ) -> Result<WorkflowReplacement> {
        let (kh, rh) = lifecycle::request_hash(key, &(t, s, &req))?;
        let mut tx = self.scoped(actor, w, true).await?;
        let replay = lifecycle::retry::<WorkflowReplacement>(
            &mut tx,
            w,
            actor.user.id,
            "statuses.replace",
            &kh,
            &rh,
        )
        .await?;
        let flow = manage(
            &mut tx,
            actor,
            w,
            t,
            if replay.is_none() { Some(etag) } else { None },
        )
        .await?;
        if let Some(result) = replay {
            tx.commit().await.map_err(db_error)?;
            return Ok(result);
        }
        let source = replacement_pair(&mut tx, w, t, s, req.replacement_status_id).await?;
        let count = used_count(&mut tx, w, t, s).await?;
        let id = Uuid::new_v4();
        exactly_one(sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'workflow.status_replace',$3,$4)").bind(w).bind(id).bind(actor.user.id).bind(format!("{s}:{}",flow.version)).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        exactly_one(sqlx::query("INSERT INTO workflow_replacements(workspace_id,id,team_id,source_status_id,replacement_status_id,matched_count,remaining_count) VALUES($1,$2,$3,$4,$5,$6,$6)").bind(w).bind(id).bind(t).bind(s).bind(req.replacement_status_id).bind(count).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        sqlx::query(
            "INSERT INTO worker_workspace_registry(workspace_id) VALUES($1) ON CONFLICT DO NOTHING",
        )
        .bind(w)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        let registered: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM worker_workspace_registry WHERE workspace_id=$1)",
        )
        .bind(w)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if !registered {
            return Err(Error::Internal);
        }
        if source.is_default {
            set_default(&mut tx, w, t, req.replacement_status_id).await?;
        }
        exactly_one(sqlx::query("UPDATE workflow_statuses SET retiring_at=clock_timestamp(),replacement_status_id=$4,replacement_job_id=$5 WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(s).bind(req.replacement_status_id).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let flow = bump(&mut tx, w, t, actor.user.id).await?;
        let result = store::workflow_replacement(&mut tx, w, t, id)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        lifecycle::save_retry(&mut tx, w, actor.user.id, "statuses.replace", &kh, &result).await?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &flow.version,
            "workflow.replacement_started",
            s,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn get_workflow_replacement(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        job: Uuid,
    ) -> Result<WorkflowReplacement> {
        let mut tx = self.scoped(actor, w, false).await?;
        lock_team(&mut tx, w, t, actor.user.id, false).await?;
        let result = store::workflow_replacement(&mut tx, w, t, job)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn resume_workflow_replacement(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        job: Uuid,
        etag: &str,
    ) -> Result<WorkflowReplacement> {
        let mut tx = self.scoped(actor, w, true).await?;
        // Resolve and authorize scope before taking a known job lock; no hidden-ID oracle.
        store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        store::workflow_replacement(&mut tx, w, t, job)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        sqlx::query("SELECT id FROM jobs WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(job)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        manage(&mut tx, actor, w, t, None).await?;
        let old = store::workflow_replacement(&mut tx, w, t, job)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        lifecycle::version(etag, &old.etag(), &old)?;
        if !["blocked", "dead_letter", "retry_wait"].contains(&old.state.as_str()) {
            return Err(Error::Conflict(
                "This replacement is not waiting for a manual retry".into(),
            ));
        }
        let source = status(&mut tx, w, t, old.source_status_id).await?;
        let target = status(&mut tx, w, t, old.replacement_status_id).await?;
        if source.replacement_job_id != Some(job)
            || source.archived_at.is_some()
            || target.retiring_at.is_some()
            || target.archived_at.is_some()
        {
            return Err(Error::Conflict(
                "The replacement statuses are no longer available".into(),
            ));
        }
        exactly_one(sqlx::query("UPDATE jobs SET requested_by=$3,state='queued',retry_count=0,error_code=NULL,available_at=clock_timestamp(),finished_at=NULL,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(job).bind(actor.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let flow = bump(&mut tx, w, t, actor.user.id).await?;
        let result = store::workflow_replacement(&mut tx, w, t, job)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        emit(
            &mut tx,
            w,
            t,
            actor.user.id,
            &flow.version,
            "workflow.replacement_resumed",
            source.id,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    fn status_mac(&self, binding: &str, payload: &str) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.setup_token_hash.as_bytes())
            .map_err(|_| Error::Internal)?;
        mac.update(b"opentrack.workflow-status-cursor.v1\0");
        mac.update(binding.as_bytes());
        mac.update(b"\0");
        mac.update(payload.as_bytes());
        Ok(mac)
    }
    fn status_cursor_encode(&self, binding: &str, id: Uuid, now: i64) -> Result<String> {
        let payload = format!("{id}|{now}|{}", now + 86400);
        let tag = self.status_mac(binding, &payload)?.finalize().into_bytes();
        Ok(format!(
            "{payload}|{}",
            tag.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ))
    }
    fn status_cursor_decode(&self, binding: &str, cursor: &str, now: i64) -> Result<Uuid> {
        let invalid =
            || Error::Invalid("This status cursor is invalid or belongs to another query".into());
        if cursor.len() > 256 {
            return Err(invalid());
        }
        let (payload, sig) = cursor.rsplit_once('|').ok_or_else(invalid)?;
        if sig.len() != 64 || !sig.is_ascii() {
            return Err(invalid());
        }
        let bytes = (0..64)
            .step_by(2)
            .map(|i| u8::from_str_radix(&sig[i..i + 2], 16).map_err(|_| invalid()))
            .collect::<Result<Vec<_>>>()?;
        self.status_mac(binding, payload)?
            .verify_slice(&bytes)
            .map_err(|_| invalid())?;
        let p: Vec<_> = payload.split('|').collect();
        if p.len() != 3 {
            return Err(invalid());
        }
        let issued = p[1].parse::<i64>().map_err(|_| invalid())?;
        let expiry = p[2].parse::<i64>().map_err(|_| invalid())?;
        if issued > now + 60 || issued.checked_add(86400) != Some(expiry) {
            return Err(invalid());
        }
        if expiry <= now {
            return Err(Error::CursorExpired);
        }
        p[0].parse().map_err(|_| invalid())
    }
}
async fn set_default(tx: &mut store::Tx, w: Uuid, t: Uuid, id: Uuid) -> Result<()> {
    exactly_one(sqlx::query("UPDATE workflow_statuses SET is_default=false WHERE workspace_id=$1 AND team_id=$2 AND is_default").bind(w).bind(t).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    exactly_one(sqlx::query("UPDATE workflow_statuses SET is_default=true WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}

/// The worker already owns the enabled requester account guard, workspace guard,
/// job fence and attempt row. This handler never reads credentials or commits.
pub(crate) async fn run_replacement_batch(
    tx: &mut store::Tx,
    claim: &crate::worker::JobClaim,
) -> Result<crate::worker::BatchOutcome> {
    use crate::worker::BatchOutcome;
    let w = claim.workspace_id;
    let actor = claim.requested_by.ok_or(Error::Internal)?;
    let row=sqlx::query("SELECT team_id,source_status_id,replacement_status_id FROM workflow_replacements WHERE workspace_id=$1 AND id=$2").bind(w).bind(claim.id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::Internal)?;
    let t: Uuid = row.try_get("team_id").map_err(db_error)?;
    let source: Uuid = row.try_get("source_status_id").map_err(db_error)?;
    let target: Uuid = row.try_get("replacement_status_id").map_err(db_error)?;
    let active: bool = sqlx::query_scalar("SELECT archived_at IS NULL FROM workspaces WHERE id=$1")
        .bind(w)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .unwrap_or(false);
    if !active {
        return Ok(BatchOutcome::Blocked("parent_archived"));
    }
    let team = sqlx::query(
        "SELECT private,archived_at FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(w)
    .bind(t)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or(Error::Internal)?;
    if team
        .try_get::<Option<chrono::DateTime<Utc>>, _>("archived_at")
        .map_err(db_error)?
        .is_some()
    {
        return Ok(BatchOutcome::Blocked("parent_archived"));
    }
    let permitted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members wm LEFT JOIN team_members tm ON tm.workspace_id=wm.workspace_id AND tm.team_id=$2 AND tm.user_id=wm.user_id WHERE wm.workspace_id=$1 AND wm.user_id=$3 AND wm.active AND wm.role<>'viewer' AND (NOT $4 OR tm.user_id IS NOT NULL) AND (wm.role IN ('owner','admin') OR tm.role='maintainer'))").bind(w).bind(t).bind(actor).bind(team.try_get::<bool,_>("private").map_err(db_error)?).fetch_one(&mut **tx).await.map_err(db_error)?;
    if !permitted {
        return Ok(BatchOutcome::Blocked("access_lost"));
    }
    sqlx::query("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND id=ANY($3) ORDER BY id FOR UPDATE").bind(w).bind(t).bind(vec![source,target]).fetch_all(&mut **tx).await.map_err(db_error)?;
    let before = status(tx, w, t, source).await?;
    let replacement = status(tx, w, t, target).await?;
    if before.archived_at.is_some()
        || before.replacement_job_id != Some(claim.id)
        || before.replacement_status_id != Some(target)
        || replacement.archived_at.is_some()
        || replacement.retiring_at.is_some()
    {
        return Ok(BatchOutcome::Blocked("target_unavailable"));
    }
    let selected:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND status_id=$3 ORDER BY id LIMIT 100 FOR UPDATE").bind(w).bind(t).bind(source).fetch_all(&mut **tx).await.map_err(db_error)?;
    let mut events = Vec::with_capacity(selected.len() + 1);
    for id in &selected {
        let version:i64=sqlx::query_scalar("UPDATE issues SET status_id=$4,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3 AND status_id=$5 RETURNING version").bind(w).bind(t).bind(id).bind(target).bind(source).fetch_one(&mut **tx).await.map_err(db_error)?;
        events.push(store::Event{workspace:w,team:t,entity:*id,version,actor,kind:"issue.updated",mutation:claim.id,changes:json!({"fields":["status_id"],"status_id":{"before":source,"after":target},"status_names":{"before":before.name,"after":replacement.name},"source":"workflow_replacement","job_id":claim.id})});
    }
    let remaining = used_count(tx, w, t, source).await?;
    exactly_one(sqlx::query("UPDATE workflow_replacements SET processed_count=processed_count+$3,remaining_count=$4 WHERE workspace_id=$1 AND id=$2").bind(w).bind(claim.id).bind(selected.len() as i64).bind(remaining).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    if remaining == 0 {
        exactly_one(sqlx::query("UPDATE workflow_statuses SET archived_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3 AND replacement_job_id=$4").bind(w).bind(t).bind(source).bind(claim.id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    }
    let version:i64=sqlx::query_scalar("UPDATE teams SET workflow_version=workflow_version+1 WHERE workspace_id=$1 AND id=$2 RETURNING workflow_version").bind(w).bind(t).fetch_one(&mut **tx).await.map_err(db_error)?;
    for event in &events {
        store::append_activity(tx, event).await.map_err(db_error)?;
    }
    let action = if remaining == 0 {
        "workflow.replacement_completed"
    } else {
        "workflow.replacement_progress"
    };
    store::identity_audit(tx, w, actor, source, action, Uuid::new_v4())
        .await
        .map_err(db_error)?;
    events.push(store::Event {
        workspace: w,
        team: t,
        entity: t,
        version,
        actor,
        kind: action,
        mutation: claim.id,
        changes: json!({}),
    });
    store::append_stream_events(tx, &events)
        .await
        .map_err(db_error)?;
    Ok(if remaining == 0 {
        BatchOutcome::Complete
    } else {
        BatchOutcome::More
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn status_cursors_require_authentic_scope_and_expire() {
        let app = App {
            pool: PgPoolOptions::new()
                .connect_lazy("postgres://unused@localhost/unused")
                .unwrap(),
            setup_token_hash: digest(b"status-cursor-test"),
            hashes: Arc::new(Semaphore::new(1)),
        };
        let now = 1_800_000_000;
        let id = Uuid::new_v4();
        let binding = "workspace:team:actor:epoch:version:archived:50";
        let token = app.status_cursor_encode(binding, id, now).unwrap();
        assert_eq!(app.status_cursor_decode(binding, &token, now).unwrap(), id);
        assert!(matches!(
            app.status_cursor_decode("another-scope", &token, now),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            app.status_cursor_decode(binding, &token, now + 86400),
            Err(Error::CursorExpired)
        ));
        let expired = app.status_cursor_encode(binding, id, now - 86401).unwrap();
        assert!(matches!(
            app.status_cursor_decode(binding, &expired, now),
            Err(Error::CursorExpired)
        ));
        let future = app.status_cursor_encode(binding, id, now + 61).unwrap();
        assert!(matches!(
            app.status_cursor_decode(binding, &future, now),
            Err(Error::Invalid(_))
        ));
        let changed = token.replacen(&id.to_string(), &Uuid::new_v4().to_string(), 1);
        assert!(matches!(
            app.status_cursor_decode(binding, &changed, now),
            Err(Error::Invalid(_))
        ));
    }
}
