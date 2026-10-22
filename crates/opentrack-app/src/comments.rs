use super::*;
use crate::documents as docs;
use chrono::{DateTime, Utc};
use opentrack_domain::*;
use sqlx::Row;

pub(crate) async fn comment_scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    write: bool,
) -> Result<(Issue, Comment)> {
    let scope =
        sqlx::query("SELECT issue_id,origin_team_id FROM comments WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
    let issue = docs::issue_scope(
        tx,
        a,
        w,
        scope.try_get("issue_id").map_err(db_error)?,
        write,
    )
    .await?;
    if !store::visible_team(
        tx,
        w,
        scope.try_get("origin_team_id").map_err(db_error)?,
        a.user.id,
        false,
    )
    .await
    .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    if write {
        sqlx::query("SELECT id FROM comments WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
        sqlx::query("SELECT id FROM documents WHERE workspace_id=$1 AND comment_id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    }
    let row=sqlx::query("SELECT c.id,c.issue_id,c.author_id,u.display_name,c.version::text,c.created_at,c.edited_at,c.deleted_at,d.id AS document_id FROM comments c JOIN users u ON u.id=c.author_id JOIN documents d ON d.workspace_id=c.workspace_id AND d.comment_id=c.id WHERE c.workspace_id=$1 AND c.id=$2").bind(w).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
    let deleted_at: Option<DateTime<Utc>> = row.try_get("deleted_at").map_err(db_error)?;
    let author = row.try_get("author_id").map_err(db_error)?;
    let allowed = deleted_at.is_none()
        && docs::can_write(tx, a, w, &issue).await?
        && (a.user.id == author || docs::moderator(tx, a, w, issue.team_id).await?);
    let document = if deleted_at.is_none() {
        Some(
            docs::document_row(
                tx,
                a,
                w,
                row.try_get("document_id").map_err(db_error)?,
                allowed,
            )
            .await?,
        )
    } else {
        None
    };
    let can_edit = allowed && document.as_ref().is_some_and(|d| d.schema_supported);
    let mut comment = Comment {
        id,
        issue_id: issue.id,
        author: CommentAuthor {
            id: author,
            display_name: row.try_get("display_name").map_err(db_error)?,
        },
        document,
        version: row.try_get("version").map_err(db_error)?,
        etag: String::new(),
        created_at: row.try_get("created_at").map_err(db_error)?,
        edited_at: row.try_get("edited_at").map_err(db_error)?,
        deleted_at,
        can_edit,
        can_delete: allowed,
    };
    comment.refresh_etag();
    Ok((issue, comment))
}
impl App {
    pub async fn get_comment(&self, a: &Actor, w: Uuid, id: Uuid) -> Result<Comment> {
        let mut tx = self.scoped(a, w, false).await?;
        let (_, item) = comment_scope(&mut tx, a, w, id, false).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn list_comments(
        &self,
        a: &Actor,
        w: Uuid,
        issue: Uuid,
        query: DiscussionQuery,
    ) -> Result<Page<Comment>> {
        query.validate()?;
        let mut tx = self.scoped(a, w, false).await?;
        docs::issue_scope(&mut tx, a, w, issue, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{issue}:{}:{epoch}:{}",
            a.user.id,
            query.limit.unwrap_or(25)
        );
        let after: Option<(DateTime<Utc>, Uuid)> = query
            .cursor
            .as_deref()
            .map(|v| self.taxonomy_cursor_decode("issue-comments-v1", &binding, v))
            .transpose()?;
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT c.id FROM comments c JOIN teams t ON t.workspace_id=c.workspace_id AND t.id=c.origin_team_id WHERE c.workspace_id=$1 AND c.issue_id=$2 AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members m WHERE m.workspace_id=c.workspace_id AND m.team_id=t.id AND m.user_id=$3)) AND ($4::timestamptz IS NULL OR (c.created_at,c.id)<($4,$5)) ORDER BY c.created_at DESC,c.id DESC LIMIT $6").bind(w).bind(issue).bind(a.user.id).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(i64::from(query.limit.unwrap_or(25))+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut has_more = ids.len() > query.limit.unwrap_or(25) as usize;
        let mut data = Vec::new();
        // The native bridge bounds ordinary responses to 1 MiB. Rich comments
        // include JSON plus derived text, so item count alone cannot bound a page.
        // Reserve room for the page envelope and signed cursor. Always include
        // the first item to ensure forward progress; never truncate its body.
        const PAGE_BYTES: usize = 768 * 1024;
        const ENVELOPE_BYTES: usize = 2048;
        let mut encoded_bytes = ENVELOPE_BYTES;
        for id in ids.into_iter().take(query.limit.unwrap_or(25) as usize) {
            let comment = comment_scope(&mut tx, a, w, id, false).await?.1;
            let size = serde_json::to_vec(&comment)
                .map_err(|_| Error::Internal)?
                .len()
                + 1;
            if !data.is_empty() && encoded_bytes.saturating_add(size) > PAGE_BYTES {
                has_more = true;
                break;
            }
            encoded_bytes = encoded_bytes.saturating_add(size);
            data.push(comment);
        }
        let next_cursor = if has_more {
            data.last()
                .map(|c| {
                    self.taxonomy_cursor_encode(
                        "issue-comments-v1",
                        &binding,
                        &(c.created_at, c.id),
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
    pub async fn create_comment(
        &self,
        a: &Actor,
        w: Uuid,
        issue: Uuid,
        key: &str,
        request: DocumentWrite,
    ) -> Result<Comment> {
        let plain = request.validate()?;
        if plain.trim().is_empty() && !request.has_attachments() {
            return Err(Error::Invalid("Write a comment before sending".into()));
        }
        if request.moderation_reason.is_some() {
            return Err(Error::Invalid(
                "Moderation reason does not apply to creating a comment".into(),
            ));
        }
        let (key, hash) = lifecycle::request_hash(key, &request)?;
        let mut tx = self.scoped(a, w, false).await?;
        // Current issue authority is checked before returning any stored response.
        let parent = graph::current(&mut tx, a, w, issue).await?;
        let route = format!("issues.{issue}.comments.create");
        let replay: Option<Comment> =
            lifecycle::retry(&mut tx, w, a.user.id, &route, &key, &hash).await?;
        if let Some(old) = replay {
            let (_, current) = comment_scope(&mut tx, a, w, old.id, false).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(current);
        }
        crate::issue_references::lock_new_targets(
            &mut tx,
            a,
            w,
            parent.team_id,
            &request.body,
            &serde_json::Value::Null,
        )
        .await?;
        if graph::current(&mut tx, a, w, parent.id).await?.team_id != parent.team_id {
            return Err(Error::Conflict(
                "The source issue moved. Reload before commenting".into(),
            ));
        }
        let parent = docs::issue_scope(&mut tx, a, w, parent.id, true).await?;
        let id = Uuid::new_v4();
        let document = Uuid::new_v4();
        graph::one(sqlx::query("INSERT INTO comments(id,workspace_id,issue_id,origin_team_id,author_id) VALUES($1,$2,$3,$4,$5)").bind(id).bind(w).bind(issue).bind(parent.team_id).bind(a.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        graph::one(sqlx::query("INSERT INTO documents(id,workspace_id,comment_id,schema_version,body,plain_text,updated_by_user_id) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(document).bind(w).bind(id).bind(request.schema_version).bind(&request.body).bind(plain).bind(a.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = comment_scope(&mut tx, a, w, id, false).await?.1;
        let mut receipt = serde_json::to_value(&item).map_err(|_| Error::Internal)?;
        if let Some(document) = receipt
            .get_mut("document")
            .and_then(serde_json::Value::as_object_mut)
        {
            // Replay uses only the identity and reloads the authorized document.
            // Never persist target labels in an otherwise ID-only reference.
            document.remove("issue_references");
        }
        graph::one(sqlx::query("UPDATE idempotency_records SET response_body=$5 WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4").bind(w).bind(a.user.id).bind(&route).bind(key).bind(receipt).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        docs::effects(
            &mut tx,
            a,
            w,
            &parent,
            docs::DocumentEffect {
                target: id,
                version: 1,
                kind: "comment.created",
                reason: None,
                document: item.document.as_ref(),
            },
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn update_comment(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        request: DocumentWrite,
    ) -> Result<Comment> {
        let plain = request.validate()?;
        if plain.trim().is_empty() && !request.has_attachments() {
            return Err(Error::Invalid("A comment cannot be empty".into()));
        }
        let mut tx = self.scoped(a, w, false).await?;
        if !request.issue_reference_ids().is_empty() {
            let document: Uuid = sqlx::query_scalar("SELECT id FROM documents WHERE workspace_id=$1 AND comment_id=$2 AND deleted_at IS NULL")
                .bind(w).bind(id).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
            crate::issue_references::lock_document_targets(&mut tx, a, w, document, &request.body)
                .await?;
        }
        let (issue, current) = comment_scope(&mut tx, a, w, id, true).await?;
        let document = current.document.as_ref().ok_or(Error::NotFound)?;
        let reason = docs::moderation_reason(
            a,
            current.author.id,
            docs::moderator(&mut tx, a, w, issue.team_id).await?,
            request.moderation_reason.as_deref(),
        )?;
        if !document.schema_supported {
            return Err(Error::UnsupportedDocumentSchema);
        }
        lifecycle::version(etag, &current.etag, &current)?;
        docs::save_body(&mut tx, a, w, document.id, &request, &plain).await?;
        graph::one(sqlx::query("UPDATE comments SET version=version+1,edited_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = comment_scope(&mut tx, a, w, id, false).await?.1;
        docs::effects(
            &mut tx,
            a,
            w,
            &issue,
            docs::DocumentEffect {
                target: id,
                version: item.version.parse().map_err(|_| Error::Internal)?,
                kind: "comment.updated",
                reason: reason.as_deref(),
                document: item.document.as_ref(),
            },
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn delete_comment(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        request: CommentDelete,
    ) -> Result<()> {
        let mut tx = self.scoped(a, w, false).await?;
        let (issue, current) = comment_scope(&mut tx, a, w, id, true).await?;
        let document = current.document.as_ref().ok_or(Error::NotFound)?;
        let reason = docs::moderation_reason(
            a,
            current.author.id,
            docs::moderator(&mut tx, a, w, issue.team_id).await?,
            request.moderation_reason.as_deref(),
        )?;
        lifecycle::version(etag, &current.etag, &current)?;
        graph::one(sqlx::query("UPDATE comments SET version=version+1,deleted_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        graph::one(sqlx::query("UPDATE documents SET deleted_at=clock_timestamp(),body='{\"type\":\"doc\",\"content\":[]}'::jsonb,plain_text='',version=version+1,updated_by_user_id=$3,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 AND deleted_at IS NULL").bind(w).bind(document.id).bind(a.user.id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        docs::effects(
            &mut tx,
            a,
            w,
            &issue,
            docs::DocumentEffect {
                target: id,
                version: current
                    .version
                    .parse::<i64>()
                    .map_err(|_| Error::Internal)?
                    + 1,
                kind: "comment.deleted",
                reason: reason.as_deref(),
                document: None,
            },
        )
        .await?;
        tx.commit().await.map_err(db_error)
    }
}
