use super::*;
use chrono::{DateTime, NaiveDate, Utc};
use opentrack_domain::*;
use serde_json::json;
use sqlx::{Row, postgres::PgRow};

async fn scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    write: bool,
    manage: bool,
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
    let can_plan = active && writable(&workspace.role);
    let can_manage = active && team.can_manage;
    if write && !active {
        return Err(Error::Conflict(
            "Restore the workspace and team before changing cycles".into(),
        ));
    }
    if write && (!can_plan || manage && !can_manage) {
        return Err(Error::Forbidden);
    }
    Ok((can_plan, can_manage))
}
fn cycle_error(e: sqlx::Error) -> Error {
    if e.as_database_error()
        .is_some_and(|d| d.code().as_deref() == Some("23P01"))
    {
        Error::Conflict("Cycle dates overlap another cycle in this team".into())
    } else {
        db_error(e)
    }
}
async fn cycle(tx: &mut store::Tx, a: &Actor, w: Uuid, t: Uuid, id: Uuid) -> Result<Cycle> {
    let (can_plan, can_manage) = scope(tx, a, w, t, false, false).await?;
    let r=sqlx::query("SELECT *,(SELECT r.id FROM cycle_close_reports r WHERE r.workspace_id=cycles.workspace_id AND r.cycle_id=cycles.id ORDER BY r.closed_at DESC,r.id DESC LIMIT 1) AS latest_report_id,start_date::timestamp AT TIME ZONE timezone AS starts_at,end_date_exclusive::timestamp AT TIME ZONE timezone AS ends_at FROM cycles WHERE workspace_id=$1 AND team_id=$2 AND deleted_at IS NULL AND id=$3").bind(w).bind(t).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    cycle_row(w, t, r, can_plan, can_manage)
}
fn cycle_row(w: Uuid, t: Uuid, r: PgRow, can_plan: bool, can_manage: bool) -> Result<Cycle> {
    let mut c = Cycle {
        id: r.try_get("id").map_err(db_error)?,
        workspace_id: w,
        team_id: t,
        name: r.try_get("name").map_err(db_error)?,
        start_date: r.try_get("start_date").map_err(db_error)?,
        end_date_exclusive: r.try_get("end_date_exclusive").map_err(db_error)?,
        timezone: r.try_get("timezone").map_err(db_error)?,
        starts_at: r.try_get("starts_at").map_err(db_error)?,
        ends_at: r.try_get("ends_at").map_err(db_error)?,
        state: match r.try_get::<&str, _>("state").map_err(db_error)? {
            "planned" => CycleState::Planned,
            "active" => CycleState::Active,
            "closed" => CycleState::Closed,
            _ => return Err(Error::Internal),
        },
        version: r
            .try_get::<i64, _>("version")
            .map_err(db_error)?
            .to_string(),
        etag: String::new(),
        closed_at: r.try_get("closed_at").map_err(db_error)?,
        latest_report_id: r.try_get("latest_report_id").map_err(db_error)?,
        created_at: r.try_get("created_at").map_err(db_error)?,
        updated_at: r.try_get("updated_at").map_err(db_error)?,
        can_plan: can_plan && r.try_get::<&str, _>("state").map_err(db_error)? != "closed",
        can_manage,
    };
    c.etag = format!(
        "\"cycle:{}:{}:{}\"",
        c.id,
        c.version,
        digest(&serde_json::to_vec(&c).map_err(|_| Error::Internal)?)
    );
    Ok(c)
}
async fn lock(tx: &mut store::Tx, w: Uuid, t: Uuid, ids: &[Uuid]) -> Result<()> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        sqlx::query(
            "SELECT id FROM cycles WHERE workspace_id=$1 AND team_id=$2 AND deleted_at IS NULL AND id=$3 FOR UPDATE",
        )
        .bind(w)
        .bind(t)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    }
    Ok(())
}
async fn bump(tx: &mut store::Tx, w: Uuid, id: Uuid) -> Result<()> {
    graph::one(sqlx::query("UPDATE cycles SET version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}
async fn emit(tx: &mut store::Tx, a: &Actor, c: &Cycle, action: &str) -> Result<()> {
    store::identity_event(
        tx,
        store::IdentityEvent {
            workspace: c.workspace_id,
            actor: a.user.id,
            entity: c.id,
            version: c.version.parse().map_err(|_| Error::Internal)?,
            team: Some(c.team_id),
            action,
            access_changed: false,
        },
    )
    .await
    .map_err(db_error)
}
async fn current_scope(tx: &mut store::Tx, c: &Cycle, kind: &str) -> Result<CycleSnapshot> {
    let (mut rows, clock) = if kind == "current" {
        let (rows, now, date) =
            store::current_cycle_planning_rows(tx, c.workspace_id, c.team_id, c.id, &c.timezone)
                .await
                .map_err(db_error)?;
        (rows, Some((now, date)))
    } else {
        (sqlx::query("SELECT i.id,i.version,i.estimate,s.category,(i.deleted_at IS NOT NULL OR i.archived_at IS NOT NULL) AS excluded FROM issues i JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.id=ANY(ARRAY(SELECT issue_id FROM cycle_issues WHERE workspace_id=$1 AND team_id=$2 AND cycle_id=$3 ORDER BY issue_id LIMIT 10001)) ORDER BY i.id").bind(c.workspace_id).bind(c.team_id).bind(c.id).fetch_all(&mut **tx).await.map_err(db_error)?,None)
    };
    if rows.len() > 10000 {
        return Err(Error::Conflict(
            "A cycle scope is limited to 10,000 issues".into(),
        ));
    }
    let mut redacted = false;
    if kind == "current" {
        let mut visible = Vec::with_capacity(rows.len());
        for row in rows {
            if row.try_get::<bool, _>("deleted").map_err(db_error)? {
                redacted = true;
            } else {
                visible.push(row);
            }
        }
        rows = visible;
    }
    let ready = kind == "current"
        && !rows.is_empty()
        && rows.iter().try_fold(true, |ready, row| {
            Ok::<_, Error>(ready && row.try_get::<bool, _>("fact_current").map_err(db_error)?)
        })?;
    let projected = if ready {
        let buckets = rows
            .iter()
            .map(|row| {
                Ok(PlanningBucket {
                    category: row.try_get("fact_category").map_err(db_error)?,
                    excluded: row.try_get("fact_excluded").map_err(db_error)?,
                    count: 1,
                    estimate_total: row
                        .try_get::<Option<i32>, _>("fact_estimate")
                        .map_err(db_error)?
                        .map(i64::from),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let counts = PlanningCounts::from_buckets(buckets);
        let dates = rows
            .iter()
            .map(|row| {
                row.try_get::<DateTime<Utc>, _>("calculated_at")
                    .map_err(db_error)
            })
            .collect::<Result<Vec<_>>>()?;
        (counts.total > 0).then_some((counts, dates.into_iter().min()))
    } else {
        None
    };
    let items = rows
        .into_iter()
        .map(|r| {
            Ok(CycleScopeItem {
                issue_id: r.try_get("id").map_err(db_error)?,
                issue_version: r
                    .try_get::<i64, _>("version")
                    .map_err(db_error)?
                    .to_string(),
                category: r.try_get("category").map_err(db_error)?,
                excluded: r.try_get("excluded").map_err(db_error)?,
                estimate: r.try_get("estimate").map_err(db_error)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let (now, date): (DateTime<Utc>, NaiveDate) = if let Some(clock) = clock {
        clock
    } else {
        sqlx::query_as("SELECT statement_timestamp(),(statement_timestamp() AT TIME ZONE $1)::date")
            .bind(&c.timezone)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?
    };
    let (counts, calculation_source, facts_as_of) = match projected {
        Some((counts, date)) => (counts, PlanningCalculationSource::Projection, date),
        None => (
            scope_counts(&items),
            if kind == "current" {
                PlanningCalculationSource::Live
            } else {
                PlanningCalculationSource::Snapshot
            },
            None,
        ),
    };
    Ok(CycleSnapshot {
        redacted,
        id: Uuid::new_v4(),
        cycle_id: c.id,
        local_date: date,
        captured_at: now,
        calculation_source,
        facts_as_of,
        kind: kind.into(),
        definition_version: 1,
        scope_count: counts.total,
        completed_count: counts.completed,
        canceled_count: counts.canceled,
        unfinished_count: counts.unfinished(),
        excluded_count: counts.excluded,
        estimate_total: counts.estimate_total,
        items,
    })
}
fn scope_counts(items: &[CycleScopeItem]) -> PlanningCounts {
    PlanningCounts::from_buckets(items.iter().map(|item| PlanningBucket {
        category: &item.category,
        excluded: item.excluded,
        count: 1,
        estimate_total: item.estimate.map(i64::from),
    }))
}
async fn save_snapshot(tx: &mut store::Tx, c: &Cycle, s: &CycleSnapshot) -> Result<()> {
    graph::one(sqlx::query("INSERT INTO cycle_snapshots(id,workspace_id,team_id,cycle_id,local_date,captured_at,kind,definition_version,scope_count,completed_count,canceled_count,unfinished_count,excluded_count,estimate_total,items) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)").bind(s.id).bind(c.workspace_id).bind(c.team_id).bind(c.id).bind(s.local_date).bind(s.captured_at).bind(&s.kind).bind(s.definition_version).bind(s.scope_count).bind(s.completed_count).bind(s.canceled_count).bind(s.unfinished_count).bind(s.excluded_count).bind(s.estimate_total).bind(serde_json::to_value(&s.items).map_err(|_|Error::Internal)?).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}
fn snapshot_row(r: PgRow) -> Result<CycleSnapshot> {
    Ok(CycleSnapshot {
        redacted: false,
        id: r.try_get("id").map_err(db_error)?,
        cycle_id: r.try_get("cycle_id").map_err(db_error)?,
        local_date: r.try_get("local_date").map_err(db_error)?,
        captured_at: r.try_get("captured_at").map_err(db_error)?,
        calculation_source: PlanningCalculationSource::Snapshot,
        facts_as_of: None,
        kind: r.try_get("kind").map_err(db_error)?,
        definition_version: r.try_get("definition_version").map_err(db_error)?,
        scope_count: r.try_get("scope_count").map_err(db_error)?,
        completed_count: r.try_get("completed_count").map_err(db_error)?,
        canceled_count: r.try_get("canceled_count").map_err(db_error)?,
        unfinished_count: r.try_get("unfinished_count").map_err(db_error)?,
        excluded_count: r.try_get("excluded_count").map_err(db_error)?,
        estimate_total: r.try_get("estimate_total").map_err(db_error)?,
        items: serde_json::from_value(r.try_get("items").map_err(db_error)?)
            .map_err(|_| Error::Internal)?,
    })
}
fn review(c: &Cycle, s: &CycleSnapshot) -> Result<String> {
    Ok(digest(
        &serde_json::to_vec(&(&c.etag, &s.items)).map_err(|_| Error::Internal)?,
    ))
}
fn limit(q: &CycleQuery) -> Result<usize> {
    let n = q.limit.unwrap_or(50);
    if !(1..=100).contains(&n) {
        return Err(Error::Invalid("Page size must be 1–100".into()));
    }
    Ok(n as usize)
}
fn open(c: &Cycle) -> Result<()> {
    if c.state == CycleState::Closed {
        Err(Error::Conflict(
            "Explicitly reopen this closed cycle before changing its scope or dates".into(),
        ))
    } else {
        Ok(())
    }
}

impl App {
    pub async fn list_cycles(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        q: CycleQuery,
    ) -> Result<Page<Cycle>> {
        let n = limit(&q)?;
        let mut tx = self.scoped(a, w, false).await?;
        let (plan, manage) = scope(&mut tx, a, w, t, false, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!("{w}:{t}:{}:{epoch}:{n}", a.user.id);
        let after: Option<(NaiveDate, Uuid)> = q
            .cursor
            .as_deref()
            .map(|s| self.taxonomy_cursor_decode("cycles-v1", &binding, s))
            .transpose()?;
        let rows=sqlx::query("SELECT *,(SELECT r.id FROM cycle_close_reports r WHERE r.workspace_id=cycles.workspace_id AND r.cycle_id=cycles.id ORDER BY r.closed_at DESC,r.id DESC LIMIT 1) AS latest_report_id,start_date::timestamp AT TIME ZONE timezone AS starts_at,end_date_exclusive::timestamp AT TIME ZONE timezone AS ends_at FROM cycles WHERE workspace_id=$1 AND team_id=$2 AND deleted_at IS NULL AND ($3::date IS NULL OR (start_date,id)<($3,$4)) ORDER BY start_date DESC,id DESC LIMIT $5").bind(w).bind(t).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(n as i64+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > n;
        let next_cursor = if has_more {
            let r = &rows[n - 1];
            Some(self.taxonomy_cursor_encode(
                "cycles-v1",
                &binding,
                &(
                    r.try_get::<NaiveDate, _>("start_date").map_err(db_error)?,
                    r.try_get::<Uuid, _>("id").map_err(db_error)?,
                ),
            )?)
        } else {
            None
        };
        let data = rows
            .into_iter()
            .take(n)
            .map(|r| cycle_row(w, t, r, plan, manage))
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
    pub async fn get_cycle(&self, a: &Actor, w: Uuid, t: Uuid, id: Uuid) -> Result<Cycle> {
        let mut tx = self.scoped(a, w, false).await?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(c)
    }
    pub async fn create_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        key: &str,
        mut req: CreateCycle,
    ) -> Result<Cycle> {
        req.name = bounded_text(&req.name, "Cycle name", 160)?;
        req.timezone = bounded_text(&req.timezone, "Cycle timezone", 100)?;
        validate_cycle_dates(req.start_date, req.end_date_exclusive)?;
        if req.timezone.len() > 100 || !(req.timezone.contains('/') || req.timezone == "UTC") {
            return Err(Error::Invalid("Choose an IANA timezone".into()));
        }
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let route = format!("cycles.create:{t}");
        let mut tx = self.scoped(a, w, true).await?;
        let retry = lifecycle::retry::<Cycle>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        scope(&mut tx, a, w, t, true, true).await?;
        if let Some(c) = retry {
            cycle(&mut tx, a, w, t, c.id).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(c);
        }
        let id = Uuid::new_v4();
        graph::one(sqlx::query("INSERT INTO cycles(id,workspace_id,team_id,name,start_date,end_date_exclusive,timezone) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(w).bind(t).bind(req.name).bind(req.start_date).bind(req.end_date_exclusive).bind(req.timezone).execute(&mut *tx).await.map_err(cycle_error)?.rows_affected())?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &c).await?;
        emit(&mut tx, a, &c, "cycle.created").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(c)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn update_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        req: UpdateCycle,
    ) -> Result<Cycle> {
        if req.name.is_none() && req.start_date.is_none() && req.end_date_exclusive.is_none() {
            return Err(Error::Invalid("Provide a cycle field to update".into()));
        }
        let mut tx = self.scoped(a, w, true).await?;
        scope(&mut tx, a, w, t, true, true).await?;
        lock(&mut tx, w, t, &[id]).await?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &c.etag, &c)?;
        open(&c)?;
        let name = bounded_text(req.name.as_deref().unwrap_or(&c.name), "Cycle name", 160)?;
        let start = req.start_date.unwrap_or(c.start_date);
        let end = req.end_date_exclusive.unwrap_or(c.end_date_exclusive);
        validate_cycle_dates(start, end)?;
        graph::one(sqlx::query("UPDATE cycles SET name=$4,start_date=$5,end_date_exclusive=$6,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(id).bind(name).bind(start).bind(end).execute(&mut *tx).await.map_err(cycle_error)?.rows_affected())?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        emit(&mut tx, a, &c, "cycle.updated").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(c)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn set_cycle_issue(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        issue: Uuid,
        etag: &str,
        req: CycleMembership,
    ) -> Result<Cycle> {
        let mut tx = self.scoped(a, w, true).await?;
        scope(&mut tx, a, w, t, true, false).await?;
        lock(&mut tx, w, t, &[id]).await?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &c.etag, &c)?;
        open(&c)?;
        graph::lock_issues(&mut tx, w, t, &[issue]).await?;
        let i = graph::current(&mut tx, a, w, issue).await?;
        if i.team_id != t || req.included && i.archived_at.is_some() {
            return Err(Error::NotFound);
        }
        graph::source_version(&i, &req.issue_etag)?;
        let linked: Option<Uuid> = sqlx::query_scalar(
            "SELECT cycle_id FROM cycle_issues WHERE workspace_id=$1 AND issue_id=$2",
        )
        .bind(w)
        .bind(issue)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        if req.included && linked.is_some_and(|old| old != id) {
            return Err(Error::Conflict(
                "Remove the issue from its existing cycle before assigning it here".into(),
            ));
        }
        let changed = if req.included {
            linked.is_none()
        } else {
            linked == Some(id)
        };
        let mut events = vec![];
        if changed {
            if req.included {
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM cycle_issues WHERE workspace_id=$1 AND cycle_id=$2",
                )
                .bind(w)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_error)?;
                if count >= 10000 {
                    return Err(Error::Conflict(
                        "A cycle scope is limited to 10,000 issues".into(),
                    ));
                }
                graph::one(sqlx::query("INSERT INTO cycle_issues(workspace_id,team_id,cycle_id,issue_id) VALUES($1,$2,$3,$4)").bind(w).bind(t).bind(id).bind(issue).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            } else {
                graph::one(sqlx::query("DELETE FROM cycle_issues WHERE workspace_id=$1 AND cycle_id=$2 AND issue_id=$3").bind(w).bind(id).bind(issue).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            }
            events = graph::bump(
                &mut tx,
                a,
                w,
                t,
                &[issue],
                "issue.cycle_changed",
                json!({"fields":["cycle"]}),
            )
            .await?;
            bump(&mut tx, w, id).await?;
        }
        let c = cycle(&mut tx, a, w, t, id).await?;
        if changed {
            if c.state == CycleState::Active {
                let s = current_scope(&mut tx, &c, "scope_changed").await?;
                save_snapshot(&mut tx, &c, &s).await?;
            }
            graph::effects(&mut tx, &events).await?;
            emit(&mut tx, a, &c, "cycle.scope_changed").await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(c)
    }
    pub async fn cycle_scope(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<CycleSnapshot> {
        let mut tx = self.scoped(a, w, false).await?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        let mut s = current_scope(&mut tx, &c, "current").await?;
        redact_snapshot(&mut tx, a, w, &mut s).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(s)
    }
    pub async fn cycle_close_preview(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<CycleClosePreview> {
        let mut tx = self.scoped(a, w, false).await?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        let mut s = current_scope(&mut tx, &c, "closed").await?;
        let token = review(&c, &s)?;
        redact_snapshot(&mut tx, a, w, &mut s).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(CycleClosePreview {
            cycle: c,
            scope: s,
            review_token: token,
        })
    }
    pub async fn open_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
    ) -> Result<Cycle> {
        self.transition_cycle(a, w, t, id, etag, None).await
    }
    pub async fn reopen_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        req: ReopenCycle,
    ) -> Result<Cycle> {
        let reason = bounded_text(&req.reason, "Reopen reason", 1000)?;
        self.transition_cycle(a, w, t, id, etag, Some(reason)).await
    }
    #[allow(clippy::too_many_arguments)]
    async fn transition_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        reason: Option<String>,
    ) -> Result<Cycle> {
        let mut tx = self.scoped(a, w, true).await?;
        scope(&mut tx, a, w, t, true, true).await?;
        lock(&mut tx, w, t, &[id]).await?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &c.etag, &c)?;
        if c.state
            != if reason.is_some() {
                CycleState::Closed
            } else {
                CycleState::Planned
            }
        {
            return Err(Error::Conflict(
                "This lifecycle transition is not available".into(),
            ));
        }
        graph::one(sqlx::query("UPDATE cycles SET state='active',closed_at=NULL,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        let kind = if reason.is_some() {
            "reopened"
        } else {
            "opened"
        };
        let s = current_scope(&mut tx, &c, kind).await?;
        save_snapshot(&mut tx, &c, &s).await?;
        if let Some(reason) = reason {
            graph::one(sqlx::query("INSERT INTO cycle_reopen_reasons(id,workspace_id,actor_id,cycle_id,reason,team_id) VALUES($1,$2,$3,$4,$5,$6)").bind(Uuid::new_v4()).bind(w).bind(a.user.id).bind(id).bind(reason).bind(t).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        emit(
            &mut tx,
            a,
            &c,
            if kind == "opened" {
                "cycle.opened"
            } else {
                "cycle.reopened"
            },
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(c)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn close_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        key: &str,
        req: CloseCycle,
    ) -> Result<CycleCloseReport> {
        let (kh, rh) = lifecycle::request_hash(key, &(etag, &req))?;
        let route = format!("cycles.close:{id}");
        let mut tx = self.scoped(a, w, true).await?;
        let retry =
            lifecycle::retry::<CycleCloseReport>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        scope(&mut tx, a, w, t, true, true).await?;
        if let Some(mut r) = retry {
            cycle(&mut tx, a, w, t, id).await?;
            redact_snapshot(&mut tx, a, w, &mut r.scope).await?;
            r.cycle.can_manage = scope(&mut tx, a, w, t, false, false).await?.1;
            tx.commit().await.map_err(db_error)?;
            return Ok(r);
        }
        lock(&mut tx, w, t, &[id]).await?;
        let old = cycle(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &old.etag, &old)?;
        if old.state != CycleState::Active {
            return Err(Error::Conflict("Only an open cycle can be closed".into()));
        }
        let s = current_scope(&mut tx, &old, "closed").await?;
        if req.review_token != review(&old, &s)? {
            return Err(Error::Conflict(
                "Cycle scope changed. Review a fresh close preview".into(),
            ));
        }
        graph::one(sqlx::query("UPDATE cycles SET state='closed',closed_at=clock_timestamp(),version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let mut c = cycle(&mut tx, a, w, t, id).await?;
        save_snapshot(&mut tx, &c, &s).await?;
        let report_id = Uuid::new_v4();
        c.latest_report_id = Some(report_id);
        c.etag = String::new();
        c.etag = format!(
            "\"cycle:{}:{}:{}\"",
            c.id,
            c.version,
            digest(&serde_json::to_vec(&c).map_err(|_| Error::Internal)?)
        );
        let mut r = CycleCloseReport {
            id: report_id,
            closed_at: c.closed_at.ok_or(Error::Internal)?,
            cycle: c.clone(),
            scope: s,
        };
        graph::one(sqlx::query("INSERT INTO cycle_close_reports(id,workspace_id,team_id,cycle_id,snapshot_id,cycle_version,cycle_body,closed_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(r.id).bind(w).bind(t).bind(id).bind(r.scope.id).bind(c.version.parse::<i64>().map_err(|_|Error::Internal)?).bind(serde_json::to_value(&c).map_err(|_|Error::Internal)?).bind(r.closed_at).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &r).await?;
        emit(&mut tx, a, &c, "cycle.closed").await?;
        redact_snapshot(&mut tx, a, w, &mut r.scope).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(r)
    }
    pub async fn cycle_report(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        report_id: Uuid,
    ) -> Result<CycleCloseReport> {
        let mut tx = self.scoped(a, w, false).await?;
        cycle(&mut tx, a, w, t, id).await?;
        let mut r = report(&mut tx, w, t, id, report_id).await?;
        redact_snapshot(&mut tx, a, w, &mut r.scope).await?;
        r.cycle.can_manage = cycle(&mut tx, a, w, t, id).await?.can_manage;
        tx.commit().await.map_err(db_error)?;
        Ok(r)
    }
    pub async fn capture_cycle_snapshot(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
    ) -> Result<CycleSnapshot> {
        let mut tx = self.scoped(a, w, true).await?;
        scope(&mut tx, a, w, t, true, true).await?;
        lock(&mut tx, w, t, &[id]).await?;
        let c = cycle(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &c.etag, &c)?;
        if c.state != CycleState::Active {
            return Err(Error::Conflict(
                "Daily snapshots require an open cycle".into(),
            ));
        }
        let s = current_scope(&mut tx, &c, "daily").await?;
        let old=sqlx::query("SELECT * FROM cycle_snapshots WHERE workspace_id=$1 AND cycle_id=$2 AND local_date=$3 AND kind='daily'").bind(w).bind(id).bind(s.local_date).fetch_optional(&mut *tx).await.map_err(db_error)?;
        let mut s = if let Some(r) = old {
            snapshot_row(r)?
        } else {
            save_snapshot(&mut tx, &c, &s).await?;
            emit(&mut tx, a, &c, "cycle.snapshot_captured").await?;
            s
        };
        redact_snapshot(&mut tx, a, w, &mut s).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(s)
    }
    pub async fn cycle_snapshots(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        q: CycleQuery,
    ) -> Result<Page<CycleSnapshotPoint>> {
        let n = limit(&q)?;
        let mut tx = self.scoped(a, w, false).await?;
        cycle(&mut tx, a, w, t, id).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!("{w}:{t}:{id}:{}:{epoch}:{n}", a.user.id);
        let after: Option<(DateTime<Utc>, Uuid)> = q
            .cursor
            .as_deref()
            .map(|s| self.taxonomy_cursor_decode("cycle-snapshots-v1", &binding, s))
            .transpose()?;
        let mut data =
            store::cycle_snapshot_points(&mut tx, w, a.user.id, t, id, after, n as i64 + 1)
                .await
                .map_err(db_error)?;
        let has_more = data.len() > n;
        let next_cursor = if has_more {
            Some(self.taxonomy_cursor_encode(
                "cycle-snapshots-v1",
                &binding,
                &(data[n - 1].captured_at, data[n - 1].id),
            )?)
        } else {
            None
        };
        data.truncate(n);
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
}
async fn report(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    id: Uuid,
    report_id: Uuid,
) -> Result<CycleCloseReport> {
    let r=sqlx::query("SELECT * FROM cycle_close_reports WHERE workspace_id=$1 AND team_id=$2 AND cycle_id=$3 AND id=$4").bind(w).bind(t).bind(id).bind(report_id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    let snapshot: Uuid = r.try_get("snapshot_id").map_err(db_error)?;
    let s = sqlx::query("SELECT * FROM cycle_snapshots WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(snapshot)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(CycleCloseReport {
        id: report_id,
        cycle: serde_json::from_value(r.try_get("cycle_body").map_err(db_error)?)
            .map_err(|_| Error::Internal)?,
        scope: snapshot_row(s)?,
        closed_at: r.try_get("closed_at").map_err(db_error)?,
    })
}

fn carry_row(r: PgRow) -> Result<CycleCarryOperation> {
    let id: Uuid = r.try_get("id").map_err(db_error)?;
    let version = r
        .try_get::<i64, _>("version")
        .map_err(db_error)?
        .to_string();
    Ok(CycleCarryOperation {
        redacted: false,
        id,
        source_cycle_id: r.try_get("source_cycle_id").map_err(db_error)?,
        destination_cycle_id: r.try_get("destination_cycle_id").map_err(db_error)?,
        report_id: r.try_get("report_id").map_err(db_error)?,
        etag: format!("\"cycle-carry:{id}:{version}\""),
        version,
        state: r.try_get("state").map_err(db_error)?,
        results: serde_json::from_value(r.try_get("results").map_err(db_error)?)
            .map_err(|_| Error::Internal)?,
        created_at: r.try_get("created_at").map_err(db_error)?,
        updated_at: r.try_get("updated_at").map_err(db_error)?,
    })
}
async fn operation(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    id: Uuid,
    op: Uuid,
) -> Result<CycleCarryOperation> {
    carry_row(sqlx::query("SELECT * FROM cycle_carry_operations WHERE workspace_id=$1 AND team_id=$2 AND source_cycle_id=$3 AND id=$4").bind(w).bind(t).bind(id).bind(op).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?)
}
fn validate_items(items: &[CarryForwardItem]) -> Result<()> {
    let unique = items
        .iter()
        .map(|i| i.issue_id)
        .collect::<std::collections::HashSet<_>>();
    if items.is_empty()
        || items.len() > 100
        || unique.len() != items.len()
        || items
            .iter()
            .any(|i| i.issue_version.parse::<i64>().map_or(true, |v| v < 1))
    {
        return Err(Error::Invalid(
            "Review 1–100 distinct issues with their current versions".into(),
        ));
    }
    Ok(())
}
async fn carry_items(
    tx: &mut store::Tx,
    a: &Actor,
    source: &Cycle,
    dest: &Cycle,
    r: &CycleCloseReport,
    items: &[CarryForwardItem],
) -> Result<(Vec<CycleCarryResult>, Vec<store::Event<'static>>)> {
    for i in items {
        if !r.scope.items.iter().any(|v| {
            v.issue_id == i.issue_id
                && !v.excluded
                && !matches!(v.category.as_str(), "completed" | "canceled")
        }) {
            return Err(Error::Invalid(
                "Carry-forward accepts only unfinished issues from the reviewed close report"
                    .into(),
            ));
        }
    }
    let ids = items.iter().map(|i| i.issue_id).collect::<Vec<_>>();
    // Team scope already excludes issue edits/moves, so absence can be a per-item result.
    sqlx::query("SELECT id FROM issues WHERE workspace_id=$1 AND team_id=$2 AND id=ANY($3) ORDER BY id FOR UPDATE").bind(source.workspace_id).bind(source.team_id).bind(&ids).fetch_all(&mut **tx).await.map_err(db_error)?;
    let mut results = vec![];
    let mut moved = vec![];
    let mut destination_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cycle_issues WHERE workspace_id=$1 AND cycle_id=$2",
    )
    .bind(source.workspace_id)
    .bind(dest.id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    for input in items {
        let row=sqlx::query("SELECT i.version,s.category,ci.cycle_id FROM issues i JOIN workflow_statuses s ON s.workspace_id=i.workspace_id AND s.team_id=i.team_id AND s.id=i.status_id LEFT JOIN cycle_issues ci ON ci.workspace_id=i.workspace_id AND ci.issue_id=i.id WHERE i.workspace_id=$1 AND i.team_id=$2 AND i.id=$3 AND i.deleted_at IS NULL AND i.archived_at IS NULL").bind(source.workspace_id).bind(source.team_id).bind(input.issue_id).fetch_optional(&mut **tx).await.map_err(db_error)?;
        let outcome = if let Some(row) = row {
            let v: i64 = row.try_get("version").map_err(db_error)?;
            let category: &str = row.try_get("category").map_err(db_error)?;
            let actual: Option<Uuid> = row.try_get("cycle_id").map_err(db_error)?;
            if actual != Some(source.id) {
                "scope_conflict"
            } else if v.to_string() != input.issue_version {
                "version_conflict"
            } else if matches!(category, "completed" | "canceled") {
                "no_longer_unfinished"
            } else if destination_count >= 10000 {
                "destination_full"
            } else {
                graph::one(sqlx::query("UPDATE cycle_issues SET cycle_id=$4 WHERE workspace_id=$1 AND cycle_id=$2 AND issue_id=$3").bind(source.workspace_id).bind(source.id).bind(input.issue_id).bind(dest.id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
                moved.push(input.issue_id);
                destination_count += 1;
                "moved"
            }
        } else {
            "unavailable"
        };
        results.push(CycleCarryResult {
            issue_id: input.issue_id,
            outcome: outcome.into(),
        });
    }
    let events = if moved.is_empty() {
        vec![]
    } else {
        let events = graph::bump(
            tx,
            a,
            source.workspace_id,
            source.team_id,
            &moved,
            "issue.cycle_changed",
            json!({"fields":["cycle"]}),
        )
        .await?;
        bump(tx, source.workspace_id, source.id).await?;
        bump(tx, source.workspace_id, dest.id).await?;
        events
    };
    Ok((results, events))
}
#[allow(clippy::too_many_arguments)]
async fn carry_scopes(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    id: Uuid,
    destination: Uuid,
    report_id: Uuid,
    destination_etag: &str,
) -> Result<(Cycle, Cycle, CycleCloseReport)> {
    lock(tx, w, t, &[id, destination]).await?;
    let c = cycle(tx, a, w, t, id).await?;
    let dest = cycle(tx, a, w, t, destination).await?;
    lifecycle::version(destination_etag, &dest.etag, &dest)?;
    if id == destination
        || c.state != CycleState::Closed
        || dest.state == CycleState::Closed
        || dest.start_date < c.end_date_exclusive
    {
        return Err(Error::Conflict(
            "Choose a later planned or open cycle in the same team".into(),
        ));
    }
    let r = report(tx, w, t, id, report_id).await?;
    if r.closed_at != c.closed_at.ok_or(Error::Internal)? {
        return Err(Error::Conflict(
            "Review the latest close report after reopening a cycle".into(),
        ));
    }
    Ok((c, dest, r))
}
async fn carry_effects(
    tx: &mut store::Tx,
    a: &Actor,
    source: &Cycle,
    dest: &Cycle,
    events: &[store::Event<'_>],
) -> Result<()> {
    if !events.is_empty() {
        let source = cycle(tx, a, source.workspace_id, source.team_id, source.id).await?;
        let dest = cycle(tx, a, dest.workspace_id, dest.team_id, dest.id).await?;
        let s = current_scope(tx, &source, "carry_forward").await?;
        save_snapshot(tx, &source, &s).await?;
        let s = current_scope(tx, &dest, "carry_forward").await?;
        save_snapshot(tx, &dest, &s).await?;
        graph::effects(tx, events).await?;
        emit(tx, a, &source, "cycle.carry_forward").await?;
        emit(tx, a, &dest, "cycle.scope_changed").await?;
    }
    Ok(())
}
impl App {
    #[allow(clippy::too_many_arguments)]
    pub async fn carry_forward_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        key: &str,
        req: CarryForwardCycle,
    ) -> Result<CycleCarryOperation> {
        validate_items(&req.items)?;
        let (kh, rh) = lifecycle::request_hash(key, &(etag, &req))?;
        let route = format!("cycles.carry:{id}");
        let mut tx = self.scoped(a, w, true).await?;
        let retry =
            lifecycle::retry::<CycleCarryOperation>(&mut tx, w, a.user.id, &route, &kh, &rh)
                .await?;
        scope(&mut tx, a, w, t, true, true).await?;
        if let Some(mut r) = retry {
            cycle(&mut tx, a, w, t, id).await?;
            cycle(&mut tx, a, w, t, r.destination_cycle_id).await?;
            redact_operation(&mut tx, a, w, &mut r).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(r);
        }
        let (c, dest, r) = carry_scopes(
            &mut tx,
            a,
            w,
            t,
            id,
            req.destination_cycle_id,
            req.report_id,
            &req.destination_etag,
        )
        .await?;
        lifecycle::version(etag, &c.etag, &c)?;
        let (results, events) = carry_items(&mut tx, a, &c, &dest, &r, &req.items).await?;
        let state = if results.iter().all(|r| r.outcome == "moved") {
            "complete"
        } else {
            "needs_review"
        };
        let op = Uuid::new_v4();
        let mut r=carry_row(sqlx::query("INSERT INTO cycle_carry_operations(id,workspace_id,team_id,source_cycle_id,destination_cycle_id,report_id,state,results) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *").bind(op).bind(w).bind(t).bind(id).bind(dest.id).bind(req.report_id).bind(state).bind(serde_json::to_value(results).map_err(|_|Error::Internal)?).fetch_one(&mut *tx).await.map_err(db_error)?)?;
        lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &r).await?;
        carry_effects(&mut tx, a, &c, &dest, &events).await?;
        // Even an all-conflict attempt is durable and auditable.
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: a.user.id,
                entity: op,
                version: 1,
                team: Some(t),
                action: "cycle.carry_reviewed",
                access_changed: false,
            },
        )
        .await
        .map_err(db_error)?;
        redact_operation(&mut tx, a, w, &mut r).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(r)
    }
    pub async fn cycle_carry_operation(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        op: Uuid,
    ) -> Result<CycleCarryOperation> {
        let mut tx = self.scoped(a, w, false).await?;
        cycle(&mut tx, a, w, t, id).await?;
        let mut r = operation(&mut tx, w, t, id, op).await?;
        cycle(&mut tx, a, w, t, r.destination_cycle_id).await?;
        redact_operation(&mut tx, a, w, &mut r).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(r)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn resume_cycle_carry(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        op: Uuid,
        etag: &str,
        key: &str,
        req: ResumeCycleCarry,
    ) -> Result<CycleCarryOperation> {
        validate_items(&req.items)?;
        let (kh, rh) = lifecycle::request_hash(key, &(etag, &req))?;
        let route = format!("cycles.carry.resume:{op}");
        let mut tx = self.scoped(a, w, true).await?;
        let retry =
            lifecycle::retry::<CycleCarryOperation>(&mut tx, w, a.user.id, &route, &kh, &rh)
                .await?;
        scope(&mut tx, a, w, t, true, true).await?;
        if let Some(mut r) = retry {
            cycle(&mut tx, a, w, t, id).await?;
            cycle(&mut tx, a, w, t, r.destination_cycle_id).await?;
            redact_operation(&mut tx, a, w, &mut r).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(r);
        }
        let mut old = operation(&mut tx, w, t, id, op).await?;
        let mut projected = old.clone();
        redact_operation(&mut tx, a, w, &mut projected).await?;
        lifecycle::version(etag, &projected.etag, &projected)?;
        for i in &req.items {
            if !projected
                .results
                .iter()
                .any(|r| r.issue_id == i.issue_id && r.outcome != "moved")
            {
                return Err(Error::Invalid(
                    "Resume accepts only unresolved items from this operation".into(),
                ));
            }
        }
        let (c, dest, r) = carry_scopes(
            &mut tx,
            a,
            w,
            t,
            id,
            old.destination_cycle_id,
            old.report_id,
            &req.destination_etag,
        )
        .await?;
        let (results, events) = carry_items(&mut tx, a, &c, &dest, &r, &req.items).await?;
        for result in results {
            let row = old
                .results
                .iter_mut()
                .find(|r| r.issue_id == result.issue_id)
                .ok_or(Error::Internal)?;
            *row = result;
        }
        let state = if old.results.iter().all(|r| r.outcome == "moved") {
            "complete"
        } else {
            "needs_review"
        };
        let mut r=carry_row(sqlx::query("UPDATE cycle_carry_operations SET results=$4,state=$5,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3 RETURNING *").bind(w).bind(t).bind(op).bind(serde_json::to_value(old.results).map_err(|_|Error::Internal)?).bind(state).fetch_one(&mut *tx).await.map_err(db_error)?)?;
        lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &r).await?;
        carry_effects(&mut tx, a, &c, &dest, &events).await?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: a.user.id,
                entity: op,
                version: r.version.parse().map_err(|_| Error::Internal)?,
                team: Some(t),
                action: "cycle.carry_resumed",
                access_changed: false,
            },
        )
        .await
        .map_err(db_error)?;
        redact_operation(&mut tx, a, w, &mut r).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(r)
    }
}

impl App {
    pub async fn delete_cycle(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
    ) -> Result<()> {
        let mut tx = self.scoped(a, w, true).await?;
        scope(&mut tx, a, w, t, true, true).await?;
        lock(&mut tx, w, t, &[id]).await?;
        let mut c = cycle(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &c.etag, &c)?;
        let used:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cycle_issues WHERE workspace_id=$1 AND cycle_id=$2) OR EXISTS(SELECT 1 FROM cycle_snapshots WHERE workspace_id=$1 AND cycle_id=$2)").bind(w).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
        if c.state != CycleState::Planned || used {
            return Err(Error::Conflict(
                "Only an unused planned cycle can be deleted; retain opened cycles as history"
                    .into(),
            ));
        }
        graph::one(sqlx::query("UPDATE cycles SET deleted_at=clock_timestamp(),version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        c.version = (c.version.parse::<i64>().map_err(|_| Error::Internal)? + 1).to_string();
        emit(&mut tx, a, &c, "cycle.deleted").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }
}

async fn redact_snapshot(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    s: &mut CycleSnapshot,
) -> Result<()> {
    let ids = s.items.iter().map(|i| i.issue_id).collect::<Vec<_>>();
    let visible = visible_sources(tx, a, w, &ids).await?;
    if visible.len() == s.items.len() {
        return Ok(());
    }
    s.redacted = true;
    if s.kind == "current" {
        s.calculation_source = PlanningCalculationSource::Live;
        s.facts_as_of = None;
    }
    s.items.retain(|i| visible.contains(&i.issue_id));
    let counts = scope_counts(&s.items);
    s.scope_count = counts.total;
    s.completed_count = counts.completed;
    s.canceled_count = counts.canceled;
    s.unfinished_count = counts.unfinished();
    s.excluded_count = counts.excluded;
    s.estimate_total = counts.estimate_total;
    Ok(())
}

async fn redact_operation(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    r: &mut CycleCarryOperation,
) -> Result<()> {
    let ids = r.results.iter().map(|i| i.issue_id).collect::<Vec<_>>();
    let visible = visible_sources(tx, a, w, &ids).await?;
    if visible.len() == r.results.len() {
        return Ok(());
    }
    r.redacted = true;
    r.results.retain(|i| visible.contains(&i.issue_id));
    r.state = "restricted".into();
    r.etag = format!(
        "\"cycle-carry:{}:{}:{}\"",
        r.id,
        r.version,
        digest(&serde_json::to_vec(&(&r.results, r.redacted)).map_err(|_| Error::Internal)?)
    );
    Ok(())
}
async fn visible_sources(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    ids: &[Uuid],
) -> Result<std::collections::HashSet<Uuid>> {
    Ok(sqlx::query_scalar::<_,Uuid>("SELECT i.id FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id WHERE i.workspace_id=$1 AND i.id=ANY($2) AND i.deleted_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=t.workspace_id AND m.team_id=t.id AND m.user_id=$3))").bind(w).bind(ids).bind(a.user.id).fetch_all(&mut **tx).await.map_err(db_error)?.into_iter().collect())
}

impl App {
    pub async fn cycle_issues(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        q: CycleQuery,
    ) -> Result<Page<CycleIssue>> {
        let n = limit(&q)?;
        let mut tx = self.scoped(a, w, false).await?;
        cycle(&mut tx, a, w, t, id).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!("{w}:{t}:{id}:{}:{epoch}:{n}", a.user.id);
        let after: Option<Uuid> = q
            .cursor
            .as_deref()
            .map(|s| self.taxonomy_cursor_decode("cycle-issues-v1", &binding, s))
            .transpose()?;
        let mut data = store::cycle_issue_page(&mut tx, w, a.user.id, t, id, after, n as i64 + 1)
            .await
            .map_err(db_error)?;
        let has_more = data.len() > n;
        let next_cursor = if has_more {
            Some(self.taxonomy_cursor_encode("cycle-issues-v1", &binding, &data[n - 1].id)?)
        } else {
            None
        };
        data.truncate(n);
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
}
