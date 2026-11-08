//! Saved definitions never confer access to the underlying source rows.
use super::*;
use opentrack_domain::*;
use serde_json::json;
use sqlx::{Postgres, QueryBuilder};

async fn editable(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    visibility: DiscoveryVisibility,
    owner: Uuid,
    team: Option<Uuid>,
) -> Result<bool> {
    match visibility {
        DiscoveryVisibility::Personal => Ok(owner == a.user.id),
        DiscoveryVisibility::Team => {
            Ok(
                projects::scope(tx, a, w, team.ok_or(Error::Internal)?, false)
                    .await?
                    .1,
            )
        }
        DiscoveryVisibility::Workspace => Ok(lifecycle::manager(
            &store::workspace_role(tx, a, w)
                .await
                .map_err(db_error)?
                .ok_or(Error::NotFound)?,
        )),
    }
}
async fn authorized_definition(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    item: &mut SavedDiscovery,
) -> Result<()> {
    if let Some(definition) = &item.definition {
        let validation = async {
            issue_query::query_clock(tx, &definition.query.timezone).await?;
            issue_query::validate_targets(tx, a, w, &definition.query).await
        }
        .await;
        match validation {
            Ok(()) => {}
            Err(Error::Invalid(_)) => {
                item.definition = None;
                item.repair_required = Some(
                    "Saved filters or their query timezone are unavailable; ask an editor to repair this definition"
                        .into(),
                );
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
async fn read(tx: &mut store::Tx, a: &Actor, w: Uuid, id: Uuid) -> Result<SavedDiscovery> {
    let mut item = store::discovery(tx, w, id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    item.can_edit = editable(tx, a, w, item.visibility, item.owner_id, item.team_id).await?;
    authorized_definition(tx, a, w, &mut item).await?;
    Ok(item)
}
async fn write_scope(tx: &mut store::Tx, a: &Actor, w: Uuid, item: &SavedDiscovery) -> Result<()> {
    if !item.can_edit {
        return Err(Error::Forbidden);
    }
    if item.visibility != DiscoveryVisibility::Personal {
        let active: bool =
            sqlx::query_scalar("SELECT archived_at IS NULL FROM workspaces WHERE id=$1")
                .bind(w)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_error)?;
        if !active {
            return Err(Error::Conflict(
                "Restore the workspace before editing shared views".into(),
            ));
        }
        if let Some(team) = item.team_id {
            projects::scope(tx, a, w, team, true).await?;
        }
    }
    Ok(())
}
async fn lock_personal(tx: &mut store::Tx, w: Uuid, user: Uuid) -> Result<()> {
    sqlx::query("SELECT user_id FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 AND active FOR UPDATE").bind(w).bind(user).fetch_one(&mut **tx).await.map_err(db_error)?;
    Ok(())
}
async fn record(
    tx: &mut store::Tx,
    a: &Actor,
    w: Uuid,
    item: &SavedDiscovery,
    action: &str,
    deleted: bool,
    personal_only: bool,
) -> Result<()> {
    let mutation = Uuid::new_v4();
    if item.visibility == DiscoveryVisibility::Personal || personal_only {
        let version:i64=sqlx::query_scalar("INSERT INTO discovery_personal_heads(workspace_id,user_id,version) VALUES($1,$2,1) ON CONFLICT(workspace_id,user_id) DO UPDATE SET version=discovery_personal_heads.version+1 RETURNING version").bind(w).bind(a.user.id).fetch_one(&mut **tx).await.map_err(db_error)?;
        graph::one(sqlx::query("INSERT INTO discovery_personal_events(workspace_id,id,user_id,discovery_id,version,action) VALUES($1,$2,$3,$4,$5,$6)").bind(w).bind(mutation).bind(a.user.id).bind(item.id).bind(version).bind(action).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
        store::identity_event_correlated(
            tx,
            store::IdentityEvent {
                workspace: w,
                actor: a.user.id,
                entity: a.user.id,
                version,
                team: None,
                action: "discovery.personal_changed",
                access_changed: false,
            },
            mutation,
        )
        .await
        .map_err(db_error)?;
    } else {
        let version =
            item.version.parse::<i64>().map_err(|_| Error::Internal)? + i64::from(deleted);
        graph::one(sqlx::query("INSERT INTO discovery_view_sources(workspace_id,id,owner_id,visibility,team_id,version,deleted_at) VALUES($1,$2,$3,$4,$5,$6,CASE WHEN $7 THEN clock_timestamp() END) ON CONFLICT(workspace_id,id) DO UPDATE SET version=EXCLUDED.version,deleted_at=EXCLUDED.deleted_at").bind(w).bind(item.id).bind(item.owner_id).bind(item.visibility.as_str()).bind(item.team_id).bind(version).bind(deleted).execute(&mut **tx).await.map_err(db_error)?.rows_affected())?;
        let event = match action {
            "discovery.created" => "view.created",
            "discovery.updated" => "view.updated",
            "discovery.deleted" => "view.deleted",
            _ => return Err(Error::Internal),
        };
        store::identity_event_correlated(
            tx,
            store::IdentityEvent {
                workspace: w,
                actor: a.user.id,
                entity: item.id,
                version,
                team: item.team_id,
                action: event,
                access_changed: false,
            },
            mutation,
        )
        .await
        .map_err(db_error)?;
    }
    Ok(())
}

impl App {
    pub async fn discoveries(
        &self,
        a: &Actor,
        w: Uuid,
        q: DiscoveryListQuery,
    ) -> Result<Page<SavedDiscovery>> {
        let limit = q.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) || q.cursor.as_ref().is_some_and(|s| s.len() > 2048) {
            return Err(Error::Invalid(
                "Saved definitions support pages of 1–100 entries".into(),
            ));
        }
        let mut tx = self.scoped(a, w, false).await?;
        let epoch = store::auth_epoch(&mut tx, w).await.map_err(db_error)?;
        let binding = format!(
            "{w}:{}:{epoch}:{limit}:{:?}:{}",
            a.user.id, q.kind, q.favorites
        );
        let after: Option<Uuid> = q
            .cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("discoveries-v1", &binding, c))
            .transpose()?;
        let mut sql = QueryBuilder::<Postgres>::new(store::DISCOVERY_SELECT);
        sql.push(" WHERE d.workspace_id=").push_bind(w);
        if let Some(kind) = q.kind {
            sql.push(" AND d.kind=").push_bind(kind.as_str());
        }
        if q.favorites {
            sql.push(" AND EXISTS(SELECT 1 FROM discovery_favorites f WHERE f.workspace_id=d.workspace_id AND f.discovery_id=d.id)");
        }
        if let Some(after) = after {
            sql.push(" AND d.id>").push_bind(after);
        }
        sql.push(" ORDER BY d.id LIMIT ").push_bind(limit + 1);
        let mut rows = sql.build().fetch_all(&mut *tx).await.map_err(db_error)?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let mut data = Vec::new();
        for row in rows {
            let mut d = store::discovery_from(row).map_err(db_error)?;
            d.can_edit = editable(&mut tx, a, w, d.visibility, d.owner_id, d.team_id).await?;
            authorized_definition(&mut tx, a, w, &mut d).await?;
            data.push(d);
        }
        let next_cursor = if has_more {
            Some(self.taxonomy_cursor_encode(
                "discoveries-v1",
                &binding,
                &data.last().ok_or(Error::Internal)?.id,
            )?)
        } else {
            None
        };
        tx.commit().await.map_err(db_error)?;
        Ok(Page {
            data,
            next_cursor,
            has_more,
        })
    }
    pub async fn discovery(&self, a: &Actor, w: Uuid, id: Uuid) -> Result<SavedDiscovery> {
        let mut tx = self.scoped(a, w, false).await?;
        let item = read(&mut tx, a, w, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn create_discovery(
        &self,
        a: &Actor,
        w: Uuid,
        key: &str,
        mut request: SaveDiscovery,
    ) -> Result<SavedDiscovery> {
        request.validate()?;
        let (kh, rh) = lifecycle::request_hash(key, &request)?;
        let mut tx = self.scoped(a, w, false).await?;
        let replay =
            lifecycle::retry::<Uuid>(&mut tx, w, a.user.id, "discovery-create", &kh, &rh).await?;
        if let Some(id) = replay {
            let item = read(&mut tx, a, w, id).await?;
            tx.commit().await.map_err(db_error)?;
            return Ok(item);
        }
        if !editable(
            &mut tx,
            a,
            w,
            request.visibility,
            a.user.id,
            request.team_id,
        )
        .await?
        {
            return Err(Error::Forbidden);
        }
        let provisional = SavedDiscovery {
            id: Uuid::nil(),
            workspace_id: w,
            owner_id: a.user.id,
            name: request.name.clone(),
            kind: request.kind,
            visibility: request.visibility,
            team_id: request.team_id,
            definition: None,
            repair_required: None,
            favorite: false,
            can_edit: true,
            version: "0".into(),
            etag: String::new(),
        };
        write_scope(&mut tx, a, w, &provisional).await?;
        issue_query::query_clock(&mut tx, &request.definition.query.timezone).await?;
        issue_query::validate_targets(&mut tx, a, w, &request.definition.query).await?;
        // Lock even the first insert so concurrent creates cannot pass the cap together.
        sqlx::query(
            "SELECT user_id FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 FOR UPDATE",
        )
        .bind(w)
        .bind(a.user.id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM saved_discoveries WHERE workspace_id=$1 AND owner_id=$2 AND kind=$3").bind(w).bind(a.user.id).bind(request.kind.as_str()).fetch_one(&mut *tx).await.map_err(db_error)?;
        if count >= 100 {
            return Err(Error::Conflict(
                "Save at most 100 searches and 100 owned views per workspace".into(),
            ));
        }
        let id = Uuid::new_v4();
        graph::one(sqlx::query("INSERT INTO saved_discoveries(workspace_id,id,owner_id,kind,visibility,team_id,name,definition) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(w).bind(id).bind(a.user.id).bind(request.kind.as_str()).bind(request.visibility.as_str()).bind(request.team_id).bind(&request.name).bind(serde_json::to_value(request.definition).map_err(|_|Error::Internal)?).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        lifecycle::save_retry(&mut tx, w, a.user.id, "discovery-create", &kh, &id).await?;
        let item = read(&mut tx, a, w, id).await?;
        record(&mut tx, a, w, &item, "discovery.created", false, false).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn update_discovery(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        etag: &str,
        key: &str,
        mut request: SaveDiscovery,
    ) -> Result<SavedDiscovery> {
        request.validate()?;
        let (kh, rh) = lifecycle::request_hash(key, &json!([etag, request]))?;
        let route = format!("discovery-update:{id}");
        let mut tx = self.scoped(a, w, false).await?;
        let replay = lifecycle::retry::<Uuid>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        lock_personal(&mut tx, w, a.user.id).await?;
        sqlx::query("SELECT id FROM saved_discoveries WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let old = read(&mut tx, a, w, id).await?;
        write_scope(&mut tx, a, w, &old).await?;
        if old.kind != request.kind
            || old.visibility != request.visibility
            || old.team_id != request.team_id
        {
            return Err(Error::Invalid(
                "Create a separate definition to change its kind or audience".into(),
            ));
        }
        if replay.is_none() {
            lifecycle::version(etag, &old.etag, &old)?;
            issue_query::query_clock(&mut tx, &request.definition.query.timezone).await?;
            issue_query::validate_targets(&mut tx, a, w, &request.definition.query).await?;
            graph::one(sqlx::query("UPDATE saved_discoveries SET name=$3,definition=$4,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).bind(request.name).bind(serde_json::to_value(request.definition).map_err(|_|Error::Internal)?).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
            lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &id).await?;
            let changed = read(&mut tx, a, w, id).await?;
            record(&mut tx, a, w, &changed, "discovery.updated", false, false).await?;
        }
        let item = read(&mut tx, a, w, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn delete_discovery(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        kind: DiscoveryKind,
        etag: &str,
        key: &str,
    ) -> Result<()> {
        let (kh, rh) = lifecycle::request_hash(key, &json!([kind, id, etag]))?;
        let route = format!("discovery-delete:{}:{id}", kind.as_str());
        let mut tx = self.scoped(a, w, false).await?;
        let replay = lifecycle::retry::<bool>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        lock_personal(&mut tx, w, a.user.id).await?;
        if replay.is_none() {
            sqlx::query(
                "SELECT id FROM saved_discoveries WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(w)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
            let item = read(&mut tx, a, w, id).await?;
            if item.kind != kind {
                return Err(Error::NotFound);
            }
            write_scope(&mut tx, a, w, &item).await?;
            lifecycle::version(etag, &item.etag, &item)?;
            graph::one(
                sqlx::query("DELETE FROM saved_discoveries WHERE workspace_id=$1 AND id=$2")
                    .bind(w)
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db_error)?
                    .rows_affected(),
            )?;
            lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &true).await?;
            record(&mut tx, a, w, &item, "discovery.deleted", true, false).await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }
    pub async fn favorite_discovery(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        key: &str,
        favorite: bool,
    ) -> Result<SavedDiscovery> {
        let (kh, rh) = lifecycle::request_hash(key, &json!(favorite))?;
        let route = format!("discovery-favorite:{id}");
        let mut tx = self.scoped(a, w, false).await?;
        let replay = lifecycle::retry::<bool>(&mut tx, w, a.user.id, &route, &kh, &rh).await?;
        lock_personal(&mut tx, w, a.user.id).await?;
        let old = read(&mut tx, a, w, id).await?;
        if replay.is_none() {
            if favorite {
                sqlx::query("INSERT INTO discovery_favorites(workspace_id,user_id,discovery_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(w).bind(a.user.id).bind(id).execute(&mut *tx).await.map_err(db_error)?;
            } else {
                sqlx::query("DELETE FROM discovery_favorites WHERE workspace_id=$1 AND user_id=$2 AND discovery_id=$3").bind(w).bind(a.user.id).bind(id).execute(&mut *tx).await.map_err(db_error)?;
            }
            let current = read(&mut tx, a, w, id).await?;
            if current.favorite != favorite {
                return Err(Error::Internal);
            }
            lifecycle::save_retry(&mut tx, w, a.user.id, &route, &kh, &favorite).await?;
            if old.favorite != favorite {
                record(
                    &mut tx,
                    a,
                    w,
                    &current,
                    if favorite {
                        "discovery.favorited"
                    } else {
                        "discovery.unfavorited"
                    },
                    false,
                    true,
                )
                .await?;
            }
        }
        let item = read(&mut tx, a, w, id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(item)
    }
    pub async fn execute_discovery(
        &self,
        a: &Actor,
        w: Uuid,
        id: Uuid,
        cursor: Option<String>,
    ) -> Result<DiscoveryExecution> {
        // Keep the authorization guard until execution finishes. Revocation cannot
        // race the definition read and reintroduce a private saved query.
        let mut tx = self.scoped(a, w, false).await?;
        let item = read(&mut tx, a, w, id).await?;
        let mut query = item
            .definition
            .ok_or_else(|| Error::Invalid("Repair this saved definition before running it".into()))?
            .query;
        query.cursor = cursor;
        let result = if item.kind == DiscoveryKind::Search {
            DiscoveryExecution {
                issues: None,
                search: Some(
                    self.search_in(
                        &mut tx,
                        a,
                        w,
                        SearchRequest {
                            query,
                            types: vec![SearchEntity::Issue],
                        },
                    )
                    .await?,
                ),
            }
        } else {
            DiscoveryExecution {
                issues: Some(self.query_issues_in(&mut tx, a, w, query).await?),
                search: None,
            }
        };
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SearchCursor {
    context: IssueQueryContext,
    position: store::SearchPosition,
}
impl App {
    pub async fn search(&self, a: &Actor, w: Uuid, request: SearchRequest) -> Result<SearchPage> {
        request.validate()?;
        let mut tx = self.scoped(a, w, false).await?;
        let result = self.search_in(&mut tx, a, w, request).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
    async fn search_in(
        &self,
        tx: &mut store::Tx,
        a: &Actor,
        w: Uuid,
        mut request: SearchRequest,
    ) -> Result<SearchPage> {
        request.validate()?;
        request.query.q = request.query.q.map(|q| q.trim().to_owned());
        sqlx::query("SET LOCAL statement_timeout='1000ms'")
            .execute(&mut **tx)
            .await
            .map_err(db_error)?;
        issue_query::validate_targets(tx, a, w, &request.query).await?;
        let epoch = store::auth_epoch(tx, w).await.map_err(db_error)?;
        let as_of = issue_query::query_clock(tx, &request.query.timezone).await?;
        let cursor = request.query.cursor.take();
        let binding = format!(
            "{w}:{}:{epoch}:{}",
            a.user.id,
            serde_json::to_string(&request).map_err(|_| Error::Internal)?
        );
        let saved: Option<SearchCursor> = cursor
            .as_deref()
            .map(|c| self.taxonomy_cursor_decode("search-v1", &binding, c))
            .transpose()?;
        let (context, after) = saved.map(|s| (s.context, Some(s.position))).unwrap_or((
            IssueQueryContext {
                timezone: request.query.timezone.clone(),
                as_of,
            },
            None,
        ));
        let mut rows = store::search(tx, w, a.user.id, &request, &context, after)
            .await
            .map_err(db_error)?;
        let has_more = rows.len() > request.query.limit as usize;
        rows.truncate(request.query.limit as usize);
        let next_cursor = if has_more {
            Some(self.taxonomy_cursor_encode(
                "search-v1",
                &binding,
                &SearchCursor {
                    context: context.clone(),
                    position: rows.last().ok_or(Error::Internal)?.position.clone(),
                },
            )?)
        } else {
            None
        };
        Ok(SearchPage{data:rows.into_iter().map(|r|r.hit).collect(),has_more,next_cursor,query_context:context,fuzzy_degraded:true,notice:Some("Exact keys and full-text search are available. Fuzzy title matching is not enabled in this increment.".into())})
    }
}
