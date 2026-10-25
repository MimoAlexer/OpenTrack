use crate::{
    App, activity, db_error, digest, graph, lifecycle, tracking,
    worker::{BatchOutcome, JobClaim},
};
use opentrack_domain::{
    Actor, Error, IssueFilter, IssueOrderMaintenance, IssueOrderPhase, IssuePage, IssuePosition,
    PositionIssue, Result,
};
use opentrack_store as store;
use sqlx::Row;
use uuid::Uuid;

pub(crate) enum RankAllocation {
    Ready(String),
    Maintenance(IssueOrderMaintenance),
}

#[derive(sqlx::FromRow)]
struct RankedIssue {
    id: Uuid,
    rank: String,
}

async fn ranked(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    status: Uuid,
    id: Uuid,
) -> Result<RankedIssue> {
    sqlx::query_as("SELECT i.id,i.rank::text AS rank FROM issues i WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.status_id=$3 AND i.id=$4 AND i.archived_at IS NULL AND i.deleted_at IS NULL")
        .bind(w).bind(t).bind(status).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?
        .ok_or_else(||Error::Conflict("An issue ordering neighbor is unavailable; refresh the order".into()))
}

async fn adjacent(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    status: Uuid,
    source: Uuid,
    bound: Option<&RankedIssue>,
    previous: bool,
) -> Result<Option<RankedIssue>> {
    let query = if previous {
        "SELECT i.id,i.rank::text AS rank FROM issues i WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.status_id=$3 AND i.id<>$4 AND i.archived_at IS NULL AND i.deleted_at IS NULL AND (i.rank,i.id)<(coalesce($5::numeric,100000000000000000000::numeric),coalesce($6::uuid,'00000000-0000-0000-0000-000000000000'::uuid)) ORDER BY i.rank DESC,i.id DESC LIMIT 1"
    } else {
        "SELECT i.id,i.rank::text AS rank FROM issues i WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.status_id=$3 AND i.id<>$4 AND i.archived_at IS NULL AND i.deleted_at IS NULL AND (i.rank,i.id)>(coalesce($5::numeric,-100000000000000000000::numeric),coalesce($6::uuid,'00000000-0000-0000-0000-000000000000'::uuid)) ORDER BY i.rank,i.id LIMIT 1"
    };
    sqlx::query_as(query)
        .bind(w)
        .bind(t)
        .bind(status)
        .bind(source)
        .bind(bound.map(|row| row.rank.as_str()))
        .bind(bound.map(|row| row.id))
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)
}

async fn neighbors(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    status: Uuid,
    source: Uuid,
    request: &PositionIssue,
) -> Result<(Option<RankedIssue>, Option<RankedIssue>)> {
    if let Some(id) = request.before_id {
        let before = ranked(tx, w, t, status, id).await?;
        let after = adjacent(tx, w, t, status, source, Some(&before), true).await?;
        if request.after_id.is_some() && after.as_ref().map(|row| row.id) != request.after_id {
            return Err(Error::Conflict(
                "The ordering neighbors are no longer adjacent; refresh the order".into(),
            ));
        }
        Ok((after, Some(before)))
    } else if let Some(id) = request.after_id {
        let after = ranked(tx, w, t, status, id).await?;
        let before = adjacent(tx, w, t, status, source, Some(&after), false).await?;
        Ok((Some(after), before))
    } else {
        Ok((adjacent(tx, w, t, status, source, None, true).await?, None))
    }
}

async fn between(
    tx: &mut store::Tx,
    after: Option<&RankedIssue>,
    before: Option<&RankedIssue>,
) -> Result<Option<String>> {
    sqlx::query_scalar("WITH candidate AS(SELECT CASE WHEN $1::numeric IS NULL AND $2::numeric IS NULL THEN 1024 WHEN $1::numeric IS NULL THEN $2::numeric-1024 WHEN $2::numeric IS NULL THEN $1::numeric+1024 ELSE ($1::numeric+$2::numeric)/2 END AS value),rounded AS(SELECT CASE WHEN value>-100000000000000000000::numeric AND value<100000000000000000000::numeric THEN value::numeric(38,18) END AS rank FROM candidate) SELECT CASE WHEN rank IS NOT NULL AND ($1::numeric IS NULL OR rank>$1::numeric) AND ($2::numeric IS NULL OR rank<$2::numeric) THEN rank::text END FROM rounded")
        .bind(after.map(|row|row.rank.as_str())).bind(before.map(|row|row.rank.as_str())).fetch_one(&mut **tx).await.map_err(db_error)
}

async fn order_version(tx: &mut store::Tx, w: Uuid, t: Uuid) -> Result<String> {
    let (order, workflow): (i64, i64) = sqlx::query_as(
        "SELECT ordering_version,workflow_version FROM teams WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(t)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(format!("{order}:{workflow}"))
}

async fn maintenance(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    id: Uuid,
) -> Result<IssueOrderMaintenance> {
    let team = store::team_detail(tx, w, t, a.user.id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    let row = sqlx::query("SELECT n.status_id,n.phase,n.staged_count::text,n.applied_count::text,n.total_count::text,j.state,j.version::text,j.error_code,g.epoch,(workspace.archived_at IS NULL AND team.archived_at IS NULL AND status.archived_at IS NULL AND status.retiring_at IS NULL AND EXISTS(SELECT 1 FROM issue_order_guards guard WHERE guard.workspace_id=n.workspace_id AND guard.team_id=n.team_id AND guard.status_id=n.status_id AND guard.job_id=n.id) AND NOT EXISTS(SELECT 1 FROM workflow_statuses retiring WHERE retiring.workspace_id=n.workspace_id AND retiring.team_id=n.team_id AND retiring.retiring_at IS NOT NULL AND (retiring.id=n.status_id OR retiring.replacement_status_id=n.status_id))) AS resumable_scope FROM issue_order_normalizations n JOIN jobs j USING(workspace_id,id) JOIN workspaces workspace ON workspace.id=n.workspace_id JOIN teams team ON team.workspace_id=n.workspace_id AND team.id=n.team_id JOIN workflow_statuses status ON status.workspace_id=n.workspace_id AND status.team_id=n.team_id AND status.id=n.status_id JOIN workspace_auth_guards g ON g.workspace_id=n.workspace_id WHERE n.workspace_id=$1 AND n.team_id=$2 AND n.id=$3 AND j.kind='issues.order_normalize'")
        .bind(w).bind(t).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    let state: String = row.try_get("state").map_err(db_error)?;
    let version: String = row.try_get("version").map_err(db_error)?;
    let phase = match row.try_get::<&str, _>("phase").map_err(db_error)? {
        "staging" => IssueOrderPhase::Staging,
        "applying" => IssueOrderPhase::Applying,
        "completed" => IssueOrderPhase::Completed,
        _ => return Err(Error::Internal),
    };
    let can_resume = team.can_manage
        && row
            .try_get::<bool, _>("resumable_scope")
            .map_err(db_error)?
        && ["blocked", "dead_letter", "retry_wait"].contains(&state.as_str())
        && !matches!(phase, IssueOrderPhase::Completed);
    let epoch: i64 = row.try_get("epoch").map_err(db_error)?;
    let binding = digest(&serde_json::to_vec(&(w, a.user.id, epoch)).map_err(|_| Error::Internal)?);
    Ok(IssueOrderMaintenance {
        id,
        team_id: t,
        status_id: row.try_get("status_id").map_err(db_error)?,
        state,
        phase,
        staged_count: row.try_get("staged_count").map_err(db_error)?,
        applied_count: row.try_get("applied_count").map_err(db_error)?,
        total_count: row.try_get("total_count").map_err(db_error)?,
        error_code: row.try_get("error_code").map_err(db_error)?,
        etag: IssueOrderMaintenance::etag(id, &version, can_resume, &binding),
        version,
        can_resume,
    })
}

pub(crate) async fn active(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    status: Option<Uuid>,
) -> Result<Option<IssueOrderMaintenance>> {
    let id: Option<Uuid> = sqlx::query_scalar("SELECT job_id FROM issue_order_guards WHERE workspace_id=$1 AND team_id=$2 AND ($3::uuid IS NULL OR status_id=$3) ORDER BY status_id LIMIT 1")
        .bind(w).bind(t).bind(status).fetch_optional(&mut **tx).await.map_err(db_error)?;
    match id {
        Some(id) => maintenance(tx, a, w, t, id).await.map(Some),
        None => Ok(None),
    }
}

pub(crate) async fn require_groups(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    groups: &[(Uuid, Uuid)],
) -> Result<()> {
    let mut progress = Vec::new();
    for &(team, status) in groups {
        if let Some(current) = active(tx, a, w, team, Some(status)).await?
            && !progress
                .iter()
                .any(|item: &IssueOrderMaintenance| item.id == current.id)
        {
            progress.push(current);
        }
    }
    if progress.is_empty() {
        Ok(())
    } else {
        Err(Error::IssueOrderMaintenance(progress))
    }
}

pub(crate) async fn append(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    status: Uuid,
) -> Result<RankAllocation> {
    if let Some(progress) = active(tx, a, w, t, Some(status)).await? {
        return Ok(RankAllocation::Maintenance(progress));
    }
    if let Some(rank) = store::append_issue_rank(tx, w, t, status)
        .await
        .map_err(db_error)?
    {
        return Ok(RankAllocation::Ready(rank));
    }
    let inline: bool = sqlx::query_scalar("SELECT issue_order_rebalance_inline($1,$2,$3)")
        .bind(w)
        .bind(t)
        .bind(status)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    if inline {
        return Ok(RankAllocation::Ready(
            store::append_issue_rank(tx, w, t, status)
                .await
                .map_err(db_error)?
                .ok_or(Error::Internal)?,
        ));
    }
    enqueue(tx, a, w, t, status)
        .await
        .map(RankAllocation::Maintenance)
}

async fn enqueue(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    status: Uuid,
) -> Result<IssueOrderMaintenance> {
    let generation: i64 =
        sqlx::query_scalar("SELECT ordering_version FROM teams WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(t)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    let id = Uuid::new_v4();
    graph::one(sqlx::query("INSERT INTO jobs(workspace_id,id,kind,requested_by,dedupe_key) VALUES($1,$2,'issues.order_normalize',$3,$4)").bind(w).bind(id).bind(a.user.id).bind(format!("{t}:{status}:{generation}")).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    graph::one(sqlx::query("INSERT INTO issue_order_normalizations(workspace_id,id,team_id,status_id,original_requested_by) VALUES($1,$2,$3,$4,$5)").bind(w).bind(id).bind(t).bind(status).bind(a.user.id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    graph::one(sqlx::query("INSERT INTO issue_order_guards(workspace_id,team_id,status_id,job_id) VALUES($1,$2,$3,$4)").bind(w).bind(t).bind(status).bind(id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    sqlx::query(
        "INSERT INTO worker_workspace_registry(workspace_id) VALUES($1) ON CONFLICT DO NOTHING",
    )
    .bind(w)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    let registered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM worker_workspace_registry WHERE workspace_id=$1)",
    )
    .bind(w)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if !registered {
        return Err(Error::Internal);
    }
    store::identity_audit(
        tx,
        w,
        a.user.id,
        id,
        "issue.order_maintenance_requested",
        id,
    )
    .await
    .map_err(db_error)?;
    maintenance(tx, a, w, t, id).await
}

impl App {
    pub(crate) async fn manual_issues(
        &self,
        mut tx: store::Tx,
        a: &Actor,
        w: Uuid,
        mut filter: IssueFilter,
        epoch: i64,
    ) -> Result<IssuePage> {
        let t = filter
            .team_id
            .ok_or_else(|| Error::Invalid("Manual order requires an explicit team".into()))?;
        if !store::visible_team(&mut tx, w, t, a.user.id, false)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let active_team: bool = sqlx::query_scalar(
            "SELECT archived_at IS NULL FROM teams WHERE workspace_id=$1 AND id=$2 FOR SHARE",
        )
        .bind(w)
        .bind(t)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if !store::visible_team(&mut tx, w, t, a.user.id, false)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        if !active_team {
            return Err(Error::Conflict(
                "Restore this team before using manual order".into(),
            ));
        }
        if let Some(progress) = active(&mut tx, a, w, t, None).await? {
            return Err(Error::IssueOrderMaintenance(vec![progress]));
        }
        let version = order_version(&mut tx, w, t).await?;
        filter.q = filter
            .q
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_owned);
        filter.limit = Some(filter.limit.unwrap_or(50));
        filter.label_mode = Some(filter.label_mode.unwrap_or_else(|| "any".into()));
        let mut fingerprint = filter.clone();
        fingerprint.cursor = None;
        let binding = format!(
            "{w}:{}:{epoch}:{version}:{}",
            a.user.id,
            serde_json::to_string(&fingerprint).map_err(|_| Error::Internal)?
        );
        let after: Option<(i32, i32, Uuid, String, Uuid)> = filter
            .cursor
            .as_deref()
            .map(|cursor| self.taxonomy_cursor_decode("issues-manual-v1", &binding, cursor))
            .transpose()?;
        let mut data = store::list_issues(
            &mut tx,
            w,
            a.user.id,
            &filter,
            after.map(|(c, p, s, r, id)| store::IssueListCursor::Manual(c, p, s, r, id)),
        )
        .await
        .map_err(db_error)?;
        let limit = filter.limit.unwrap_or(50) as usize;
        let has_more = data.len() > limit;
        data.truncate(limit);
        let next_cursor = if has_more {
            let last = data.last().ok_or(Error::Internal)?;
            let key: (i32,i32,Uuid,String,Uuid)=sqlx::query_as("SELECT CASE s.category WHEN 'triage' THEN 0 WHEN 'backlog' THEN 1 WHEN 'unstarted' THEN 2 WHEN 'started' THEN 3 WHEN 'completed' THEN 4 ELSE 5 END,s.position,s.id,i.rank::text,i.id FROM issues i JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.id=$3")
                .bind(w).bind(t).bind(last.id).fetch_one(&mut *tx).await.map_err(db_error)?;
            Some(self.taxonomy_cursor_encode("issues-manual-v1", &binding, &key)?)
        } else {
            None
        };
        tx.commit().await.map_err(db_error)?;
        Ok(IssuePage {
            data,
            next_cursor,
            has_more,
            order_version: Some(version),
            order_maintenance: Vec::new(),
        })
    }

    pub async fn position_issue(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        key: &str,
        etag: &str,
        request: PositionIssue,
    ) -> Result<IssuePosition> {
        request.validate(id)?;
        let (kh, rh) = lifecycle::request_hash(key, &(id, &request, etag))?;
        let route = format!("issues.position:{id}");
        let mut tx = self.scoped(a, w, true).await?;
        if let Some(saved) =
            lifecycle::retry::<IssuePosition>(&mut tx, w, a.user.id, &route, &kh, &rh).await?
        {
            graph::receipt_authority(&mut tx, a, w, &saved.issue).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(saved);
        }
        let scope = graph::current(&mut tx, a, w, id).await?;
        let t = scope.team_id;
        if !store::visible_team(&mut tx, w, t, a.user.id, true)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let old = store::issue(&mut tx, w, a.user.id, id, true)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        graph::source_version(&old, etag)?;
        if old.team_id != t {
            return Err(Error::Conflict(
                "This issue moved teams; refresh before positioning".into(),
            ));
        }
        lifecycle::require_active_team(&mut tx, w, t).await?;
        if old.archived_at.is_some() {
            return Err(Error::Conflict(
                "Restore this issue before positioning it".into(),
            ));
        }
        let status = request.status_id.unwrap_or(old.status_id);
        let target = store::workflow_status(&mut tx, w, t, status)
            .await
            .map_err(db_error)?
            .ok_or_else(|| Error::Invalid("Status is unavailable for this team".into()))?;
        if target.archived_at.is_some() || target.retiring_at.is_some() {
            return Err(Error::Invalid("Status is unavailable for this team".into()));
        }
        tracking::status_transition(&mut tx, w, &old, status, old.assignee_id, false).await?;
        let mut maintenance = Vec::new();
        for group in [old.status_id, status] {
            if let Some(progress) = active(&mut tx, a, w, t, Some(group)).await?
                && !maintenance
                    .iter()
                    .any(|p: &IssueOrderMaintenance| p.id == progress.id)
            {
                maintenance.push(progress);
            }
        }
        if !maintenance.is_empty() {
            tx.commit().await.map_err(db_error)?;
            return Err(Error::IssueOrderMaintenance(maintenance));
        }
        let version = order_version(&mut tx, w, t).await?;
        if request.order_version != version {
            return Err(Error::Conflict(
                "Issue order changed; refresh before positioning".into(),
            ));
        }
        let (mut after, mut before) = neighbors(&mut tx, w, t, status, id, &request).await?;
        let current = ranked(&mut tx, w, t, old.status_id, id).await?;
        let previous_after = adjacent(&mut tx, w, t, old.status_id, id, Some(&current), true)
            .await?
            .map(|row| row.id);
        let previous_before = adjacent(&mut tx, w, t, old.status_id, id, Some(&current), false)
            .await?
            .map(|row| row.id);
        if status == old.status_id
            && previous_after == after.as_ref().map(|row| row.id)
            && previous_before == before.as_ref().map(|row| row.id)
        {
            let result = IssuePosition {
                issue: old,
                order_version: version,
            };
            graph::save(&mut tx, w, a.user.id, &route, &kh, &result).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(result);
        }
        let rank = match between(&mut tx, after.as_ref(), before.as_ref()).await? {
            Some(rank) => rank,
            None => {
                let inline: bool =
                    sqlx::query_scalar("SELECT issue_order_rebalance_inline($1,$2,$3)")
                        .bind(w)
                        .bind(t)
                        .bind(status)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(db_error)?;
                if !inline {
                    let progress = enqueue(&mut tx, a, w, t, status).await?;
                    tx.commit().await.map_err(db_error)?;
                    return Err(Error::IssueOrderMaintenance(vec![progress]));
                }
                (after, before) = neighbors(&mut tx, w, t, status, id, &request).await?;
                between(&mut tx, after.as_ref(), before.as_ref())
                    .await?
                    .ok_or(Error::Internal)?
            }
        };
        graph::one(sqlx::query("UPDATE issues SET rank=$4::numeric,status_id=$5,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(id).bind(rank).bind(status).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let issue = graph::current(&mut tx, a, w, id).await?;
        let mut changes = activity::issue_changes(&mut tx, &old, &issue).await?;
        changes
            .get_mut("fields")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or(Error::Internal)?
            .push(serde_json::json!("position"));
        changes["position"] = serde_json::json!({"before":{"status_id":old.status_id,"after_id":previous_after,"before_id":previous_before},"after":{"status_id":status,"after_id":after.as_ref().map(|row|row.id),"before_id":before.as_ref().map(|row|row.id)}});
        changes["source"] = serde_json::json!("manual_order");
        let result = IssuePosition {
            issue,
            order_version: order_version(&mut tx, w, t).await?,
        };
        graph::save(&mut tx, w, a.user.id, &route, &kh, &result).await?;
        graph::effects(
            &mut tx,
            &[store::Event {
                workspace: w,
                team: t,
                entity: id,
                version: result.issue.version.parse().map_err(|_| Error::Internal)?,
                actor: a.user.id,
                kind: "issue.updated",
                mutation: Uuid::new_v4(),
                changes,
            }],
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }

    pub async fn issue_order_maintenance(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<IssueOrderMaintenance> {
        let mut tx = self.scoped(a, w, false).await?;
        let result = maintenance(&mut tx, a, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }

    pub async fn resume_issue_order_maintenance(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
    ) -> Result<IssueOrderMaintenance> {
        let mut tx = self.scoped(a, w, true).await?;
        maintenance(&mut tx, a, w, t, id).await?;
        sqlx::query("SELECT id FROM jobs WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if !store::visible_team(&mut tx, w, t, a.user.id, true)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let team = store::team_detail(&mut tx, w, t, a.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !team.can_manage {
            return Err(Error::Forbidden);
        }
        let current = maintenance(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &current.etag, &current)?;
        if !current.can_resume {
            return Err(Error::Conflict(
                "This maintenance operation cannot currently be resumed".into(),
            ));
        }
        graph::one(sqlx::query("UPDATE jobs SET requested_by=$3,state='queued',retry_count=0,error_code=NULL,available_at=clock_timestamp(),finished_at=NULL,updated_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).bind(a.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        store::identity_audit(
            &mut tx,
            w,
            a.user.id,
            id,
            "issue.order_maintenance_resumed",
            Uuid::new_v4(),
        )
        .await
        .map_err(db_error)?;
        let result = maintenance(&mut tx, a, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}

pub(crate) async fn run_batch(tx: &mut store::Tx, claim: &JobClaim) -> Result<BatchOutcome> {
    let outcome: String = sqlx::query_scalar("SELECT worker_issue_order_batch($1,$2,$3,$4,$5)")
        .bind(claim.workspace_id)
        .bind(claim.id)
        .bind(claim.requested_by.ok_or(Error::Internal)?)
        .bind(claim.lease_owner)
        .bind(claim.lease_token)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(match outcome.as_str() {
        "completed" => BatchOutcome::Complete,
        "more" => BatchOutcome::More,
        "access_lost" => BatchOutcome::Blocked("access_lost"),
        "parent_archived" => BatchOutcome::Blocked("parent_archived"),
        "target_unavailable" => BatchOutcome::Blocked("target_unavailable"),
        _ => return Err(Error::Internal),
    })
}
