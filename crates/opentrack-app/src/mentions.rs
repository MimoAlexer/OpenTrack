use super::*;
use opentrack_domain::*;
use serde_json::{Value, json};
use sqlx::Row;

pub(crate) async fn projection(
    tx: &mut store::Tx,
    w: Uuid,
    id: Uuid,
    body: &Value,
    origin: Option<Uuid>,
) -> Result<Vec<MentionReference>> {
    let ids = document_mention_ids(body);
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let row=sqlx::query("SELECT coalesce(i.team_id,p.team_id) AS team_id,coalesce(c.origin_team_id,i.team_id,p.team_id) AS origin FROM documents d LEFT JOIN comments c ON c.workspace_id=d.workspace_id AND c.id=d.comment_id LEFT JOIN issues i ON i.workspace_id=d.workspace_id AND i.id=coalesce(d.issue_id,c.issue_id) LEFT JOIN projects p ON p.workspace_id=d.workspace_id AND p.id=d.project_id WHERE d.workspace_id=$1 AND d.id=$2").bind(w).bind(id).fetch_one(&mut **tx).await.map_err(db_error)?;
    let team: Uuid = row.try_get("team_id").map_err(db_error)?;
    let historical: Uuid = row.try_get("origin").map_err(db_error)?;
    let revision_origin = origin.unwrap_or(historical);
    let rows=sqlx::query("SELECT requested.id,CASE WHEN mention_eligible($1,$2,$3,requested.id) AND mention_eligible($1,$2,$5,requested.id) THEN u.display_name ELSE NULL END AS display_name FROM unnest($4::uuid[]) requested(id) LEFT JOIN users u ON u.id=requested.id ORDER BY requested.id").bind(w).bind(team).bind(historical).bind(ids).bind(revision_origin).fetch_all(&mut **tx).await.map_err(db_error)?;
    rows.into_iter()
        .map(|r| {
            let display_name: Option<String> = r.try_get("display_name").map_err(db_error)?;
            Ok(MentionReference {
                id: r.try_get("id").map_err(db_error)?,
                available: display_name.is_some(),
                display_name,
            })
        })
        .collect()
}
pub(crate) async fn scope(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    q: &MentionQuery,
) -> Result<(Uuid, Uuid)> {
    if let Some(doc) = q.document_id {
        let scope = documents::document_scope(tx, a, w, doc, false).await?;
        let origin = if let Some(c) = scope.comment {
            sqlx::query_scalar(
                "SELECT origin_team_id FROM comments WHERE workspace_id=$1 AND id=$2",
            )
            .bind(w)
            .bind(c.id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?
        } else {
            scope.team_id
        };
        return Ok((scope.team_id, origin));
    }
    if let Some(issue) = q.issue_id {
        let item = documents::issue_scope(tx, a, w, issue, false).await?;
        return Ok((item.team_id, item.team_id));
    }
    let team = q.team_id.ok_or(Error::Internal)?;
    if !store::visible_team(tx, w, team, a.user.id, false)
        .await
        .map_err(db_error)?
    {
        return Err(Error::NotFound);
    }
    Ok((team, team))
}
async fn subscription(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    issue: Uuid,
) -> Result<IssueSubscription> {
    let row=sqlx::query("SELECT preference,source_creator,source_assignment,source_mention,version FROM issue_subscriptions WHERE workspace_id=$1 AND issue_id=$2 AND user_id=$3").bind(w).bind(issue).bind(a.user.id).fetch_optional(&mut **tx).await.map_err(db_error)?;
    let (mut sources, mut version, mut preference) =
        (vec![], 0_i64, SubscriptionPreference::Automatic);
    if let Some(row) = row {
        version = row.try_get("version").map_err(db_error)?;
        preference = match row.try_get::<&str, _>("preference").map_err(db_error)? {
            "automatic" => SubscriptionPreference::Automatic,
            "subscribed" => SubscriptionPreference::Subscribed,
            "unsubscribed" => SubscriptionPreference::Unsubscribed,
            _ => return Err(Error::Internal),
        };
        if row.try_get::<bool, _>("source_creator").map_err(db_error)? {
            sources.push(SubscriptionSource::Creator);
        }
        if row
            .try_get::<bool, _>("source_assignment")
            .map_err(db_error)?
        {
            sources.push(SubscriptionSource::Assignment);
        }
        if row.try_get::<bool, _>("source_mention").map_err(db_error)? {
            sources.push(SubscriptionSource::Mention);
        }
    }
    let subscribed = match preference {
        SubscriptionPreference::Automatic => !sources.is_empty(),
        SubscriptionPreference::Subscribed => true,
        SubscriptionPreference::Unsubscribed => false,
    };
    Ok(IssueSubscription {
        issue_id: issue,
        preference,
        subscribed,
        sources,
        version: version.to_string(),
        etag: format!("\"subscription:{issue}:{}:{version}:true\"", a.user.id),
        can_change: true,
    })
}
impl App {
    pub async fn mention_candidates(
        &self,
        a: &Actor,
        w: Uuid,
        mut q: MentionQuery,
    ) -> Result<Page<MentionCandidate>> {
        q.validate()?;
        q.q = Some(q.q.as_deref().unwrap_or("").trim().to_owned());
        q.limit = Some(q.limit.unwrap_or(20));
        let mut tx = self.scoped(a, w, false).await?;
        let (team, origin) = scope(&mut tx, a, w, &q).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let mut fingerprint = q.clone();
        fingerprint.cursor = None;
        let binding = format!(
            "{w}:{}:{epoch}:{team}:{origin}:{}",
            a.user.id,
            serde_json::to_string(&fingerprint).map_err(|_| Error::Internal)?
        );
        let after: Option<(String, Uuid)> = q
            .cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("mentions-v1", &binding, c))
            .transpose()?;
        let rows=sqlx::query("SELECT u.id,u.display_name,lower(u.display_name) AS sort_name FROM users u JOIN workspace_members m ON m.user_id=u.id AND m.workspace_id=$1 AND m.active WHERE mention_eligible($1,$2,$3,u.id) AND strpos(lower(u.display_name),lower($4))>0 AND ($5::text IS NULL OR (lower(u.display_name),u.id)>($5,$6)) ORDER BY lower(u.display_name),u.id LIMIT $7").bind(w).bind(team).bind(origin).bind(q.q.as_deref().unwrap_or("")).bind(after.as_ref().map(|v|&v.0)).bind(after.as_ref().map(|v|v.1)).bind(i64::from(q.limit.unwrap_or(20))+1).fetch_all(&mut *tx).await.map_err(db_error)?;
        let limit = q.limit.unwrap_or(20) as usize;
        let has_more = rows.len() > limit;
        let next_cursor = if has_more {
            let row = &rows[limit - 1];
            Some(self.taxonomy_cursor_encode(
                "mentions-v1",
                &binding,
                &(
                    row.try_get::<String, _>("sort_name").map_err(db_error)?,
                    row.try_get::<Uuid, _>("id").map_err(db_error)?,
                ),
            )?)
        } else {
            None
        };
        let data = rows
            .iter()
            .take(limit)
            .map(|r| {
                Ok(MentionCandidate {
                    id: r.try_get("id").map_err(db_error)?,
                    display_name: r.try_get("display_name").map_err(db_error)?,
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
    pub async fn issue_subscription(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
    ) -> Result<IssueSubscription> {
        let mut tx = self.scoped(a, w, false).await?;
        documents::issue_scope(&mut tx, a, w, id, false).await?;
        let result = subscription(&mut tx, a, w, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    pub async fn update_issue_subscription(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        req: UpdateIssueSubscription,
    ) -> Result<IssueSubscription> {
        let mut tx = self.scoped(a, w, false).await?;
        let issue = documents::issue_scope(&mut tx, a, w, id, false).await?;
        if !store::visible_team(&mut tx, w, issue.team_id, a.user.id, true)
            .await
            .map_err(db_error)?
        {
            return Err(Error::NotFound);
        }
        graph::lock_issues(&mut tx, w, issue.team_id, &[id]).await?;
        documents::issue_scope(&mut tx, a, w, id, false).await?;
        let old = subscription(&mut tx, a, w, id).await?;
        lifecycle::version(etag, &old.etag, &old)?;
        if old.preference == req.preference {
            tx.commit().await.map_err(db_error)?;
            return Ok(old);
        }
        graph::one(sqlx::query("INSERT INTO issue_subscriptions(workspace_id,issue_id,user_id,preference) VALUES($1,$2,$3,$4) ON CONFLICT(workspace_id,issue_id,user_id) DO UPDATE SET preference=EXCLUDED.preference,version=issue_subscriptions.version+1,updated_at=clock_timestamp()").bind(w).bind(id).bind(a.user.id).bind(req.preference.as_str()).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        let result = subscription(&mut tx, a, w, id).await?;
        let mutation = Uuid::new_v4();
        graph::one(sqlx::query("INSERT INTO subscription_events(workspace_id,id,user_id,issue_id,subscription_version,preference) VALUES($1,$2,$3,$4,$5,$6)").bind(w).bind(mutation).bind(a.user.id).bind(id).bind(result.version.parse::<i64>().map_err(|_|Error::Internal)?).bind(req.preference.as_str()).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        graph::one(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id,metadata) VALUES($1,$2,$3,'issue.subscription_changed',$4,$5)").bind(mutation).bind(w).bind(a.user.id).bind(id).bind(json!({"preference":req.preference})).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}
