use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::{Bootstrap, CreateIssue, Issue, IssueFilter, IssuePage, apply_issue_patch};
use serde_json::{Value, json};
use sqlx::Row;

fn required(count: u64) -> Result<()> {
    if count == 1 {
        Ok(())
    } else {
        Err(Error::Internal)
    }
}

pub(crate) async fn status_transition(
    tx: &mut store::Tx,
    w: Uuid,
    old: &Issue,
    status: Uuid,
    assignee: Option<Uuid>,
    restoring: bool,
) -> Result<()> {
    if (restoring || status != old.status_id) && !sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND id=$3 AND retiring_at IS NULL AND archived_at IS NULL)").bind(w).bind(old.team_id).bind(status).fetch_one(&mut **tx).await.map_err(db_error)? {
        return Err(Error::Invalid("Status is unavailable for this team".into()));
    }
    let reopening = if status != old.status_id {
        sqlx::query_scalar::<_, bool>(
            "SELECT previous.category IN ('completed','canceled') AND current.category NOT IN ('completed','canceled')
             FROM workflow_statuses previous JOIN workflow_statuses current
               ON current.workspace_id=previous.workspace_id AND current.team_id=previous.team_id
             WHERE previous.workspace_id=$1 AND previous.team_id=$2 AND previous.id=$3 AND current.id=$4",
        )
        .bind(w)
        .bind(old.team_id)
        .bind(old.status_id)
        .bind(status)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?
    } else {
        false
    };
    if restoring || reopening || assignee != old.assignee_id {
        // Check assignee eligibility independently: unchanged retiring status
        // and historical unavailable assignees must not block unrelated edits.
        let default: Uuid = sqlx::query_scalar(
            "SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default",
        )
        .bind(w)
        .bind(old.team_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
        if !store::valid_references(tx, w, old.team_id, default, assignee)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Invalid(if reopening {
                "Choose an eligible assignee or Unassigned before reopening this issue".into()
            } else {
                "Assignee is unavailable for this team".into()
            }));
        }
    }
    Ok(())
}

impl App {
    pub async fn bootstrap(&self, actor: &Actor, w: Uuid) -> Result<Bootstrap> {
        let mut tx = self.scoped(actor, w, false).await?;
        let auth_epoch = store::auth_epoch(&mut tx, w)
            .await
            .map_err(db_error)?
            .to_string();
        let teams = store::teams(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?;
        let statuses = store::statuses(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?;
        let members = store::members(&mut tx, w).await.map_err(db_error)?;
        let mut issues = store::list_issues(
            &mut tx,
            w,
            actor.user.id,
            &IssueFilter {
                limit: Some(100),
                ..Default::default()
            },
            None,
        )
        .await
        .map_err(db_error)?;
        issues.truncate(100);
        tx.commit().await.map_err(db_error)?;
        Ok(Bootstrap {
            auth_epoch,
            teams,
            statuses,
            members,
            issues,
        })
    }
    pub async fn list_issues(
        &self,
        actor: &Actor,
        w: Uuid,
        mut f: IssueFilter,
    ) -> Result<IssuePage> {
        opentrack_domain::normalize_label_ids(&mut f.label_ids)?;
        f.validate()?;
        let mut tx = self.scoped(actor, w, false).await?;
        let auth_epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        for id in &f.label_ids {
            if store::label(&mut tx, w, actor.user.id, *id)
                .await
                .map_err(db_error)?
                .is_none()
            {
                return Err(Error::Invalid("A selected label is unavailable".into()));
            }
        }
        if f.sort.as_deref() == Some("manual") {
            return self.manual_issues(tx, actor, w, f, auth_epoch).await;
        }
        let mut fingerprint = f.clone();
        fingerprint.cursor = None;
        fingerprint.label_mode = Some(f.label_mode.as_deref().unwrap_or("any").into());
        fingerprint.sort = Some(f.sort.as_deref().unwrap_or("created").into());
        fingerprint.limit = Some(f.limit.unwrap_or(50));
        let binding = format!(
            "{w}:{auth_epoch}:{}:{}",
            actor.user.id,
            serde_json::to_string(&fingerprint).map_err(|_| Error::Internal)?
        );
        let after: Option<(i16, DateTime<Utc>, Uuid)> = f
            .cursor
            .as_deref()
            .map(|cursor| self.taxonomy_cursor_decode("issues-v2", &binding, cursor))
            .transpose()?;
        let mut data = store::list_issues(
            &mut tx,
            w,
            actor.user.id,
            &f,
            after.map(|(p, d, id)| store::IssueListCursor::Standard(p, d, id)),
        )
        .await
        .map_err(db_error)?;
        let limit = f.limit.unwrap_or(50) as usize;
        let has_more = data.len() > limit;
        data.truncate(limit);
        let next_cursor = if has_more {
            data.last()
                .map(|i| {
                    self.taxonomy_cursor_encode(
                        "issues-v2",
                        &binding,
                        &(
                            if i.priority == 0 { 5_i16 } else { i.priority },
                            i.created_at,
                            i.id,
                        ),
                    )
                })
                .transpose()?
        } else {
            None
        };
        let mut order_maintenance = Vec::new();
        if let Some(t) = f.team_id
            && store::visible_team(&mut tx, w, t, actor.user.id, false)
                .await
                .map_err(db_error)?
            && let Some(progress) =
                crate::issue_ordering::active(&mut tx, actor, w, t, None).await?
        {
            order_maintenance.push(progress);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(IssuePage {
            data,
            next_cursor,
            has_more,
            order_version: None,
            order_maintenance,
        })
    }
    pub async fn get_issue(&self, actor: &Actor, w: Uuid, id: Uuid) -> Result<Issue> {
        let mut tx = self.scoped(actor, w, false).await?;
        let item = store::issue(&mut tx, w, actor.user.id, id, false)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn issue_activity(&self, actor: &Actor, w: Uuid, id: Uuid) -> Result<Vec<Value>> {
        let mut tx = self.scoped(actor, w, false).await?;
        store::issue(&mut tx, w, actor.user.id, id, false)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let activity = store::activity(&mut tx, w, id, actor.user.id)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(activity)
    }
    pub async fn create_issue(
        &self,
        actor: &Actor,
        w: Uuid,
        key: &str,
        mut req: CreateIssue,
    ) -> Result<Issue> {
        if key.len() < 8 || key.len() > 128 || !key.is_ascii() {
            return Err(Error::Invalid(
                "Provide an Idempotency-Key of 8–128 ASCII characters".into(),
            ));
        }
        req.validate()?;
        let initial_document = req
            .document
            .clone()
            .unwrap_or_else(|| opentrack_domain::DocumentWrite::from_plain_text(&req.description));
        let request_hash = digest(&serde_json::to_vec(&req).map_err(|_| Error::Internal)?);
        let key_hash = digest(key.as_bytes());
        let mut tx = self.scoped(actor, w, true).await?;
        sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash) VALUES($1,$2,'issues.create',$3,$4) ON CONFLICT DO NOTHING").bind(w).bind(actor.user.id).bind(&key_hash).bind(&request_hash).execute(&mut *tx).await.map_err(db_error)?;
        let record=sqlx::query("SELECT request_hash,response_body,expires_at>now() AS valid FROM idempotency_records WHERE workspace_id=$1 AND principal_id=$2 AND route='issues.create' AND key_hash=$3 FOR UPDATE").bind(w).bind(actor.user.id).bind(&key_hash).fetch_one(&mut *tx).await.map_err(db_error)?;
        if record
            .try_get::<String, _>("request_hash")
            .map_err(db_error)?
            != request_hash
        {
            return Err(Error::Conflict(
                "This idempotency key was used for a different request".into(),
            ));
        }
        if !record.try_get::<bool, _>("valid").map_err(db_error)? {
            return Err(Error::Conflict(
                "The retry window expired. Reconcile the original issue before creating another"
                    .into(),
            ));
        }
        if let Some(body) = record
            .try_get::<Option<Value>, _>("response_body")
            .map_err(db_error)?
        {
            let missing_assignee = body.get("assignee").is_none();
            let mut issue: Issue = serde_json::from_value(body).map_err(|_| Error::Internal)?;
            graph::receipt_authority(&mut tx, actor, w, &issue).await?;
            store::hydrate_legacy_issue(&mut tx, &mut issue, missing_assignee)
                .await
                .map_err(db_error)?;
            tx.commit().await.map_err(db_error)?;
            return Ok(issue);
        }
        let initial_plain_text = initial_document.validate()?;
        crate::issue_references::lock_new_targets(
            &mut tx,
            actor,
            w,
            req.team_id,
            &initial_document.body,
            &Value::Null,
        )
        .await?;
        if !store::visible_team(&mut tx, w, req.team_id, actor.user.id, true)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        lifecycle::require_active_team(&mut tx, w, req.team_id).await?;
        let template_source =
            templates::selected(&mut tx, actor, w, req.team_id, req.template.as_ref()).await?;
        if let Some(parent) = req.parent_id {
            graph::target(
                &mut tx,
                actor,
                w,
                req.team_id,
                parent,
                req.parent_etag.as_deref().ok_or(Error::Internal)?,
            )
            .await?;
            graph::lock_issues(&mut tx, w, req.team_id, &[parent]).await?;
        }
        let status = if let Some(id) = req.status_id {
            id
        } else {
            sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default AND archived_at IS NULL AND retiring_at IS NULL").bind(w).bind(req.team_id).fetch_one(&mut *tx).await.map_err(db_error)?
        };
        if !store::valid_references(&mut tx, w, req.team_id, status, req.assignee_id)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Invalid(
                "Status or assignee is unavailable for this team".into(),
            ));
        }
        if !store::validate_issue_labels(&mut tx, w, req.team_id, &req.label_ids, &[])
            .await
            .map_err(db_error)?
        {
            return Err(Error::Invalid(
                "A selected label is unavailable for this team".into(),
            ));
        }
        let rank =
            match crate::issue_ordering::append(&mut tx, actor, w, req.team_id, status).await? {
                crate::issue_ordering::RankAllocation::Ready(rank) => rank,
                crate::issue_ordering::RankAllocation::Maintenance(progress) => {
                    tx.commit().await.map_err(db_error)?;
                    return Err(Error::IssueOrderMaintenance(vec![progress]));
                }
            };
        let number:i64=sqlx::query_scalar("UPDATE teams SET next_issue_number=next_issue_number+1 WHERE workspace_id=$1 AND id=$2 RETURNING next_issue_number-1").bind(w).bind(req.team_id).fetch_one(&mut *tx).await.map_err(db_error)?;
        let id = Uuid::new_v4();
        sqlx::query("SELECT set_config('opentrack.initial_document',$1,true)")
            .bind(initial_document.body.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        required(sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,description,status_id,priority,assignee_id,estimate,due_date,parent_id,rank) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14::numeric)").bind(id).bind(w).bind(req.team_id).bind(actor.user.id).bind(number).bind(&req.title).bind(&initial_plain_text).bind(status).bind(req.priority).bind(req.assignee_id).bind(req.estimate).bind(req.due_date).bind(req.parent_id).bind(rank).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        if let Some((template, version, hash)) = template_source {
            required(sqlx::query("INSERT INTO issue_template_sources(workspace_id,issue_id,source_team_id,template_id,template_version,definition_sha256) VALUES($1,$2,$3,$4,$5,$6)").bind(w).bind(id).bind(req.team_id).bind(template).bind(version).bind(hash).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        sqlx::query("SELECT set_config('opentrack.initial_document','',true)")
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        store::set_issue_labels(&mut tx, w, id, &req.label_ids)
            .await
            .map_err(db_error)?;
        let issue = store::issue(&mut tx, w, actor.user.id, id, false)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        required(sqlx::query("UPDATE idempotency_records SET response_body=$4 WHERE workspace_id=$1 AND principal_id=$2 AND route='issues.create' AND key_hash=$3").bind(w).bind(actor.user.id).bind(key_hash).bind(serde_json::to_value(&issue).map_err(|_|Error::Internal)?).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let mut events = if let Some(parent) = req.parent_id {
            graph::bump(
                &mut tx,
                actor,
                w,
                req.team_id,
                &[parent],
                "issue.parent_changed",
                json!({"source":"manual","action":"child_created"}),
            )
            .await?
        } else {
            Vec::new()
        };
        let mutation = req
            .mutation_id
            .or_else(|| events.first().map(|event| event.mutation))
            .unwrap_or_else(Uuid::new_v4);
        for event in &mut events {
            event.mutation = mutation;
        }
        let has_documents: bool = sqlx::query_scalar("SELECT to_regclass('documents') IS NOT NULL")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if has_documents {
            let document: Uuid = sqlx::query_scalar(
                "SELECT id FROM documents WHERE workspace_id=$1 AND issue_id=$2",
            )
            .bind(w)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
            required(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id) VALUES($1,$2,$3,'document.created',$4)").bind(Uuid::new_v4()).bind(w).bind(actor.user.id).bind(document).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        events.push(store::Event {
                workspace: w,
                team: req.team_id,
                entity: id,
                version: 1,
                actor: actor.user.id,
                kind: "issue.created",
                mutation,
                changes: json!({"label_ids":{"before":[],"after":req.label_ids},"assignee_id":{"before":null,"after":req.assignee_id},"source":"manual"}),
            });
        graph::effects(&mut tx, &events).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(issue)
    }
    pub async fn update_issue(
        &self,
        actor: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        patch: Value,
    ) -> Result<Issue> {
        let mut tx = self.scoped(actor, w, true).await?;
        let scope = store::issue(&mut tx, w, actor.user.id, id, false)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !store::visible_team(&mut tx, w, scope.team_id, actor.user.id, true)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let old = store::issue(&mut tx, w, actor.user.id, id, true)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if old.team_id != scope.team_id {
            return Err(Error::Conflict(
                "This issue moved teams; reload before editing".into(),
            ));
        }
        lifecycle::require_active_team(&mut tx, w, old.team_id).await?;
        if etag != old.etag() {
            return Err(Error::Stale(Box::new(old)));
        }
        if old.archived_at.is_some()
            && !patch.as_object().is_some_and(|fields| {
                fields.get("archived") == Some(&Value::Bool(false))
                    && fields.keys().all(|key| {
                        ["archived", "status_id", "assignee_id", "label_ids"]
                            .contains(&key.as_str())
                    })
            })
        {
            return Err(Error::Conflict(
                "Restore this issue before editing it".into(),
            ));
        }
        let document_change = if let Some(description) = patch.get("description") {
            let description = description
                .as_str()
                .ok_or_else(|| Error::Invalid("Description must be text".into()))?;
            let row = sqlx::query("SELECT id,schema_version,body FROM documents WHERE workspace_id=$1 AND issue_id=$2 AND deleted_at IS NULL FOR UPDATE").bind(w).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
            if row.try_get::<i32, _>("schema_version").map_err(db_error)? != 1 {
                return Err(Error::UnsupportedDocumentSchema);
            }
            if !opentrack_domain::plain_document_shape(
                &row.try_get::<Value, _>("body").map_err(db_error)?,
            ) {
                return Err(Error::Invalid(
                    "Use the document editor to preserve this structured body".into(),
                ));
            }
            let document: Uuid = row.try_get("id").map_err(db_error)?;
            let request = opentrack_domain::DocumentWrite::from_plain_text(description);
            let plain = request.validate()?;
            documents::save_body(&mut tx, actor, w, document, &request, &plain).await?;
            Some(documents::document_row(&mut tx, actor, w, document, true).await?)
        } else {
            None
        };
        let item = apply_issue_patch(&old, &patch)?;
        let restoring = old.archived_at.is_some() && item.archived_at.is_none();
        status_transition(
            &mut tx,
            w,
            &old,
            item.status_id,
            item.assignee_id,
            restoring,
        )
        .await?;
        if old.status_id != item.status_id || old.archived_at != item.archived_at {
            crate::issue_ordering::require_groups(
                &mut tx,
                actor,
                w,
                &[(old.team_id, old.status_id), (old.team_id, item.status_id)],
            )
            .await?;
        }
        if !store::validate_issue_labels(&mut tx, w, old.team_id, &item.label_ids, &old.label_ids)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Invalid(
                "A selected label is unavailable for this team".into(),
            ));
        }
        store::set_issue_labels(&mut tx, w, id, &item.label_ids)
            .await
            .map_err(db_error)?;
        required(sqlx::query("UPDATE issues SET title=$3,description=$4,status_id=$5,priority=$6,assignee_id=$7,estimate=$8,due_date=$9,archived_at=$10,version=version+1,updated_at=now() WHERE workspace_id=$1 AND id=$2")
            .bind(w).bind(id).bind(item.title).bind(item.description).bind(item.status_id).bind(item.priority).bind(item.assignee_id).bind(item.estimate).bind(item.due_date).bind(item.archived_at).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let issue = store::issue(&mut tx, w, actor.user.id, id, false)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        let changes = activity::issue_changes(&mut tx, &old, &issue).await?;
        let mutation = Uuid::new_v4();
        let event = store::Event {
            workspace: w,
            team: old.team_id,
            entity: id,
            version: issue.version.parse().map_err(|_| Error::Internal)?,
            actor: actor.user.id,
            kind: "issue.updated",
            mutation,
            changes,
        };
        store::append_activity(&mut tx, &event)
            .await
            .map_err(db_error)?;
        let mut events = vec![event];
        if let Some(document) = document_change {
            required(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id) VALUES($1,$2,$3,'document.updated',$4)").bind(mutation).bind(w).bind(actor.user.id).bind(document.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            events.push(store::Event {
                workspace: w,
                team: old.team_id,
                entity: document.id,
                version: document.version.parse().map_err(|_| Error::Internal)?,
                actor: actor.user.id,
                kind: "document.updated",
                mutation,
                changes: json!({}),
            });
        }
        store::append_stream_events(&mut tx, &events)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(issue)
    }
}
