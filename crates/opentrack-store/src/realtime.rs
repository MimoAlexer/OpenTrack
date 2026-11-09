//! Durable replay metadata and current-source authorization.
//!
//! The application must acquire its account/workspace authorization guards and
//! validate the live session before calling these helpers, and again before a
//! later emission. No transaction or source authorization survives network I/O.
//! Recorded event teams restrict historical audiences; they never substitute
//! for resolving the source's current owner.
use crate::Tx;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, Row};
use uuid::Uuid;

/// Internal replay metadata. Actor identities and content/history values are
/// deliberately absent; this is not a wire DTO.
#[derive(Debug, Clone, FromRow)]
pub struct StreamRow {
    pub seq: i64,
    pub event_id: Uuid,
    pub event_type: String,
    pub schema_version: i32,
    pub team_id: Option<Uuid>,
    pub entity_id: Uuid,
    pub entity_version: i64,
    pub mutation_id: Uuid,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamSource {
    /// The registered source type; its ID is the original row's entity_id.
    Visible(&'static str),
    /// Advance only the opaque checkpoint, without any event/source metadata.
    Hidden,
    /// Clear the scope without disclosing the event or its source identity.
    ScopeReset,
}

pub async fn stream_head(tx: &mut Tx, workspace: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1")
        .bind(workspace)
        .fetch_one(&mut **tx)
        .await
}

/// Returns a bounded ordered page, including hidden rows. The caller checks
/// sequence continuity against its cursor/head and resets after a retention gap;
/// filtering here would make hidden rows indistinguishable from missing ones.
pub async fn stream_page(
    tx: &mut Tx,
    workspace: Uuid,
    after: i64,
    limit: u32,
) -> Result<Vec<StreamRow>, sqlx::Error> {
    if after < 0 || !(1..=256).contains(&limit) {
        return Err(sqlx::Error::Protocol("Invalid replay page bounds".into()));
    }
    sqlx::query_as("SELECT seq,event_id,event_type,schema_version,team_id,entity_id,entity_version,mutation_id,occurred_at FROM workspace_events WHERE workspace_id=$1 AND seq>$2 ORDER BY seq LIMIT $3")
        .bind(workspace)
        .bind(after)
        .bind(i64::from(limit))
        .fetch_all(&mut **tx)
        .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Issue,
    Document,
    Comment,
    Project,
    Milestone,
    Cycle,
    CycleCarryOperation,
    Team,
    Workspace,
    WorkflowStatus,
    Label,
    Template,
    Attachment,
    Invitation,
    Preferences,
    Notification,
    DiscoveryPersonal,
    DiscoveryView,
    Access,
}

/// Registrations describe writer contracts, not prefixes: e.g. milestone order
/// events identify a project, while cycle review events identify an operation.
fn registration(kind: &str) -> Option<(SourceKind, &'static str)> {
    use SourceKind::*;
    Some(match kind {
        "issue.created"
        | "issue.updated"
        | "issue.deleted"
        | "issue.restored"
        | "issue.moved_out"
        | "issue.moved_in"
        | "issue.parent_changed"
        | "issue.relation_added"
        | "issue.relation_removed"
        | "issue.cycle_changed" => (Issue, "issue"),
        "document.updated" => (Document, "document"),
        "comment.created" | "comment.updated" | "comment.deleted" => (Comment, "comment"),
        "project.created"
        | "project.updated"
        | "project.archived"
        | "project.restored"
        | "project.scope_changed"
        | "project.scope_agreed"
        | "project.update_published"
        | "milestone.order_changed"
        | "milestone.assignment_changed" => (Project, "project"),
        "milestone.created" | "milestone.updated" | "milestone.deleted" => (Milestone, "milestone"),
        "cycle.created"
        | "cycle.updated"
        | "cycle.scope_changed"
        | "cycle.opened"
        | "cycle.reopened"
        | "cycle.closed"
        | "cycle.snapshot_captured"
        | "cycle.carry_forward"
        | "cycle.deleted" => (Cycle, "cycle"),
        "cycle.carry_reviewed" | "cycle.carry_resumed" => {
            (CycleCarryOperation, "cycle_carry_operation")
        }
        "team.created"
        | "team.updated"
        | "workflow.reordered"
        | "workflow.replacement_progress"
        | "workflow.replacement_completed" => (Team, "team"),
        "workspace.created" | "workspace.updated" => (Workspace, "workspace"),
        "workflow.status_created"
        | "workflow.status_updated"
        | "workflow.default_changed"
        | "workflow.status_archived"
        | "workflow.status_restored"
        | "workflow.replacement_started"
        | "workflow.replacement_resumed" => (WorkflowStatus, "workflow_status"),
        "label.created" | "label.updated" | "label.archived" | "label.restored" => (Label, "label"),
        "template.created" | "template.updated" | "template.archived" | "template.restored" => {
            (Template, "template")
        }
        "attachment.reserved"
        | "attachment.upload_started"
        | "attachment.quarantined"
        | "attachment.ready"
        | "attachment.rejected"
        | "attachment.scan_unavailable"
        | "attachment.upload_interrupted"
        | "attachment.deleted" => (Attachment, "attachment"),
        "invitation.created" | "invitation.revoked" => (Invitation, "invitation"),
        "user.preferences_changed" => (Preferences, "user_preferences"),
        "notification.created" | "notification.updated" => (Notification, "notification"),
        "discovery.personal_changed" => (DiscoveryPersonal, "discovery_personal"),
        "view.created" | "view.updated" | "view.deleted" => (DiscoveryView, "view"),
        "access.changed" => (Access, "workspace"),
        _ => return None,
    })
}

/// Authorizes only metadata from the source's current scope. A workspace RLS
/// match or a recorded team alone is not sufficient to emit an entity ID.
pub async fn stream_source(
    tx: &mut Tx,
    workspace: Uuid,
    user: Uuid,
    row: &StreamRow,
) -> Result<StreamSource, sqlx::Error> {
    let Some((source, entity_type)) = registration(&row.event_type) else {
        return Ok(StreamSource::Hidden);
    };
    if row.schema_version != 1 || row.seq < 1 || row.entity_version < 1 {
        return Ok(StreamSource::Hidden);
    }
    // Also fail closed if a caller mixes an authenticated transaction with a
    // different principal/workspace. Session checks remain the app's job.
    let role: Option<String> = sqlx::query_scalar(
        "SELECT m.role FROM workspace_members m JOIN users u ON u.id=m.user_id
         WHERE m.workspace_id=$1 AND m.user_id=$2 AND m.active AND u.disabled_at IS NULL
         AND $1=nullif(current_setting('opentrack.workspace_id',true),'')::uuid
         AND $2=nullif(current_setting('opentrack.user_id',true),'')::uuid",
    )
    .bind(workspace)
    .bind(user)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(role) = role else {
        return Ok(StreamSource::Hidden);
    };
    if let Some(team) = row.team_id
        && !crate::visible_team(tx, workspace, team, user, false).await?
    {
        return Ok(StreamSource::Hidden);
    }
    use SourceKind::*;
    let readable = match source {
        DiscoveryPersonal | DiscoveryView => {
            // Replay remains usable on migration 0033 without the later saved
            // discovery schema; an absent source fails closed.
            let installed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_class WHERE relnamespace=current_schema()::regnamespace AND relname='discovery_view_sources' AND relkind IN('r','p')) AND EXISTS(SELECT 1 FROM pg_class WHERE relnamespace=current_schema()::regnamespace AND relname='discovery_personal_heads' AND relkind IN('r','p'))")
                .fetch_one(&mut **tx).await?;
            installed
                && (source != DiscoveryPersonal || row.team_id.is_none())
                && crate::discovery_event_visible(tx, workspace, user, &row.event_type, row.entity_id).await?
        }
        Notification => {
            // M07 is optional for this earlier migration. Never fall through to
            // another schema or treat a recipient-private event as workspace-wide.
            let installed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_class WHERE relnamespace=current_schema()::regnamespace AND relname='notifications' AND relkind IN('r','p'))")
                .fetch_one(&mut **tx).await?;
            installed && row.team_id.is_none() && sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND id=$3)")
                .bind(workspace).bind(user).bind(row.entity_id).fetch_one(&mut **tx).await?
        }
        Access => {
            return Ok(if row.team_id.is_none() && row.entity_id == workspace {
                StreamSource::ScopeReset
            } else {
                StreamSource::Hidden
            });
        }
        Preferences => {
            row.team_id.is_none()
                && row.entity_id == user
                && sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM user_preferences WHERE workspace_id=$1 AND user_id=$2)")
                    .bind(workspace).bind(user).fetch_one(&mut **tx).await?
        }
        Workspace => row.team_id.is_none() && row.entity_id == workspace,
        Issue => {
            if row.team_id.is_none() {
                false
            } else {
                let team: Option<Uuid> = sqlx::query_scalar(
                    "SELECT team_id FROM issues WHERE workspace_id=$1 AND id=$2",
                )
                .bind(workspace)
                .bind(row.entity_id)
                .fetch_optional(&mut **tx)
                .await?;
                let visible = match team {
                    Some(team) => crate::visible_team(tx, workspace, team, user, false).await?,
                    None => false,
                };
                if !visible && row.event_type == "issue.moved_out" {
                    // The earlier team is still readable, but the current owner
                    // is hidden. Purge stale cached state without disclosing ID.
                    return Ok(StreamSource::ScopeReset);
                }
                visible
            }
        }
        Document => {
            row.team_id.is_some()
                && document_version(tx, workspace, user, row.entity_id)
                    .await?
                    .is_some()
        }
        Comment => {
            let teams: Option<(Uuid, Uuid)> = sqlx::query_as(
                "SELECT i.team_id,c.origin_team_id FROM comments c JOIN issues i
                 ON i.workspace_id=c.workspace_id AND i.id=c.issue_id
                 WHERE c.workspace_id=$1 AND c.id=$2 AND i.deleted_at IS NULL",
            )
            .bind(workspace)
            .bind(row.entity_id)
            .fetch_optional(&mut **tx)
            .await?;
            match teams {
                Some((current, origin)) if row.team_id == Some(origin) => {
                    crate::visible_team(tx, workspace, current, user, false).await?
                        && crate::visible_team(tx, workspace, origin, user, false).await?
                }
                _ => false,
            }
        }
        Project | Milestone | Cycle | CycleCarryOperation | Team | WorkflowStatus | Template => {
            let statement = match source {
                Project => "SELECT team_id FROM projects WHERE workspace_id=$1 AND id=$2",
                Milestone => "SELECT m.team_id FROM milestones m JOIN projects p ON p.workspace_id=m.workspace_id AND p.team_id=m.team_id AND p.id=m.project_id WHERE m.workspace_id=$1 AND m.id=$2",
                Cycle => "SELECT team_id FROM cycles WHERE workspace_id=$1 AND id=$2",
                CycleCarryOperation => "SELECT o.team_id FROM cycle_carry_operations o JOIN cycles s ON s.workspace_id=o.workspace_id AND s.team_id=o.team_id AND s.id=o.source_cycle_id JOIN cycles d ON d.workspace_id=o.workspace_id AND d.team_id=o.team_id AND d.id=o.destination_cycle_id WHERE o.workspace_id=$1 AND o.id=$2",
                Team => "SELECT id FROM teams WHERE workspace_id=$1 AND id=$2",
                WorkflowStatus => "SELECT team_id FROM workflow_statuses WHERE workspace_id=$1 AND id=$2",
                Template => "SELECT team_id FROM issue_templates WHERE workspace_id=$1 AND id=$2",
                _ => unreachable!(),
            };
            let team: Option<Uuid> = sqlx::query_scalar(statement)
                .bind(workspace)
                .bind(row.entity_id)
                .fetch_optional(&mut **tx)
                .await?;
            if source == Milestone
                && team.is_none()
                && row.event_type == "milestone.deleted"
                && row.team_id.is_some()
            {
                // Milestones are hard-deleted. The recorded team was checked
                // above; reset without attempting to expose a missing source.
                return Ok(StreamSource::ScopeReset);
            }
            team.is_some() && team == row.team_id
        }
        Label => {
            let team: Option<Option<Uuid>> = sqlx::query_scalar(
                "SELECT team_id FROM labels WHERE workspace_id=$1 AND id=$2",
            )
            .bind(workspace)
            .bind(row.entity_id)
            .fetch_optional(&mut **tx)
            .await?;
            team.is_some_and(|team| team == row.team_id)
        }
        Attachment => {
            row.team_id.is_some() && attachment_visible(tx, workspace, user, row.entity_id).await?
        }
        Invitation => {
            row.team_id.is_none()
                && matches!(role.as_str(), "owner" | "admin")
                && sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM invitations i WHERE i.workspace_id=$1 AND i.id=$2
                     AND NOT EXISTS(SELECT 1 FROM invitation_teams g JOIN teams t
                       ON t.workspace_id=g.workspace_id AND t.id=g.team_id
                       WHERE g.workspace_id=i.workspace_id AND g.invitation_id=i.id AND t.private
                       AND NOT EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id
                         AND tm.team_id=t.id AND tm.user_id=$3)))",
                )
                .bind(workspace)
                .bind(row.entity_id)
                .bind(user)
                .fetch_one(&mut **tx)
                .await?
        }
    };
    Ok(if readable {
        StreamSource::Visible(entity_type)
    } else {
        StreamSource::Hidden
    })
}

/// Resolve all three document owner types without loading body, author or
/// history text. Deleted comments expose a comment tombstone, not a document.
async fn document_version(
    tx: &mut Tx,
    workspace: Uuid,
    user: Uuid,
    id: Uuid,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT d.version FROM documents d
         LEFT JOIN issues i ON i.workspace_id=d.workspace_id AND i.id=d.issue_id
         LEFT JOIN comments c ON c.workspace_id=d.workspace_id AND c.id=d.comment_id
         LEFT JOIN issues ci ON ci.workspace_id=c.workspace_id AND ci.id=c.issue_id
         LEFT JOIN projects p ON p.workspace_id=d.workspace_id AND p.id=d.project_id
         JOIN teams t ON t.workspace_id=d.workspace_id AND t.id=coalesce(i.team_id,ci.team_id,p.team_id)
         LEFT JOIN teams origin ON origin.workspace_id=c.workspace_id AND origin.id=c.origin_team_id
         WHERE d.workspace_id=$1 AND d.id=$2 AND d.deleted_at IS NULL
         AND ((d.issue_id IS NOT NULL AND i.id IS NOT NULL AND i.deleted_at IS NULL)
           OR (d.comment_id IS NOT NULL AND c.id IS NOT NULL AND c.deleted_at IS NULL AND ci.id IS NOT NULL AND ci.deleted_at IS NULL)
           OR (d.project_id IS NOT NULL AND p.id IS NOT NULL))
         AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm
           WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3))
         AND (d.comment_id IS NULL OR (origin.id IS NOT NULL AND (NOT origin.private
           OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=origin.workspace_id
             AND tm.team_id=origin.id AND tm.user_id=$3))))",
    )
    .bind(workspace)
    .bind(id)
    .bind(user)
    .fetch_optional(&mut **tx)
    .await
}

async fn attachment_visible(
    tx: &mut Tx,
    workspace: Uuid,
    user: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT a.upload_team_id,a.uploader_id,a.reserved_document_id,
           a.expires_at>clock_timestamp() AS unexpired,l.document_id
         FROM attachments a LEFT JOIN document_attachments l
           ON l.workspace_id=a.workspace_id AND l.attachment_id=a.id
         WHERE a.workspace_id=$1 AND a.id=$2",
    )
    .bind(workspace)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else { return Ok(false) };
    let linked: Option<Uuid> = row.try_get("document_id")?;
    let reserved: Option<Uuid> = row.try_get("reserved_document_id")?;
    if linked.is_none()
        && (row.try_get::<Uuid, _>("uploader_id")? != user
            || !row.try_get::<bool, _>("unexpired")?)
    {
        return Ok(false);
    }
    let Some(document) = linked.or(reserved) else {
        return crate::visible_team(tx, workspace, row.try_get("upload_team_id")?, user, false)
            .await;
    };
    let Some(version) = document_version(tx, workspace, user, document).await? else {
        return Ok(false);
    };
    if linked.is_none() {
        return Ok(true);
    }
    // Matches attachment reads: current references are readable; older retained
    // references additionally require a recent revision and its origin ACL.
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM document_attachment_revisions ar JOIN document_revisions r
           ON r.workspace_id=ar.workspace_id AND r.document_id=ar.document_id AND r.revision=ar.revision
         JOIN teams t ON t.workspace_id=r.workspace_id AND t.id=r.origin_team_id
         WHERE ar.workspace_id=$1 AND ar.document_id=$2 AND ar.attachment_id=$3
         AND (ar.revision=$4 OR (r.created_at>clock_timestamp()-interval '90 days'
           AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm
             WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$5)))))",
    )
    .bind(workspace)
    .bind(document)
    .bind(id)
    .bind(version)
    .bind(user)
    .fetch_one(&mut **tx)
    .await
}

#[cfg(test)]
mod tests {
    use super::{SourceKind, registration};

    #[test]
    fn saved_discovery_events_register_private_and_shared_sources_explicitly() {
        assert_eq!(
            registration("discovery.personal_changed"),
            Some((SourceKind::DiscoveryPersonal, "discovery_personal"))
        );
        for kind in ["view.created", "view.updated", "view.deleted"] {
            assert_eq!(
                registration(kind),
                Some((SourceKind::DiscoveryView, "view"))
            );
        }
        assert_eq!(registration("view.private_preview"), None);
        assert_eq!(registration("discovery.created"), None);
    }

    #[test]
    fn notification_invalidations_require_the_explicit_recipient_policy() {
        for kind in ["notification.created", "notification.updated"] {
            assert_eq!(
                registration(kind),
                Some((SourceKind::Notification, "notification"))
            );
        }
        assert_eq!(registration("notification.recipient_changed"), None);
    }

    #[test]
    fn aggregate_and_operation_events_keep_the_writers_actual_entity_type() {
        assert_eq!(
            registration("milestone.assignment_changed"),
            Some((SourceKind::Project, "project"))
        );
        assert_eq!(
            registration("milestone.order_changed"),
            Some((SourceKind::Project, "project"))
        );
        assert_eq!(
            registration("cycle.carry_reviewed"),
            Some((SourceKind::CycleCarryOperation, "cycle_carry_operation"))
        );
        assert_eq!(
            registration("workflow.replacement_started"),
            Some((SourceKind::WorkflowStatus, "workflow_status"))
        );
        assert_eq!(
            registration("workflow.replacement_completed"),
            Some((SourceKind::Team, "team"))
        );
    }

    #[test]
    fn unregistered_event_names_do_not_inherit_a_family_policy() {
        for kind in [
            "issue.private_draft_created",
            "workspace.member.updated",
            "team.member.removed",
            "invitation.accepted",
            "user.preferences_changed.extra",
            "access.changed.extra",
            "notification.private_preview",
        ] {
            assert_eq!(registration(kind), None, "{kind}");
        }
    }
}
