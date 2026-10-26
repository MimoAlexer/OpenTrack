use super::*;
use opentrack_domain::*;
use sqlx::{Row, postgres::PgRow};

async fn project_scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    p: Uuid,
    write: bool,
    manage: bool,
) -> Result<Project> {
    projects::scope(tx, a, w, t, write).await?;
    if write {
        projects::lock(tx, w, t, p).await?;
    }
    let project = projects::project(tx, a, w, t, p).await?;
    if write && !project.can_edit {
        return Err(Error::Conflict(
            "Restore the project before changing milestones".into(),
        ));
    }
    if manage && !can_manage(a, &project) {
        return Err(Error::Forbidden);
    }
    Ok(project)
}
fn can_manage(a: &Actor, p: &Project) -> bool {
    p.can_edit && (p.can_manage || p.lead_id == Some(a.user.id))
}
fn milestone(r: &PgRow, a: &Actor, p: &Project) -> Result<Milestone> {
    let mut m = Milestone {
        id: r.try_get("id").map_err(db_error)?,
        workspace_id: p.workspace_id,
        team_id: p.team_id,
        project_id: p.id,
        title: r.try_get("title").map_err(db_error)?,
        target_date: r.try_get("target_date").map_err(db_error)?,
        rank: r.try_get::<i64, _>("rank").map_err(db_error)?.to_string(),
        state: match r.try_get::<&str, _>("state").map_err(db_error)? {
            "planned" => MilestoneState::Planned,
            "completed" => MilestoneState::Completed,
            _ => return Err(Error::Internal),
        },
        completed_at: r.try_get("completed_at").map_err(db_error)?,
        version: r
            .try_get::<i64, _>("version")
            .map_err(db_error)?
            .to_string(),
        etag: String::new(),
        can_manage: can_manage(a, p),
        can_assign: p.can_edit,
        created_at: r.try_get("created_at").map_err(db_error)?,
        updated_at: r.try_get("updated_at").map_err(db_error)?,
    };
    m.etag = format!(
        "\"milestone:{}:{}:{}\"",
        m.id,
        m.version,
        digest(&serde_json::to_vec(&m).map_err(|_| Error::Internal)?)
    );
    Ok(m)
}
async fn get(tx: &mut store::Tx, a: &Actor, p: &Project, id: Uuid) -> Result<Milestone> {
    let r =
        sqlx::query("SELECT * FROM milestones WHERE workspace_id=$1 AND project_id=$2 AND id=$3")
            .bind(p.workspace_id)
            .bind(p.id)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
    milestone(&r, a, p)
}
async fn changed(
    tx: &mut store::Tx,
    a: &Actor,
    p: &Project,
    entity: Uuid,
    version: i64,
    action: &str,
) -> Result<Project> {
    graph::one(sqlx::query("UPDATE projects SET version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2")
        .bind(p.workspace_id).bind(p.id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    let out = projects::project(tx, a, p.workspace_id, p.team_id, p.id).await?;
    store::identity_event(
        tx,
        store::IdentityEvent {
            workspace: p.workspace_id,
            actor: a.user.id,
            entity,
            version,
            team: Some(p.team_id),
            action,
            access_changed: false,
        },
    )
    .await
    .map_err(db_error)?;
    Ok(out)
}
impl App {
    pub async fn list_milestones(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        q: ProjectPageQuery,
    ) -> Result<MilestonePage> {
        let limit = q.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid("Page size must be 1–100".into()));
        }
        let mut tx = self.scoped(a, w, false).await?;
        let project = project_scope(&mut tx, a, w, t, p, false, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{t}:{p}:{}:{epoch}:{}:{limit}",
            a.user.id, project.version
        );
        let after: Option<(i64, Uuid)> = q
            .cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("milestones-v1", &binding, c))
            .transpose()?;
        // Aggregate all milestones in the requested page together; no count query per row.
        let rows=sqlx::query("WITH page AS (SELECT * FROM milestones WHERE workspace_id=$1 AND project_id=$2 AND ($3::bigint IS NULL OR (rank,id)>($3,$4)) ORDER BY rank,id LIMIT $5), buckets AS (SELECT m.id,coalesce(s.category,'') AS category,(i.archived_at IS NOT NULL OR i.deleted_at IS NOT NULL) AS excluded,count(*) AS count FROM page m JOIN project_issues pi ON pi.workspace_id=m.workspace_id AND pi.project_id=m.project_id AND pi.milestone_id=m.id JOIN issues i ON i.workspace_id=pi.workspace_id AND i.id=pi.issue_id LEFT JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id GROUP BY m.id,s.category,excluded), counts AS (SELECT id,jsonb_agg(jsonb_build_array(category,excluded,count)) AS buckets FROM buckets GROUP BY id) SELECT m.*,coalesce(c.buckets,'[]'::jsonb) AS buckets,statement_timestamp() AS as_of FROM page m LEFT JOIN counts c ON c.id=m.id ORDER BY m.rank,m.id")
            .bind(w).bind(p).bind(after.as_ref().map(|v|v.0)).bind(after.as_ref().map(|v|v.1)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        let next_cursor = if has_more {
            let r = &rows[limit as usize - 1];
            Some(self.taxonomy_cursor_encode(
                "milestones-v1",
                &binding,
                &(
                    r.try_get::<i64, _>("rank").map_err(db_error)?,
                    r.try_get::<Uuid, _>("id").map_err(db_error)?,
                ),
            )?)
        } else {
            None
        };
        let data = rows
            .iter()
            .take(limit as usize)
            .map(|r| {
                Ok(MilestoneOverview {
                    milestone: milestone(r, a, &project)?,
                    progress: projects::progress_counts(r)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(MilestonePage {
            data,
            has_more,
            next_cursor,
            project_etag: project.etag,
        })
    }
    pub async fn get_milestone(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        id: Uuid,
    ) -> Result<Milestone> {
        let mut tx = self.scoped(a, w, false).await?;
        let project = project_scope(&mut tx, a, w, t, p, false, false).await?;
        let out = get(&mut tx, a, &project, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn milestone_progress(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        id: Uuid,
    ) -> Result<ProjectProgress> {
        let mut tx = self.scoped(a, w, false).await?;
        let project = project_scope(&mut tx, a, w, t, p, false, false).await?;
        get(&mut tx, a, &project, id).await?;
        let r=sqlx::query("WITH buckets AS (SELECT s.category,(i.deleted_at IS NOT NULL OR i.archived_at IS NOT NULL) AS excluded,count(*) AS count FROM project_issues pi JOIN issues i ON i.workspace_id=pi.workspace_id AND i.team_id=pi.team_id AND i.id=pi.issue_id JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id WHERE pi.workspace_id=$1 AND pi.project_id=$2 AND pi.milestone_id=$3 GROUP BY s.category,excluded) SELECT coalesce(jsonb_agg(jsonb_build_array(category,excluded,count)),'[]'::jsonb) AS buckets,statement_timestamp() AS as_of FROM buckets")
            .bind(w).bind(p).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
        let out = projects::progress_counts(&r)?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn create_milestone(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        key: &str,
        etag: &str,
        mut req: CreateMilestone,
    ) -> Result<Milestone> {
        req.title = bounded_text(&req.title, "Milestone title", 160)?;
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let route = format!("milestones.create:{p}");
        let mut tx = self.scoped(a, w, false).await?;
        let replay = lifecycle::retry::<Milestone>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        let project = project_scope(&mut tx, a, w, t, p, true, true).await?;
        if let Some(old) = replay {
            get(&mut tx, a, &project, old.id).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(old);
        }
        projects::version(etag, &project)?;
        let (count,rank):(i64,i64)=sqlx::query_as("SELECT count(*),coalesce(max(rank),0)+1 FROM milestones WHERE workspace_id=$1 AND project_id=$2").bind(w).bind(p).fetch_one(&mut *tx).await.map_err(db_error)?;
        if count >= 500 {
            return Err(Error::Conflict(
                "A project supports at most 500 milestones".into(),
            ));
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO milestones(id,workspace_id,team_id,project_id,title,target_date,rank) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(w).bind(t).bind(p).bind(req.title).bind(req.target_date).bind(rank).execute(&mut *tx).await.map_err(db_error)?;
        let out = get(&mut tx, a, &project, id).await?;
        lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &out).await?;
        changed(&mut tx, a, &project, id, 1, "milestone.created").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn update_milestone(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        id: Uuid,
        etag: &str,
        req: UpdateMilestone,
    ) -> Result<Milestone> {
        if req.title.is_none() && req.target_date.is_none() && req.state.is_none() {
            return Err(Error::Invalid("Provide a milestone field to update".into()));
        }
        let mut tx = self.scoped(a, w, false).await?;
        let project = project_scope(&mut tx, a, w, t, p, true, true).await?;
        let old = get(&mut tx, a, &project, id).await?;
        lifecycle::version(etag, &old.etag, &old)?;
        let title = req
            .title
            .map(|v| bounded_text(&v, "Milestone title", 160))
            .transpose()?
            .unwrap_or(old.title);
        let state = req.state.unwrap_or(old.state);
        sqlx::query("UPDATE milestones SET title=$4,target_date=$5,state=$6,completed_at=CASE WHEN $6='completed' THEN coalesce(completed_at,clock_timestamp()) ELSE NULL END,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND project_id=$2 AND id=$3")
            .bind(w).bind(p).bind(id).bind(title).bind(req.target_date.unwrap_or(old.target_date)).bind(state.as_str()).execute(&mut *tx).await.map_err(db_error)?;
        let out = get(&mut tx, a, &project, id).await?;
        changed(
            &mut tx,
            a,
            &project,
            id,
            out.version.parse().map_err(|_| Error::Internal)?,
            "milestone.updated",
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_milestone(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        id: Uuid,
        etag: &str,
    ) -> Result<()> {
        let mut tx = self.scoped(a, w, false).await?;
        let project = project_scope(&mut tx, a, w, t, p, true, true).await?;
        let old = get(&mut tx, a, &project, id).await?;
        lifecycle::version(etag, &old.etag, &old)?;
        // The scoped FK clears only milestone_id. Project membership and issue rows survive.
        graph::one(
            sqlx::query("DELETE FROM milestones WHERE workspace_id=$1 AND project_id=$2 AND id=$3")
                .bind(w)
                .bind(p)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?
                .rows_affected(),
        )?;
        changed(
            &mut tx,
            a,
            &project,
            id,
            old.version.parse::<i64>().map_err(|_| Error::Internal)? + 1,
            "milestone.deleted",
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn order_milestones(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        etag: &str,
        req: MilestoneOrder,
    ) -> Result<Project> {
        if req.milestone_ids.len() > 500 {
            return Err(Error::Invalid(
                "An order contains at most 500 milestones".into(),
            ));
        }
        let mut tx = self.scoped(a, w, false).await?;
        let project = project_scope(&mut tx, a, w, t, p, true, true).await?;
        projects::version(etag, &project)?;
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM milestones WHERE workspace_id=$1 AND project_id=$2 ORDER BY rank,id",
        )
        .bind(w)
        .bind(p)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_error)?;
        let mut expected = ids.clone();
        expected.sort_unstable();
        let mut actual = req.milestone_ids.clone();
        actual.sort_unstable();
        if expected != actual {
            return Err(Error::Invalid(
                "Provide every milestone in this project exactly once".into(),
            ));
        }
        let out = if ids != req.milestone_ids {
            sqlx::query("UPDATE milestones m SET rank=o.rank,version=m.version+1,updated_at=clock_timestamp() FROM unnest($3::uuid[]) WITH ORDINALITY AS o(id,rank) WHERE m.workspace_id=$1 AND m.project_id=$2 AND m.id=o.id AND m.rank<>o.rank").bind(w).bind(p).bind(&req.milestone_ids).execute(&mut *tx).await.map_err(db_error)?;
            changed(
                &mut tx,
                a,
                &project,
                p,
                project
                    .version
                    .parse::<i64>()
                    .map_err(|_| Error::Internal)?
                    + 1,
                "milestone.order_changed",
            )
            .await?
        } else {
            project
        };
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn assign_milestone(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        p: Uuid,
        issue: Uuid,
        etag: &str,
        req: MilestoneAssignment,
    ) -> Result<Project> {
        let mut tx = self.scoped(a, w, false).await?;
        let project = project_scope(&mut tx, a, w, t, p, true, false).await?;
        projects::version(etag, &project)?;
        if let Some(id) = req.milestone_id {
            get(&mut tx, a, &project, id).await.map_err(|e| {
                if matches!(e, Error::NotFound) {
                    Error::Invalid("Choose a milestone from this project".into())
                } else {
                    e
                }
            })?;
        }
        let current:Option<Uuid>=sqlx::query_scalar("SELECT pi.milestone_id FROM project_issues pi JOIN issues i ON i.workspace_id=pi.workspace_id AND i.team_id=pi.team_id AND i.id=pi.issue_id WHERE pi.workspace_id=$1 AND pi.team_id=$2 AND pi.project_id=$3 AND pi.issue_id=$4 AND i.deleted_at IS NULL AND ($5 OR i.archived_at IS NULL)")
            .bind(w).bind(t).bind(p).bind(issue).bind(req.milestone_id.is_none()).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        let out = if current != req.milestone_id {
            sqlx::query("UPDATE project_issues SET milestone_id=$4 WHERE workspace_id=$1 AND project_id=$2 AND issue_id=$3").bind(w).bind(p).bind(issue).bind(req.milestone_id).execute(&mut *tx).await.map_err(db_error)?;
            changed(
                &mut tx,
                a,
                &project,
                p,
                project
                    .version
                    .parse::<i64>()
                    .map_err(|_| Error::Internal)?
                    + 1,
                "milestone.assignment_changed",
            )
            .await?
        } else {
            project
        };
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
}
