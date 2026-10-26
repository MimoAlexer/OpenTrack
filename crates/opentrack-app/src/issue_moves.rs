use super::*;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use opentrack_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;

#[derive(Serialize, Deserialize)]
struct MoveReceipt {
    issue: Issue,
    source_team_id: Uuid,
}
async fn move_scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    id: Uuid,
    destination: Uuid,
    write: bool,
) -> Result<Issue> {
    let before = graph::current(tx, a, w, id).await?;
    if before.team_id == destination {
        return Err(Error::Conflict(
            "Choose a different destination team".into(),
        ));
    }
    if !store::lock_graph_teams(tx, w, a.user.id, &[before.team_id, destination], write)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    let issue = graph::current(tx, a, w, id).await?;
    if issue.team_id != before.team_id {
        return Err(Error::Conflict(
            "The issue moved. Reload before continuing".into(),
        ));
    }
    graph::active(tx, w, &issue).await?;
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM project_issues WHERE workspace_id=$1 AND issue_id=$2)",
    )
    .bind(w)
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?
    {
        return Err(Error::Conflict(
            "Remove this issue from its project before moving it to another team".into(),
        ));
    }
    let linked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cycle_issues WHERE workspace_id=$1 AND issue_id=$2)",
    )
    .bind(w)
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if linked {
        return Err(Error::Conflict(
            "Remove this issue from its cycle before moving it to another team".into(),
        ));
    }
    lifecycle::require_active_team(tx, w, destination).await?;
    Ok(issue)
}
async fn destination(tx: &mut store::Tx, w: Uuid, id: Uuid) -> Result<MoveDestination> {
    let r=sqlx::query("SELECT id,name,key,private FROM teams WHERE workspace_id=$1 AND id=$2 AND archived_at IS NULL").bind(w).bind(id).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
    Ok(MoveDestination {
        id: r.try_get("id").map_err(db_error)?,
        name: r.try_get("name").map_err(db_error)?,
        key: r.try_get("key").map_err(db_error)?,
        private: r.try_get("private").map_err(db_error)?,
        can_move_issue: true,
    })
}
async fn blockers(tx: &mut store::Tx, w: Uuid, id: Uuid) -> Result<Vec<MoveBlocker>> {
    let row=sqlx::query("SELECT parent_id IS NOT NULL AS parent,EXISTS(SELECT 1 FROM issues c WHERE c.workspace_id=$1 AND c.parent_id=$2) AS children,EXISTS(SELECT 1 FROM issue_relations r WHERE r.workspace_id=$1 AND r.source_id=$2 AND r.kind='blocks') AS blocks,EXISTS(SELECT 1 FROM issue_relations r WHERE r.workspace_id=$1 AND r.target_id=$2 AND r.kind='blocks') AS blocked_by,EXISTS(SELECT 1 FROM issue_relations r WHERE r.workspace_id=$1 AND (r.source_id=$2 OR r.target_id=$2) AND r.kind='related') AS related,EXISTS(SELECT 1 FROM issue_relations r WHERE r.workspace_id=$1 AND r.source_id=$2 AND r.kind='duplicate') AS duplicate_of,EXISTS(SELECT 1 FROM issue_relations r WHERE r.workspace_id=$1 AND r.target_id=$2 AND r.kind='duplicate') AS has_duplicate FROM issues WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
    let mut out = vec![];
    for (name, kind) in [
        ("parent", MoveBlocker::Parent),
        ("children", MoveBlocker::Children),
        ("blocks", MoveBlocker::Blocks),
        ("blocked_by", MoveBlocker::BlockedBy),
        ("related", MoveBlocker::Related),
        ("duplicate_of", MoveBlocker::DuplicateOf),
        ("has_duplicate", MoveBlocker::HasDuplicate),
    ] {
        if row.try_get::<bool, _>(name).map_err(db_error)? {
            out.push(kind)
        }
    }
    Ok(out)
}
async fn preview_state(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    issue: &Issue,
    to: Uuid,
) -> Result<(String, i64, bool)> {
    let source = sqlx::query(
        "SELECT private,graph_version,version FROM teams WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(issue.team_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    let dest = sqlx::query(
        "SELECT workflow_version,graph_version,version FROM teams WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(to)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    let workflow: i64 = dest.try_get("workflow_version").map_err(db_error)?;
    let epoch = store::auth_epoch(tx, w).await.map_err(db_error)?;
    let values = json!({"workspace":w,"actor":a.user.id,"issue":issue.id,"etag":issue.etag,"source":issue.team_id,"destination":to,"epoch":epoch,"source_graph":source.try_get::<i64,_>("graph_version").map_err(db_error)?,"source_version":source.try_get::<i64,_>("version").map_err(db_error)?,"destination_graph":dest.try_get::<i64,_>("graph_version").map_err(db_error)?,"destination_version":dest.try_get::<i64,_>("version").map_err(db_error)?,"workflow":workflow});
    Ok((
        digest(&serde_json::to_vec(&values).map_err(|_| Error::Internal)?),
        workflow,
        source.try_get("private").map_err(db_error)?,
    ))
}
fn preview_error() -> Error {
    Error::Conflict("The move preview changed or expired. Preview the move again".into())
}
impl App {
    fn move_mac(&self, payload: &str) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.setup_token_hash.as_bytes())
            .map_err(|_| Error::Internal)?;
        mac.update(b"opentrack.issue-move-preview.v1\0");
        mac.update(payload.as_bytes());
        Ok(mac)
    }
    fn sign_preview(&self, binding: &str) -> Result<(String, DateTime<Utc>)> {
        let issued = Utc::now().timestamp();
        let expires = issued + 600;
        let payload =
            serde_json::to_string(&(issued, expires, binding)).map_err(|_| Error::Internal)?;
        let signature = self.move_mac(&payload)?.finalize().into_bytes();
        Ok((
            format!(
                "{payload}|{}",
                signature
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
            DateTime::from_timestamp(expires, 0).ok_or(Error::Internal)?,
        ))
    }
    fn verify_preview(&self, binding: &str, token: &str) -> Result<()> {
        if token.len() > 4096 {
            return Err(preview_error());
        }
        let (payload, signature) = token.rsplit_once('|').ok_or_else(preview_error)?;
        if signature.len() != 64 {
            return Err(preview_error());
        }
        let signature = (0..32)
            .map(|n| {
                u8::from_str_radix(
                    signature.get(n * 2..n * 2 + 2).ok_or_else(preview_error)?,
                    16,
                )
                .map_err(|_| preview_error())
            })
            .collect::<Result<Vec<_>>>()?;
        self.move_mac(payload)?
            .verify_slice(&signature)
            .map_err(|_| preview_error())?;
        let (issued, expires, state): (i64, i64, String) =
            serde_json::from_str(payload).map_err(|_| preview_error())?;
        let now = Utc::now().timestamp();
        if issued > now + 60
            || issued.checked_add(600) != Some(expires)
            || expires <= now
            || state != binding
        {
            return Err(preview_error());
        }
        Ok(())
    }
    pub async fn issue_move_destinations(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        mut q: MoveDestinationQuery,
    ) -> Result<Page<MoveDestination>> {
        let limit = q.limit.unwrap_or(20);
        if !(1..=50).contains(&limit) || q.q.as_ref().is_some_and(|s| s.chars().count() > 100) {
            return Err(Error::Invalid("Invalid destination query".into()));
        }
        let cursor = q.cursor.take();
        q.limit = Some(limit);
        q.q = Some(q.q.as_deref().unwrap_or("").trim().into());
        let mut tx = self.scoped(a, w, true).await?;
        let issue = graph::scope(&mut tx, a, w, id, false).await?;
        graph::active(&mut tx, w, &issue).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{}:{id}:{}:{epoch}:{}",
            a.user.id,
            issue.team_id,
            serde_json::to_string(&q).map_err(|_| Error::Internal)?
        );
        let after: Option<(String, Uuid)> = cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("move-destinations", &binding, c))
            .transpose()?;
        let text = q.q.as_deref().unwrap_or("");
        let pattern = format!(
            "%{}%",
            text.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut rows=sqlx::query("SELECT id,name,key,private,lower(name) AS sort_name FROM teams t WHERE workspace_id=$1 AND id<>$2 AND archived_at IS NULL AND (NOT private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$3)) AND (name ILIKE $4 OR key ILIKE $4) AND ($5::text IS NULL OR (lower(name),id)>($5,$6)) ORDER BY lower(name),id LIMIT $7").bind(w).bind(issue.team_id).bind(a.user.id).bind(pattern).bind(after.as_ref().map(|x|&x.0)).bind(after.as_ref().map(|x|x.1)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_cursor = if has_more {
            rows.last()
                .map(|r| {
                    self.taxonomy_cursor_encode(
                        "move-destinations",
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
                Ok(MoveDestination {
                    id: r.try_get("id").map_err(db_error)?,
                    name: r.try_get("name").map_err(db_error)?,
                    key: r.try_get("key").map_err(db_error)?,
                    private: r.try_get("private").map_err(db_error)?,
                    can_move_issue: true,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
    pub async fn preview_issue_move(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        to: Uuid,
    ) -> Result<IssueMovePreview> {
        let mut tx = self.scoped(a, w, true).await?;
        let issue = move_scope(&mut tx, a, w, id, to, false).await?;
        let destination = destination(&mut tx, w, to).await?;
        let (binding, workflow, private) = preview_state(&mut tx, a, w, &issue, to).await?;
        let default=sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default AND archived_at IS NULL AND retiring_at IS NULL").bind(w).bind(to).fetch_one(&mut *tx).await.map_err(db_error)?;
        let audience_change = match (private, destination.private) {
            (false, false) => MoveAudienceChange::None,
            (false, true) => MoveAudienceChange::PublicToPrivate,
            (true, false) => MoveAudienceChange::PrivateToPublic,
            (true, true) => MoveAudienceChange::BetweenPrivateTeams,
        };
        let current_assignee = store::issue_assignee(&mut tx, w, to, issue.assignee_id)
            .await
            .map_err(db_error)?;
        let retained_label_ids = issue
            .labels
            .iter()
            .filter(|l| l.team_id.is_none())
            .map(|l| l.id)
            .collect();
        let removed_label_ids = issue
            .labels
            .iter()
            .filter(|l| l.team_id.is_some())
            .map(|l| l.id)
            .collect();
        let blockers = blockers(&mut tx, w, id).await?;
        let (preview_token, expires_at) = self.sign_preview(&binding)?;
        let out = IssueMovePreview {
            issue_id: id,
            issue_etag: issue.etag,
            source_team_id: issue.team_id,
            destination,
            destination_workflow_version: workflow.to_string(),
            destination_default_status_id: default,
            audience_change,
            current_assignee,
            retained_label_ids,
            removed_label_ids,
            blockers,
            preview_token,
            expires_at,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
    pub async fn move_issue(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        key: &str,
        etag: &str,
        mut req: MoveIssue,
    ) -> Result<Issue> {
        normalize_label_ids(&mut req.label_ids)?;
        let (kh, rh) = lifecycle::request_hash(key, &(id, etag, &req))?;
        let mut tx = self.scoped(a, w, true).await?;
        if let Some(receipt) =
            lifecycle::retry::<MoveReceipt>(&mut tx, w, a.user.id, "issues.move", &kh, &rh).await?
        {
            graph::receipt_authority(&mut tx, a, w, &receipt.issue).await?;
            if !store::visible_team(&mut tx, w, receipt.source_team_id, a.user.id, false)
                .await
                .map_err(db_error)?
            {
                return Err(Error::NotFound);
            }
            tx.commit().await.map_err(db_error)?;
            return Ok(receipt.issue);
        }
        let issue = move_scope(&mut tx, a, w, id, req.destination_team_id, true).await?;
        graph::source_version(&issue, etag)?;
        let (binding, _, _) = preview_state(&mut tx, a, w, &issue, req.destination_team_id).await?;
        self.verify_preview(&binding, &req.preview_token)?;
        if !blockers(&mut tx, w, id).await?.is_empty() {
            return Err(Error::Conflict(
                "Remove the issue's parent, children and relationships before moving it".into(),
            ));
        }
        graph::lock_issues(&mut tx, w, issue.team_id, &[id]).await?;
        if !store::valid_references(
            &mut tx,
            w,
            req.destination_team_id,
            req.status_id,
            req.assignee_id,
        )
        .await
        .map_err(db_error)?
        {
            return Err(Error::Invalid(
                "Status or assignee is unavailable in the destination team".into(),
            ));
        }
        if !store::validate_issue_labels(
            &mut tx,
            w,
            req.destination_team_id,
            &req.label_ids,
            &issue.label_ids,
        )
        .await
        .map_err(db_error)?
        {
            return Err(Error::Invalid(
                "A selected label is incompatible with the destination team".into(),
            ));
        }
        crate::issue_ordering::require_groups(
            &mut tx,
            a,
            w,
            &[
                (issue.team_id, issue.status_id),
                (req.destination_team_id, req.status_id),
            ],
        )
        .await?;
        // Remove old links while the source team still owns the issue, preserving
        // shared archived links that remain selected. New links follow the move.
        let retained: Vec<Uuid> = issue
            .label_ids
            .iter()
            .filter(|id| req.label_ids.contains(id))
            .copied()
            .collect();
        store::set_issue_labels(&mut tx, w, id, &retained)
            .await
            .map_err(db_error)?;
        let number:Option<i64>=sqlx::query_scalar("UPDATE teams SET next_issue_number=next_issue_number+1 WHERE workspace_id=$1 AND id=$2 RETURNING next_issue_number-1").bind(w).bind(req.destination_team_id).fetch_optional(&mut *tx).await.map_err(db_error)?;
        let version:Option<i64>=sqlx::query_scalar("UPDATE issues SET team_id=$3,number=$4,status_id=$5,assignee_id=$6,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2 RETURNING version").bind(w).bind(id).bind(req.destination_team_id).bind(number.ok_or(Error::Internal)?).bind(req.status_id).bind(req.assignee_id).fetch_optional(&mut *tx).await.map_err(db_error)?;
        let version = version.ok_or(Error::Internal)?;
        store::set_issue_labels(&mut tx, w, id, &req.label_ids)
            .await
            .map_err(db_error)?;
        let (prefix, old_number) = issue.key.rsplit_once('-').ok_or(Error::Internal)?;
        let old_number = old_number.parse::<i64>().map_err(|_| Error::Internal)?;
        graph::one(sqlx::query("INSERT INTO issue_key_aliases(workspace_id,team_prefix,number,issue_id) VALUES($1,$2,$3,$4)").bind(w).bind(prefix).bind(old_number).bind(id).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        for team in [
            issue.team_id.min(req.destination_team_id),
            issue.team_id.max(req.destination_team_id),
        ] {
            graph::one(sqlx::query("UPDATE teams SET graph_version=graph_version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(team).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        }
        let out = graph::current(&mut tx, a, w, id).await?;
        graph::save(
            &mut tx,
            w,
            a.user.id,
            "issues.move",
            &kh,
            &MoveReceipt {
                issue: out.clone(),
                source_team_id: issue.team_id,
            },
        )
        .await?;
        let mutation = Uuid::new_v4();
        let events = [
            store::Event {
                workspace: w,
                team: issue.team_id,
                entity: id,
                version,
                actor: a.user.id,
                kind: "issue.moved_out",
                mutation,
                changes: json!({"source":"manual","action":"moved_out"}),
            },
            store::Event {
                workspace: w,
                team: req.destination_team_id,
                entity: id,
                version,
                actor: a.user.id,
                kind: "issue.moved_in",
                mutation,
                changes: json!({"source":"manual","action":"moved_in"}),
            },
        ];
        graph::effects(&mut tx, &events).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn move_preview_authenticates_state_and_ten_minute_lifetime() {
        let app = App {
            pool: PgPoolOptions::new()
                .connect_lazy("postgres://unused:unused@127.0.0.1/unused")
                .unwrap(),
            setup_token_hash: digest(b"isolated preview unit key"),
            hashes: Arc::new(Semaphore::new(1)),
        };
        let (token, expires) = app
            .sign_preview("actor/workspace/issue/reference-state")
            .unwrap();
        app.verify_preview("actor/workspace/issue/reference-state", &token)
            .unwrap();
        assert!(expires > Utc::now());
        assert!(matches!(
            app.verify_preview("other-state", &token),
            Err(Error::Conflict(_))
        ));
        let changed = token.replacen("actor/workspace", "other/workspace", 1);
        assert!(
            app.verify_preview("actor/workspace/issue/reference-state", &changed)
                .is_err()
        );
        let now = Utc::now().timestamp();
        for (issued, ttl) in [(now - 601, 600), (now + 120, 600), (now, 3600)] {
            let payload = serde_json::to_string(&(issued, issued + ttl, "state")).unwrap();
            let mac = app.move_mac(&payload).unwrap().finalize().into_bytes();
            let token = format!(
                "{payload}|{}",
                mac.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            assert!(matches!(
                app.verify_preview("state", &token),
                Err(Error::Conflict(_))
            ));
        }
    }
}
