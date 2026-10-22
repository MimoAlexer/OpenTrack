mod cycles;
pub use cycles::*;
mod preferences;
pub use preferences::*;
mod deletion;
pub use deletion::*;
mod graph;
pub use graph::*;
mod identity;
mod taxonomy;
pub use taxonomy::*;
mod workflow;
use chrono::{DateTime, Utc};
pub use identity::*;
use opentrack_domain::{Actor, Issue, IssueFilter, SessionAudience, Status, Team, User, Workspace};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row};
use uuid::Uuid;
pub use workflow::*;
mod transaction;
pub use transaction::{Tx, begin};
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn context(tx: &mut Tx, user: Uuid, workspace: Option<Uuid>) -> Result<(), sqlx::Error> {
    context_values(tx, user, workspace).await?;
    // Account guard comes before every workspace guard and remains until commit.
    // Missing rows are permitted only for bootstrap/new-account/anonymous contexts;
    // authenticated app commands separately recheck active_session under this guard.
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(())
}
/// Acquire the account mutation guard directly, never after a shared context.
/// NO KEY UPDATE blocks ordinary FOR SHARE commands while allowing unrelated
/// foreign-key KEY SHARE checks; this avoids cross-user membership lock cycles.
pub async fn context_exclusive(
    tx: &mut Tx,
    user: Uuid,
    workspace: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    context_values(tx, user, workspace).await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR NO KEY UPDATE")
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(())
}
/// Acquire both account guards before any workspace guard, in UUID order.
/// The actor remains shared unless also the target; the target is directly
/// exclusive. Call this on a new transaction, never after context()/lock upgrade.
pub async fn context_access_change(
    tx: &mut Tx,
    actor: Uuid,
    target: Uuid,
    workspace: Uuid,
) -> Result<(), sqlx::Error> {
    context_values(tx, actor, Some(workspace)).await?;
    if actor == target {
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR NO KEY UPDATE")
            .bind(target)
            .fetch_optional(&mut **tx)
            .await?;
    } else {
        for user in [actor.min(target), actor.max(target)] {
            let query = if user == target {
                "SELECT id FROM users WHERE id=$1 FOR NO KEY UPDATE"
            } else {
                "SELECT id FROM users WHERE id=$1 FOR SHARE"
            };
            sqlx::query(query)
                .bind(user)
                .fetch_optional(&mut **tx)
                .await?;
        }
    }
    Ok(())
}
async fn context_values(
    tx: &mut Tx,
    user: Uuid,
    workspace: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true),set_config('statement_timeout','3000',true),set_config('lock_timeout','1000',true),set_config('idle_in_transaction_session_timeout','5000',true)")
        .bind(user.to_string()).bind(workspace.map(|v|v.to_string()).unwrap_or_default()).execute(&mut **tx).await?;
    Ok(())
}
pub async fn user_exists(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users)")
        .fetch_one(pool)
        .await
}
pub async fn find_actor(pool: &PgPool, hash: &str) -> Result<Option<Actor>, sqlx::Error> {
    find_actor_for_audience(pool, hash, SessionAudience::Web).await
}
pub async fn find_actor_for_audience(
    pool: &PgPool,
    hash: &str,
    audience: SessionAudience,
) -> Result<Option<Actor>, sqlx::Error> {
    let mut tx = begin(pool).await?;
    // Find only the account lock key, then authorize again after acquiring it.
    let user: Option<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1 AND audience=$2")
            .bind(hash)
            .bind(audience.as_str())
            .fetch_optional(&mut *tx)
            .await?;
    let Some(user) = user else {
        tx.commit().await?;
        return Ok(None);
    };
    context(&mut tx, user, None).await?;
    let row=sqlx::query("UPDATE sessions s SET last_seen_at=clock_timestamp() FROM users u WHERE s.token_hash=$1 AND s.user_id=$2 AND s.audience=$3 AND s.user_id=u.id AND u.disabled_at IS NULL AND s.revoked_at IS NULL AND s.expires_at>clock_timestamp() AND s.last_seen_at>clock_timestamp()-interval '12 hours' RETURNING u.id,u.email,u.display_name,s.csrf_token").bind(hash).bind(user).bind(audience.as_str()).fetch_optional(&mut *tx).await?;
    let actor = row
        .map(|r| {
            Ok::<Actor, sqlx::Error>(Actor {
                audience,
                user: User {
                    id: r.try_get("id")?,
                    email: r.try_get("email")?,
                    display_name: r.try_get("display_name")?,
                },
                token_hash: hash.into(),
                csrf_token: r.try_get("csrf_token")?,
            })
        })
        .transpose()?;
    tx.commit().await?;
    Ok(actor)
}
pub async fn active_session(tx: &mut Tx, actor: &Actor) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.user_id=$2 AND s.audience=$3 AND s.revoked_at IS NULL AND s.expires_at>clock_timestamp() AND s.last_seen_at>clock_timestamp()-interval '12 hours' AND u.disabled_at IS NULL)").bind(&actor.token_hash).bind(actor.user.id).bind(actor.audience.as_str()).fetch_one(&mut **tx).await
}
pub async fn workspace_role(
    tx: &mut Tx,
    actor: &Actor,
    w: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR SHARE")
        .bind(w)
        .fetch_optional(&mut **tx)
        .await?;
    sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 AND active",
    )
    .bind(w)
    .bind(actor.user.id)
    .fetch_optional(&mut **tx)
    .await
}
pub async fn workspaces(tx: &mut Tx, user: Uuid) -> Result<Vec<Workspace>, sqlx::Error> {
    let rows=sqlx::query("SELECT w.id,w.name,w.slug,m.role FROM workspaces w JOIN workspace_members m ON m.workspace_id=w.id WHERE m.user_id=$1 AND m.active ORDER BY w.name,w.id").bind(user).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|r| {
            Ok(Workspace {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                slug: r.try_get("slug")?,
                role: r.try_get("role")?,
            })
        })
        .collect()
}
pub async fn teams(tx: &mut Tx, w: Uuid, user: Uuid) -> Result<Vec<Team>, sqlx::Error> {
    let rows=sqlx::query("SELECT t.id,t.name,t.key,t.private,t.archived_at FROM teams t WHERE t.workspace_id=$1 AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$2)) ORDER BY t.name,t.id").bind(w).bind(user).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|r| {
            Ok(Team {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                key: r.try_get("key")?,
                private: r.try_get("private")?,
                archived_at: r.try_get("archived_at")?,
            })
        })
        .collect()
}
pub async fn visible_team(
    tx: &mut Tx,
    w: Uuid,
    team: Uuid,
    user: Uuid,
    lock: bool,
) -> Result<bool, sqlx::Error> {
    let suffix = if lock { " FOR UPDATE OF t" } else { "" };
    let mut query = sqlx::QueryBuilder::<Postgres>::new(
        "SELECT t.id FROM teams t WHERE t.workspace_id=$1 AND t.id=$2 AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))",
    );
    query.push(suffix);
    Ok(query
        .build_query_scalar::<Uuid>()
        .bind(w)
        .bind(team)
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?
        .is_some())
}
pub async fn statuses(tx: &mut Tx, w: Uuid, user: Uuid) -> Result<Vec<Status>, sqlx::Error> {
    let rows=sqlx::query("SELECT s.id,s.team_id,s.name,s.category,s.color,s.position,s.is_default,s.retiring_at,s.replacement_status_id,s.replacement_job_id,s.archived_at FROM workflow_statuses s JOIN teams t ON t.workspace_id=s.workspace_id AND t.id=s.team_id WHERE s.workspace_id=$1 AND s.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$2)) ORDER BY s.position,s.id").bind(w).bind(user).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|r| {
            Ok(Status {
                id: r.try_get("id")?,
                team_id: r.try_get("team_id")?,
                name: r.try_get("name")?,
                category: r.try_get("category")?,
                color: r.try_get("color")?,
                position: r.try_get("position")?,
                is_default: r.try_get("is_default")?,
                retiring_at: r.try_get("retiring_at")?,
                replacement_status_id: r.try_get("replacement_status_id")?,
                replacement_job_id: r.try_get("replacement_job_id")?,
                archived_at: r.try_get("archived_at")?,
            })
        })
        .collect()
}
pub async fn members(tx: &mut Tx, w: Uuid) -> Result<Vec<User>, sqlx::Error> {
    let rows=sqlx::query("SELECT u.id,u.email,u.display_name FROM users u JOIN workspace_members m ON m.user_id=u.id WHERE m.workspace_id=$1 AND m.active AND u.disabled_at IS NULL ORDER BY u.display_name,u.id LIMIT 100").bind(w).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|r| {
            Ok(User {
                id: r.try_get("id")?,
                email: r.try_get("email")?,
                display_name: r.try_get("display_name")?,
            })
        })
        .collect()
}
pub async fn valid_references(
    tx: &mut Tx,
    w: Uuid,
    team: Uuid,
    status: Uuid,
    assignee: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND id=$3 AND retiring_at IS NULL AND archived_at IS NULL) AND ($4::uuid IS NULL OR EXISTS(SELECT 1 FROM workspace_members wm JOIN users u ON u.id=wm.user_id JOIN teams t ON t.workspace_id=wm.workspace_id AND t.id=$2 WHERE wm.workspace_id=$1 AND wm.user_id=$4 AND wm.active AND wm.role!='viewer' AND u.disabled_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=$1 AND tm.team_id=$2 AND tm.user_id=$4))))")
        .bind(w).bind(team).bind(status).bind(assignee).fetch_one(&mut **tx).await
}
const ISSUE_SELECT: &str = "SELECT jsonb_build_object('id',i.id,'workspace_id',i.workspace_id,'team_id',i.team_id,'key',t.key||'-'||i.number,'title',i.title,'description',i.description,'body_version',coalesce(to_jsonb(i)->>'body_version','0'),'status_id',i.status_id,'priority',i.priority,'assignee_id',i.assignee_id,'assignee',(SELECT jsonb_build_object('id',u.id,'display_name',u.display_name,'assignable',u.disabled_at IS NULL AND m.active AND m.role<>'viewer' AND t.archived_at IS NULL AND EXISTS(SELECT 1 FROM workspaces aw WHERE aw.id=i.workspace_id AND aw.archived_at IS NULL) AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members atm WHERE atm.workspace_id=i.workspace_id AND atm.team_id=i.team_id AND atm.user_id=u.id))) FROM users u JOIN workspace_members m ON m.user_id=u.id AND m.workspace_id=i.workspace_id WHERE u.id=i.assignee_id),'label_ids',COALESCE((SELECT jsonb_agg(il.label_id ORDER BY il.label_id) FROM issue_labels il WHERE il.workspace_id=i.workspace_id AND il.issue_id=i.id),'[]'::jsonb),'labels',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',l.id,'team_id',l.team_id,'name',l.name,'color',l.color,'archived_at',l.archived_at) ORDER BY l.id) FROM issue_labels il JOIN labels l ON l.workspace_id=il.workspace_id AND l.id=il.label_id WHERE il.workspace_id=i.workspace_id AND il.issue_id=i.id),'[]'::jsonb),'estimate',i.estimate,'due_date',i.due_date,'version',i.version::text,'created_at',i.created_at,'updated_at',i.updated_at,'archived_at',i.archived_at) AS body,i.id AS issue_id,i.deleted_at,i.created_by_user_id,i.team_id,i.archived_at FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id ";
const VISIBLE: &str = " AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$2))";
fn issue_from(value: Value) -> Result<Issue, sqlx::Error> {
    let mut issue: Issue =
        serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    issue.refresh_etag();
    Ok(issue)
}
pub async fn issue(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    id: Uuid,
    lock: bool,
) -> Result<Option<Issue>, sqlx::Error> {
    let suffix = if lock { " FOR UPDATE OF i" } else { "" };
    let mut query = sqlx::QueryBuilder::<Postgres>::new(ISSUE_SELECT);
    query
        .push(" WHERE i.workspace_id=$1 AND i.id=$3 AND i.deleted_at IS NULL ")
        .push(VISIBLE)
        .push(suffix);
    let body = query
        .build_query_scalar::<Value>()
        .bind(w)
        .bind(user)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    body.map(issue_from).transpose()
}
pub async fn list_issues(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    f: &IssueFilter,
    after: Option<(i16, DateTime<Utc>, Uuid)>,
) -> Result<Vec<Issue>, sqlx::Error> {
    let q = f.q.as_ref().map(|q| {
        format!(
            "%{}%",
            q.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let mut query = sqlx::QueryBuilder::<Postgres>::new(ISSUE_SELECT);
    query.push(" WHERE i.workspace_id=$1 ").push(VISIBLE).push(" AND i.deleted_at IS NULL AND ($3::uuid IS NULL OR i.team_id=$3) AND ($4::uuid IS NULL OR i.status_id=$4) AND ($5::uuid IS NULL OR i.assignee_id=$5) AND ($6::smallint IS NULL OR i.priority=$6) AND ($7::text IS NULL OR i.title ILIKE $7 OR t.key||'-'||i.number ILIKE $7) AND ((i.archived_at IS NOT NULL) OR (t.archived_at IS NOT NULL))=$8 AND (cardinality($12::uuid[])=0 OR ($13='any' AND EXISTS(SELECT 1 FROM issue_labels il WHERE il.workspace_id=i.workspace_id AND il.issue_id=i.id AND il.label_id=ANY($12))) OR ($13='all' AND (SELECT count(*) FROM issue_labels il WHERE il.workspace_id=i.workspace_id AND il.issue_id=i.id AND il.label_id=ANY($12))=cardinality($12))) AND ($9::timestamptz IS NULL OR ($14='created' AND (i.created_at,i.id)<($9,$10)) OR ($14='priority' AND (CASE WHEN i.priority=0 THEN 5 ELSE i.priority END>$15 OR (CASE WHEN i.priority=0 THEN 5 ELSE i.priority END=$15 AND (i.created_at,i.id)<($9,$10))))) ORDER BY CASE WHEN $14='priority' THEN CASE WHEN i.priority=0 THEN 5 ELSE i.priority END ELSE 0 END,i.created_at DESC,i.id DESC LIMIT $11");
    let rows = query
        .build_query_scalar::<Value>()
        .bind(w)
        .bind(user)
        .bind(f.team_id)
        .bind(f.status_id)
        .bind(f.assignee_id)
        .bind(f.priority)
        .bind(q)
        .bind(f.archived)
        .bind(after.map(|a| a.1))
        .bind(after.map(|a| a.2))
        .bind(f.limit.unwrap_or(50) + 1)
        .bind(&f.label_ids)
        .bind(f.label_mode.as_deref().unwrap_or("any"))
        .bind(f.sort.as_deref().unwrap_or("created"))
        .bind(after.map(|a| a.0))
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter().map(issue_from).collect()
}
pub struct Event<'a> {
    pub workspace: Uuid,
    pub team: Uuid,
    pub entity: Uuid,
    pub version: i64,
    pub actor: Uuid,
    pub kind: &'a str,
    pub mutation: Uuid,
    pub changes: Value,
}
pub async fn event_admission_ready(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT (SELECT count(*)=3 FROM pg_attribute WHERE attrelid=to_regclass('workspace_stream_heads') AND attname IN ('max_pending_events','max_retained_event_bytes','retained_event_bytes') AND NOT attisdropped)
         AND EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('workspace_events') AND attname='storage_bytes' AND NOT attisdropped)
         AND (SELECT count(*)=4 FROM pg_trigger WHERE tgrelid=to_regclass('workspace_events') AND tgname IN ('event_storage_charge','event_storage_insert','event_storage_update','event_storage_delete') AND tgenabled IN ('O','A'))
         AND to_regprocedure('operator_event_admission_status(uuid)') IS NOT NULL
         AND to_regprocedure('operator_event_admission_limits(uuid,bigint,bigint,text)') IS NOT NULL",
    ).fetch_one(pool).await
}
pub async fn append_activity(tx: &mut Tx, event: &Event<'_>) -> Result<(), sqlx::Error> {
    let appended = sqlx::query("INSERT INTO activity_entries(id,workspace_id,team_id,entity_id,actor_id,event_kind,changes) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(Uuid::new_v4()).bind(event.workspace).bind(event.team).bind(event.entity).bind(event.actor).bind(event.kind).bind(&event.changes).execute(&mut **tx).await?;
    if appended.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "Required issue activity was not appended".into(),
        ));
    }
    Ok(())
}
/// Call only after every entity, progress and activity write. One bounded batch
/// reserves a contiguous range while taking the stream-head lock exactly once.
pub async fn append_stream_events(tx: &mut Tx, events: &[Event<'_>]) -> Result<(), sqlx::Error> {
    let Some(first) = events.first() else {
        return Ok(());
    };
    if events.len() > 101 || events.iter().any(|e| e.workspace != first.workspace) {
        return Err(sqlx::Error::Protocol(
            "Invalid event batch scope or size".into(),
        ));
    }
    let end:i64=sqlx::query_scalar("UPDATE workspace_stream_heads SET last_seq=last_seq+$2 WHERE workspace_id=$1 RETURNING last_seq").bind(first.workspace).bind(events.len() as i64).fetch_one(&mut **tx).await?;
    for (index, event) in events.iter().enumerate() {
        let seq = end - events.len() as i64 + index as i64 + 1;
        let appended=sqlx::query("INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,team_id,entity_id,entity_version,actor_id,mutation_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(event.workspace).bind(seq).bind(Uuid::new_v4()).bind(event.kind).bind(event.team).bind(event.entity).bind(event.version).bind(event.actor).bind(event.mutation).execute(&mut **tx).await?;
        if appended.rows_affected() != 1 {
            return Err(sqlx::Error::Protocol(
                "Required issue event was not appended".into(),
            ));
        }
    }
    Ok(())
}
pub async fn append_event(tx: &mut Tx, event: Event<'_>) -> Result<(), sqlx::Error> {
    append_activity(tx, &event).await?;
    append_stream_events(tx, &[event]).await
}
pub async fn activity(
    tx: &mut Tx,
    w: Uuid,
    entity: Uuid,
    user: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    let mut entries = sqlx::query_scalar("SELECT jsonb_build_object('id',a.id,'event_kind',a.event_kind,'actor_id',a.actor_id,'actor',CASE WHEN a.actor_kind='system' THEN 'OpenTrack' ELSE u.display_name END,'created_at',a.created_at,'changes',a.changes) FROM activity_entries a LEFT JOIN users u ON u.id=a.actor_id JOIN teams t ON t.workspace_id=a.workspace_id AND t.id=a.team_id WHERE a.workspace_id=$1 AND a.entity_id=$2 AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=a.workspace_id AND tm.team_id=a.team_id AND tm.user_id=$3)) ORDER BY a.created_at DESC,a.id DESC LIMIT 100").bind(w).bind(entity).bind(user).fetch_all(&mut **tx).await?;
    project_activity(&mut entries);
    Ok(entries)
}

/// History is a non-content projection, including records written by older servers.
/// Call only after both current issue and recorded-team authorization checks.
pub fn project_activity(entries: &mut [Value]) {
    const FIELDS: &[&str] = &[
        "title",
        "description",
        "status_id",
        "priority",
        "assignee_id",
        "label_ids",
        "estimate",
        "due_date",
        "archived",
        "parent",
        "relations",
    ];
    const FACTS: &[&str] = &[
        "fields",
        "label_ids",
        "assignee_id",
        "status_id",
        "status_names",
        "priority",
        "estimate",
        "due_date",
        "archived",
        "job_id",
        "source",
        "action",
        "relation_id",
        "document_id",
        "document_version",
        "comment_id",
    ];
    for entry in entries {
        if let Some(changes) = entry.get_mut("changes").and_then(Value::as_object_mut) {
            changes.retain(|key, _| FACTS.contains(&key.as_str()));
            if let Some(fields) = changes.get_mut("fields").and_then(Value::as_array_mut) {
                fields.retain(|field| field.as_str().is_some_and(|field| FIELDS.contains(&field)));
            }
        }
    }
}
