use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::*;
use serde_json::json;
use sqlx::{Row, postgres::PgRow};

pub(crate) async fn scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    write: bool,
) -> Result<(bool, bool)> {
    if !store::visible_team(tx, w, t, a.user.id, write)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    let team = store::team_detail(tx, w, t, a.user.id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    let workspace = store::workspace_detail(tx, w, a.user.id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    let active = team.archived_at.is_none() && workspace.archived_at.is_none();
    if write && !active {
        return Err(Error::Conflict(
            "Restore the workspace and team before changing projects".into(),
        ));
    }
    let edit = active && writable(&workspace.role);
    if write && !edit {
        return Err(Error::Forbidden);
    }
    Ok((edit, active && team.can_manage))
}
async fn lead(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    id: Option<Uuid>,
) -> Result<Option<IssueAssignee>> {
    let Some(id) = id else { return Ok(None) };
    let r=sqlx::query("SELECT u.id,u.display_name,(u.disabled_at IS NULL AND m.active AND m.role<>'viewer' AND t.archived_at IS NULL AND w.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=$2 AND tm.user_id=u.id))) AS assignable FROM users u JOIN workspace_members m ON m.user_id=u.id AND m.workspace_id=$1 JOIN teams t ON t.workspace_id=$1 AND t.id=$2 JOIN workspaces w ON w.id=$1 WHERE u.id=$3").bind(w).bind(t).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?;
    r.map(|r| {
        Ok(IssueAssignee {
            id: r.try_get("id").map_err(db_error)?,
            display_name: r.try_get("display_name").map_err(db_error)?,
            assignable: r.try_get("assignable").map_err(db_error)?,
        })
    })
    .transpose()
}
async fn validate_lead(tx: &mut store::Tx, w: Uuid, t: Uuid, id: Option<Uuid>) -> Result<()> {
    if id.is_some() && !lead(tx, w, t, id).await?.is_some_and(|v| v.assignable) {
        return Err(Error::Invalid(
            "Choose an active contributor who can access this team".into(),
        ));
    }
    Ok(())
}
fn state(v: &str) -> Result<ProjectState> {
    match v {
        "planned" => Ok(ProjectState::Planned),
        "active" => Ok(ProjectState::Active),
        "paused" => Ok(ProjectState::Paused),
        "completed" => Ok(ProjectState::Completed),
        "canceled" => Ok(ProjectState::Canceled),
        _ => Err(Error::Internal),
    }
}
fn health(v: &str) -> Result<ProjectHealth> {
    match v {
        "on_track" => Ok(ProjectHealth::OnTrack),
        "at_risk" => Ok(ProjectHealth::AtRisk),
        "off_track" => Ok(ProjectHealth::OffTrack),
        _ => Err(Error::Internal),
    }
}
pub(crate) async fn project(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    id: Uuid,
) -> Result<Project> {
    let (edit, manage) = scope(tx, a, w, t, false).await?;
    let row = sqlx::query("SELECT * FROM projects WHERE workspace_id=$1 AND team_id=$2 AND id=$3")
        .bind(w)
        .bind(t)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    project_row(tx, w, t, &row, edit, manage).await
}
async fn project_row(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    r: &PgRow,
    edit: bool,
    manage: bool,
) -> Result<Project> {
    let id: Uuid = r.try_get("id").map_err(db_error)?;
    let document = sqlx::query("SELECT schema_version,body,plain_text,version::text FROM documents WHERE workspace_id=$1 AND project_id=$2 AND deleted_at IS NULL")
        .bind(w).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
    let body: serde_json::Value = document.try_get("body").map_err(db_error)?;
    let schema_version = document.try_get("schema_version").map_err(db_error)?;
    let supported = validate_project_narrative(&DocumentWrite {
        schema_version,
        body: body.clone(),
        moderation_reason: None,
    })
    .is_ok();
    let lead_id = r.try_get("lead_id").map_err(db_error)?;
    let mut item = Project {
        id,
        workspace_id: w,
        team_id: t,
        name: r.try_get("name").map_err(db_error)?,
        summary: r.try_get("summary").map_err(db_error)?,
        narrative: TemplateDocument {
            schema_version,
            body,
            plain_text: document.try_get("plain_text").map_err(db_error)?,
            schema_supported: supported,
        },
        narrative_version: Some(document.try_get("version").map_err(db_error)?),
        lead_id,
        lead: lead(tx, w, t, lead_id).await?,
        start_date: r.try_get("start_date").map_err(db_error)?,
        target_date: r.try_get("target_date").map_err(db_error)?,
        state: state(r.try_get("state").map_err(db_error)?)?,
        health: health(r.try_get("health").map_err(db_error)?)?,
        version: r
            .try_get::<i64, _>("version")
            .map_err(db_error)?
            .to_string(),
        etag: String::new(),
        metadata_etag: String::new(),
        archived_at: r.try_get("archived_at").map_err(db_error)?,
        created_at: r.try_get("created_at").map_err(db_error)?,
        updated_at: r.try_get("updated_at").map_err(db_error)?,
        can_edit: edit
            && r.try_get::<Option<DateTime<Utc>>, _>("archived_at")
                .map_err(db_error)?
                .is_none(),
        can_manage: manage,
    };
    refresh_etags(&mut item)?;
    Ok(item)
}
fn refresh_etags(item: &mut Project) -> Result<()> {
    let mut summary = ProjectSummary::from(&*item);
    summary.etag.clear();
    item.metadata_etag = format!(
        "\"project-metadata:{}:{}:{}\"",
        item.id,
        item.version,
        digest(&serde_json::to_vec(&summary).map_err(|_| Error::Internal)?)
    );
    item.etag.clear();
    item.etag = format!(
        "\"project:{}:{}:{}\"",
        item.id,
        item.version,
        digest(&serde_json::to_vec(&item).map_err(|_| Error::Internal)?)
    );
    Ok(())
}
pub(crate) fn metadata(item: &Project) -> ProjectSummary {
    let mut value = ProjectSummary::from(item);
    value.etag = item.metadata_etag.clone();
    value
}
pub(crate) fn version(etag: &str, item: &Project) -> Result<()> {
    if etag == item.metadata_etag {
        Ok(())
    } else {
        lifecycle::version(etag, &item.etag, item)
    }
}
async fn progress(tx: &mut store::Tx, w: Uuid, t: Uuid, id: Uuid) -> Result<ProjectProgress> {
    let r=sqlx::query("WITH buckets AS (SELECT s.category,(i.deleted_at IS NOT NULL OR i.archived_at IS NOT NULL) AS excluded,count(*) AS count FROM project_issues pi JOIN issues i ON i.workspace_id=pi.workspace_id AND i.team_id=pi.team_id AND i.id=pi.issue_id JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id WHERE pi.workspace_id=$1 AND pi.team_id=$2 AND pi.project_id=$3 GROUP BY s.category,excluded) SELECT coalesce(jsonb_agg(jsonb_build_array(category,excluded,count)),'[]'::jsonb) AS buckets,statement_timestamp() AS as_of FROM buckets").bind(w).bind(t).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
    progress_counts(&r)
}
pub(crate) fn progress_counts(r: &PgRow) -> Result<ProjectProgress> {
    let sqlx::types::Json(buckets): sqlx::types::Json<Vec<(String, bool, i64)>> =
        r.try_get("buckets").map_err(db_error)?;
    let counts = PlanningCounts::from_buckets(buckets.iter().map(|(category, excluded, count)| {
        PlanningBucket {
            category,
            excluded: *excluded,
            count: *count,
            estimate_total: None,
        }
    }));
    Ok(ProjectProgress {
        total: counts.total,
        completed: counts.completed,
        canceled: counts.canceled,
        eligible: counts.eligible(),
        open: counts.unfinished(),
        unscoped: counts.unscoped(),
        as_of: r.try_get("as_of").map_err(db_error)?,
    })
}
async fn emit(tx: &mut store::Tx, a: &Actor, w: Uuid, item: &Project, action: &str) -> Result<()> {
    store::identity_event(
        tx,
        store::IdentityEvent {
            workspace: w,
            actor: a.user.id,
            entity: item.id,
            version: item.version.parse().map_err(|_| Error::Internal)?,
            team: Some(item.team_id),
            action,
            access_changed: false,
        },
    )
    .await
    .map_err(db_error)
}
async fn emit_after(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    item: &Project,
    action: &'static str,
    mut events: Vec<store::Event<'static>>,
) -> Result<()> {
    let mutation = Uuid::new_v4();
    store::identity_audit(tx, w, a.user.id, item.id, action, mutation)
        .await
        .map_err(db_error)?;
    events.push(store::Event {
        workspace: w,
        team: item.team_id,
        entity: item.id,
        version: item.version.parse().map_err(|_| Error::Internal)?,
        actor: a.user.id,
        kind: action,
        mutation,
        changes: json!({}),
    });
    store::append_stream_events(tx, &events)
        .await
        .map_err(db_error)
}
pub(crate) async fn lock(tx: &mut store::Tx, w: Uuid, t: Uuid, id: Uuid) -> Result<()> {
    sqlx::query(
        "SELECT id FROM projects WHERE workspace_id=$1 AND team_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(w)
    .bind(t)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or(Error::NotFound)?;
    Ok(())
}
fn page_limit(limit: Option<u32>) -> Result<usize> {
    let limit = limit.unwrap_or(25);
    if !(1..=100).contains(&limit) {
        return Err(Error::Invalid("Page size must be 1–100".into()));
    }
    Ok(limit as usize)
}
async fn receipt<T: serde::Serialize>(
    tx: &mut store::Tx,
    w: Uuid,
    a: &Actor,
    route: &str,
    key: &str,
    item: &T,
) -> Result<()> {
    graph::one(sqlx::query("UPDATE idempotency_records SET response_body=$5 WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4").bind(w).bind(a.user.id).bind(route).bind(key).bind(serde_json::to_value(item).map_err(|_|Error::Internal)?).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}
enum ProjectMatch<'a> {
    Full(&'a str),
    Metadata(&'a str),
}
impl App {
    pub async fn list_projects(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        mut q: ProjectQuery,
    ) -> Result<Page<ProjectSummary>> {
        let limit = page_limit(q.limit)?;
        let term = q.q.as_deref().unwrap_or("").trim().to_owned();
        if term.chars().count() > 100 {
            return Err(Error::Invalid(
                "Project search is limited to 100 characters".into(),
            ));
        }
        q.q = Some(term.clone());
        q.limit = Some(limit as u32);
        q.archived = Some(q.archived.unwrap_or(false));
        let mut tx = self.scoped(a, w, false).await?;
        let (edit, manage) = scope(&mut tx, a, w, t, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let mut fingerprint = q.clone();
        fingerprint.cursor = None;
        let binding = format!(
            "{w}:{t}:{}:{epoch}:{}",
            a.user.id,
            serde_json::to_string(&fingerprint).map_err(|_| Error::Internal)?
        );
        let after: Option<(String, Uuid)> = q
            .cursor
            .as_deref()
            .map(|s| self.taxonomy_cursor_decode("projects-v1", &binding, s))
            .transpose()?;
        let rows=sqlx::query("SELECT *,lower(name) AS sort_name FROM projects WHERE workspace_id=$1 AND team_id=$2 AND (archived_at IS NOT NULL)=$3 AND strpos(lower(name),lower($4))>0 AND ($5::text IS NULL OR (lower(name),id)>($5,$6)) ORDER BY lower(name),id LIMIT $7").bind(w).bind(t).bind(q.archived.unwrap_or(false)).bind(term).bind(after.as_ref().map(|v|&v.0)).bind(after.as_ref().map(|v|v.1)).bind(limit as i64+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit;
        let next_cursor = if has_more {
            let r = &rows[limit - 1];
            Some(self.taxonomy_cursor_encode(
                "projects-v1",
                &binding,
                &(
                    r.try_get::<String, _>("sort_name").map_err(db_error)?,
                    r.try_get::<Uuid, _>("id").map_err(db_error)?,
                ),
            )?)
        } else {
            None
        };
        let mut data = Vec::new();
        for r in rows.iter().take(limit) {
            data.push((&project_row(&mut tx, w, t, r, edit, manage).await?).into());
        }
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
    pub async fn get_project(&self, a: &Actor, w: Uuid, t: Uuid, id: Uuid) -> Result<Project> {
        let mut tx = self.scoped(a, w, false).await?;
        let item = project(&mut tx, a, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn get_project_metadata(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<ProjectSummary> {
        Ok(metadata(&self.get_project(a, w, t, id).await?))
    }
    pub async fn project_progress(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<ProjectProgress> {
        let mut tx = self.scoped(a, w, false).await?;
        project(&mut tx, a, w, t, id).await?;
        let p = progress(&mut tx, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(p)
    }
    pub async fn create_project(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        key: &str,
        mut req: CreateProject,
    ) -> Result<Project> {
        req.name = bounded_text(&req.name, "Project name", 160)?;
        req.summary = project_summary(&req.summary)?;
        validate_project_dates(req.start_date, req.target_date)?;
        let narrative = req.narrative.clone().unwrap_or(DocumentWrite {
            schema_version: 1,
            body: json!({"type":"doc","content":[{"type":"paragraph"}]}),
            moderation_reason: None,
        });
        let plain = validate_project_narrative(&narrative)?;
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let route = format!("projects.create:{t}");
        let mut tx = self.scoped(a, w, false).await?;
        let replay = lifecycle::retry::<Project>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        scope(&mut tx, a, w, t, false).await?;
        if let Some(mut previous) = replay {
            let current = project(&mut tx, a, w, t, previous.id).await?;
            previous.can_edit = current.can_edit;
            previous.can_manage = current.can_manage;
            previous.lead = lead(&mut tx, w, t, previous.lead_id).await?;
            refresh_etags(&mut previous)?;
            tx.commit().await.map_err(db_error)?;
            return Ok(previous);
        }
        crate::issue_references::lock_new_targets(
            &mut tx,
            a,
            w,
            t,
            &narrative.body,
            &serde_json::Value::Null,
        )
        .await?;
        scope(&mut tx, a, w, t, true).await?;
        validate_lead(&mut tx, w, t, req.lead_id).await?;
        let id = Uuid::new_v4();
        graph::one(sqlx::query("INSERT INTO projects(id,workspace_id,team_id,name,summary,narrative_schema_version,narrative_body,narrative_plain_text,lead_id,start_date,target_date) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)").bind(id).bind(w).bind(t).bind(req.name).bind(req.summary).bind(narrative.schema_version).bind(&narrative.body).bind(&plain).bind(req.lead_id).bind(req.start_date).bind(req.target_date).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        documents::create_project_document(&mut tx, a, w, id, &narrative, &plain).await?;
        let item = project(&mut tx, a, w, t, id).await?;
        receipt(&mut tx, w, a, &route, &kh, &item).await?;
        emit(&mut tx, a, w, &item, "project.created").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn update_project(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        req: UpdateProject,
    ) -> Result<Project> {
        self.update_project_fields(a, w, t, id, ProjectMatch::Full(etag), req)
            .await
    }
    pub async fn update_project_metadata(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        req: UpdateProject,
    ) -> Result<ProjectSummary> {
        Ok(metadata(
            &self
                .update_project_fields(a, w, t, id, ProjectMatch::Metadata(etag), req)
                .await?,
        ))
    }
    async fn update_project_fields(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        validator: ProjectMatch<'_>,
        req: UpdateProject,
    ) -> Result<Project> {
        if req.narrative.is_some() && matches!(&validator, ProjectMatch::Metadata(_)) {
            return Err(Error::Invalid(
                "Save the project brief through its independent document resource".into(),
            ));
        }
        if let Some(narrative) = &req.narrative {
            validate_project_narrative(narrative)?;
        }
        let changed = req.changes_metadata();
        if !changed && req.archived.is_none() {
            return Err(Error::Invalid("Provide a project field to update".into()));
        }
        let mut tx = self.scoped(a, w, false).await?;
        let document_id = if let Some(narrative) = &req.narrative {
            scope(&mut tx, a, w, t, false).await?;
            let document: Uuid = sqlx::query_scalar("SELECT d.id FROM documents d JOIN projects p ON p.workspace_id=d.workspace_id AND p.id=d.project_id WHERE d.workspace_id=$1 AND d.project_id=$2 AND p.team_id=$3 AND d.deleted_at IS NULL")
                .bind(w).bind(id).bind(t).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
            crate::issue_references::lock_document_targets(
                &mut tx,
                a,
                w,
                document,
                &narrative.body,
            )
            .await?;
            Some(document)
        } else {
            None
        };
        let (_, manage) = scope(&mut tx, a, w, t, true).await?;
        lock(&mut tx, w, t, id).await?;
        let old = project(&mut tx, a, w, t, id).await?;
        match validator {
            ProjectMatch::Metadata(etag) => {
                let current = metadata(&old);
                lifecycle::version(etag, &current.etag, &current)?;
            }
            ProjectMatch::Full(etag) => lifecycle::version(etag, &old.etag, &old)?,
        }
        if req.archived.is_some() && !manage {
            return Err(Error::Forbidden);
        }
        if old.archived_at.is_some() && (req.archived != Some(false) || changed) {
            return Err(Error::Conflict(
                "Restore this project before editing it".into(),
            ));
        }
        let name = req
            .name
            .map(|v| bounded_text(&v, "Project name", 160))
            .transpose()?
            .unwrap_or(old.name.clone());
        let summary = req
            .summary
            .map(|v| project_summary(&v))
            .transpose()?
            .unwrap_or(old.summary.clone());
        let start = req.start_date.unwrap_or(old.start_date);
        let target = req.target_date.unwrap_or(old.target_date);
        validate_project_dates(start, target)?;
        let lead_id = req.lead_id.unwrap_or(old.lead_id);
        if req.lead_id.is_some() {
            validate_lead(&mut tx, w, t, lead_id).await?;
        }
        let next_state = req.state.unwrap_or(old.state);
        if next_state == ProjectState::Completed
            && old.state != ProjectState::Completed
            && progress(&mut tx, w, t, id).await?.open > 0
            && !req.acknowledge_open_issues
        {
            return Err(Error::Conflict("This project has open issues. Acknowledge them to complete the project without changing their states".into()));
        }
        let next_health = req.health.unwrap_or(old.health);
        let archived = req.archived.unwrap_or(old.archived_at.is_some());
        let mut events = Vec::new();
        if let (Some(document_id), Some(narrative)) = (document_id, &req.narrative)
            && (narrative.schema_version != old.narrative.schema_version
                || narrative.body != old.narrative.body)
        {
            let owner = documents::document_scope(&mut tx, a, w, document_id, true).await?;
            let expected = owner.document.etag.clone();
            let (_, prepared) =
                documents::write_locked(&mut tx, a, w, owner, &expected, narrative).await?;
            events = prepared;
        }
        graph::one(sqlx::query("UPDATE projects SET name=$4,summary=$5,lead_id=$6,start_date=$7,target_date=$8,state=$9,health=$10,archived_at=CASE WHEN $11 THEN coalesce(archived_at,clock_timestamp()) ELSE NULL END,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(id).bind(name).bind(summary).bind(lead_id).bind(start).bind(target).bind(next_state.as_str()).bind(next_health.as_str()).bind(archived).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = project(&mut tx, a, w, t, id).await?;
        let action = match (old.archived_at.is_some(), archived) {
            (false, true) => "project.archived",
            (true, false) => "project.restored",
            _ => "project.updated",
        };
        emit_after(&mut tx, a, w, &item, action, events).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn project_issues(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        q: ProjectPageQuery,
    ) -> Result<Page<ProjectIssue>> {
        let limit = page_limit(q.limit)?;
        let mut tx = self.scoped(a, w, false).await?;
        project(&mut tx, a, w, t, id).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!("{w}:{t}:{id}:{}:{epoch}:{limit}", a.user.id);
        let after: Option<Uuid> = q
            .cursor
            .as_deref()
            .map(|s| self.taxonomy_cursor_decode("project-issues-v1", &binding, s))
            .transpose()?;
        let rows=sqlx::query("SELECT i.id,pi.milestone_id,t.key||'-'||i.number AS key,i.title,i.status_id,s.name AS status_name,s.category AS status_category,i.priority,i.archived_at FROM project_issues pi JOIN issues i ON i.workspace_id=pi.workspace_id AND i.team_id=pi.team_id AND i.id=pi.issue_id JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE pi.workspace_id=$1 AND pi.team_id=$2 AND pi.project_id=$3 AND i.deleted_at IS NULL AND i.archived_at IS NULL AND ($4::uuid IS NULL OR i.id>$4) ORDER BY i.id LIMIT $5").bind(w).bind(t).bind(id).bind(after).bind(limit as i64+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit;
        let next_cursor = if has_more {
            Some(self.taxonomy_cursor_encode(
                "project-issues-v1",
                &binding,
                &rows[limit - 1].try_get::<Uuid, _>("id").map_err(db_error)?,
            )?)
        } else {
            None
        };
        let data = rows
            .iter()
            .take(limit)
            .map(|r| {
                Ok(ProjectIssue {
                    id: r.try_get("id").map_err(db_error)?,
                    milestone_id: r.try_get("milestone_id").map_err(db_error)?,
                    key: r.try_get("key").map_err(db_error)?,
                    title: r.try_get("title").map_err(db_error)?,
                    status_id: r.try_get("status_id").map_err(db_error)?,
                    status_name: r.try_get("status_name").map_err(db_error)?,
                    status_category: r.try_get("status_category").map_err(db_error)?,
                    priority: r.try_get("priority").map_err(db_error)?,
                    archived_at: r.try_get("archived_at").map_err(db_error)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn set_project_issue(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        issue: Uuid,
        etag: &str,
        req: ProjectMembership,
    ) -> Result<Project> {
        let mut tx = self.scoped(a, w, false).await?;
        scope(&mut tx, a, w, t, true).await?;
        lock(&mut tx, w, t, id).await?;
        let old = project(&mut tx, a, w, t, id).await?;
        version(etag, &old)?;
        if old.archived_at.is_some() {
            return Err(Error::Conflict(
                "Restore this project before changing its scope".into(),
            ));
        }
        // Look up within the selected team: foreign-team/private identifiers never
        // produce a membership/ownership disclosure in the validation response.
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM issues WHERE workspace_id=$1 AND team_id=$2 AND id=$3 AND deleted_at IS NULL AND (NOT $4 OR archived_at IS NULL))").bind(w).bind(t).bind(issue).bind(req.included).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !exists {
            return Err(Error::NotFound);
        }
        let linked: Option<Uuid> = sqlx::query_scalar(
            "SELECT project_id FROM project_issues WHERE workspace_id=$1 AND issue_id=$2",
        )
        .bind(w)
        .bind(issue)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        if req.included && linked.is_some_and(|v| v != id) {
            return Err(Error::Conflict(
                "This issue belongs to another project. Remove it there before adding it here"
                    .into(),
            ));
        }
        let changed = if req.included {
            linked.is_none()
        } else {
            linked == Some(id)
        };
        if changed {
            if req.included {
                graph::one(sqlx::query("INSERT INTO project_issues(workspace_id,team_id,project_id,issue_id) VALUES($1,$2,$3,$4)").bind(w).bind(t).bind(id).bind(issue).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            } else {
                graph::one(sqlx::query("DELETE FROM project_issues WHERE workspace_id=$1 AND project_id=$2 AND issue_id=$3").bind(w).bind(id).bind(issue).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            }
            graph::one(sqlx::query("UPDATE projects SET version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        let item = project(&mut tx, a, w, t, id).await?;
        if changed {
            emit(&mut tx, a, w, &item, "project.scope_changed").await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn project_updates(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        q: ProjectPageQuery,
    ) -> Result<Page<ProjectUpdate>> {
        let limit = page_limit(q.limit)?;
        if limit > 25 {
            return Err(Error::Invalid(
                "Project update pages are limited to 25 entries".into(),
            ));
        }
        let mut tx = self.scoped(a, w, false).await?;
        project(&mut tx, a, w, t, id).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!("{w}:{t}:{id}:{}:{epoch}:{limit}", a.user.id);
        let after: Option<(DateTime<Utc>, Uuid)> = q
            .cursor
            .as_deref()
            .map(|s| self.taxonomy_cursor_decode("project-updates-v1", &binding, s))
            .transpose()?;
        let rows=sqlx::query("SELECT p.*,u.display_name AS author_name FROM project_updates p JOIN users u ON u.id=p.author_id WHERE p.workspace_id=$1 AND p.team_id=$2 AND p.project_id=$3 AND ($4::timestamptz IS NULL OR (p.created_at,p.id)<($4,$5)) ORDER BY p.created_at DESC,p.id DESC LIMIT $6").bind(w).bind(t).bind(id).bind(after.as_ref().map(|v|v.0)).bind(after.as_ref().map(|v|v.1)).bind(limit as i64+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit;
        let next_cursor = if has_more {
            let r = &rows[limit - 1];
            Some(
                self.taxonomy_cursor_encode(
                    "project-updates-v1",
                    &binding,
                    &(
                        r.try_get::<DateTime<Utc>, _>("created_at")
                            .map_err(db_error)?,
                        r.try_get::<Uuid, _>("id").map_err(db_error)?,
                    ),
                )?,
            )
        } else {
            None
        };
        let data = rows
            .iter()
            .take(limit)
            .map(update_row)
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
    pub async fn get_project_update(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        update: Uuid,
    ) -> Result<ProjectUpdate> {
        let mut tx = self.scoped(a, w, false).await?;
        project(&mut tx, a, w, t, id).await?;
        let r=sqlx::query("SELECT p.*,u.display_name AS author_name FROM project_updates p JOIN users u ON u.id=p.author_id WHERE p.workspace_id=$1 AND p.team_id=$2 AND p.project_id=$3 AND p.id=$4").bind(w).bind(t).bind(id).bind(update).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        let out = update_row(&r)?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn create_project_update(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        key: &str,
        mut req: CreateProjectUpdate,
    ) -> Result<ProjectUpdate> {
        req.body = req.body.trim().to_owned();
        if req.body.is_empty()
            || req.body.chars().count() > 8000
            || req
                .body
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r'))
        {
            return Err(Error::Invalid(
                "Project updates require 1–8000 characters without unsupported control characters"
                    .into(),
            ));
        }
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let route = format!("projects.update.create:{id}");
        let mut tx = self.scoped(a, w, false).await?;
        let replay =
            lifecycle::retry::<ProjectUpdate>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        scope(&mut tx, a, w, t, true).await?;
        lock(&mut tx, w, t, id).await?;
        let item = project(&mut tx, a, w, t, id).await?;
        if let Some(previous) = replay {
            tx.commit().await.map_err(db_error)?;
            return Ok(previous);
        }
        if item.archived_at.is_some() {
            return Err(Error::Conflict(
                "Restore this project before publishing an update".into(),
            ));
        }
        let p = progress(&mut tx, w, t, id).await?;
        let update_id = Uuid::new_v4();
        let r=sqlx::query("INSERT INTO project_updates(id,workspace_id,team_id,project_id,author_id,body,state,health,completed,eligible) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING *, (SELECT display_name FROM users WHERE id=$5) AS author_name").bind(update_id).bind(w).bind(t).bind(id).bind(a.user.id).bind(req.body).bind(item.state.as_str()).bind(item.health.as_str()).bind(p.completed).bind(p.eligible).fetch_one(&mut *tx).await.map_err(db_error)?;
        let update = update_row(&r)?;
        receipt(&mut tx, w, a, &route, &kh, &update).await?;
        emit(&mut tx, a, w, &item, "project.update_published").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(update)
    }
}
fn update_row(r: &PgRow) -> Result<ProjectUpdate> {
    Ok(ProjectUpdate {
        id: r.try_get("id").map_err(db_error)?,
        author_id: r.try_get("author_id").map_err(db_error)?,
        author_name: r.try_get("author_name").map_err(db_error)?,
        body: r.try_get("body").map_err(db_error)?,
        state: state(r.try_get("state").map_err(db_error)?)?,
        health: health(r.try_get("health").map_err(db_error)?)?,
        completed: r.try_get("completed").map_err(db_error)?,
        eligible: r.try_get("eligible").map_err(db_error)?,
        created_at: r.try_get("created_at").map_err(db_error)?,
    })
}
