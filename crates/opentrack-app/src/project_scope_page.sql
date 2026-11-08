-- Scope comparisons and event pages share one snapshot of live source visibility.
WITH visible AS NOT MATERIALIZED (
    SELECT i.id, ct.key || '-' || i.number AS key, i.title
    FROM issues i JOIN teams ct ON ct.workspace_id = i.workspace_id AND ct.id = i.team_id
    WHERE i.workspace_id = $1 AND i.deleted_at IS NULL
      AND (NOT ct.private OR EXISTS (
          SELECT 1 FROM team_members tm
          WHERE tm.workspace_id = $1 AND tm.team_id = ct.id AND tm.user_id = $4
      ))
), membership AS (
    SELECT issue_id, bool_or(included) AS included, bool_or(agreed) AS agreed
    FROM (
        SELECT issue_id, true AS included, false AS agreed FROM project_issues
        WHERE workspace_id = $1 AND project_id = $3
        UNION ALL
        SELECT issue_id, false, true FROM project_scope_baseline_items
        WHERE workspace_id = $1 AND baseline_id = $5
    ) sources
    GROUP BY issue_id
), states AS (
    SELECT v.*, m.included, m.agreed FROM membership m
    CROSS JOIN LATERAL (SELECT * FROM visible WHERE id = m.issue_id LIMIT 1) v
), paged AS (
    SELECT * FROM states WHERE $9::uuid IS NULL OR id > $9 ORDER BY id LIMIT $8
), items AS (
    SELECT p.*, coalesce(last.action,'removed') AS last_action, coalesce(last.reason,'') AS reason
    FROM (SELECT * FROM paged ORDER BY id LIMIT $10) p
    LEFT JOIN LATERAL (
        SELECT e.action, e.reason FROM project_scope_events e
        WHERE e.workspace_id = $1 AND e.project_id = $3 AND e.issue_id = p.id AND e.ordinal > $6
          AND $5 IS NOT NULL AND p.included <> p.agreed
        ORDER BY e.ordinal DESC LIMIT 1
    ) last ON true
), events AS (
    SELECT e.*, v.key, v.title, u.display_name AS actor_name
    FROM project_scope_events e JOIN visible v ON v.id = e.issue_id
    LEFT JOIN users u ON u.id = e.actor_id
    WHERE e.workspace_id = $1 AND e.team_id = $2 AND e.project_id = $3 AND e.ordinal > $6
      AND e.ordinal <= $7
    ORDER BY e.ordinal DESC LIMIT $8
)
SELECT
 (SELECT count(*) FROM paged)>$10 AS items_has_more,
 (SELECT id FROM items ORDER BY id DESC LIMIT 1) AS last_item,
 (SELECT count(*) FROM states WHERE included AND agreed) AS unchanged,
 coalesce((SELECT jsonb_agg(jsonb_build_object('id',id,'key',key,'title',title,'reason','') ORDER BY key,id) FROM items WHERE included),'[]') AS current,
 coalesce((SELECT jsonb_agg(jsonb_build_object('id',id,'key',key,'title',title,'reason',reason) ORDER BY key,id) FROM items WHERE included AND NOT agreed),'[]') AS added,
 coalesce((SELECT jsonb_agg(jsonb_build_object('id',id,'key',key,'title',title,'reason',reason) ORDER BY key,id) FROM items WHERE agreed AND NOT included AND last_action<>'postponed'),'[]') AS removed,
 coalesce((SELECT jsonb_agg(jsonb_build_object('id',id,'key',key,'title',title,'reason',reason) ORDER BY key,id) FROM items WHERE agreed AND NOT included AND last_action='postponed'),'[]') AS postponed,
 coalesce((SELECT jsonb_agg(jsonb_build_object('ordinal',ordinal,'id',id,'action',action,'reason',reason,'issue',jsonb_build_object('id',issue_id,'key',key,'title',title,'reason',''),'actor_id',actor_id,'actor_name',actor_name,'created_at',created_at) ORDER BY ordinal DESC) FROM events),'[]') AS events
