use super::*;
use opentrack_domain::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::Row;

pub(crate) fn manager(role: &str) -> bool {
    matches!(role, "owner" | "admin")
}
fn workspace_access_increased(old: &WorkspaceMember, role: &str, active: bool) -> bool {
    active
        && (!old.active
            || matches!(
                (old.role.as_str(), role),
                ("viewer", "member" | "admin" | "owner")
                    | ("member", "admin" | "owner")
                    | ("admin", "owner")
            ))
}
async fn require_enabled_user(tx: &mut store::Tx, user: Uuid) -> Result<()> {
    let enabled: bool = sqlx::query_scalar("SELECT disabled_at IS NULL FROM users WHERE id=$1")
        .bind(user)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    if !enabled {
        return Err(Error::Invalid("This account is disabled".into()));
    }
    Ok(())
}
pub(crate) async fn recent(tx: &mut store::Tx, actor: &Actor) -> Result<()> {
    if !store::active_session(tx, actor).await.map_err(db_error)? {
        return Err(Error::Unauthorized);
    }
    let fresh:bool=sqlx::query_scalar("SELECT auth_time>clock_timestamp()-interval '10 minutes' FROM sessions WHERE token_hash=$1 AND user_id=$2").bind(&actor.token_hash).bind(actor.user.id).fetch_one(&mut **tx).await.map_err(db_error)?;
    if !fresh {
        return Err(Error::ReauthenticationRequired);
    }
    Ok(())
}
pub(crate) async fn require_active_team(tx: &mut store::Tx, w: Uuid, t: Uuid) -> Result<()> {
    let active: bool =
        sqlx::query_scalar("SELECT archived_at IS NULL FROM teams WHERE workspace_id=$1 AND id=$2")
            .bind(w)
            .bind(t)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
    if !active {
        return Err(Error::Conflict(
            "Restore the team before making changes".into(),
        ));
    }
    Ok(())
}
pub(crate) fn version<T: Serialize>(provided: &str, expected: &str, current: &T) -> Result<()> {
    if provided.is_empty() {
        return Err(Error::PreconditionRequired);
    }
    if provided != expected {
        return Err(Error::VersionConflict(
            serde_json::to_value(current).map_err(|_| Error::Internal)?,
        ));
    }
    Ok(())
}
pub(crate) fn request_hash<T: Serialize>(key: &str, request: &T) -> Result<(String, String)> {
    if !(8..=128).contains(&key.len()) || !key.is_ascii() {
        return Err(Error::Invalid(
            "Provide an Idempotency-Key of 8–128 ASCII characters".into(),
        ));
    }
    Ok((
        digest(key.as_bytes()),
        digest(&serde_json::to_vec(request).map_err(|_| Error::Internal)?),
    ))
}
pub(crate) async fn retry<T: DeserializeOwned>(
    tx: &mut store::Tx,
    w: Uuid,
    u: Uuid,
    route: &str,
    key: &str,
    hash: &str,
) -> Result<Option<T>> {
    sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(w).bind(u).bind(route).bind(key).bind(hash).execute(&mut **tx).await.map_err(db_error)?;
    let r=sqlx::query("SELECT request_hash,response_body,expires_at>now() AS valid FROM idempotency_records WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4 FOR UPDATE").bind(w).bind(u).bind(route).bind(key).fetch_one(&mut **tx).await.map_err(db_error)?;
    retry_body(r, hash)
}
fn retry_body<T: DeserializeOwned>(r: sqlx::postgres::PgRow, hash: &str) -> Result<Option<T>> {
    if r.try_get::<String, _>("request_hash").map_err(db_error)? != hash {
        return Err(Error::Conflict(
            "This idempotency key was used for a different request".into(),
        ));
    }
    if !r.try_get::<bool, _>("valid").map_err(db_error)? {
        return Err(Error::Conflict(
            "The retry window expired. Reconcile the original result before trying again".into(),
        ));
    }
    r.try_get::<Option<Value>, _>("response_body")
        .map_err(db_error)?
        .map(|v| serde_json::from_value(v).map_err(|_| Error::Internal))
        .transpose()
}
pub(crate) async fn save_retry<T: Serialize>(
    tx: &mut store::Tx,
    w: Uuid,
    u: Uuid,
    route: &str,
    key: &str,
    item: &T,
) -> Result<()> {
    sqlx::query("UPDATE idempotency_records SET response_body=$5 WHERE workspace_id=$1 AND principal_id=$2 AND route=$3 AND key_hash=$4").bind(w).bind(u).bind(route).bind(key).bind(serde_json::to_value(item).map_err(|_|Error::Internal)?).execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}
pub(crate) async fn seed_team(
    tx: &mut store::Tx,
    w: Uuid,
    t: Uuid,
    u: Uuid,
    name: &str,
    key: &str,
    private: bool,
) -> Result<()> {
    sqlx::query("INSERT INTO teams(id,workspace_id,name,key,private) VALUES($1,$2,$3,$4,$5)")
        .bind(t)
        .bind(w)
        .bind(name)
        .bind(key)
        .bind(private)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    sqlx::query("INSERT INTO team_prefixes(workspace_id,prefix,team_id) VALUES($1,$2,$3)")
        .bind(w)
        .bind(key)
        .bind(t)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    sqlx::query(
        "INSERT INTO team_members(workspace_id,team_id,user_id,role) VALUES($1,$2,$3,'maintainer')",
    )
    .bind(w)
    .bind(t)
    .bind(u)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    for (position, (name, category, color)) in [
        ("Backlog", "backlog", "gray"),
        ("Todo", "unstarted", "slate"),
        ("In progress", "started", "amber"),
        ("Done", "completed", "green"),
        ("Canceled", "canceled", "gray"),
    ]
    .iter()
    .enumerate()
    {
        sqlx::query("INSERT INTO workflow_statuses(id,workspace_id,team_id,name,category,color,position,is_default) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(Uuid::new_v4()).bind(w).bind(t).bind(name).bind(category).bind(color).bind(position as i32).bind(position==0).execute(&mut **tx).await.map_err(db_error)?;
    }
    Ok(())
}
pub(crate) async fn directory(
    tx: &mut store::Tx,
    actor: &Actor,
    w: Uuid,
    scope: &str,
    q: &DirectoryQuery,
    states: &[&str],
) -> Result<(u32, String, Option<Uuid>, String)> {
    let limit = q.limit.unwrap_or(50);
    let state = q.state.clone().unwrap_or_else(|| states[0].into());
    if !(1..=100).contains(&limit) || !states.contains(&state.as_str()) {
        return Err(Error::Invalid(
            "Invalid directory state or page size".into(),
        ));
    }
    let epoch = store::auth_epoch(tx, w).await.map_err(db_error)?;
    let binding =
        digest(format!("{w}:{}:{scope}:{state}:{limit}:{epoch}", actor.user.id).as_bytes());
    let after = if let Some(c) = &q.cursor {
        let (id, b) = c
            .split_once('|')
            .ok_or_else(|| Error::Invalid("Invalid page cursor".into()))?;
        if b != binding {
            return Err(Error::Invalid(
                "This page cursor expired or belongs to another query".into(),
            ));
        }
        Some(
            id.parse()
                .map_err(|_| Error::Invalid("Invalid page cursor".into()))?,
        )
    } else {
        None
    };
    Ok((limit, state, after, binding))
}
pub(crate) fn page<T>(
    mut data: Vec<T>,
    mut ids: Vec<Uuid>,
    limit: u32,
    binding: String,
) -> Page<T> {
    let has_more = data.len() > limit as usize;
    data.truncate(limit as usize);
    ids.truncate(limit as usize);
    let next_cursor = if has_more {
        ids.last().map(|id| format!("{id}|{binding}"))
    } else {
        None
    };
    Page {
        data,
        next_cursor,
        has_more,
    }
}
impl App {
    /// Acquire the exclusive access guard directly; never upgrade a shared lock.
    pub(crate) async fn identity_scope(
        &self,
        actor: &Actor,
        w: Uuid,
        allow_archived: bool,
    ) -> Result<(store::Tx, String)> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, actor.user.id, Some(w))
            .await
            .map_err(db_error)?;
        Self::finish_identity_scope(tx, actor, w, allow_archived).await
    }
    async fn identity_scope_for_member(
        &self,
        actor: &Actor,
        w: Uuid,
        target: Uuid,
    ) -> Result<(store::Tx, String)> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context_access_change(&mut tx, actor.user.id, target, w)
            .await
            .map_err(db_error)?;
        Self::finish_identity_scope(tx, actor, w, false).await
    }
    async fn finish_identity_scope(
        mut tx: store::Tx,
        actor: &Actor,
        w: Uuid,
        allow_archived: bool,
    ) -> Result<(store::Tx, String)> {
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
        let detail = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !allow_archived && detail.archived_at.is_some() {
            return Err(Error::Conflict(
                "Restore the workspace before making changes".into(),
            ));
        }
        recent(&mut tx, actor).await?;
        Ok((tx, detail.role))
    }
    pub async fn organizations(&self, actor: &Actor) -> Result<Vec<OrganizationSummary>> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, actor.user.id, None)
            .await
            .map_err(db_error)?;
        if !store::active_session(&mut tx, actor)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Unauthorized);
        }
        let rows=sqlx::query("SELECT o.id,o.name,m.role FROM organizations o JOIN organization_members m ON m.organization_id=o.id WHERE m.user_id=$1 ORDER BY o.name,o.id LIMIT 100").bind(actor.user.id).fetch_all(&mut *tx).await.map_err(db_error)?;
        let result = rows
            .into_iter()
            .map(|r| {
                Ok(OrganizationSummary {
                    id: r.try_get("id").map_err(db_error)?,
                    name: r.try_get("name").map_err(db_error)?,
                    role: r.try_get("role").map_err(db_error)?,
                })
            })
            .collect::<Result<_>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn create_workspace(
        &self,
        actor: &Actor,
        key: &str,
        mut req: CreateWorkspace,
    ) -> Result<WorkspaceDetail> {
        req.name = bounded_text(&req.name, "Workspace name", 100)?;
        req.team_name = bounded_text(&req.team_name, "Team name", 100)?;
        req.team_key = validate_team_key(&req.team_key)?;
        let (kh, rh) = request_hash(key, &req)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, actor.user.id, None)
            .await
            .map_err(db_error)?;
        recent(&mut tx, actor).await?;
        // Organization creation authority does not confer membership in any workspace.
        let org:Option<Uuid>=sqlx::query_scalar("SELECT o.id FROM organizations o JOIN organization_members m ON m.organization_id=o.id WHERE o.id=$1 AND m.user_id=$2 AND m.role IN ('owner','admin') FOR UPDATE OF o").bind(req.organization_id).bind(actor.user.id).fetch_optional(&mut *tx).await.map_err(db_error)?;
        if org.is_none() {
            return Err(Error::NotFound);
        }
        sqlx::query("INSERT INTO organization_idempotency_records(organization_id,principal_id,route,key_hash,request_hash) VALUES($1,$2,'workspaces.create',$3,$4) ON CONFLICT DO NOTHING").bind(req.organization_id).bind(actor.user.id).bind(&kh).bind(&rh).execute(&mut *tx).await.map_err(db_error)?;
        let row=sqlx::query("SELECT request_hash,response_body,expires_at>now() AS valid FROM organization_idempotency_records WHERE organization_id=$1 AND principal_id=$2 AND route='workspaces.create' AND key_hash=$3 FOR UPDATE").bind(req.organization_id).bind(actor.user.id).bind(&kh).fetch_one(&mut *tx).await.map_err(db_error)?;
        if let Some(item) = retry_body::<WorkspaceDetail>(row, &rh)? {
            store::context(&mut tx, actor.user.id, Some(item.id))
                .await
                .map_err(db_error)?;
            sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR SHARE")
                .bind(item.id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
            store::workspace_detail(&mut tx, item.id, actor.user.id)
                .await
                .map_err(db_error)?
                .ok_or(Error::NotFound)?;
            tx.commit().await.map_err(db_error)?;
            return Ok(item);
        }
        let w = Uuid::new_v4();
        let t = Uuid::new_v4();
        store::context(&mut tx, actor.user.id, Some(w))
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,$3,$4)")
            .bind(w)
            .bind(req.organization_id)
            .bind(req.name)
            .bind(format!("workspace-{w}"))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query(
            "INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'owner')",
        )
        .bind(w)
        .bind(actor.user.id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("INSERT INTO workspace_auth_guards(workspace_id) VALUES($1)")
            .bind(w)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO workspace_stream_heads(workspace_id) VALUES($1)")
            .bind(w)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        seed_team(
            &mut tx,
            w,
            t,
            actor.user.id,
            &req.team_name,
            &req.team_key,
            false,
        )
        .await?;
        let item = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        sqlx::query("UPDATE organization_idempotency_records SET response_body=$4 WHERE organization_id=$1 AND principal_id=$2 AND route='workspaces.create' AND key_hash=$3").bind(req.organization_id).bind(actor.user.id).bind(kh).bind(serde_json::to_value(&item).map_err(|_|Error::Internal)?).execute(&mut *tx).await.map_err(db_error)?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: w,
                version: 1,
                team: None,
                action: "workspace.created",
                access_changed: false,
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn workspace(&self, actor: &Actor, w: Uuid) -> Result<WorkspaceDetail> {
        let mut tx = self.scoped(actor, w, false).await?;
        let item = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn update_workspace(
        &self,
        actor: &Actor,
        w: Uuid,
        etag: &str,
        req: UpdateWorkspace,
    ) -> Result<WorkspaceDetail> {
        if req.name.is_none() && req.archived.is_none() {
            return Err(Error::Invalid("Provide a field to update".into()));
        }
        let name = req
            .name
            .as_ref()
            .map(|s| bounded_text(s, "Workspace name", 100))
            .transpose()?;
        let (mut tx, role) = self.identity_scope(actor, w, true).await?;
        if !manager(&role) {
            return Err(Error::Forbidden);
        }
        let old = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        version(etag, &old.etag(), &old)?;
        if old.archived_at.is_some() && !(req.archived == Some(false) && req.name.is_none()) {
            return Err(Error::Conflict(
                "Restore the workspace before making changes".into(),
            ));
        }
        sqlx::query("UPDATE workspaces SET name=COALESCE($2,name),archived_at=CASE WHEN $3::boolean IS NULL THEN archived_at WHEN $3 THEN COALESCE(archived_at,now()) ELSE NULL END,version=version+1 WHERE id=$1").bind(w).bind(name).bind(req.archived).execute(&mut *tx).await.map_err(db_error)?;
        let item = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: w,
                version: item.version.parse().map_err(|_| Error::Internal)?,
                team: None,
                action: "workspace.updated",
                access_changed: req.archived.is_some(),
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn list_teams(
        &self,
        actor: &Actor,
        w: Uuid,
        mut q: TeamQuery,
    ) -> Result<Page<TeamDetail>> {
        let mut tx = self.scoped(actor, w, false).await?;
        let limit = q.limit.unwrap_or(50);
        let state = q.state.as_deref().unwrap_or("active").to_owned();
        let term = q.q.as_deref().unwrap_or("").trim().to_owned();
        if !(1..=100).contains(&limit) || !["active", "archived", "all"].contains(&state.as_str()) {
            return Err(Error::Invalid(
                "Invalid directory state or page size".into(),
            ));
        }
        if term.chars().count() > 100 || term.chars().any(char::is_control) {
            return Err(Error::Invalid(
                "Team search must contain at most 100 characters without control characters".into(),
            ));
        }
        let cursor = q.cursor.take();
        q.q = Some(term.clone());
        q.limit = Some(limit);
        q.state = Some(state.clone());
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{}:{epoch}:{}",
            actor.user.id,
            serde_json::to_string(&q).map_err(|_| Error::Internal)?
        );
        let after: Option<Uuid> = cursor
            .as_deref()
            .map(|value| self.taxonomy_cursor_decode("teams-v1", &binding, value))
            .transpose()?;
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT t.id FROM teams t WHERE t.workspace_id=$1 AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=t.id AND tm.user_id=$2)) AND ($3='all' OR (t.archived_at IS NOT NULL)=($3='archived')) AND ($4::uuid IS NULL OR t.id>$4) AND (strpos(lower(t.name),lower($6))>0 OR strpos(lower(t.key),lower($6))>0) ORDER BY t.id LIMIT $5").bind(w).bind(actor.user.id).bind(state).bind(after).bind(i64::from(limit)+1).bind(term).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = ids.len() > limit as usize;
        let next_cursor = if has_more {
            Some(self.taxonomy_cursor_encode("teams-v1", &binding, &ids[limit as usize - 1])?)
        } else {
            None
        };
        let mut data = Vec::new();
        for id in ids.iter().take(limit as usize) {
            data.push(
                store::team_detail(&mut tx, w, *id, actor.user.id)
                    .await
                    .map_err(db_error)?
                    .ok_or(Error::Internal)?,
            );
        }
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
    pub async fn create_team(
        &self,
        actor: &Actor,
        w: Uuid,
        key: &str,
        mut req: CreateTeam,
    ) -> Result<TeamDetail> {
        req.name = bounded_text(&req.name, "Team name", 100)?;
        req.key = validate_team_key(&req.key)?;
        let (kh, rh) = request_hash(key, &req)?;
        let (mut tx, role) = self.identity_scope(actor, w, false).await?;
        if !manager(&role) {
            return Err(Error::Forbidden);
        }
        if let Some(item) =
            retry::<TeamDetail>(&mut tx, w, actor.user.id, "teams.create", &kh, &rh).await?
        {
            store::team_detail(&mut tx, w, item.id, actor.user.id)
                .await
                .map_err(db_error)?
                .ok_or(Error::NotFound)?;
            tx.commit().await.map_err(db_error)?;
            return Ok(item);
        }
        let id = Uuid::new_v4();
        seed_team(
            &mut tx,
            w,
            id,
            actor.user.id,
            &req.name,
            &req.key,
            req.private,
        )
        .await?;
        let item = store::team_detail(&mut tx, w, id, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        save_retry(&mut tx, w, actor.user.id, "teams.create", &kh, &item).await?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: id,
                version: 1,
                team: Some(id),
                action: "team.created",
                access_changed: true,
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn update_team(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        etag: &str,
        req: UpdateTeam,
    ) -> Result<TeamDetail> {
        if req.name.is_none() && req.private.is_none() && req.archived.is_none() {
            return Err(Error::Invalid("Provide a field to update".into()));
        }
        let name = req
            .name
            .as_ref()
            .map(|s| bounded_text(s, "Team name", 100))
            .transpose()?;
        let (mut tx, _) = self.identity_scope(actor, w, false).await?;
        let old = store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !old.can_manage {
            return Err(Error::Forbidden);
        }
        version(etag, &old.etag(), &old)?;
        if old.archived_at.is_some()
            && !(req.archived == Some(false) && req.name.is_none() && req.private.is_none())
        {
            return Err(Error::Conflict(
                "Restore the team before making changes".into(),
            ));
        }
        if req.private == Some(true) && !old.private {
            // The actor consciously makes a public team private and must retain a
            // real maintainer grant; no workspace administrator bypass exists.
            sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id,role,version) VALUES($1,$2,$3,'maintainer',(SELECT epoch+1 FROM workspace_auth_guards WHERE workspace_id=$1)) ON CONFLICT(workspace_id,team_id,user_id) DO UPDATE SET role='maintainer',version=team_members.version+1").bind(w).bind(t).bind(actor.user.id).execute(&mut *tx).await.map_err(db_error)?;
        }
        sqlx::query("UPDATE teams SET name=COALESCE($3,name),private=COALESCE($4,private),archived_at=CASE WHEN $5::boolean IS NULL THEN archived_at WHEN $5 THEN COALESCE(archived_at,now()) ELSE NULL END,version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(t).bind(name).bind(req.private).bind(req.archived).execute(&mut *tx).await.map_err(db_error)?;
        let item = store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: t,
                version: item.version.parse().map_err(|_| Error::Internal)?,
                team: Some(t),
                action: "team.updated",
                access_changed: req.private.is_some() || req.archived.is_some(),
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn workspace_members(
        &self,
        actor: &Actor,
        w: Uuid,
        q: DirectoryQuery,
    ) -> Result<Page<WorkspaceMember>> {
        let mut tx = self.scoped(actor, w, false).await?;
        let (limit, state, after, binding) = directory(
            &mut tx,
            actor,
            w,
            "members",
            &q,
            &["active", "inactive", "all"],
        )
        .await?;
        let own = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if state != "active" && !manager(&own.role) {
            return Err(Error::Forbidden);
        }
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT user_id FROM workspace_members WHERE workspace_id=$1 AND ($2='all' OR active=($2='active')) AND ($3::uuid IS NULL OR user_id>$3) ORDER BY user_id LIMIT $4").bind(w).bind(state).bind(after).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut data = Vec::new();
        for id in &ids {
            data.push(
                store::workspace_member(&mut tx, w, *id)
                    .await
                    .map_err(db_error)?
                    .ok_or(Error::Internal)?,
            );
        }
        tx.commit().await.map_err(db_error)?;
        Ok(page(data, ids, limit, binding))
    }
    pub async fn update_member(
        &self,
        actor: &Actor,
        w: Uuid,
        u: Uuid,
        etag: &str,
        req: UpdateMember,
    ) -> Result<AccessMutation<WorkspaceMember>> {
        if req.role.is_none() && req.active.is_none() {
            return Err(Error::Invalid("Provide a field to update".into()));
        }
        if req
            .role
            .as_ref()
            .is_some_and(|r| !matches!(r.as_str(), "owner" | "admin" | "member" | "viewer"))
        {
            return Err(Error::Invalid("Invalid workspace role".into()));
        }
        let (mut tx, role) = self.identity_scope_for_member(actor, w, u).await?;
        if !manager(&role) {
            return Err(Error::Forbidden);
        }
        let old = store::workspace_member(&mut tx, w, u)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if role != "owner" && (old.role == "owner" || req.role.as_deref() == Some("owner")) {
            return Err(Error::Forbidden);
        }
        version(etag, &old.etag(w), &old)?;
        require_enabled_user(&mut tx, u).await?;
        let next_role = req.role.as_deref().unwrap_or(&old.role);
        let active = req.active.unwrap_or(old.active);
        let elevated = workspace_access_increased(&old, next_role, active);
        if old.role == "owner" && old.active && (next_role != "owner" || !active) {
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND m.active AND m.role='owner' AND u.disabled_at IS NULL").bind(w).fetch_one(&mut *tx).await.map_err(db_error)?;
            if count <= 1 {
                return Err(Error::Conflict(
                    "Assign another enabled active owner first".into(),
                ));
            }
        }
        sqlx::query("UPDATE workspace_members SET role=$3,active=$4,version=version+1 WHERE workspace_id=$1 AND user_id=$2").bind(w).bind(u).bind(next_role).bind(active).execute(&mut *tx).await.map_err(db_error)?;
        if !active {
            sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND user_id=$2")
                .bind(w)
                .bind(u)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        let item = store::workspace_member(&mut tx, w, u)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        let correlation = Uuid::new_v4();
        let current_session_revoked = if elevated {
            crate::sessions::revoke_for_elevation(&mut tx, actor, u, None, correlation).await?
        } else {
            false
        };
        store::identity_event_correlated(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: u,
                version: item.version.parse().map_err(|_| Error::Internal)?,
                team: None,
                action: "workspace.member.updated",
                access_changed: true,
            },
            correlation,
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(AccessMutation {
            entity: item,
            current_session_revoked,
        })
    }
    pub async fn team_members(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        q: DirectoryQuery,
    ) -> Result<Page<TeamMember>> {
        let mut tx = self.scoped(actor, w, false).await?;
        store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let (limit, _, after, binding) = directory(
            &mut tx,
            actor,
            w,
            &format!("team-members:{t}"),
            &q,
            &["active"],
        )
        .await?;
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT tm.user_id FROM team_members tm JOIN workspace_members wm ON wm.workspace_id=tm.workspace_id AND wm.user_id=tm.user_id AND wm.active WHERE tm.workspace_id=$1 AND tm.team_id=$2 AND ($3::uuid IS NULL OR tm.user_id>$3) ORDER BY tm.user_id LIMIT $4").bind(w).bind(t).bind(after).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut data = Vec::new();
        for id in &ids {
            data.push(
                store::team_member(&mut tx, w, t, *id)
                    .await
                    .map_err(db_error)?
                    .ok_or(Error::Internal)?,
            );
        }
        tx.commit().await.map_err(db_error)?;
        Ok(page(data, ids, limit, binding))
    }
    pub async fn put_team_member(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        u: Uuid,
        etag: Option<&str>,
        req: PutTeamMember,
    ) -> Result<AccessMutation<TeamMember>> {
        if !matches!(req.role.as_str(), "maintainer" | "contributor") {
            return Err(Error::Invalid("Invalid team role".into()));
        }
        let (mut tx, _) = self.identity_scope_for_member(actor, w, u).await?;
        let team = store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !team.can_manage {
            return Err(Error::Forbidden);
        }
        require_active_team(&mut tx, w, t).await?;
        let target = store::workspace_member(&mut tx, w, u)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !target.active {
            return Err(Error::Invalid(
                "Reactivate this workspace member first".into(),
            ));
        }
        if target.role == "viewer" && req.role == "maintainer" {
            return Err(Error::Invalid("A viewer cannot maintain a team".into()));
        }
        let old = store::team_member(&mut tx, w, t, u)
            .await
            .map_err(db_error)?;
        match (&old, etag) {
            (Some(item), Some(e)) => version(e, &item.etag(t), item)?,
            (Some(item), None) => {
                return Err(Error::VersionConflict(
                    serde_json::to_value(item).map_err(|_| Error::Internal)?,
                ));
            }
            (None, Some(_)) => return Err(Error::NotFound),
            (None, None) => {}
        }
        require_enabled_user(&mut tx, u).await?;
        let elevated = (old.is_none() && team.private)
            || (req.role == "maintainer"
                && old
                    .as_ref()
                    .is_none_or(|member| member.role != "maintainer"));
        sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id,role,version) VALUES($1,$2,$3,$4,(SELECT epoch+1 FROM workspace_auth_guards WHERE workspace_id=$1)) ON CONFLICT(workspace_id,team_id,user_id) DO UPDATE SET role=excluded.role,version=team_members.version+1").bind(w).bind(t).bind(u).bind(req.role).execute(&mut *tx).await.map_err(db_error)?;
        let item = store::team_member(&mut tx, w, t, u)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        let correlation = Uuid::new_v4();
        let current_session_revoked = if elevated {
            crate::sessions::revoke_for_elevation(&mut tx, actor, u, None, correlation).await?
        } else {
            false
        };
        store::identity_event_correlated(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: u,
                version: item.version.parse().map_err(|_| Error::Internal)?,
                team: Some(t),
                action: "team.member.updated",
                access_changed: true,
            },
            correlation,
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(AccessMutation {
            entity: item,
            current_session_revoked,
        })
    }
    pub async fn remove_team_member(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        u: Uuid,
        etag: &str,
    ) -> Result<()> {
        let (mut tx, _) = self.identity_scope(actor, w, false).await?;
        let team = store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !team.can_manage {
            return Err(Error::Forbidden);
        }
        require_active_team(&mut tx, w, t).await?;
        let old = store::team_member(&mut tx, w, t, u)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        version(etag, &old.etag(t), &old)?;
        sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
            .bind(w)
            .bind(t)
            .bind(u)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: u,
                version: old.version.parse::<i64>().map_err(|_| Error::Internal)? + 1,
                team: Some(t),
                action: "team.member.removed",
                access_changed: true,
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }
}

impl App {
    pub async fn get_team(&self, actor: &Actor, w: Uuid, t: Uuid) -> Result<TeamDetail> {
        let mut tx = self.scoped(actor, w, false).await?;
        let item = store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn get_workspace_member(
        &self,
        actor: &Actor,
        w: Uuid,
        u: Uuid,
    ) -> Result<WorkspaceMember> {
        let mut tx = self.scoped(actor, w, false).await?;
        let own = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let item = store::workspace_member(&mut tx, w, u)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !item.active && !manager(&own.role) {
            return Err(Error::NotFound);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn get_team_member(
        &self,
        actor: &Actor,
        w: Uuid,
        t: Uuid,
        u: Uuid,
    ) -> Result<TeamMember> {
        let mut tx = self.scoped(actor, w, false).await?;
        store::team_detail(&mut tx, w, t, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let item = store::team_member(&mut tx, w, t, u)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
}

impl App {
    pub async fn workspace_access(&self, actor: &Actor, w: Uuid) -> Result<WorkspaceAccess> {
        let mut tx = self.scoped(actor, w, false).await?;
        let workspace = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let auth_epoch = store::auth_epoch(&mut tx, w)
            .await
            .map_err(db_error)?
            .to_string();
        tx.commit().await.map_err(db_error)?;
        Ok(WorkspaceAccess {
            auth_epoch,
            role: workspace.role,
            archived: workspace.archived_at.is_some(),
        })
    }
}
