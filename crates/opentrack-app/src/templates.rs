use super::*;
use chrono::{DateTime, Utc};
use opentrack_domain::*;
use serde_json::json;
use sqlx::{Row, postgres::PgRow};

async fn team_scope(
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
    let active = workspace.archived_at.is_none() && team.archived_at.is_none();
    let manage = active && team.can_manage;
    let use_template = active && writable(&workspace.role);
    if write {
        if !active {
            return Err(Error::Conflict(
                "Restore the workspace and team before changing templates".into(),
            ));
        }
        if !manage {
            return Err(Error::Forbidden);
        }
    }
    Ok((manage, use_template))
}
fn summary(row: &PgRow, manage: bool, use_template: bool) -> Result<IssueTemplateSummary> {
    let id: Uuid = row.try_get("id").map_err(db_error)?;
    let version: i64 = row.try_get("version").map_err(db_error)?;
    let archived_at = row
        .try_get::<Option<DateTime<Utc>>, _>("archived_at")
        .map_err(db_error)?;
    let can_use = use_template && archived_at.is_none();
    Ok(IssueTemplateSummary {
        id,
        team_id: row.try_get("team_id").map_err(db_error)?,
        name: row.try_get("name").map_err(db_error)?,
        version: version.to_string(),
        etag: format!(
            "\"template:{id}:{version}:{}:{}\"",
            u8::from(manage),
            u8::from(can_use)
        ),
        archived_at,
        updated_at: row.try_get("updated_at").map_err(db_error)?,
        can_manage: manage,
        can_use,
    })
}
async fn detail(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    id: Uuid,
) -> Result<IssueTemplate> {
    let (manage, use_template) = team_scope(tx, a, w, t, false).await?;
    let r =
        sqlx::query("SELECT * FROM issue_templates WHERE workspace_id=$1 AND team_id=$2 AND id=$3")
            .bind(w)
            .bind(t)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
    let schema_version = r.try_get("document_schema_version").map_err(db_error)?;
    let body: serde_json::Value = r.try_get("document_body").map_err(db_error)?;
    let supported = validate_template_document(&DocumentWrite {
        schema_version,
        body: body.clone(),
        moderation_reason: None,
    })
    .is_ok();
    Ok(IssueTemplate {
        summary: summary(&r, manage, use_template)?,
        title: r.try_get("title").map_err(db_error)?,
        title_mode: match r.try_get::<&str, _>("title_mode").map_err(db_error)? {
            "literal" => TemplateTitleMode::Literal,
            "pattern" => TemplateTitleMode::Pattern,
            _ => return Err(Error::Internal),
        },
        defaults: TemplateDefaults {
            status_id: r.try_get("default_status_id").map_err(db_error)?,
            priority: r.try_get("default_priority").map_err(db_error)?,
            label_ids: r.try_get("default_label_ids").map_err(db_error)?,
        },
        document: TemplateDocument {
            schema_version,
            body,
            plain_text: r.try_get("document_plain_text").map_err(db_error)?,
            schema_supported: supported,
        },
    })
}
async fn active_defaults(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    d: &TemplateDefaults,
) -> Result<()> {
    if let Some(id) = d.status_id {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND id=$3 AND archived_at IS NULL AND retiring_at IS NULL)").bind(w).bind(t).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
        if !valid {
            return Err(Error::Invalid(
                "The template status is unavailable for this team".into(),
            ));
        }
    }
    for id in &d.label_ids {
        let label = store::label(tx, w, a.user.id, *id)
            .await
            .map_err(db_error)?
            .ok_or_else(|| Error::Invalid("A template label is unavailable".into()))?;
        if label.archived_at.is_some() || label.team_id.is_some_and(|team| team != t) {
            return Err(Error::Invalid(
                "A template label is unavailable for this team".into(),
            ));
        }
    }
    Ok(())
}
fn definition_hash(
    name: &str,
    title: &str,
    title_mode: TemplateTitleMode,
    defaults: &TemplateDefaults,
    document: &DocumentWrite,
) -> Result<String> {
    let mut value = json!({"name":name,"title":title,"defaults":defaults,"schema_version":document.schema_version,"body":document.body});
    if !title_mode.is_literal() {
        value["title_mode"] = json!(title_mode);
    }
    Ok(digest(
        &serde_json::to_vec(&value).map_err(|_| Error::Internal)?,
    ))
}
async fn emit(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    item: &IssueTemplate,
    action: &str,
) -> Result<()> {
    store::identity_event(
        tx,
        store::IdentityEvent {
            workspace: w,
            actor: a.user.id,
            entity: item.summary.id,
            version: item.summary.version.parse().map_err(|_| Error::Internal)?,
            team: Some(item.summary.team_id),
            action,
            access_changed: false,
        },
    )
    .await
    .map_err(db_error)
}
impl App {
    pub async fn list_templates(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        mut q: TemplateQuery,
    ) -> Result<Page<IssueTemplateSummary>> {
        if !(1..=100).contains(&q.limit.unwrap_or(25))
            || q.q.as_ref().is_some_and(|v| v.chars().count() > 100)
        {
            return Err(Error::Invalid(
                "Template page size must be 1–100 and search at most 100 characters".into(),
            ));
        }
        q.q = Some(q.q.as_deref().unwrap_or("").trim().to_owned());
        q.limit = Some(q.limit.unwrap_or(25));
        q.archived = Some(q.archived.unwrap_or(false));
        let mut tx = self.scoped(a, w, false).await?;
        let (manage, use_template) = team_scope(&mut tx, a, w, t, false).await?;
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
            .map(|v| self.taxonomy_cursor_decode("templates-v1", &binding, v))
            .transpose()?;
        let search = q.q.as_deref().unwrap_or("");
        let rows=sqlx::query("SELECT id,team_id,name,version,archived_at,updated_at,lower(name) AS sort_name FROM issue_templates WHERE workspace_id=$1 AND team_id=$2 AND (archived_at IS NOT NULL)=$3 AND strpos(lower(name),lower($4))>0 AND ($5::text IS NULL OR (lower(name),id)>($5,$6)) ORDER BY lower(name),id LIMIT $7")
            .bind(w).bind(t).bind(q.archived.unwrap_or(false)).bind(search).bind(after.as_ref().map(|v|&v.0)).bind(after.as_ref().map(|v|v.1)).bind(i64::from(q.limit.unwrap_or(25))+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let limit = q.limit.unwrap_or(25) as usize;
        let has_more = rows.len() > limit;
        let next_cursor = if has_more {
            let r = &rows[limit - 1];
            Some(self.taxonomy_cursor_encode(
                "templates-v1",
                &binding,
                &(
                    r.try_get::<String, _>("sort_name").map_err(db_error)?,
                    r.try_get::<Uuid, _>("id").map_err(db_error)?,
                ),
            )?)
        } else {
            None
        };
        let data = rows
            .iter()
            .take(limit)
            .map(|r| summary(r, manage, use_template))
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            has_more,
            next_cursor,
        })
    }
    pub async fn get_template(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<IssueTemplate> {
        let mut tx = self.scoped(a, w, false).await?;
        let item = detail(&mut tx, a, w, t, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn preview_template(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
    ) -> Result<TemplatePreview> {
        let mut tx = self.scoped(a, w, false).await?;
        team_scope(&mut tx, a, w, t, false).await?;
        sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR SHARE")
            .bind(w)
            .bind(t)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let template = detail(&mut tx, a, w, t, id).await?;
        let status:Option<serde_json::Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM workflow_statuses s WHERE workspace_id=$1 AND team_id=$2 AND (($3::uuid IS NOT NULL AND id=$3) OR ($3 IS NULL AND is_default AND archived_at IS NULL AND retiring_at IS NULL))").bind(w).bind(t).bind(template.defaults.status_id).fetch_optional(&mut *tx).await.map_err(db_error)?;
        let resolved_status: Option<Status> = status
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| Error::Internal)?;
        let mut diagnostics = Vec::new();
        let team = store::team_detail(&mut tx, w, t, a.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let title_context_at = Utc::now();
        let materialized_title = match materialize_template_title(
            &template.title,
            template.title_mode,
            &TemplateTitleContext {
                team_name: &team.name,
                team_key: &team.key,
                now: title_context_at,
            },
        ) {
            Ok(title) => Some(title),
            Err(error) => {
                diagnostics.push(TemplateDiagnostic {
                    field: "title".into(),
                    code: error.code().into(),
                    reference_id: None,
                });
                None
            }
        };
        let status_error = match &resolved_status {
            None => Some("unavailable"),
            Some(s) if s.archived_at.is_some() => Some("archived"),
            Some(s) if s.retiring_at.is_some() => Some("retiring"),
            _ => None,
        };
        if let Some(code) = status_error {
            diagnostics.push(TemplateDiagnostic {
                field: "status_id".into(),
                code: code.into(),
                reference_id: template.defaults.status_id,
            });
        }
        let mut labels = Vec::new();
        for id in &template.defaults.label_ids {
            match store::label(&mut tx, w, a.user.id, *id)
                .await
                .map_err(db_error)?
            {
                Some(label) if label.team_id.is_none_or(|team| team == t) => {
                    if label.archived_at.is_some() {
                        diagnostics.push(TemplateDiagnostic {
                            field: "label_ids".into(),
                            code: "archived".into(),
                            reference_id: Some(*id),
                        });
                    }
                    labels.push(label);
                }
                _ => diagnostics.push(TemplateDiagnostic {
                    field: "label_ids".into(),
                    code: "unavailable".into(),
                    reference_id: Some(*id),
                }),
            }
        }
        if !template.document.schema_supported {
            diagnostics.push(TemplateDiagnostic {
                field: "document".into(),
                code: "unsupported_schema".into(),
                reference_id: None,
            });
        }
        let can_create = template.summary.can_use && diagnostics.is_empty();
        tx.commit().await.map_err(db_error)?;
        Ok(TemplatePreview {
            template,
            materialized_title,
            title_context_at,
            resolved_status,
            labels,
            diagnostics,
            can_create,
        })
    }
    pub async fn create_template(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        key: &str,
        mut req: CreateIssueTemplate,
    ) -> Result<IssueTemplate> {
        let plain = req.validate()?;
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let mut tx = self.scoped(a, w, false).await?;
        let replay = lifecycle::retry::<IssueTemplate>(
            &mut tx,
            w,
            a.user.id,
            &format!("templates.create:{t}"),
            &kh,
            &rh,
        )
        .await?;
        team_scope(&mut tx, a, w, t, true).await?;
        if let Some(previous) = replay {
            let item = detail(&mut tx, a, w, t, previous.summary.id).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(item);
        }
        active_defaults(&mut tx, a, w, t, &req.defaults).await?;
        let id = Uuid::new_v4();
        let hash = definition_hash(
            &req.name,
            &req.title,
            req.title_mode,
            &req.defaults,
            &req.document,
        )?;
        graph::one(sqlx::query("INSERT INTO issue_templates(id,workspace_id,team_id,name,title,default_status_id,default_priority,default_label_ids,document_schema_version,document_body,document_plain_text,definition_sha256,title_mode) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)").bind(id).bind(w).bind(t).bind(req.name).bind(req.title).bind(req.defaults.status_id).bind(req.defaults.priority).bind(req.defaults.label_ids).bind(req.document.schema_version).bind(req.document.body).bind(plain).bind(hash).bind(req.title_mode.as_str()).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = detail(&mut tx, a, w, t, id).await?;
        graph::one(sqlx::query("UPDATE idempotency_records SET response_body=$5 WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4").bind(w).bind(a.user.id).bind(format!("templates.create:{t}")).bind(kh).bind(serde_json::to_value(&item).map_err(|_|Error::Internal)?).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        emit(&mut tx, a, w, &item, "template.created").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn update_template(
        &self,
        a: &Actor,
        w: Uuid,
        t: Uuid,
        id: Uuid,
        etag: &str,
        req: UpdateIssueTemplate,
    ) -> Result<IssueTemplate> {
        let changed = req.name.is_some()
            || req.title.is_some()
            || req.title_mode.is_some()
            || req.defaults.is_some()
            || req.document.is_some();
        if !changed && req.archived.is_none() {
            return Err(Error::Invalid("Provide a template field to update".into()));
        }
        let mut tx = self.scoped(a, w, false).await?;
        team_scope(&mut tx, a, w, t, true).await?;
        sqlx::query("SELECT id FROM issue_templates WHERE workspace_id=$1 AND team_id=$2 AND id=$3 FOR UPDATE").bind(w).bind(t).bind(id).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        let old = detail(&mut tx, a, w, t, id).await?;
        lifecycle::version(etag, &old.summary.etag, &old)?;
        if old.summary.archived_at.is_some() && (req.archived != Some(false) || changed) {
            return Err(Error::Conflict(
                "Restore this template before editing it".into(),
            ));
        }
        let name = if let Some(v) = req.name {
            bounded_text(&v, "Template name", 100)?
        } else {
            old.summary.name
        };
        let title_mode = req.title_mode.unwrap_or(old.title_mode);
        let title =
            validate_template_title(req.title.as_deref().unwrap_or(&old.title), title_mode)?;
        let defaults = if let Some(mut d) = req.defaults {
            d.validate()?;
            active_defaults(&mut tx, a, w, t, &d).await?;
            d
        } else {
            old.defaults
        };
        let (document, plain) = if let Some(d) = req.document {
            let p = validate_template_document(&d)?;
            (d, p)
        } else {
            (
                DocumentWrite {
                    schema_version: old.document.schema_version,
                    body: old.document.body,
                    moderation_reason: None,
                },
                old.document.plain_text,
            )
        };
        let hash = definition_hash(&name, &title, title_mode, &defaults, &document)?;
        let archived = req.archived.unwrap_or(old.summary.archived_at.is_some());
        graph::one(sqlx::query("UPDATE issue_templates SET name=$4,title=$5,default_status_id=$6,default_priority=$7,default_label_ids=$8,document_schema_version=$9,document_body=$10,document_plain_text=$11,definition_sha256=$12,archived_at=CASE WHEN $13 THEN coalesce(archived_at,clock_timestamp()) ELSE NULL END,title_mode=$14,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(id).bind(name).bind(title).bind(defaults.status_id).bind(defaults.priority).bind(defaults.label_ids).bind(document.schema_version).bind(document.body).bind(plain).bind(hash).bind(archived).bind(title_mode.as_str()).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let item = detail(&mut tx, a, w, t, id).await?;
        let action = match (old.summary.archived_at.is_some(), archived) {
            (false, true) => "template.archived",
            (true, false) => "template.restored",
            _ => "template.updated",
        };
        emit(&mut tx, a, w, &item, action).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
}

pub(crate) async fn selected(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    selection: Option<&TemplateSelection>,
) -> Result<Option<(Uuid, i64, String)>> {
    let Some(selection) = selection else {
        return Ok(None);
    };
    // Caller already owns the team's write lock. Mutable template validation is
    // deliberately after the normal committed issue-receipt replay branch.
    let item = detail(tx, a, w, t, selection.id).await?;
    if item.summary.version != selection.version {
        return Err(Error::VersionConflict(
            serde_json::to_value(&item).map_err(|_| Error::Internal)?,
        ));
    }
    if !item.summary.can_use {
        return Err(Error::Conflict(
            "This template is archived or unavailable for new issues".into(),
        ));
    }
    if !item.document.schema_supported {
        return Err(Error::UnsupportedDocumentSchema);
    }
    let hash=sqlx::query_scalar("SELECT definition_sha256 FROM issue_templates WHERE workspace_id=$1 AND team_id=$2 AND id=$3").bind(w).bind(t).bind(selection.id).fetch_one(&mut **tx).await.map_err(db_error)?;
    Ok(Some((
        selection.id,
        selection.version.parse().map_err(|_| Error::Internal)?,
        hash,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_digests_retain_the_legacy_format_and_patterns_include_their_mode() {
        let defaults = TemplateDefaults::default();
        let document = DocumentWrite {
            schema_version: 1,
            body: json!({"type":"doc","content":[{"type":"paragraph"}]}),
            moderation_reason: None,
        };
        let old = digest(&serde_json::to_vec(&json!({"name":"Daily review","title":"{{date}}","defaults":defaults,"schema_version":1,"body":document.body})).unwrap());
        let literal = definition_hash(
            "Daily review",
            "{{date}}",
            TemplateTitleMode::Literal,
            &defaults,
            &document,
        )
        .unwrap();
        let pattern = definition_hash(
            "Daily review",
            "{{date}}",
            TemplateTitleMode::Pattern,
            &defaults,
            &document,
        )
        .unwrap();
        assert_eq!(literal, old);
        assert_ne!(literal, pattern);
    }
}
