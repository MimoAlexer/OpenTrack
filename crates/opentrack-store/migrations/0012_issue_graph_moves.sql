ALTER TABLE teams ADD COLUMN graph_version bigint NOT NULL DEFAULT 1 CHECK(graph_version>0);
ALTER TABLE issues ADD COLUMN parent_id uuid;
ALTER TABLE issues ADD CONSTRAINT issues_team_identity UNIQUE(workspace_id,team_id,id);
ALTER TABLE issues ADD CONSTRAINT issues_parent_same_team FOREIGN KEY(workspace_id,team_id,parent_id)
 REFERENCES issues(workspace_id,team_id,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE issues ADD CONSTRAINT issues_not_own_parent CHECK(parent_id IS DISTINCT FROM id);
CREATE INDEX issues_children ON issues(workspace_id,team_id,parent_id,id);
CREATE TABLE issue_relations (
 id uuid PRIMARY KEY,
 workspace_id uuid NOT NULL,
 team_id uuid NOT NULL,
 source_id uuid NOT NULL,
 target_id uuid NOT NULL,
 kind text NOT NULL CHECK(kind IN('blocks','related','duplicate')),
 created_by uuid NOT NULL REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK(source_id<>target_id),
 CHECK(kind<>'related' OR source_id<target_id),
 UNIQUE(workspace_id,id),
 UNIQUE(workspace_id,team_id,kind,source_id,target_id),
 FOREIGN KEY(workspace_id,team_id,source_id) REFERENCES issues(workspace_id,team_id,id) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(workspace_id,team_id,target_id) REFERENCES issues(workspace_id,team_id,id) DEFERRABLE INITIALLY DEFERRED
);
CREATE UNIQUE INDEX issue_duplicate_once ON issue_relations(workspace_id,source_id) WHERE kind='duplicate';
CREATE INDEX issue_relations_source ON issue_relations(workspace_id,team_id,source_id,id);
CREATE INDEX issue_relations_target ON issue_relations(workspace_id,team_id,target_id,id);
CREATE TABLE issue_key_aliases (
 workspace_id uuid NOT NULL,
 team_prefix text NOT NULL,
 number bigint NOT NULL CHECK(number>0),
 issue_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,team_prefix,number),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),
 FOREIGN KEY(workspace_id,team_prefix) REFERENCES teams(workspace_id,key)
);
CREATE INDEX issue_key_aliases_issue ON issue_key_aliases(workspace_id,issue_id);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['issue_relations','issue_key_aliases'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;

CREATE FUNCTION issue_parent_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ancestor uuid; visited uuid[]; frontier uuid[]; next_frontier uuid[]; ancestors integer:=0; height integer:=1;
BEGIN
 -- The restricted status worker never needs access to graph tables/columns.
 IF TG_OP='UPDATE' AND (NEW.workspace_id,NEW.team_id,NEW.parent_id) IS NOT DISTINCT FROM (OLD.workspace_id,OLD.team_id,OLD.parent_id) THEN RETURN NEW; END IF;
 IF TG_OP='UPDATE' AND NEW.workspace_id<>OLD.workspace_id THEN
  RAISE EXCEPTION 'Issue workspace is immutable' USING ERRCODE='23514';
 END IF;
 IF TG_OP='UPDATE' THEN
  PERFORM id FROM teams WHERE workspace_id=NEW.workspace_id AND id IN(OLD.team_id,NEW.team_id) ORDER BY id FOR UPDATE;
 ELSE
  PERFORM id FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
 END IF;
 -- These statements run after acquiring the graph lock, so a waiting writer
 -- validates its predecessor's committed graph rather than its earlier snapshot.
 ancestor:=NEW.parent_id; visited:=ARRAY[NEW.id];
 WHILE ancestor IS NOT NULL LOOP
  IF ancestor=ANY(visited) THEN RAISE EXCEPTION 'Parent cycle is invalid' USING ERRCODE='23514',CONSTRAINT='issue_graph_invalid'; END IF;
  ancestors:=ancestors+1;
  IF ancestors>=8 THEN RAISE EXCEPTION 'Parent depth exceeds eight levels' USING ERRCODE='23514',CONSTRAINT='issue_graph_invalid'; END IF;
  visited:=array_append(visited,ancestor);
  SELECT parent_id INTO ancestor FROM issues WHERE workspace_id=NEW.workspace_id AND team_id=NEW.team_id AND id=ancestor;
  IF NOT FOUND THEN RAISE EXCEPTION 'Parent unavailable' USING ERRCODE='23503'; END IF;
 END LOOP;
 visited:=ARRAY[NEW.id]; frontier:=visited;
 LOOP
  SELECT COALESCE(array_agg(id),'{}'::uuid[]) INTO next_frontier FROM (
   SELECT id FROM issues WHERE workspace_id=NEW.workspace_id AND team_id=NEW.team_id AND parent_id=ANY(frontier) AND NOT(id=ANY(visited)) LIMIT 10001
  ) descendants;
  EXIT WHEN cardinality(next_frontier)=0;
  visited:=visited||next_frontier;
  IF cardinality(visited)>10000 THEN RAISE EXCEPTION 'Graph traversal capacity exceeded' USING ERRCODE='23514',CONSTRAINT='issue_graph_capacity'; END IF;
  height:=height+1;
  IF ancestors+height>8 THEN RAISE EXCEPTION 'Parent depth exceeds eight levels' USING ERRCODE='23514',CONSTRAINT='issue_graph_invalid'; END IF;
  frontier:=next_frontier;
 END LOOP;
 RETURN NEW;
END $$;
CREATE TRIGGER issues_parent_guard BEFORE INSERT OR UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION issue_parent_guard();

CREATE FUNCTION issue_relation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE row_data issue_relations; visited uuid[];
BEGIN
 IF TG_OP='UPDATE' AND NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION 'Replace relationships through insert and delete' USING ERRCODE='23514'; END IF;
 IF TG_OP='DELETE' THEN row_data:=OLD; ELSE row_data:=NEW; END IF;
 PERFORM 1 FROM teams WHERE workspace_id=row_data.workspace_id AND id=row_data.team_id FOR UPDATE;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 IF NEW.kind='blocks' THEN
  WITH RECURSIVE walk(id) AS (
   SELECT NEW.target_id UNION
   -- Expand by the globally unique source issue. The scoped endpoint FKs keep
   -- every edge in its issue's team. A redundant team predicate makes cold-table
   -- plans estimate one team edge and scan the target index at every step.
   -- Keep this boundary so the source lookup stays parameterized by the frontier.
   SELECT r.target_id FROM walk x CROSS JOIN LATERAL (
    SELECT target_id FROM issue_relations r
     WHERE r.workspace_id=NEW.workspace_id AND r.kind='blocks'
      AND r.source_id=x.id OFFSET 0
   ) r
  ) SELECT array_agg(id) INTO visited FROM (SELECT id FROM walk LIMIT 10001) bounded;
  IF cardinality(visited)>10000 THEN RAISE EXCEPTION 'Graph traversal capacity exceeded' USING ERRCODE='23514',CONSTRAINT='issue_graph_capacity'; END IF;
  IF NEW.source_id=ANY(visited) THEN RAISE EXCEPTION 'Blocking cycle is invalid' USING ERRCODE='23514',CONSTRAINT='issue_graph_invalid'; END IF;
 ELSIF NEW.kind='duplicate' THEN
  IF EXISTS(SELECT 1 FROM issue_relations WHERE workspace_id=NEW.workspace_id AND kind='duplicate' AND (source_id=NEW.target_id OR target_id=NEW.source_id)) THEN
   RAISE EXCEPTION 'Duplicates must point directly to a canonical issue' USING ERRCODE='23514',CONSTRAINT='issue_graph_invalid';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issue_relations_guard BEFORE INSERT OR UPDATE OR DELETE ON issue_relations FOR EACH ROW EXECUTE FUNCTION issue_relation_guard();

CREATE FUNCTION issue_key_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE prefix text;
BEGIN
 IF TG_OP='UPDATE' AND (NEW.workspace_id,NEW.team_id,NEW.number) IS NOT DISTINCT FROM (OLD.workspace_id,OLD.team_id,OLD.number) THEN RETURN NEW; END IF;
 SELECT key INTO prefix FROM teams WHERE workspace_id=NEW.workspace_id AND id=NEW.team_id FOR UPDATE;
 IF EXISTS(SELECT 1 FROM issue_key_aliases WHERE workspace_id=NEW.workspace_id AND team_prefix=prefix AND number=NEW.number AND issue_id<>NEW.id) THEN
  RAISE EXCEPTION 'Issue key is reserved by history' USING ERRCODE='23505';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issues_key_guard BEFORE INSERT OR UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION issue_key_guard();
CREATE FUNCTION issue_alias_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Historical keys are immutable' USING ERRCODE='23514'; END IF;
 PERFORM 1 FROM teams WHERE workspace_id=NEW.workspace_id AND key=NEW.team_prefix FOR UPDATE;
 IF EXISTS(SELECT 1 FROM issues i JOIN teams t ON t.workspace_id=i.workspace_id AND t.id=i.team_id
   WHERE i.workspace_id=NEW.workspace_id AND t.key=NEW.team_prefix AND i.number=NEW.number AND i.id<>NEW.issue_id) THEN
  RAISE EXCEPTION 'Historical key is already in use' USING ERRCODE='23505';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issue_aliases_guard BEFORE INSERT OR UPDATE OR DELETE ON issue_key_aliases FOR EACH ROW EXECUTE FUNCTION issue_alias_guard();
CREATE FUNCTION team_key_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF (NEW.workspace_id,NEW.id,NEW.key) IS DISTINCT FROM (OLD.workspace_id,OLD.id,OLD.key) THEN RAISE EXCEPTION 'Team key and scope are immutable' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER teams_key_immutable BEFORE UPDATE ON teams FOR EACH ROW EXECUTE FUNCTION team_key_immutable();
DO $$ DECLARE f regprocedure; BEGIN
 FOREACH f IN ARRAY ARRAY['issue_parent_guard()'::regprocedure,'issue_relation_guard()'::regprocedure,'issue_key_guard()'::regprocedure,'issue_alias_guard()'::regprocedure,'team_key_immutable()'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path TO pg_catalog,%I,pg_temp',f,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
END $$;
