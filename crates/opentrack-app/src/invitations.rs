use super::lifecycle::*;
use super::*;
use opentrack_domain::*;
use sqlx::Row;

fn token_workspace(token: &str) -> Result<Uuid> {
    let (w, s) = token.split_once('.').ok_or(Error::NotFound)?;
    if s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::NotFound);
    }
    w.parse().map_err(|_| Error::NotFound)
}
async fn token_invitation(tx: &mut store::Tx, w: Uuid, token: &str) -> Result<Invitation> {
    let id:Option<Uuid>=sqlx::query_scalar("SELECT i.id FROM invitations i JOIN workspaces w ON w.id=i.workspace_id WHERE i.workspace_id=$1 AND i.token_hash=$2 AND i.revoked_at IS NULL AND i.accepted_at IS NULL AND i.expires_at>clock_timestamp() AND w.archived_at IS NULL").bind(w).bind(digest(token.as_bytes())).fetch_optional(&mut **tx).await.map_err(db_error)?;
    store::invitation(tx, w, id.ok_or(Error::NotFound)?)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)
}
async fn visible_grants(
    tx: &mut store::Tx,
    w: Uuid,
    user: Uuid,
    item: &Invitation,
    manage: bool,
) -> Result<()> {
    for t in &item.team_ids {
        let team = store::team_detail(tx, w, *t, user)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if manage && (!team.can_manage || team.archived_at.is_some()) {
            return Err(Error::Forbidden);
        }
    }
    Ok(())
}
async fn valid_inviter(tx: &mut store::Tx, item: &Invitation) -> Result<Uuid> {
    let inviter: Uuid =
        sqlx::query_scalar("SELECT invited_by FROM invitations WHERE workspace_id=$1 AND id=$2")
            .bind(item.workspace_id)
            .bind(item.id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    let member = store::workspace_member(tx, item.workspace_id, inviter)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    let enabled: bool = sqlx::query_scalar("SELECT disabled_at IS NULL FROM users WHERE id=$1")
        .bind(inviter)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    if !member.active || !manager(&member.role) || !enabled {
        return Err(Error::NotFound);
    }
    visible_grants(tx, item.workspace_id, inviter, item, true).await?;
    Ok(inviter)
}
impl App {
    pub async fn invitations(
        &self,
        actor: &Actor,
        w: Uuid,
        q: DirectoryQuery,
    ) -> Result<Page<Invitation>> {
        let mut tx = self.scoped(actor, w, false).await?;
        let own = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !manager(&own.role) {
            return Err(Error::Forbidden);
        }
        let (limit, state, after, binding) = directory(
            &mut tx,
            actor,
            w,
            "invitations",
            &q,
            &["pending", "accepted", "revoked", "expired", "all"],
        )
        .await?;
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT i.id FROM invitations i WHERE i.workspace_id=$1 AND ($2='all' OR ($2='pending' AND i.accepted_at IS NULL AND i.revoked_at IS NULL AND i.expires_at>clock_timestamp()) OR ($2='accepted' AND i.accepted_at IS NOT NULL) OR ($2='revoked' AND i.revoked_at IS NOT NULL) OR ($2='expired' AND i.accepted_at IS NULL AND i.revoked_at IS NULL AND i.expires_at<=clock_timestamp())) AND ($3::uuid IS NULL OR i.id>$3) AND NOT EXISTS(SELECT 1 FROM invitation_teams it JOIN teams t ON t.workspace_id=it.workspace_id AND t.id=it.team_id WHERE it.workspace_id=i.workspace_id AND it.invitation_id=i.id AND t.private AND NOT EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$4)) ORDER BY i.id LIMIT $5").bind(w).bind(state).bind(after).bind(actor.user.id).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut data = Vec::new();
        for id in &ids {
            data.push(
                store::invitation(&mut tx, w, *id)
                    .await
                    .map_err(db_error)?
                    .ok_or(Error::Internal)?,
            );
        }
        tx.commit().await.map_err(db_error)?;
        Ok(page(data, ids, limit, binding))
    }
    pub async fn get_invitation(&self, actor: &Actor, w: Uuid, id: Uuid) -> Result<Invitation> {
        let mut tx = self.scoped(actor, w, false).await?;
        let own = store::workspace_detail(&mut tx, w, actor.user.id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        if !manager(&own.role) {
            return Err(Error::Forbidden);
        }
        let item = store::invitation(&mut tx, w, id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        visible_grants(&mut tx, w, actor.user.id, &item, false).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn create_invitation(
        &self,
        actor: &Actor,
        w: Uuid,
        key: &str,
        mut req: CreateInvitation,
    ) -> Result<InvitationIssued> {
        req.email = normalize_email(&req.email)?;
        if !matches!(req.role.as_str(), "admin" | "member" | "viewer") {
            return Err(Error::Invalid("Invite an admin, member, or viewer".into()));
        }
        if req.team_ids.len() > 100 {
            return Err(Error::Invalid(
                "An invitation can grant at most 100 teams".into(),
            ));
        }
        req.team_ids.sort();
        req.team_ids.dedup();
        let (kh, rh) = request_hash(key, &req)?;
        let (mut tx, role) = self.identity_scope(actor, w, false).await?;
        if !manager(&role) {
            return Err(Error::Forbidden);
        }
        for t in &req.team_ids {
            let team = store::team_detail(&mut tx, w, *t, actor.user.id)
                .await
                .map_err(db_error)?
                .ok_or(Error::NotFound)?;
            if !team.can_manage {
                return Err(Error::Forbidden);
            }
            require_active_team(&mut tx, w, *t).await?;
        }
        if let Some(item) =
            retry::<Invitation>(&mut tx, w, actor.user.id, "invitations.create", &kh, &rh).await?
        {
            visible_grants(&mut tx, w, actor.user.id, &item, false).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(InvitationIssued {
                invitation: item,
                token: None,
            });
        }
        if let Some(row)=sqlx::query("SELECT m.active,m.role FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND u.email=$2").bind(w).bind(&req.email).fetch_optional(&mut *tx).await.map_err(db_error)?{
            if row.try_get::<bool,_>("active").map_err(db_error)?{return Err(Error::Conflict("This person is already an active member".into()));}
            if role!="owner"&&row.try_get::<String,_>("role").map_err(db_error)?=="owner"{return Err(Error::Forbidden);}
        }
        let email_hash = digest(req.email.as_bytes());
        let retired: Vec<Uuid> = sqlx::query_scalar("UPDATE invitations SET revoked_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND email_hash=$2 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at<=clock_timestamp() RETURNING id")
            .bind(w).bind(&email_hash).fetch_all(&mut *tx).await.map_err(db_error)?;
        for retired_id in retired {
            // An expired invitation may contain private grants this actor cannot
            // inspect. Retire it in restricted audit only: no old IDs or grant
            // metadata enter the response or workspace stream, and no usable
            // access changed, so this does not advance the authorization epoch.
            store::identity_audit(
                &mut tx,
                w,
                actor.user.id,
                retired_id,
                "invitation.expired_replaced",
                Uuid::new_v4(),
            )
            .await
            .map_err(db_error)?;
        }
        let id = Uuid::new_v4();
        let token = format!("{w}.{}", random_secret());
        sqlx::query("INSERT INTO invitations(id,workspace_id,email,email_hash,role,token_hash,invited_by,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,clock_timestamp()+interval '48 hours')").bind(id).bind(w).bind(req.email).bind(email_hash).bind(req.role).bind(digest(token.as_bytes())).bind(actor.user.id).execute(&mut *tx).await.map_err(db_error)?;
        for t in req.team_ids {
            sqlx::query(
                "INSERT INTO invitation_teams(workspace_id,invitation_id,team_id) VALUES($1,$2,$3)",
            )
            .bind(w)
            .bind(id)
            .bind(t)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        }
        let item = store::invitation(&mut tx, w, id)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        // Persist metadata only. A raw bearer secret must never enter a retry record.
        save_retry(&mut tx, w, actor.user.id, "invitations.create", &kh, &item).await?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: id,
                version: 1,
                team: None,
                action: "invitation.created",
                access_changed: false,
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(InvitationIssued {
            invitation: item,
            token: Some(token),
        })
    }
    pub async fn revoke_invitation(
        &self,
        actor: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
    ) -> Result<Invitation> {
        let (mut tx, role) = self.identity_scope(actor, w, false).await?;
        if !manager(&role) {
            return Err(Error::Forbidden);
        }
        let old = store::invitation(&mut tx, w, id)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        visible_grants(&mut tx, w, actor.user.id, &old, false).await?;
        version(etag, &old.etag(), &old)?;
        if old.accepted_at.is_some() {
            return Err(Error::Conflict(
                "This invitation was already accepted; manage the member's access instead".into(),
            ));
        }
        sqlx::query("UPDATE invitations SET revoked_at=COALESCE(revoked_at,clock_timestamp()),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.map_err(db_error)?;
        let item = store::invitation(&mut tx, w, id)
            .await
            .map_err(db_error)?
            .ok_or(Error::Internal)?;
        store::identity_event(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: actor.user.id,
                entity: id,
                version: item.version.parse().map_err(|_| Error::Internal)?,
                team: None,
                action: "invitation.revoked",
                access_changed: false,
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn invitation_preview(&self, token: &str) -> Result<InvitationPreview> {
        let w = token_workspace(token)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, Uuid::nil(), Some(w))
            .await
            .map_err(db_error)?;
        sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR SHARE")
            .bind(w)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
        let item = token_invitation(&mut tx, w, token).await?;
        valid_inviter(&mut tx, &item).await?;
        let name: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id=$1")
            .bind(w)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(InvitationPreview {
            workspace_id: w,
            workspace_name: name,
            email: item.email,
            role: item.role,
            expires_at: item.expires_at,
        })
    }
    pub async fn accept_invitation(
        &self,
        actor: Option<&Actor>,
        req: AcceptInvitation,
    ) -> Result<(Session, String)> {
        if actor.is_some_and(|actor| actor.audience != SessionAudience::Web) {
            return Err(Error::Forbidden);
        }
        let w = token_workspace(&req.token)?;
        // Inspect the token before spending password-hash capacity, then release all
        // locks. Every authority and expiry check is repeated in the write transaction.
        let preview = self.invitation_preview(&req.token).await?;
        if let Some(actor) = actor
            && actor.user.email != preview.email
        {
            return Err(Error::Forbidden);
        }
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email=$1)")
            .bind(&preview.email)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        let new_account = if actor.is_none() {
            if exists {
                return Err(Error::InviteSignInRequired);
            }
            let name = bounded_text(req.display_name.as_deref().unwrap_or(""), "Name", 100)?;
            let password = req
                .password
                .ok_or_else(|| Error::Invalid("Choose a password for your new account".into()))?;
            validate_password(&password)?;
            Some((
                Uuid::new_v4(),
                name,
                self.password_work(password, None).await?,
            ))
        } else {
            None
        };
        let user_id = actor
            .map(|a| a.user.id)
            .or_else(|| new_account.as_ref().map(|v| v.0))
            .ok_or(Error::Internal)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        // Existing-account acceptance rotates its current session. Take the
        // exclusive account guard before email/workspace locks, without upgrade.
        store::context_exclusive(&mut tx, user_id, Some(w))
            .await
            .map_err(db_error)?;
        // Serializes concurrent new-account acceptances across different workspaces.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,734516))")
            .bind(&preview.email)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE")
            .bind(w)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(Error::NotFound)?;
        let item = token_invitation(&mut tx, w, &req.token).await?;
        let inviter = valid_inviter(&mut tx, &item).await?;
        let user = if let Some(actor) = actor {
            recent(&mut tx, actor).await?;
            let current_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
                .bind(actor.user.id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_error)?;
            if current_email != item.email {
                return Err(Error::Forbidden);
            }
            actor.user.clone()
        } else {
            if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE email=$1)")
                .bind(&item.email)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_error)?
            {
                return Err(Error::InviteSignInRequired);
            }
            let (id, name, hash) = new_account.ok_or(Error::Internal)?;
            sqlx::query(
                "INSERT INTO users(id,email,display_name,password_hash) VALUES($1,$2,$3,$4)",
            )
            .bind(id)
            .bind(&item.email)
            .bind(&name)
            .bind(hash)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
            User {
                id,
                email: item.email.clone(),
                display_name: name,
            }
        };
        if let Some(member) = store::workspace_member(&mut tx, w, user.id)
            .await
            .map_err(db_error)?
        {
            if member.active {
                return Err(Error::Conflict(
                    "This account is already an active member".into(),
                ));
            }
            let inviter_role = store::workspace_member(&mut tx, w, inviter)
                .await
                .map_err(db_error)?
                .ok_or(Error::NotFound)?
                .role;
            if member.role == "owner" && inviter_role != "owner" {
                return Err(Error::Forbidden);
            }
            // Clear every previous team grant, including grants left by older versions.
            sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND user_id=$2")
                .bind(w)
                .bind(user.id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role,active) VALUES($1,$2,$3,true) ON CONFLICT(workspace_id,user_id) DO UPDATE SET role=excluded.role,active=true,version=workspace_members.version+1").bind(w).bind(user.id).bind(&item.role).execute(&mut *tx).await.map_err(db_error)?;
        for t in &item.team_ids {
            sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id,role,version) VALUES($1,$2,$3,'contributor',(SELECT epoch+1 FROM workspace_auth_guards WHERE workspace_id=$1))").bind(w).bind(t).bind(user.id).execute(&mut *tx).await.map_err(db_error)?;
        }
        let expected_version: i64 = item.version.parse().map_err(|_| Error::Internal)?;
        // Recheck the bearer at the consuming write. Time can advance while
        // validating account/membership changes even with the workspace guarded.
        // Zero rows also catches a silently suppressed update; all earlier
        // account and grant changes roll back with this transaction.
        let consumed = sqlx::query("UPDATE invitations SET accepted_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2 AND token_hash=$3 AND version=$4 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at>clock_timestamp()")
            .bind(w).bind(item.id).bind(digest(req.token.as_bytes())).bind(expected_version)
            .execute(&mut *tx).await.map_err(db_error)?;
        if consumed.rows_affected() != 1 {
            return Err(Error::NotFound);
        }
        let correlation = Uuid::new_v4();
        let (new_id, token, csrf) = if let Some(actor) = actor {
            crate::sessions::mint_rotated_session(&mut tx, actor).await?
        } else {
            crate::sessions::mint_session(&mut tx, user.id).await?
        };
        if let Some(actor) = actor {
            crate::sessions::rotate_previous_correlated(
                &mut tx,
                user.id,
                &actor.token_hash,
                new_id,
                "session.invitation_rotated",
                correlation,
            )
            .await?;
            // Acceptance grants workspace access. Only the replacement cookie
            // can carry that access; other pre-existing sessions must sign in.
            crate::sessions::revoke_for_elevation(
                &mut tx,
                actor,
                user.id,
                Some(new_id),
                correlation,
            )
            .await?;
        }
        let workspaces = store::workspaces(&mut tx, user.id)
            .await
            .map_err(db_error)?;
        store::identity_event_correlated(
            &mut tx,
            store::IdentityEvent {
                workspace: w,
                actor: user.id,
                entity: item.id,
                version: expected_version + 1,
                team: None,
                action: "invitation.accepted",
                access_changed: true,
            },
            correlation,
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok((
            Session {
                audience: SessionAudience::Web,
                user,
                csrf_token: csrf,
                workspaces,
            },
            token,
        ))
    }
}
