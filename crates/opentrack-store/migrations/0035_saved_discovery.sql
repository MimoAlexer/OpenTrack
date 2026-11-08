-- Definitions are durable; cursors and query clocks are never persisted here.
CREATE TABLE saved_discoveries (
 workspace_id uuid NOT NULL,id uuid NOT NULL,owner_id uuid NOT NULL,
 kind text NOT NULL CHECK(kind IN('search','view')),
 visibility text NOT NULL CHECK(visibility IN('personal','team','workspace')),
 team_id uuid,name text NOT NULL CHECK(length(name) BETWEEN 1 AND 100 AND name=btrim(name) AND name !~ '[[:cntrl:]]'),
 definition jsonb NOT NULL CHECK(jsonb_typeof(definition)='object'),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,id),
 FOREIGN KEY(workspace_id,owner_id) REFERENCES workspace_members(workspace_id,user_id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),
 CHECK((visibility='team')=(team_id IS NOT NULL)),CHECK(kind<>'search' OR visibility='personal')
);
CREATE INDEX saved_discoveries_owner ON saved_discoveries(workspace_id,owner_id,kind,id);
CREATE INDEX saved_discoveries_audience ON saved_discoveries(workspace_id,visibility,team_id,id);
CREATE TABLE discovery_favorites (
 workspace_id uuid NOT NULL,user_id uuid NOT NULL,discovery_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,user_id,discovery_id),
 FOREIGN KEY(workspace_id,user_id) REFERENCES workspace_members(workspace_id,user_id),
 FOREIGN KEY(workspace_id,discovery_id) REFERENCES saved_discoveries(workspace_id,id) ON DELETE CASCADE
);
ALTER TABLE saved_discoveries ENABLE ROW LEVEL SECURITY;
ALTER TABLE saved_discoveries FORCE ROW LEVEL SECURITY;
CREATE POLICY discovery_scope ON saved_discoveries USING (
 workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid
 AND EXISTS(SELECT 1 FROM workspace_members m WHERE m.workspace_id=saved_discoveries.workspace_id AND m.user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid AND m.active)
 AND (visibility='workspace' OR visibility='personal' AND owner_id=nullif(current_setting('opentrack.user_id',true),'')::uuid
 OR visibility='team' AND EXISTS(SELECT 1 FROM teams t WHERE t.workspace_id=saved_discoveries.workspace_id AND t.id=saved_discoveries.team_id AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid))))
);
ALTER TABLE discovery_favorites ENABLE ROW LEVEL SECURITY;
ALTER TABLE discovery_favorites FORCE ROW LEVEL SECURITY;
CREATE POLICY favorites_scope ON discovery_favorites USING (
 workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid AND user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid
);
-- Generated search projections follow source writes, including document triggers.
ALTER TABLE issues ADD COLUMN search_document tsvector GENERATED ALWAYS AS (
 setweight(to_tsvector('simple'::regconfig,title),'A') || setweight(to_tsvector('simple'::regconfig,description),'B')
) STORED;
CREATE INDEX issues_search_document ON issues USING gin(search_document) WHERE deleted_at IS NULL;
ALTER TABLE projects ADD COLUMN search_document tsvector GENERATED ALWAYS AS (
 setweight(to_tsvector('simple'::regconfig,name),'A')
) STORED;
CREATE INDEX projects_search_document ON projects USING gin(search_document);

-- Project narrative snapshots on projects are historical. Current briefs live
-- in documents, so index that authoritative plain text, never the old snapshot.
ALTER TABLE documents ADD COLUMN search_document tsvector GENERATED ALWAYS AS (
 CASE WHEN project_id IS NOT NULL THEN setweight(to_tsvector('simple'::regconfig,plain_text),'B') ELSE ''::tsvector END
) STORED;
CREATE INDEX project_documents_search ON documents USING gin(search_document) WHERE project_id IS NOT NULL AND deleted_at IS NULL;

-- A personal invalidation targets the principal, never a private definition UUID.
-- Its private ledger retains source attribution even after definition deletion.
CREATE TABLE discovery_personal_heads (
 workspace_id uuid NOT NULL,user_id uuid NOT NULL,version bigint NOT NULL CHECK(version>0),
 PRIMARY KEY(workspace_id,user_id),FOREIGN KEY(workspace_id,user_id) REFERENCES workspace_members(workspace_id,user_id)
);
CREATE TABLE discovery_personal_events (
 workspace_id uuid NOT NULL,id uuid NOT NULL,user_id uuid NOT NULL,discovery_id uuid NOT NULL,
 version bigint NOT NULL CHECK(version>0),action text NOT NULL CHECK(action IN('discovery.created','discovery.updated','discovery.deleted','discovery.favorited','discovery.unfavorited')),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,id),UNIQUE(workspace_id,user_id,version),
 FOREIGN KEY(workspace_id,user_id) REFERENCES discovery_personal_heads(workspace_id,user_id)
);
CREATE INDEX discovery_personal_source ON discovery_personal_events(workspace_id,user_id,discovery_id,version);
-- Shared audience facts survive deletion so the ordinary event registry can
-- authorize a tombstone without retaining a title, query, AST or snippet.
CREATE TABLE discovery_view_sources (
 workspace_id uuid NOT NULL,id uuid NOT NULL,owner_id uuid NOT NULL,visibility text NOT NULL CHECK(visibility IN('team','workspace')),team_id uuid,
 version bigint NOT NULL CHECK(version>0),deleted_at timestamptz,
 PRIMARY KEY(workspace_id,id),FOREIGN KEY(workspace_id,owner_id) REFERENCES workspace_members(workspace_id,user_id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),CHECK((visibility='team')=(team_id IS NOT NULL))
);
DO $$ DECLARE tab text;BEGIN
 FOREACH tab IN ARRAY ARRAY['discovery_personal_heads','discovery_personal_events'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY personal_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid AND user_id=nullif(current_setting(''opentrack.user_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;
ALTER TABLE discovery_view_sources ENABLE ROW LEVEL SECURITY;
ALTER TABLE discovery_view_sources FORCE ROW LEVEL SECURITY;
CREATE POLICY view_source_scope ON discovery_view_sources USING (
 workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid
 AND (visibility='workspace' OR EXISTS(SELECT 1 FROM teams t WHERE t.workspace_id=discovery_view_sources.workspace_id AND t.id=discovery_view_sources.team_id AND (NOT t.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=t.workspace_id AND tm.team_id=t.id AND tm.user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid))))
);
CREATE TRIGGER discovery_personal_events_immutable BEFORE UPDATE OR DELETE ON discovery_personal_events FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER discovery_personal_events_no_truncate BEFORE TRUNCATE ON discovery_personal_events FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER discovery_personal_heads_no_delete BEFORE DELETE ON discovery_personal_heads FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER discovery_view_sources_no_delete BEFORE DELETE ON discovery_view_sources FOR EACH ROW EXECUTE FUNCTION audit_append_only();
