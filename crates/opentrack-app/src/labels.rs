use super::*;
use chrono::Utc;
use hmac::{Hmac, Mac};
use opentrack_domain::*;
use serde::{Serialize, de::DeserializeOwned};
use sqlx::Row;

fn required(n: u64) -> Result<()> {
    if n == 1 { Ok(()) } else { Err(Error::Internal) }
}
fn valid_color(s: &str) -> Result<()> {
    if STATUS_COLORS.contains(&s) {
        Ok(())
    } else {
        Err(Error::Invalid("Choose an available label color".into()))
    }
}
fn search(q: Option<&str>) -> Result<String> {
    let value = q.unwrap_or("").trim();
    if value.chars().count() > 100 {
        return Err(Error::Invalid(
            "Search must contain at most 100 characters".into(),
        ));
    }
    Ok(value.into())
}
fn pattern(q: &str) -> String {
    format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}
async fn read_label(tx: &mut store::Tx, w: Uuid, user: Uuid, id: Uuid) -> Result<Label> {
    store::label(tx, w, user, id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)
}
async fn manage(tx: &mut store::Tx, actor: &Actor, w: Uuid, team: Option<Uuid>) -> Result<()> {
    if let Some(t) = team {
        if !store::visible_team(tx, w, t, actor.user.id, true)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        lifecycle::require_active_team(tx, w, t).await?;
        if !store::team_detail(tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?
            .can_manage
        {
            return Err(Error::Forbidden);
        }
    } else {
        let role: String = sqlx::query_scalar(
            "SELECT role FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 AND active",
        )
        .bind(w)
        .bind(actor.user.id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
        if !lifecycle::manager(&role) {
            return Err(Error::Forbidden);
        }
    }
    Ok(())
}
async fn emit(tx: &mut store::Tx, w: Uuid, user: Uuid, label: &Label, action: &str) -> Result<()> {
    store::identity_event(
        tx,
        store::IdentityEvent {
            workspace: w,
            actor: user,
            entity: label.id,
            version: label.version.parse().map_err(|_| Error::Internal)?,
            team: label.team_id,
            action,
            access_changed: false,
        },
    )
    .await
    .map_err(db_error)
}
impl App {
    // Shared labels affect every team; acquire the exclusive workspace guard directly.
    // Team labels need only the usual workspace guard followed by that team's lock.
    async fn label_scope(&self, actor: &Actor, w: Uuid, team: Option<Uuid>) -> Result<store::Tx> {
        if team.is_some() {
            return self.scoped(actor, w, true).await;
        }
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, actor.user.id, Some(w))
            .await
            .map_err(db_error)?;
        sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE")
            .bind(w)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !store::active_session(&mut tx, actor)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Unauthorized);
        }
        let workspace = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if workspace.archived_at.is_some() {
            return Err(Error::Conflict(
                "Restore the workspace before making changes".into(),
            ));
        }
        if !writable(&workspace.role) {
            return Err(Error::Forbidden);
        }
        Ok(tx)
    }
    pub async fn get_label(&self, actor: &Actor, w: Uuid, id: Uuid) -> Result<Label> {
        let mut tx = self.scoped(actor, w, false).await?;
        let result = read_label(&mut tx, w, actor.user.id, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn create_label(
        &self,
        actor: &Actor,
        w: Uuid,
        key: &str,
        mut req: CreateLabel,
    ) -> Result<Label> {
        req.name = bounded_text(&req.name, "Label name", 100)?;
        valid_color(&req.color)?;
        let (kh, rh) = lifecycle::request_hash(key, &req)?;
        let mut tx = self.label_scope(actor, w, req.team_id).await?;
        let replay =
            lifecycle::retry::<Label>(&mut tx, w, actor.user.id, "labels.create", &kh, &rh).await?;
        manage(&mut tx, actor, w, req.team_id).await?;
        if let Some(result) = replay {
            tx.commit().await.map_err(db_error)?;
            return Ok(result);
        }
        let id = Uuid::new_v4();
        required(
            sqlx::query(
                "INSERT INTO labels(id,workspace_id,team_id,name,color) VALUES($1,$2,$3,$4,$5)",
            )
            .bind(id)
            .bind(w)
            .bind(req.team_id)
            .bind(req.name)
            .bind(req.color)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?
            .rows_affected(),
        )?;
        let result = read_label(&mut tx, w, actor.user.id, id).await?;
        required(sqlx::query("UPDATE idempotency_records SET response_body=$4 WHERE workspace_id=$1 AND principal_id=$2 AND route='labels.create' AND key_hash=$3")
            .bind(w).bind(actor.user.id).bind(&kh).bind(serde_json::to_value(&result).map_err(|_|Error::Internal)?)
            .execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        emit(&mut tx, w, actor.user.id, &result, "label.created").await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn update_label(
        &self,
        actor: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        mut req: UpdateLabel,
    ) -> Result<Label> {
        if req.name.is_none() && req.color.is_none() && req.archived.is_none() {
            return Err(Error::Invalid("Provide a field to update".into()));
        }
        if let Some(name) = &mut req.name {
            *name = bounded_text(name, "Label name", 100)?;
        }
        if let Some(color) = &req.color {
            valid_color(color)?;
        }
        // Scope is immutable. The first authorized read selects the lock mode;
        // the second transaction reauthorizes and rereads under that guard.
        let known = self.get_label(actor, w, id).await?;
        let mut tx = self.label_scope(actor, w, known.team_id).await?;
        manage(&mut tx, actor, w, known.team_id).await?;
        sqlx::query("SELECT id FROM labels WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let old = read_label(&mut tx, w, actor.user.id, id).await?;
        lifecycle::version(etag, &old.etag(), &old)?;
        if old.archived_at.is_some() && req.archived != Some(false) {
            return Err(Error::Conflict(
                "Restore this label before editing it".into(),
            ));
        }
        let action = match (old.archived_at.is_some(), req.archived) {
            (false, Some(true)) => "label.archived",
            (true, Some(false)) => "label.restored",
            _ => "label.updated",
        };
        required(sqlx::query("UPDATE labels SET name=coalesce($3,name),color=coalesce($4,color),archived_at=CASE WHEN $5::boolean IS NULL THEN archived_at WHEN $5 THEN coalesce(archived_at,clock_timestamp()) ELSE NULL END,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).bind(req.name).bind(req.color).bind(req.archived).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let result = read_label(&mut tx, w, actor.user.id, id).await?;
        emit(&mut tx, w, actor.user.id, &result, action).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn list_labels(
        &self,
        actor: &Actor,
        w: Uuid,
        mut q: LabelQuery,
    ) -> Result<Page<Label>> {
        let limit = q.limit.unwrap_or(20);
        let scope = q.scope.as_deref().unwrap_or("visible").to_string();
        let state = q.state.as_deref().unwrap_or("active").to_string();
        let term = search(q.q.as_deref())?;
        if !(1..=100).contains(&limit)
            || !["workspace", "team", "applicable", "visible"].contains(&scope.as_str())
            || !["active", "archived", "all"].contains(&state.as_str())
            || ["team", "applicable"].contains(&scope.as_str()) != q.team_id.is_some()
        {
            return Err(Error::Invalid("Invalid label directory query".into()));
        }
        let mut tx = self.scoped(actor, w, false).await?;
        if let Some(t) = q.team_id {
            store::team_detail(&mut tx, w, t, actor.user.id)
                .await
                .map_err(db_error)?
                .ok_or(Error::NotFound)?;
        }
        let cursor = q.cursor.take();
        q.q = Some(term.clone());
        q.scope = Some(scope.clone());
        q.state = Some(state.clone());
        q.limit = Some(limit);
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{}:{epoch}:{}",
            actor.user.id,
            serde_json::to_string(&q).map_err(|_| Error::Internal)?
        );
        let after: Option<(String, Uuid)> = cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("labels", &binding, c))
            .transpose()?;
        let mut rows = store::label_page(&mut tx, w, actor.user.id, &q, after.as_ref())
            .await
            .map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_cursor = if has_more {
            rows.last()
                .map(|(label, sort_name)| {
                    self.taxonomy_cursor_encode("labels", &binding, &(sort_name, label.id))
                })
                .transpose()?
        } else {
            None
        };
        let data = rows.into_iter().map(|(label, _)| label).collect();
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
    pub async fn eligible_assignees(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        mut q: AssigneeQuery,
    ) -> Result<AssigneePage> {
        let limit = q.limit.unwrap_or(20);
        let term = search(q.q.as_deref())?;
        if !(1..=20).contains(&limit) {
            return Err(Error::Invalid(
                "Assignee page size must be between 1 and 20".into(),
            ));
        }
        let mut tx = self.scoped(actor, w, false).await?;
        store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let cursor = q.cursor.take();
        q.q = Some(term.clone());
        q.limit = Some(limit);
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{t}:{}:{epoch}:{}",
            actor.user.id,
            serde_json::to_string(&q).map_err(|_| Error::Internal)?
        );
        let after: Option<(String, Uuid)> = cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("assignees", &binding, c))
            .transpose()?;
        let self_assignable = store::assignable(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?;
        let mut rows=sqlx::query("SELECT u.id,u.display_name,lower(u.display_name) AS sort_name FROM workspace_members m JOIN users u ON u.id=m.user_id JOIN workspaces w ON w.id=m.workspace_id JOIN teams t ON t.workspace_id=m.workspace_id AND t.id=$2 WHERE m.workspace_id=$1 AND m.active AND m.role<>'viewer' AND u.disabled_at IS NULL AND w.archived_at IS NULL AND t.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=$2 AND tm.user_id=u.id)) AND u.display_name ILIKE $3 AND ($4::text IS NULL OR (lower(u.display_name),u.id)>($4,$5)) ORDER BY lower(u.display_name),u.id LIMIT $6").bind(w).bind(t).bind(pattern(&term)).bind(after.as_ref().map(|a|&a.0)).bind(after.as_ref().map(|a|a.1)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_cursor = if has_more {
            rows.last()
                .map(|r| -> Result<String> {
                    self.taxonomy_cursor_encode(
                        "assignees",
                        &binding,
                        &(
                            r.try_get::<String, _>("sort_name").map_err(db_error)?,
                            r.try_get::<Uuid, _>("id").map_err(db_error)?,
                        ),
                    )
                })
                .transpose()?
        } else {
            None
        };
        let data = rows
            .into_iter()
            .map(|r| {
                Ok(AssignableUser {
                    id: r.try_get("id").map_err(db_error)?,
                    display_name: r.try_get("display_name").map_err(db_error)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(AssigneePage {
            data,
            next_cursor,
            has_more,
            self_assignable,
        })
    }
    pub(crate) fn taxonomy_cursor_encode<T: Serialize>(
        &self,
        domain: &str,
        binding: &str,
        key: &T,
    ) -> Result<String> {
        let now = Utc::now().timestamp();
        let payload =
            serde_json::to_string(&(now, now + 86400, key)).map_err(|_| Error::Internal)?;
        let signature = self
            .taxonomy_mac(domain, binding, &payload)?
            .finalize()
            .into_bytes();
        Ok(format!(
            "{payload}|{}",
            signature
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
    }
    pub(crate) fn taxonomy_cursor_decode<T: DeserializeOwned>(
        &self,
        domain: &str,
        binding: &str,
        cursor: &str,
    ) -> Result<T> {
        let invalid =
            || Error::Invalid("This cursor is invalid or belongs to another query".into());
        if cursor.len() > 2048 {
            return Err(invalid());
        }
        let (payload, signature) = cursor.rsplit_once('|').ok_or_else(invalid)?;
        if signature.len() != 64 || !signature.is_ascii() {
            return Err(invalid());
        }
        let bytes = (0..64)
            .step_by(2)
            .map(|i| u8::from_str_radix(&signature[i..i + 2], 16).map_err(|_| invalid()))
            .collect::<Result<Vec<_>>>()?;
        self.taxonomy_mac(domain, binding, payload)?
            .verify_slice(&bytes)
            .map_err(|_| invalid())?;
        let (issued, expiry, key): (i64, i64, T) =
            serde_json::from_str(payload).map_err(|_| invalid())?;
        let now = Utc::now().timestamp();
        if issued > now + 60 || issued.checked_add(86400) != Some(expiry) {
            return Err(invalid());
        }
        if expiry <= now {
            return Err(Error::CursorExpired);
        }
        Ok(key)
    }
    fn taxonomy_mac(&self, domain: &str, binding: &str, payload: &str) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.setup_token_hash.as_bytes())
            .map_err(|_| Error::Internal)?;
        mac.update(b"opentrack.taxonomy-cursor.v1\0");
        mac.update(domain.as_bytes());
        mac.update(b"\0");
        mac.update(binding.as_bytes());
        mac.update(b"\0");
        mac.update(payload.as_bytes());
        Ok(mac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn taxonomy_cursor_authenticates_scope_domain_and_expiry() {
        let app = App {
            pool: PgPoolOptions::new()
                .connect_lazy("postgres://unused@localhost/unused")
                .unwrap(),
            setup_token_hash: digest(b"taxonomy-cursor-test"),
            hashes: Arc::new(Semaphore::new(1)),
        };
        let key = ("a|é%_".to_string(), Uuid::new_v4());
        let cursor = app
            .taxonomy_cursor_encode("labels", "actor:scope:epoch", &key)
            .unwrap();
        assert_eq!(
            app.taxonomy_cursor_decode::<(String, Uuid)>("labels", "actor:scope:epoch", &cursor)
                .unwrap(),
            key
        );
        assert!(matches!(
            app.taxonomy_cursor_decode::<(String, Uuid)>("assignees", "actor:scope:epoch", &cursor),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            app.taxonomy_cursor_decode::<(String, Uuid)>(
                "labels",
                "another-actor:scope:epoch",
                &cursor
            ),
            Err(Error::Invalid(_))
        ));
        let now = Utc::now().timestamp();
        for (issued, expected_expired) in [(now - 86401, true), (now + 120, false)] {
            let payload = serde_json::to_string(&(issued, issued + 86400, &key)).unwrap();
            let signature = app
                .taxonomy_mac("labels", "scope", &payload)
                .unwrap()
                .finalize()
                .into_bytes();
            let signed = format!(
                "{payload}|{}",
                signature
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            let result = app.taxonomy_cursor_decode::<(String, Uuid)>("labels", "scope", &signed);
            assert!(if expected_expired {
                matches!(result, Err(Error::CursorExpired))
            } else {
                matches!(result, Err(Error::Invalid(_)))
            });
        }
        let tampered = cursor.replacen("a|é%_", "a|different", 1);
        assert!(
            app.taxonomy_cursor_decode::<(String, Uuid)>("labels", "actor:scope:epoch", &tampered)
                .is_err()
        );
    }
}
