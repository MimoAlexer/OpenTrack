use super::*;
use opentrack_domain::{Invitation, TeamDetail, TeamMember, WorkspaceDetail, WorkspaceMember};
use serde::de::DeserializeOwned;

pub fn decode<T: DeserializeOwned>(value: Value) -> Result<T, sqlx::Error> {
    serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}
pub async fn auth_epoch(tx: &mut Tx, w: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1")
        .bind(w)
        .fetch_one(&mut **tx)
        .await
}
pub async fn workspace_detail(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
) -> Result<Option<WorkspaceDetail>, sqlx::Error> {
    let body:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',w.id,'organization_id',w.organization_id,'name',w.name,'slug',w.slug,'role',m.role,'version',w.version::text,'archived_at',w.archived_at) FROM workspaces w JOIN workspace_members m ON m.workspace_id=w.id WHERE w.id=$1 AND m.user_id=$2 AND m.active").bind(w).bind(user).fetch_optional(&mut **tx).await?;
    body.map(decode).transpose()
}
pub async fn team_detail(
    tx: &mut Tx,
    w: Uuid,
    t: Uuid,
    user: Uuid,
) -> Result<Option<TeamDetail>, sqlx::Error> {
    let body:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',t.id,'workspace_id',t.workspace_id,'name',t.name,'key',t.key,'private',t.private,'version',t.version::text,'archived_at',t.archived_at,'can_manage',(m.role IN ('owner','admin') OR COALESCE(tm.role='maintainer',false)) AND m.role!='viewer') FROM teams t JOIN workspace_members m ON m.workspace_id=t.workspace_id AND m.user_id=$3 AND m.active LEFT JOIN team_members tm ON tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3 WHERE t.workspace_id=$1 AND t.id=$2 AND (NOT t.private OR tm.user_id IS NOT NULL)").bind(w).bind(t).bind(user).fetch_optional(&mut **tx).await?;
    body.map(decode).transpose()
}
pub async fn workspace_member(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
) -> Result<Option<WorkspaceMember>, sqlx::Error> {
    let body:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('user',jsonb_build_object('id',u.id,'email',u.email,'display_name',u.display_name),'role',m.role,'active',m.active,'version',m.version::text) FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND m.user_id=$2").bind(w).bind(user).fetch_optional(&mut **tx).await?;
    body.map(decode).transpose()
}
pub async fn team_member(
    tx: &mut Tx,
    w: Uuid,
    t: Uuid,
    user: Uuid,
) -> Result<Option<TeamMember>, sqlx::Error> {
    let body:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('user',jsonb_build_object('id',u.id,'email',u.email,'display_name',u.display_name),'role',m.role,'version',m.version::text) FROM team_members m JOIN users u ON u.id=m.user_id JOIN workspace_members wm ON wm.workspace_id=m.workspace_id AND wm.user_id=m.user_id AND wm.active WHERE m.workspace_id=$1 AND m.team_id=$2 AND m.user_id=$3").bind(w).bind(t).bind(user).fetch_optional(&mut **tx).await?;
    body.map(decode).transpose()
}
pub async fn invitation(tx: &mut Tx, w: Uuid, id: Uuid) -> Result<Option<Invitation>, sqlx::Error> {
    let body:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',i.id,'workspace_id',i.workspace_id,'email',i.email,'role',i.role,'team_ids',COALESCE((SELECT jsonb_agg(it.team_id ORDER BY it.team_id) FROM invitation_teams it WHERE it.workspace_id=i.workspace_id AND it.invitation_id=i.id),'[]'::jsonb),'expires_at',i.expires_at,'accepted_at',i.accepted_at,'revoked_at',i.revoked_at,'version',i.version::text) FROM invitations i WHERE i.workspace_id=$1 AND i.id=$2").bind(w).bind(id).fetch_optional(&mut **tx).await?;
    body.map(decode).transpose()
}
pub struct IdentityEvent<'a> {
    pub workspace: Uuid,
    pub actor: Uuid,
    pub entity: Uuid,
    pub version: i64,
    pub team: Option<Uuid>,
    pub action: &'a str,
    pub access_changed: bool,
}
/// Append required restricted security metadata without emitting an event or
/// changing access. The caller holds the applicable identity/workspace guards.
pub async fn identity_audit(
    tx: &mut Tx,
    workspace: Uuid,
    actor: Uuid,
    entity: Uuid,
    action: &str,
    correlation: Uuid,
) -> Result<(), sqlx::Error> {
    let appended = sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id) VALUES($1,$2,$3,$4,$5)")
        .bind(correlation).bind(workspace).bind(actor).bind(action).bind(entity)
        .execute(&mut **tx).await?;
    if appended.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "Required identity audit was not appended".into(),
        ));
    }
    Ok(())
}
pub async fn identity_event(tx: &mut Tx, event: IdentityEvent<'_>) -> Result<(), sqlx::Error> {
    identity_event_correlated(tx, event, Uuid::new_v4()).await
}
/// Link account-scoped consequences to this workspace audit's actual actor.
/// Account security user_id identifies the affected account, not the initiator.
pub async fn identity_event_correlated(
    tx: &mut Tx,
    event: IdentityEvent<'_>,
    correlation: Uuid,
) -> Result<(), sqlx::Error> {
    let IdentityEvent {
        workspace: w,
        actor,
        entity,
        version,
        team,
        action,
        access_changed,
    } = event;
    identity_audit(tx, w, actor, entity, action, correlation).await?;
    let event_version = if access_changed {
        sqlx::query_scalar::<_, i64>(
            "UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1 RETURNING epoch",
        )
        .bind(w)
        .fetch_one(&mut **tx)
        .await?
    } else {
        // Commands that do not advance the epoch still require a real guard.
        // fetch_one also rejects a suppressed epoch update in the branch above.
        sqlx::query_scalar::<_, i64>(
            "SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1",
        )
        .bind(w)
        .fetch_one(&mut **tx)
        .await?;
        version
    };
    let seq:i64=sqlx::query_scalar("UPDATE workspace_stream_heads SET last_seq=last_seq+1 WHERE workspace_id=$1 RETURNING last_seq").bind(w).fetch_one(&mut **tx).await?;
    // Access events carry no private team/member metadata; clients purge and refetch.
    let (kind, stream_team, stream_entity) = if access_changed {
        ("access.changed", None, w)
    } else {
        (action, team, entity)
    };
    let appended = sqlx::query("INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,team_id,entity_id,entity_version,actor_id,mutation_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(w).bind(seq).bind(Uuid::new_v4()).bind(kind).bind(stream_team).bind(stream_entity).bind(event_version).bind(actor).bind(correlation).execute(&mut **tx).await?;
    if appended.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "Required identity event was not appended".into(),
        ));
    }
    Ok(())
}
