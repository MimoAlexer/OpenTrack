CREATE TABLE labels (
 id uuid PRIMARY KEY,
 workspace_id uuid NOT NULL REFERENCES workspaces(id),
 team_id uuid,
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 100 AND name=btrim(name) AND name !~ '[[:cntrl:]]'),
 color text NOT NULL CHECK(color IN('gray','slate','red','orange','amber','green','teal','blue','indigo','purple','pink')),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 archived_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(workspace_id,id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id)
);
CREATE UNIQUE INDEX labels_live_name ON labels(workspace_id,team_id,lower(name)) NULLS NOT DISTINCT WHERE archived_at IS NULL;
CREATE INDEX labels_directory ON labels(workspace_id,lower(name),id);
CREATE TABLE issue_labels (
 workspace_id uuid NOT NULL,
 issue_id uuid NOT NULL,
 label_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,issue_id,label_id),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),
 FOREIGN KEY(workspace_id,label_id) REFERENCES labels(workspace_id,id)
);
CREATE INDEX issue_labels_reverse ON issue_labels(workspace_id,label_id,issue_id);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['labels','issue_labels'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;
CREATE FUNCTION label_scope_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF (NEW.id,NEW.workspace_id,NEW.team_id) IS DISTINCT FROM (OLD.id,OLD.workspace_id,OLD.team_id) THEN
  RAISE EXCEPTION 'Label identity and scope are immutable' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION issue_label_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE w uuid; i uuid; t uuid; label_team uuid; label_archived timestamptz;
BEGIN
 IF TG_OP='UPDATE' AND NEW IS DISTINCT FROM OLD THEN
  RAISE EXCEPTION 'Replace label links through insert and delete' USING ERRCODE='23514';
 END IF;
 w:=COALESCE(NEW.workspace_id,OLD.workspace_id); i:=COALESCE(NEW.issue_id,OLD.issue_id);
 SELECT team_id INTO t FROM issues WHERE workspace_id=w AND id=i;
 PERFORM 1 FROM teams WHERE workspace_id=w AND id=t FOR UPDATE;
 SELECT team_id INTO t FROM issues WHERE workspace_id=w AND id=i FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Issue unavailable' USING ERRCODE='23503'; END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 SELECT team_id,archived_at INTO label_team,label_archived FROM labels WHERE workspace_id=w AND id=NEW.label_id FOR SHARE;
 IF NOT FOUND OR (label_team IS NOT NULL AND label_team<>t) THEN
  RAISE EXCEPTION 'Label is incompatible with the issue' USING ERRCODE='23514';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM issue_labels WHERE workspace_id=w AND issue_id=i AND label_id=NEW.label_id) THEN
  IF label_archived IS NOT NULL OR (SELECT count(*) FROM issue_labels WHERE workspace_id=w AND issue_id=i)>=50 THEN
   RAISE EXCEPTION 'New label link is unavailable or exceeds the limit' USING ERRCODE='23514';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION issue_label_team_consistent() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 -- Status-only replacement workers do not need label-table authority.
 IF (NEW.workspace_id,NEW.team_id) IS NOT DISTINCT FROM (OLD.workspace_id,OLD.team_id) THEN RETURN NULL; END IF;
 IF EXISTS(SELECT 1 FROM issues i JOIN issue_labels il ON il.workspace_id=i.workspace_id AND il.issue_id=i.id
   JOIN labels l ON l.workspace_id=il.workspace_id AND l.id=il.label_id
   WHERE i.workspace_id=NEW.workspace_id AND i.id=NEW.id AND l.team_id IS NOT NULL AND l.team_id<>i.team_id) THEN
  RAISE EXCEPTION 'Issue team is incompatible with retained labels' USING ERRCODE='23514';
 END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER labels_scope_immutable BEFORE UPDATE ON labels FOR EACH ROW EXECUTE FUNCTION label_scope_immutable();
CREATE TRIGGER issue_labels_scope BEFORE INSERT OR UPDATE OR DELETE ON issue_labels FOR EACH ROW EXECUTE FUNCTION issue_label_guard();
CREATE CONSTRAINT TRIGGER issue_labels_team_consistent AFTER UPDATE ON issues DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION issue_label_team_consistent();
DO $$ DECLARE f regprocedure; BEGIN
 FOREACH f IN ARRAY ARRAY['label_scope_immutable()'::regprocedure,'issue_label_guard()'::regprocedure,'issue_label_team_consistent()'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path TO pg_catalog,%I,pg_temp',f,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
END $$;
