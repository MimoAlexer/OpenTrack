use crate::Tx;
use opentrack_domain::{DiscoveryDefinition, SavedDiscovery};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

pub fn discovery_from(row: PgRow) -> Result<SavedDiscovery, sqlx::Error> {
    let definition = serde_json::from_value::<DiscoveryDefinition>(row.try_get("definition")?)
        .ok()
        .filter(|d| d.validate().is_ok());
    let version: i64 = row.try_get("version")?;
    let id: Uuid = row.try_get("id")?;
    let parse = |key| {
        serde_json::from_value(serde_json::Value::String(row.try_get::<String, _>(key)?))
            .map_err(|_| sqlx::Error::Protocol("Invalid discovery metadata".into()))
    };
    Ok(SavedDiscovery {
        id,
        workspace_id: row.try_get("workspace_id")?,
        owner_id: row.try_get("owner_id")?,
        name: row.try_get("name")?,
        kind: parse("kind")?,
        visibility: serde_json::from_value(serde_json::Value::String(row.try_get("visibility")?))
            .map_err(|_| sqlx::Error::Protocol("Invalid discovery audience".into()))?,
        team_id: row.try_get("team_id")?,
        repair_required: definition
            .is_none()
            .then(|| "This saved definition needs repair before it can run".into()),
        definition,
        favorite: row.try_get("favorite")?,
        can_edit: false,
        version: version.to_string(),
        etag: format!("\"discovery:{id}:{version}\""),
    })
}
pub const DISCOVERY_SELECT: &str = "SELECT d.*,EXISTS(SELECT 1 FROM discovery_favorites f WHERE f.workspace_id=d.workspace_id AND f.discovery_id=d.id AND f.user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid) AS favorite FROM saved_discoveries d";
pub async fn discovery(
    tx: &mut Tx,
    w: Uuid,
    id: Uuid,
) -> Result<Option<SavedDiscovery>, sqlx::Error> {
    let row=sqlx::query("SELECT d.*,EXISTS(SELECT 1 FROM discovery_favorites f WHERE f.workspace_id=d.workspace_id AND f.discovery_id=d.id AND f.user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid) AS favorite FROM saved_discoveries d WHERE d.workspace_id=$1 AND d.id=$2").bind(w).bind(id).fetch_optional(&mut **tx).await?;
    row.map(discovery_from).transpose()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchPosition {
    pub exact: i32,
    pub rank: i64,
    pub kind: String,
    pub id: Uuid,
}
pub struct RankedSearchHit {
    pub hit: opentrack_domain::SearchHit,
    pub position: SearchPosition,
}
pub async fn search(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    request: &opentrack_domain::SearchRequest,
    context: &opentrack_domain::IssueQueryContext,
    after: Option<SearchPosition>,
) -> Result<Vec<RankedSearchHit>, sqlx::Error> {
    use opentrack_domain::{SearchEntity, SearchHit};
    let mut sql = build_search(w, user, request, context, after, false)?;
    let rows = sql.build().fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|r| {
            let kind: String = r.try_get("kind")?;
            let id = r.try_get("id")?;
            Ok(RankedSearchHit {
                hit: SearchHit {
                    id,
                    team_id: r.try_get("team_id")?,
                    kind: if kind == "issue" {
                        SearchEntity::Issue
                    } else {
                        SearchEntity::Project
                    },
                    key: r.try_get("key")?,
                    title: r.try_get("title")?,
                    snippet: r.try_get("snippet")?,
                },
                position: SearchPosition {
                    exact: r.try_get("exact")?,
                    rank: r.try_get("rank")?,
                    kind,
                    id,
                },
            })
        })
        .collect()
}

fn build_search(
    w: Uuid,
    user: Uuid,
    request: &opentrack_domain::SearchRequest,
    context: &opentrack_domain::IssueQueryContext,
    after: Option<SearchPosition>,
    explain: bool,
) -> Result<sqlx::QueryBuilder<sqlx::Postgres>, sqlx::Error> {
    use opentrack_domain::SearchEntity;
    use sqlx::{Postgres, QueryBuilder};
    let term = request.query.q.as_deref().unwrap_or("").trim();
    let mut sql = QueryBuilder::<Postgres>::new(if explain {
        "EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) WITH input AS (SELECT "
    } else {
        "WITH input AS (SELECT "
    });
    sql.push_bind(term.to_owned())
        .push("::text AS term,plainto_tsquery('simple',")
        .push_bind(term.to_owned())
        .push(") AS terms), query_clock AS (SELECT ")
        .push_bind(context.as_of)
        .push("::timestamptz AS as_of,")
        .push_bind(context.timezone.clone())
        .push("::text AS timezone), hits AS (");
    let mut union = false;
    if request.types.contains(&SearchEntity::Issue) {
        sql.push("SELECT i.id,i.team_id,'issue'::text AS kind,t.key||'-'||i.number AS key,i.title,left(i.description,240) AS snippet,CASE WHEN upper(input.term)=t.key||'-'||i.number OR EXISTS(SELECT 1 FROM issue_key_aliases ka WHERE ka.workspace_id=i.workspace_id AND ka.issue_id=i.id AND upper(input.term)=ka.team_prefix||'-'||ka.number) THEN 1 ELSE 0 END AS exact,round(ts_rank_cd(i.search_document,input.terms)*1000000)::bigint AS rank FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id CROSS JOIN input CROSS JOIN query_clock WHERE i.workspace_id=").push_bind(w).push(" AND i.deleted_at IS NULL AND i.archived_at IS NULL AND t.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=").push_bind(user).push(")) AND (i.search_document@@input.terms OR upper(input.term)=t.key||'-'||i.number OR EXISTS(SELECT 1 FROM issue_key_aliases ka WHERE ka.workspace_id=i.workspace_id AND ka.issue_id=i.id AND upper(input.term)=ka.team_prefix||'-'||ka.number))");
        if let Some(team) = request.query.team_id {
            sql.push(" AND i.team_id=").push_bind(team);
        }
        if let Some(node) = &request.query.filter {
            sql.push(" AND (");
            crate::issue_query::push_node(&mut sql, node)?;
            sql.push(")");
        }
        union = true;
    }
    if request.types.contains(&SearchEntity::Project) {
        if union {
            sql.push(" UNION ALL ");
        }
        sql.push("SELECT p.id,p.team_id,'project'::text AS kind,NULL::text AS key,p.name AS title,left(coalesce(d.plain_text,''),240) AS snippet,0::integer AS exact,round(ts_rank_cd(p.search_document||coalesce(d.search_document,''::tsvector),input.terms)*1000000)::bigint AS rank FROM projects p JOIN teams t ON t.workspace_id=p.workspace_id AND t.id=p.team_id LEFT JOIN documents d ON d.workspace_id=p.workspace_id AND d.project_id=p.id AND d.deleted_at IS NULL CROSS JOIN input WHERE p.workspace_id=").push_bind(w).push(" AND p.archived_at IS NULL AND t.archived_at IS NULL AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=").push_bind(user).push(")) AND (p.search_document||coalesce(d.search_document,''::tsvector))@@input.terms");
    }
    sql.push(") SELECT * FROM hits");
    if let Some(after) = after {
        sql.push(" WHERE (-exact,-rank,kind,id)>(")
            .push_bind(-after.exact)
            .push(",")
            .push_bind(-after.rank)
            .push(",")
            .push_bind(after.kind)
            .push(",")
            .push_bind(after.id)
            .push(")");
    }
    sql.push(" ORDER BY exact DESC,rank DESC,kind,id LIMIT ")
        .push_bind(request.query.limit + 1);
    Ok(sql)
}
/// Operator/test diagnostics use the exact bound production query. No API route
/// exposes this plan, which may contain authorized source identifiers.
pub async fn explain_search(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    request: &opentrack_domain::SearchRequest,
    context: &opentrack_domain::IssueQueryContext,
) -> Result<serde_json::Value, sqlx::Error> {
    let mut query = build_search(w, user, request, context, None, true)?;
    query.build_query_scalar().fetch_one(&mut **tx).await
}

/// Hook for the realtime registry. Personal invalidations target the caller's
/// principal UUID, so deletion does not require retaining a private definition.
/// Shared events use audience-only tombstones and current team visibility.
pub async fn discovery_event_visible(
    tx: &mut Tx,
    w: Uuid,
    user: Uuid,
    event_type: &str,
    entity: Uuid,
) -> Result<bool, sqlx::Error> {
    match event_type {
        "discovery.personal_changed" if entity==user=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discovery_personal_heads h JOIN workspace_members m ON m.workspace_id=h.workspace_id AND m.user_id=h.user_id WHERE h.workspace_id=$1 AND h.user_id=$2 AND m.active)").bind(w).bind(user).fetch_one(&mut **tx).await,
        "view.created"|"view.updated"|"view.deleted"=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discovery_view_sources s JOIN workspace_members m ON m.workspace_id=s.workspace_id AND m.user_id=$2 WHERE s.workspace_id=$1 AND s.id=$3 AND m.active AND (s.visibility='workspace' OR EXISTS(SELECT 1 FROM teams t WHERE t.workspace_id=s.workspace_id AND t.id=s.team_id AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=$2)))))").bind(w).bind(user).bind(entity).fetch_one(&mut **tx).await,
        _=>Ok(false),
    }
}

/// Startup checks the additive discovery relations and rebuildable projections;
/// no user content or new database executable capability is needed.
pub async fn discovery_schema_ready(pool: &sqlx::PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT (SELECT count(*)=5 AND bool_and(c.relrowsecurity AND c.relforcerowsecurity) FROM pg_class c WHERE c.relnamespace=current_schema()::regnamespace AND c.relname IN('saved_discoveries','discovery_favorites','discovery_personal_heads','discovery_personal_events','discovery_view_sources') AND c.relkind='r') AND (SELECT count(*)=3 AND bool_and(a.attgenerated='s') FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid WHERE c.relnamespace=current_schema()::regnamespace AND c.relname IN('issues','projects','documents') AND a.attname='search_document' AND NOT a.attisdropped)").fetch_one(pool).await
}
