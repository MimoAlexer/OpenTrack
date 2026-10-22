use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::*;
use serde_json::{Value, json};
use sqlx::Row;

pub(crate) async fn issue_scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    write: bool,
) -> Result<Issue> {
    let issue = graph::scope(tx, a, w, id, write).await?;
    if write {
        let role = store::workspace_role(tx, a, w)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !writable(&role) {
            return Err(Error::Forbidden);
        }
        let archived: bool =
            sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM workspaces WHERE id=$1")
                .bind(w)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_error)?;
        if archived {
            return Err(Error::Conflict(
                "Restore the workspace before editing its discussion".into(),
            ));
        }
        graph::lock_issues(tx, w, issue.team_id, &[issue.id]).await?;
    }
    Ok(issue)
}
pub(crate) async fn can_write(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    issue: &Issue,
) -> Result<bool> {
    let allowed: bool = sqlx::query_scalar("SELECT ws.archived_at IS NULL AND t.archived_at IS NULL AND m.active AND m.role<>'viewer' FROM workspaces ws JOIN teams t ON t.workspace_id=ws.id AND t.id=$2 JOIN workspace_members m ON m.workspace_id=ws.id AND m.user_id=$3 WHERE ws.id=$1").bind(w).bind(issue.team_id).bind(a.user.id).fetch_one(&mut **tx).await.map_err(db_error)?;
    Ok(allowed && issue.archived_at.is_none())
}
pub(crate) async fn moderator(tx: &mut store::Tx, a: &Actor, w: Uuid, team: Uuid) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 AND active AND role IN('owner','admin')) OR EXISTS(SELECT 1 FROM team_members WHERE workspace_id=$1 AND team_id=$3 AND user_id=$2 AND role='maintainer')").bind(w).bind(a.user.id).bind(team).fetch_one(&mut **tx).await.map_err(db_error)
}
pub(crate) fn moderation_reason(
    a: &Actor,
    author: Uuid,
    allowed: bool,
    reason: Option<&str>,
) -> Result<Option<String>> {
    if a.user.id != author && !allowed {
        return Err(Error::Forbidden);
    }
    if a.user.id == author && reason.is_none() {
        return Ok(None);
    }
    let reason = reason
        .ok_or_else(|| {
            Error::Invalid("Provide a reason when moderating another author's comment".into())
        })?
        .trim();
    if reason.is_empty() || reason.chars().count() > 500 || reason.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Moderation reason must contain 1–500 characters without control characters".into(),
        ));
    }
    Ok(Some(reason.into()))
}

pub(crate) async fn document_row(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    can_edit: bool,
) -> Result<Document> {
    let row = sqlx::query("SELECT id,issue_id,comment_id,schema_version,body,plain_text,version::text,created_at,updated_at FROM documents WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    let issue: Option<Uuid> = row.try_get("issue_id").map_err(db_error)?;
    let schema: i32 = row.try_get("schema_version").map_err(db_error)?;
    let mut item = Document {
        id,
        owner_type: if issue.is_some() { "issue" } else { "comment" }.into(),
        owner_id: issue.unwrap_or(
            row.try_get::<Option<Uuid>, _>("comment_id")
                .map_err(db_error)?
                .unwrap_or(Uuid::nil()),
        ),
        schema_supported: schema == 1,
        schema_version: schema,
        body: row.try_get("body").map_err(db_error)?,
        plain_text: row.try_get("plain_text").map_err(db_error)?,
        mentions: vec![],
        issue_references: vec![],
        version: row.try_get("version").map_err(db_error)?,
        etag: String::new(),
        created_at: row.try_get("created_at").map_err(db_error)?,
        updated_at: row.try_get("updated_at").map_err(db_error)?,
        can_edit: can_edit && schema == 1,
    };
    item.mentions = crate::mentions::projection(tx, w, id, &item.body, None).await?;
    item.issue_references = crate::issue_references::projection(tx, a, w, &item.body).await?;
    item.refresh_etag();
    Ok(item)
}
/// Resolve owner at every read/write; no cached team ID can authorize a document.
pub(crate) async fn document_scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    write: bool,
) -> Result<(Issue, Document, Option<Comment>)> {
    let row = sqlx::query("SELECT issue_id,comment_id FROM documents WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    let comment: Option<Uuid> = row.try_get("comment_id").map_err(db_error)?;
    if let Some(comment) = comment {
        let (issue, item) = crate::comments::comment_scope(tx, a, w, comment, write).await?;
        let document = item.document.clone().ok_or(Error::NotFound)?;
        return Ok((issue, document, Some(item)));
    }
    let issue = issue_scope(tx, a, w, row.try_get("issue_id").map_err(db_error)?, write).await?;
    if write {
        sqlx::query("SELECT id FROM documents WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    }
    let editable = can_write(tx, a, w, &issue).await?;
    Ok((issue, document_row(tx, a, w, id, editable).await?, None))
}

pub(crate) struct DocumentEffect<'a> {
    pub target: Uuid,
    pub version: i64,
    pub kind: &'static str,
    pub reason: Option<&'a str>,
    pub document: Option<&'a Document>,
}
pub(crate) async fn effects(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    issue: &Issue,
    effect: DocumentEffect<'_>,
) -> Result<()> {
    let DocumentEffect {
        target,
        version,
        kind,
        reason,
        document,
    } = effect;
    let event_team = if kind.starts_with("comment.") {
        sqlx::query_scalar("SELECT origin_team_id FROM comments WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(target)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?
    } else {
        issue.team_id
    };
    let mutation = Uuid::new_v4();
    graph::one(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id,metadata) VALUES($1,$2,$3,$4,$5,$6)").bind(mutation).bind(w).bind(a.user.id).bind(kind).bind(target).bind(json!({"moderation_reason":reason})).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    let changes = json!({"document_id":document.map(|d|d.id),"document_version":document.map(|d|&d.version),"comment_id":if kind.starts_with("comment."){Some(target)}else{None},"source":"manual"});
    store::append_activity(
        tx,
        &store::Event {
            workspace: w,
            team: event_team,
            entity: issue.id,
            version,
            actor: a.user.id,
            kind,
            mutation,
            changes: changes.clone(),
        },
    )
    .await
    .map_err(db_error)?;
    let mut events = vec![store::Event {
        workspace: w,
        team: event_team,
        entity: target,
        version,
        actor: a.user.id,
        kind,
        mutation,
        changes: json!({}),
    }];
    if kind == "comment.updated"
        && let Some(d) = document
    {
        events.push(store::Event {
            workspace: w,
            team: event_team,
            entity: d.id,
            version: d.version.parse().map_err(|_| Error::Internal)?,
            actor: a.user.id,
            kind: "document.updated",
            mutation,
            changes: json!({}),
        });
    }
    store::append_stream_events(tx, &events)
        .await
        .map_err(db_error)
}

pub(crate) async fn save_body(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    request: &DocumentWrite,
    plain: &str,
) -> Result<()> {
    graph::one(sqlx::query("UPDATE documents SET schema_version=$3,body=$4,plain_text=$5,version=version+1,updated_by_user_id=$6,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(id).bind(request.schema_version).bind(&request.body).bind(plain).bind(a.user.id).execute(&mut **tx).await.map_err(db_error)?.rows_affected())
}

impl App {
    pub async fn issue_document(&self, a: &Actor, w: Uuid, issue: Uuid) -> Result<Document> {
        let mut tx = self.scoped(a, w, false).await?;
        let issue = issue_scope(&mut tx, a, w, issue, false).await?;
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM documents WHERE workspace_id=$1 AND issue_id=$2 AND deleted_at IS NULL",
        )
        .bind(w)
        .bind(issue.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
        let can = can_write(&mut tx, a, w, &issue).await?;
        let document = document_row(&mut tx, a, w, id, can).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(document)
    }
    pub async fn get_document(&self, a: &Actor, w: Uuid, id: Uuid) -> Result<Document> {
        let mut tx = self.scoped(a, w, false).await?;
        let (_, document, _) = document_scope(&mut tx, a, w, id, false).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(document)
    }
    pub async fn update_document(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        request: DocumentWrite,
    ) -> Result<Document> {
        let plain = request.validate()?;
        let mut tx = self.scoped(a, w, false).await?;
        crate::issue_references::lock_document_targets(&mut tx, a, w, id, &request.body).await?;
        let (issue, current, comment) = document_scope(&mut tx, a, w, id, true).await?;
        if !current.schema_supported {
            return Err(Error::UnsupportedDocumentSchema);
        }
        if comment.is_some() && plain.trim().is_empty() && !request.has_attachments() {
            return Err(Error::Invalid("A comment cannot be empty".into()));
        }
        let reason = if let Some(c) = &comment {
            moderation_reason(
                a,
                c.author.id,
                moderator(&mut tx, a, w, issue.team_id).await?,
                request.moderation_reason.as_deref(),
            )?
        } else {
            if request.moderation_reason.is_some() {
                return Err(Error::Invalid(
                    "Moderation reason applies only to comments".into(),
                ));
            }
            None
        };
        lifecycle::version(etag, &current.etag, &current)?;
        save_body(&mut tx, a, w, id, &request, &plain).await?;
        if let Some(c) = &comment {
            graph::one(sqlx::query("UPDATE comments SET version=version+1,edited_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(c.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        let document = document_row(&mut tx, a, w, id, true).await?;
        let (target, version, kind) = if let Some(c) = comment {
            (
                c.id,
                c.version.parse::<i64>().map_err(|_| Error::Internal)? + 1,
                "comment.updated",
            )
        } else {
            (
                id,
                document.version.parse().map_err(|_| Error::Internal)?,
                "document.updated",
            )
        };
        effects(
            &mut tx,
            a,
            w,
            &issue,
            DocumentEffect {
                target,
                version,
                kind,
                reason: reason.as_deref(),
                document: Some(&document),
            },
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(document)
    }
    pub async fn document_revisions(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        query: DiscussionQuery,
    ) -> Result<Page<DocumentRevisionSummary>> {
        query.validate()?;
        let mut tx = self.scoped(a, w, false).await?;
        document_scope(&mut tx, a, w, id, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{id}:{}:{epoch}:{}",
            a.user.id,
            query.limit.unwrap_or(25)
        );
        let after: Option<i64> = query
            .cursor
            .as_deref()
            .map(|v| self.taxonomy_cursor_decode("document-revisions-v1", &binding, v))
            .transpose()?;
        let rows=sqlx::query("SELECT r.revision::text,r.schema_version,r.author_id,u.display_name,r.created_at,r.created_at+interval '90 days' AS expires_at FROM document_revisions r JOIN users u ON u.id=r.author_id JOIN teams t ON t.workspace_id=r.workspace_id AND t.id=r.origin_team_id WHERE r.workspace_id=$1 AND r.document_id=$2 AND r.created_at>clock_timestamp()-interval '90 days' AND ($4::bigint IS NULL OR r.revision<$4) AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=r.workspace_id AND m.team_id=t.id AND m.user_id=$3)) ORDER BY r.revision DESC LIMIT $5").bind(w).bind(id).bind(a.user.id).bind(after).bind(i64::from(query.limit.unwrap_or(25))+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut data = rows
            .iter()
            .map(revision_summary)
            .collect::<Result<Vec<_>>>()?;
        let has_more = data.len() > query.limit.unwrap_or(25) as usize;
        data.truncate(query.limit.unwrap_or(25) as usize);
        let next_cursor = if has_more {
            data.last()
                .map(|r| {
                    self.taxonomy_cursor_encode(
                        "document-revisions-v1",
                        &binding,
                        &r.revision.parse::<i64>().map_err(|_| Error::Internal)?,
                    )
                })
                .transpose()?
        } else {
            None
        };
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
    pub async fn document_revision(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        revision: i64,
    ) -> Result<DocumentRevision> {
        let mut tx = self.scoped(a, w, false).await?;
        document_scope(&mut tx, a, w, id, false).await?;
        let row=sqlx::query("SELECT r.revision::text,r.schema_version,r.author_id,u.display_name,r.created_at,r.created_at+interval '90 days' AS expires_at,r.body,r.plain_text,r.origin_team_id FROM document_revisions r JOIN users u ON u.id=r.author_id JOIN teams t ON t.workspace_id=r.workspace_id AND t.id=r.origin_team_id WHERE r.workspace_id=$1 AND r.document_id=$2 AND r.revision=$3 AND r.created_at>clock_timestamp()-interval '90 days' AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=r.workspace_id AND m.team_id=t.id AND m.user_id=$4))").bind(w).bind(id).bind(revision).bind(a.user.id).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        let mut result = DocumentRevision {
            summary: revision_summary(&row)?,
            body: row.try_get("body").map_err(db_error)?,
            plain_text: row.try_get("plain_text").map_err(db_error)?,
            mentions: vec![],
            issue_references: vec![],
        };
        result.mentions = crate::mentions::projection(
            &mut tx,
            w,
            id,
            &result.body,
            Some(row.try_get("origin_team_id").map_err(db_error)?),
        )
        .await?;
        result.issue_references =
            crate::issue_references::projection(&mut tx, a, w, &result.body).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn issue_activity_page(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        query: DiscussionQuery,
    ) -> Result<Page<Value>> {
        query.validate()?;
        let mut tx = self.scoped(a, w, false).await?;
        issue_scope(&mut tx, a, w, id, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{id}:{}:{epoch}:{}",
            a.user.id,
            query.limit.unwrap_or(25)
        );
        let after: Option<(DateTime<Utc>, Uuid)> = query
            .cursor
            .as_deref()
            .map(|v| self.taxonomy_cursor_decode("issue-activity-v1", &binding, v))
            .transpose()?;
        let mut data:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',a.id,'event_kind',a.event_kind,'actor_id',a.actor_id,'actor',CASE WHEN a.actor_kind='system' THEN 'OpenTrack' ELSE u.display_name END,'created_at',a.created_at,'changes',a.changes) FROM activity_entries a LEFT JOIN users u ON u.id=a.actor_id JOIN teams t ON t.workspace_id=a.workspace_id AND t.id=a.team_id WHERE a.workspace_id=$1 AND a.entity_id=$2 AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=a.workspace_id AND tm.team_id=a.team_id AND tm.user_id=$3)) AND ($4::timestamptz IS NULL OR (a.created_at,a.id)<($4,$5)) ORDER BY a.created_at DESC,a.id DESC LIMIT $6").bind(w).bind(id).bind(a.user.id).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(i64::from(query.limit.unwrap_or(25))+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = data.len() > query.limit.unwrap_or(25) as usize;
        data.truncate(query.limit.unwrap_or(25) as usize);
        let next_cursor = if has_more {
            data.last()
                .map(|v| {
                    let date: DateTime<Utc> = serde_json::from_value(v["created_at"].clone())
                        .map_err(|_| Error::Internal)?;
                    let id: Uuid =
                        serde_json::from_value(v["id"].clone()).map_err(|_| Error::Internal)?;
                    self.taxonomy_cursor_encode("issue-activity-v1", &binding, &(date, id))
                })
                .transpose()?
        } else {
            None
        };
        store::project_activity(&mut data);
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
}
fn revision_summary(r: &sqlx::postgres::PgRow) -> Result<DocumentRevisionSummary> {
    Ok(DocumentRevisionSummary {
        revision: r.try_get("revision").map_err(db_error)?,
        schema_version: r.try_get("schema_version").map_err(db_error)?,
        author: CommentAuthor {
            id: r.try_get("author_id").map_err(db_error)?,
            display_name: r.try_get("display_name").map_err(db_error)?,
        },
        created_at: r.try_get("created_at").map_err(db_error)?,
        expires_at: r.try_get("expires_at").map_err(db_error)?,
    })
}
