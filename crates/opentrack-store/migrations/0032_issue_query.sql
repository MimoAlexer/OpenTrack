-- Add access paths for structured date predicates and the supported priority
-- ordering. Existing source relations, current-access checks and rows are
-- unchanged; indexes remain rebuildable projections.
CREATE INDEX issues_query_updated ON issues(workspace_id,updated_at,id)
 WHERE deleted_at IS NULL;
CREATE INDEX issues_query_due ON issues(workspace_id,due_date,created_at DESC,id DESC)
 WHERE deleted_at IS NULL;
CREATE INDEX issues_query_priority ON issues(
 workspace_id,(CASE WHEN priority=0 THEN 5 ELSE priority END),created_at DESC,id DESC)
 WHERE deleted_at IS NULL;
