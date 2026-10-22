use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::*;
use serde_json::json;
use sqlx::{Row, postgres::PgRow};

// Deliberately neither Serialize nor Debug: storage locators and lease secrets
// stay inside the server/worker, never in public DTOs or ordinary logs.
pub struct UploadPermit {
    pub workspace: Uuid,
    pub id: Uuid,
    pub storage_key: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub generation: i64,
    pub lease_token: Uuid,
    pub lease_expires_at: DateTime<Utc>,
    pub antivirus_required: bool,
}
pub struct DownloadDescriptor {
    pub storage_key: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub detected_mime: String,
    pub original_name: String,
}
pub struct AttachmentGcClaim {
    pub workspace: Uuid,
    pub id: Uuid,
    pub storage_key: String,
    pub generation: i64,
    pub lease_token: Uuid,
    pub lease_expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Copy)]
pub enum ScanOutcome {
    Clean,
    NotScanned,
    Infected,
    Unavailable,
}
#[derive(Debug, Clone, Copy)]
pub enum UploadFailure {
    Interrupted,
    StorageUnavailable,
    ChecksumMismatch,
    SizeMismatch,
    RejectedContent,
}

struct Scope {
    attachment: Attachment,
    row: PgRow,
    event_team: Uuid,
    comment: Option<Comment>,
}
async fn row(tx: &mut store::Tx, w: Uuid, id: Uuid) -> Result<PgRow> {
    sqlx::query("SELECT a.*,l.document_id AS linked_document_id FROM attachments a LEFT JOIN document_attachments l ON l.workspace_id=a.workspace_id AND l.attachment_id=a.id WHERE a.workspace_id=$1 AND a.id=$2")
        .bind(w).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)
}
fn public(row: &PgRow, can_delete: bool) -> Result<Attachment> {
    let linked: Option<Uuid> = row.try_get("linked_document_id").map_err(db_error)?;
    let mut item = Attachment {
        id: row.try_get("id").map_err(db_error)?,
        document_id: linked,
        original_name: row.try_get("original_name").map_err(db_error)?,
        size_bytes: row.try_get("size_bytes").map_err(db_error)?,
        sha256: row.try_get("sha256").map_err(db_error)?,
        state: serde_json::from_value(json!(row.try_get::<String, _>("state").map_err(db_error)?))
            .map_err(|_| Error::Internal)?,
        detected_mime: row.try_get("detected_mime").map_err(db_error)?,
        scan_status: serde_json::from_value(json!(
            row.try_get::<String, _>("scan_status").map_err(db_error)?
        ))
        .map_err(|_| Error::Internal)?,
        version: row
            .try_get::<i64, _>("version")
            .map_err(db_error)?
            .to_string(),
        etag: String::new(),
        expires_at: if linked.is_some() {
            None
        } else {
            Some(row.try_get("expires_at").map_err(db_error)?)
        },
        created_at: row.try_get("created_at").map_err(db_error)?,
        can_delete: can_delete && row.try_get::<String, _>("state").map_err(db_error)? != "deleted",
    };
    item.refresh_etag();
    Ok(item)
}
async fn scope(tx: &mut store::Tx, a: &Actor, w: Uuid, id: Uuid, write: bool) -> Result<Scope> {
    let initial = row(tx, w, id).await?;
    let linked: Option<Uuid> = initial.try_get("linked_document_id").map_err(db_error)?;
    let reserved: Option<Uuid> = initial.try_get("reserved_document_id").map_err(db_error)?;
    let team: Uuid = initial.try_get("upload_team_id").map_err(db_error)?;
    let uploader: Uuid = initial.try_get("uploader_id").map_err(db_error)?;
    let (can_delete, event_team, comment) = if let Some(document) = linked.or(reserved) {
        if linked.is_none() && uploader != a.user.id {
            return Err(Error::NotFound);
        }
        let (issue, document, comment) =
            documents::document_scope(tx, a, w, document, write).await?;
        if linked.is_some() {
            let retained:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM document_attachment_revisions ar JOIN document_revisions r ON r.workspace_id=ar.workspace_id AND r.document_id=ar.document_id AND r.revision=ar.revision JOIN teams t ON t.workspace_id=r.workspace_id AND t.id=r.origin_team_id WHERE ar.workspace_id=$1 AND ar.document_id=$2 AND ar.attachment_id=$3 AND (ar.revision=$4 OR (r.created_at>clock_timestamp()-interval '90 days' AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=t.id AND tm.user_id=$5)))))").bind(w).bind(document.id).bind(id).bind(document.version.parse::<i64>().map_err(|_|Error::Internal)?).bind(a.user.id).fetch_one(&mut **tx).await.map_err(db_error)?;
            if !retained {
                return Err(Error::NotFound);
            }
        }
        if write && !document.can_edit {
            return Err(Error::Forbidden);
        }
        let event_team = if let Some(c) = &comment {
            sqlx::query_scalar(
                "SELECT origin_team_id FROM comments WHERE workspace_id=$1 AND id=$2",
            )
            .bind(w)
            .bind(c.id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?
        } else {
            issue.team_id
        };
        (document.can_edit, event_team, comment)
    } else {
        if uploader != a.user.id
            || !store::visible_team(tx, w, team, a.user.id, write)
                .await
                .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let active:bool=sqlx::query_scalar("SELECT ws.archived_at IS NULL AND t.archived_at IS NULL AND m.role<>'viewer' AND m.active FROM teams t JOIN workspaces ws ON ws.id=t.workspace_id JOIN workspace_members m ON m.workspace_id=ws.id AND m.user_id=$3 WHERE t.workspace_id=$1 AND t.id=$2").bind(w).bind(team).bind(a.user.id).fetch_one(&mut **tx).await.map_err(db_error)?;
        if write {
            if !active {
                return Err(Error::Conflict(
                    "Restore the parent and obtain write access before changing attachments".into(),
                ));
            }
            lifecycle::require_active_team(tx, w, team).await?;
        }
        (active, team, None)
    };
    if write {
        sqlx::query("SELECT id FROM attachments WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    }
    let row = row(tx, w, id).await?;
    if linked.is_none() {
        let valid: bool = sqlx::query_scalar(
            "SELECT expires_at>clock_timestamp() FROM attachments WHERE workspace_id=$1 AND id=$2",
        )
        .bind(w)
        .bind(id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
        if !valid {
            return Err(Error::NotFound);
        }
    }
    let attachment = public(&row, can_delete)?;
    Ok(Scope {
        attachment,
        row,
        event_team,
        comment,
    })
}
async fn effect(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    current: &Scope,
    version: i64,
    kind: &'static str,
    reason: Option<&str>,
) -> Result<()> {
    let id = current.attachment.id;
    let team = current.event_team;
    let mutation = Uuid::new_v4();
    graph::one(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id,metadata) VALUES($1,$2,$3,$4,$5,$6)").bind(mutation).bind(w).bind(a.user.id).bind(kind).bind(id).bind(json!({"moderation_reason":reason})).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
    store::append_stream_events(
        tx,
        &[store::Event {
            workspace: w,
            team,
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
fn permit(row: &PgRow, w: Uuid) -> Result<UploadPermit> {
    Ok(UploadPermit {
        workspace: w,
        id: row.try_get("id").map_err(db_error)?,
        storage_key: row
            .try_get::<Uuid, _>("storage_key")
            .map_err(db_error)?
            .to_string(),
        size_bytes: u64::try_from(row.try_get::<i64, _>("size_bytes").map_err(db_error)?)
            .map_err(|_| Error::Internal)?,
        sha256: row.try_get("sha256").map_err(db_error)?,
        generation: row.try_get("generation").map_err(db_error)?,
        lease_token: row.try_get("lease_token").map_err(db_error)?,
        lease_expires_at: row.try_get("lease_expires_at").map_err(db_error)?,
        antivirus_required: row.try_get("antivirus_required").map_err(db_error)?,
    })
}
async fn check_permit(tx: &mut store::Tx, a: &Actor, p: &UploadPermit) -> Result<Scope> {
    let current = scope(tx, a, p.workspace, p.id, true).await?;
    if current
        .row
        .try_get::<Uuid, _>("uploader_id")
        .map_err(db_error)?
        != a.user.id
    {
        return Err(Error::Forbidden);
    }
    let valid:bool=sqlx::query_scalar("SELECT coalesce(generation=$3 AND lease_token=$4 AND lease_expires_at>clock_timestamp() AND state IN('pending','quarantined') AND gc_token IS NULL AND storage_key::text=$5 AND size_bytes=$6 AND sha256=$7,false) FROM attachments WHERE workspace_id=$1 AND id=$2").bind(p.workspace).bind(p.id).bind(p.generation).bind(p.lease_token).bind(&p.storage_key).bind(p.size_bytes as i64).bind(&p.sha256).fetch_one(&mut **tx).await.map_err(db_error)?;
    if !valid {
        return Err(Error::Conflict(
            "The upload lease expired or was replaced; reload the attachment".into(),
        ));
    }
    Ok(current)
}

impl App {
    pub async fn reserve_attachment(
        &self,
        a: &Actor,
        w: Uuid,
        key: &str,
        request: CreateAttachment,
        limits: &AttachmentLimits,
    ) -> Result<Attachment> {
        request.validate(limits)?;
        let (key, hash) = lifecycle::request_hash(key, &request)?;
        let mut tx = self.scoped(a, w, false).await?;
        if let Some(old) =
            lifecycle::retry::<Attachment>(&mut tx, w, a.user.id, "attachments.create", &key, &hash)
                .await?
        {
            let current = scope(&mut tx, a, w, old.id, false).await?.attachment;
            tx.commit().await.map_err(db_error)?;
            return Ok(current);
        }
        // All quota changes serialize before team/document/attachment locks.
        sqlx::query(
            "INSERT INTO workspace_storage_usage(workspace_id) VALUES($1) ON CONFLICT DO NOTHING",
        )
        .bind(w)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query(
            "SELECT charged_bytes FROM workspace_storage_usage WHERE workspace_id=$1 FOR UPDATE",
        )
        .bind(w)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let team = if let Some(document) = request.document_id {
            let (issue, document, _) =
                documents::document_scope(&mut tx, a, w, document, true).await?;
            if !document.can_edit {
                return Err(Error::Forbidden);
            }
            issue.team_id
        } else {
            let team = request.team_id.ok_or(Error::Internal)?;
            if !store::visible_team(&mut tx, w, team, a.user.id, true)
                .await
                .map_err(db_error)?
            {
                return Err(Error::NotFound);
            }
            let role = store::workspace_role(&mut tx, a, w)
                .await
                .map_err(db_error)?
                .ok_or(Error::NotFound)?;
            if !writable(&role) {
                return Err(Error::Forbidden);
            }
            let active: bool =
                sqlx::query_scalar("SELECT archived_at IS NULL FROM workspaces WHERE id=$1")
                    .bind(w)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_error)?;
            if !active {
                return Err(Error::Conflict(
                    "Restore the workspace before uploading".into(),
                ));
            }
            lifecycle::require_active_team(&mut tx, w, team).await?;
            team
        };
        let charged=sqlx::query("UPDATE workspace_storage_usage SET charged_bytes=charged_bytes+$2 WHERE workspace_id=$1 AND charged_bytes<=$3-$2").bind(w).bind(request.size_bytes).bind(limits.workspace_quota_bytes).execute(&mut *tx).await.map_err(db_error)?.rows_affected();
        if charged != 1 {
            return Err(Error::Conflict("Workspace attachment quota is full; remove files and wait for cleanup before retrying".into()));
        }
        let id = Uuid::new_v4();
        graph::one(sqlx::query("INSERT INTO attachments(id,workspace_id,upload_team_id,uploader_id,reserved_document_id,storage_key,original_name,size_bytes,sha256,mime_hint,antivirus_required,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,clock_timestamp()+make_interval(secs=>$12))").bind(id).bind(w).bind(team).bind(a.user.id).bind(request.document_id).bind(Uuid::new_v4()).bind(&request.original_name).bind(request.size_bytes).bind(&request.sha256).bind(&request.mime_hint).bind(limits.antivirus_required).bind(f64::from(limits.unlinked_ttl_seconds)).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let current = scope(&mut tx, a, w, id, false).await?;
        graph::one(sqlx::query("UPDATE idempotency_records SET response_body=$5 WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4").bind(w).bind(a.user.id).bind("attachments.create").bind(&key).bind(serde_json::to_value(&current.attachment).map_err(|_|Error::Internal)?).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        effect(&mut tx, a, w, &current, 1, "attachment.reserved", None).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(current.attachment)
    }
    pub async fn attachment(&self, a: &Actor, w: Uuid, id: Uuid) -> Result<Attachment> {
        let mut tx = self.scoped(a, w, false).await?;
        let item = scope(&mut tx, a, w, id, false).await?.attachment;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn attachments(
        &self,
        a: &Actor,
        w: Uuid,
        query: AttachmentQuery,
    ) -> Result<Page<Attachment>> {
        query.validate()?;
        let mut tx = self.scoped(a, w, false).await?;
        if let Some(d) = query.document_id {
            documents::document_scope(&mut tx, a, w, d, false).await?;
        }
        if let Some(t) = query.team_id
            && !store::visible_team(&mut tx, w, t, a.user.id, false)
                .await
                .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{}:{epoch}:{:?}:{:?}:{}",
            a.user.id,
            query.document_id,
            query.team_id,
            query.limit.unwrap_or(25)
        );
        let after: Option<(DateTime<Utc>, Uuid)> = query
            .cursor
            .as_deref()
            .map(|v| self.taxonomy_cursor_decode("attachments-v1", &binding, v))
            .transpose()?;
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT a.id FROM attachments a LEFT JOIN document_attachments l ON l.workspace_id=a.workspace_id AND l.attachment_id=a.id WHERE a.workspace_id=$1 AND a.state<>'deleted' AND (($2::uuid IS NOT NULL AND (l.document_id=$2 OR (l.document_id IS NULL AND a.reserved_document_id=$2 AND a.uploader_id=$4 AND a.expires_at>clock_timestamp()))) OR ($3::uuid IS NOT NULL AND l.document_id IS NULL AND a.reserved_document_id IS NULL AND a.upload_team_id=$3 AND a.uploader_id=$4 AND a.expires_at>clock_timestamp())) AND ($5::timestamptz IS NULL OR (a.created_at,a.id)<($5,$6)) ORDER BY a.created_at DESC,a.id DESC LIMIT $7").bind(w).bind(query.document_id).bind(query.team_id).bind(a.user.id).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(i64::from(query.limit.unwrap_or(25))+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let more = ids.len() > query.limit.unwrap_or(25) as usize;
        let mut data = Vec::new();
        for id in ids.into_iter().take(query.limit.unwrap_or(25) as usize) {
            data.push(scope(&mut tx, a, w, id, false).await?.attachment);
        }
        let next_cursor = if more {
            data.last()
                .map(|i| {
                    self.taxonomy_cursor_encode("attachments-v1", &binding, &(i.created_at, i.id))
                })
                .transpose()?
        } else {
            None
        };
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more: more,
        })
    }
    pub async fn begin_upload(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        limits: &AttachmentLimits,
    ) -> Result<UploadPermit> {
        self.begin_attachment_transfer(a, w, id, Some(etag), limits, false)
            .await
    }
    pub async fn begin_finalize(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        limits: &AttachmentLimits,
    ) -> Result<UploadPermit> {
        self.begin_attachment_transfer(a, w, id, None, limits, true)
            .await
    }
    async fn begin_attachment_transfer(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: Option<&str>,
        limits: &AttachmentLimits,
        quarantine: bool,
    ) -> Result<UploadPermit> {
        limits.validate()?;
        let mut tx = self.scoped(a, w, false).await?;
        let current = scope(&mut tx, a, w, id, true).await?;
        if current
            .row
            .try_get::<Uuid, _>("uploader_id")
            .map_err(db_error)?
            != a.user.id
        {
            return Err(Error::Forbidden);
        }
        if let Some(etag) = etag {
            lifecycle::version(etag, &current.attachment.etag, &current.attachment)?;
        }
        let expected = if quarantine {
            AttachmentState::Quarantined
        } else {
            AttachmentState::Pending
        };
        if current.attachment.state != expected {
            return Err(Error::Conflict(
                "The attachment is not awaiting this upload step".into(),
            ));
        }
        let available:bool=sqlx::query_scalar("SELECT lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp() FROM attachments WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !available {
            return Err(Error::Conflict(
                "This attachment already has an active upload lease".into(),
            ));
        }
        graph::one(sqlx::query("UPDATE attachments SET version=version+1,generation=generation+1,lease_token=$3,lease_expires_at=clock_timestamp()+make_interval(secs=>$4),antivirus_required=antivirus_required OR $5 WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).bind(Uuid::new_v4()).bind(f64::from(limits.upload_lease_seconds)).bind(limits.antivirus_required).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = row(&mut tx, w, id).await?;
        let permit = permit(&item, w)?;
        effect(
            &mut tx,
            a,
            w,
            &current,
            item.try_get("version").map_err(db_error)?,
            "attachment.upload_started",
            None,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(permit)
    }
    pub async fn quarantine_attachment(
        &self,
        a: &Actor,
        p: &UploadPermit,
        observed_size: u64,
        observed_sha: &str,
        detected_mime: &str,
    ) -> Result<Attachment> {
        if observed_size != p.size_bytes
            || observed_sha != p.sha256
            || !valid_attachment_mime(detected_mime)
        {
            return Err(Error::Invalid(
                "Validated attachment bytes do not match the reservation".into(),
            ));
        }
        let mut tx = self.scoped(a, p.workspace, false).await?;
        let current = check_permit(&mut tx, a, p).await?;
        if current.attachment.state != AttachmentState::Pending {
            return Err(Error::Conflict(
                "The attachment is not awaiting upload".into(),
            ));
        }
        graph::one(sqlx::query("UPDATE attachments SET state='quarantined',scan_status='pending',detected_mime=$5,version=version+1 WHERE workspace_id=$1 AND id=$2 AND generation=$3 AND lease_token=$4 AND lease_expires_at>clock_timestamp()").bind(p.workspace).bind(p.id).bind(p.generation).bind(p.lease_token).bind(detected_mime).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = public(
            &row(&mut tx, p.workspace, p.id).await?,
            current.attachment.can_delete,
        )?;
        effect(
            &mut tx,
            a,
            p.workspace,
            &current,
            item.version.parse().map_err(|_| Error::Internal)?,
            "attachment.quarantined",
            None,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn finalize_attachment(
        &self,
        a: &Actor,
        p: &UploadPermit,
        outcome: ScanOutcome,
    ) -> Result<Attachment> {
        let mut tx = self.scoped(a, p.workspace, false).await?;
        let current = check_permit(&mut tx, a, p).await?;
        if current.attachment.state != AttachmentState::Quarantined {
            return Err(Error::Conflict("The attachment is not quarantined".into()));
        }
        let required: bool = current
            .row
            .try_get("antivirus_required")
            .map_err(db_error)?;
        let (state, scan, kind) = match outcome {
            ScanOutcome::Clean => ("ready", "clean", "attachment.ready"),
            ScanOutcome::NotScanned if !required => ("ready", "not_scanned", "attachment.ready"),
            ScanOutcome::Infected => ("rejected", "infected", "attachment.rejected"),
            _ => ("quarantined", "unavailable", "attachment.scan_unavailable"),
        };
        graph::one(sqlx::query("UPDATE attachments SET state=$5,scan_status=$6,version=version+1,lease_token=NULL,lease_expires_at=NULL WHERE workspace_id=$1 AND id=$2 AND generation=$3 AND lease_token=$4 AND lease_expires_at>clock_timestamp()").bind(p.workspace).bind(p.id).bind(p.generation).bind(p.lease_token).bind(state).bind(scan).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = public(
            &row(&mut tx, p.workspace, p.id).await?,
            current.attachment.can_delete,
        )?;
        effect(
            &mut tx,
            a,
            p.workspace,
            &current,
            item.version.parse().map_err(|_| Error::Internal)?,
            kind,
            None,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn fail_attachment_upload(
        &self,
        a: &Actor,
        p: &UploadPermit,
        failure: UploadFailure,
    ) -> Result<Attachment> {
        let mut tx = self.scoped(a, p.workspace, false).await?;
        let current = check_permit(&mut tx, a, p).await?;
        let rejected = matches!(
            failure,
            UploadFailure::ChecksumMismatch
                | UploadFailure::SizeMismatch
                | UploadFailure::RejectedContent
        );
        let state = if rejected {
            "rejected"
        } else if current.attachment.state == AttachmentState::Quarantined {
            "quarantined"
        } else {
            "pending"
        };
        graph::one(sqlx::query("UPDATE attachments SET state=$5,version=version+1,lease_token=NULL,lease_expires_at=NULL WHERE workspace_id=$1 AND id=$2 AND generation=$3 AND lease_token=$4 AND lease_expires_at>clock_timestamp()").bind(p.workspace).bind(p.id).bind(p.generation).bind(p.lease_token).bind(state).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = public(
            &row(&mut tx, p.workspace, p.id).await?,
            current.attachment.can_delete,
        )?;
        effect(
            &mut tx,
            a,
            p.workspace,
            &current,
            item.version.parse().map_err(|_| Error::Internal)?,
            if rejected {
                "attachment.rejected"
            } else {
                "attachment.upload_interrupted"
            },
            None,
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn delete_attachment(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        request: CommentDelete,
    ) -> Result<()> {
        let mut tx = self.scoped(a, w, false).await?;
        let current = scope(&mut tx, a, w, id, true).await?;
        lifecycle::version(etag, &current.attachment.etag, &current.attachment)?;
        let reason = if let Some(comment) = &current.comment {
            documents::moderation_reason(
                a,
                comment.author.id,
                true,
                request.moderation_reason.as_deref(),
            )?
        } else {
            if request.moderation_reason.is_some() {
                return Err(Error::Invalid(
                    "Moderation reason applies only to comment attachments".into(),
                ));
            }
            None
        };
        if current.attachment.state != AttachmentState::Deleted {
            graph::one(sqlx::query("UPDATE attachments SET state='deleted',deleted_at=clock_timestamp(),version=version+1,generation=generation+1,lease_token=NULL,lease_expires_at=NULL WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            effect(
                &mut tx,
                a,
                w,
                &current,
                current
                    .attachment
                    .version
                    .parse::<i64>()
                    .map_err(|_| Error::Internal)?
                    + 1,
                "attachment.deleted",
                reason.as_deref(),
            )
            .await?;
        }
        tx.commit().await.map_err(db_error)
    }
    pub async fn attachment_download(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
    ) -> Result<DownloadDescriptor> {
        let mut tx = self.scoped(a, w, false).await?;
        let current = scope(&mut tx, a, w, id, false).await?;
        if current.attachment.state != AttachmentState::Ready {
            return Err(Error::NotFound);
        }
        let result = DownloadDescriptor {
            storage_key: current
                .row
                .try_get::<Uuid, _>("storage_key")
                .map_err(db_error)?
                .to_string(),
            size_bytes: current.attachment.size_bytes as u64,
            sha256: current.attachment.sha256,
            detected_mime: current.attachment.detected_mime.ok_or(Error::Internal)?,
            original_name: current.attachment.original_name,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}
