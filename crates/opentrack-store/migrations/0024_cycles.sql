-- Extension ownership is database-wide even for isolated application schemas.
SELECT pg_advisory_xact_lock(hashtextextended('opentrack:btree_gist:'||current_database(),0));
CREATE EXTENSION IF NOT EXISTS btree_gist WITH SCHEMA public;
CREATE TABLE cycles (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160 AND name=btrim(name) AND name !~ '[[:cntrl:]]'),
 start_date date NOT NULL,end_date_exclusive date NOT NULL,timezone text NOT NULL,
 state text NOT NULL DEFAULT 'planned' CHECK(state IN('planned','active','closed')),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),closed_at timestamptz,deleted_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK(end_date_exclusive>start_date AND end_date_exclusive-start_date<=366),
 CHECK((state='closed')=(closed_at IS NOT NULL)),CHECK(deleted_at IS NULL OR state='planned'),
 UNIQUE(workspace_id,id),UNIQUE(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),
 CONSTRAINT cycles_no_overlap EXCLUDE USING gist (workspace_id public.gist_uuid_ops WITH =,team_id public.gist_uuid_ops WITH =,daterange(start_date,end_date_exclusive,'[)') WITH &&) WHERE(deleted_at IS NULL)
);
CREATE INDEX cycles_directory ON cycles(workspace_id,team_id,start_date DESC,id);
CREATE TABLE cycle_issues (
 workspace_id uuid NOT NULL,team_id uuid NOT NULL,cycle_id uuid NOT NULL,issue_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,issue_id),
 FOREIGN KEY(workspace_id,team_id,cycle_id) REFERENCES cycles(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,team_id,issue_id) REFERENCES issues(workspace_id,team_id,id) ON DELETE CASCADE
);
CREATE INDEX cycle_issues_scope ON cycle_issues(workspace_id,cycle_id,issue_id);
CREATE TABLE cycle_snapshots (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,cycle_id uuid NOT NULL,
 local_date date NOT NULL,captured_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 kind text NOT NULL CHECK(kind IN('opened','daily','scope_changed','closed','reopened','carry_forward')),
 definition_version integer NOT NULL CHECK(definition_version=1),scope_count bigint NOT NULL CHECK(scope_count>=0),
 completed_count bigint NOT NULL CHECK(completed_count>=0),canceled_count bigint NOT NULL CHECK(canceled_count>=0),
 unfinished_count bigint NOT NULL CHECK(unfinished_count>=0),excluded_count bigint NOT NULL CHECK(excluded_count>=0),
 estimate_total bigint NOT NULL CHECK(estimate_total>=0),items jsonb NOT NULL CHECK(jsonb_typeof(items)='array'),
 CHECK(scope_count=completed_count+canceled_count+unfinished_count),
 UNIQUE(workspace_id,id),UNIQUE(workspace_id,team_id,cycle_id,id),
 FOREIGN KEY(workspace_id,team_id,cycle_id) REFERENCES cycles(workspace_id,team_id,id)
);
CREATE UNIQUE INDEX cycle_daily_snapshot ON cycle_snapshots(workspace_id,cycle_id,local_date) WHERE kind='daily';
CREATE INDEX cycle_snapshots_history ON cycle_snapshots(workspace_id,cycle_id,captured_at DESC,id DESC);
CREATE TABLE cycle_close_reports (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,cycle_id uuid NOT NULL,snapshot_id uuid NOT NULL,
 cycle_version bigint NOT NULL,cycle_body jsonb NOT NULL,closed_at timestamptz NOT NULL,UNIQUE(workspace_id,id),UNIQUE(workspace_id,team_id,cycle_id,id),
 FOREIGN KEY(workspace_id,team_id,cycle_id,snapshot_id) REFERENCES cycle_snapshots(workspace_id,team_id,cycle_id,id)
);
CREATE INDEX cycle_reports_history ON cycle_close_reports(workspace_id,cycle_id,closed_at DESC,id DESC);
CREATE TABLE cycle_carry_operations (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,source_cycle_id uuid NOT NULL,destination_cycle_id uuid NOT NULL,
 report_id uuid NOT NULL,version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 state text NOT NULL CHECK(state IN('complete','needs_review')),results jsonb NOT NULL CHECK(jsonb_typeof(results)='array'),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK(source_cycle_id<>destination_cycle_id),
 FOREIGN KEY(workspace_id,team_id,source_cycle_id,report_id) REFERENCES cycle_close_reports(workspace_id,team_id,cycle_id,id),
 FOREIGN KEY(workspace_id,team_id,destination_cycle_id) REFERENCES cycles(workspace_id,team_id,id)
);
CREATE TABLE cycle_reopen_reasons (
 id uuid PRIMARY KEY,workspace_id uuid NOT NULL,team_id uuid NOT NULL,cycle_id uuid NOT NULL,
 actor_id uuid NOT NULL,reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 1000),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 FOREIGN KEY(workspace_id,team_id,cycle_id) REFERENCES cycles(workspace_id,team_id,id),
 FOREIGN KEY(workspace_id,actor_id) REFERENCES workspace_members(workspace_id,user_id)
);
DO $$ DECLARE t text;BEGIN
 FOREACH t IN ARRAY ARRAY['cycles','cycle_issues','cycle_snapshots','cycle_close_reports','cycle_carry_operations','cycle_reopen_reasons'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
  EXECUTE format($policy$CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid) WITH CHECK(workspace_id=nullif(current_setting('opentrack.workspace_id',true),'')::uuid)$policy$,t);
 END LOOP;
END $$;
CREATE FUNCTION cycle_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NOT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=NEW.timezone) OR NEW.timezone NOT LIKE '%/%' AND NEW.timezone<>'UTC' THEN RAISE EXCEPTION 'Choose an IANA timezone' USING ERRCODE='23514';END IF;
 ELSE
  IF (NEW.id,NEW.workspace_id,NEW.team_id,NEW.timezone,NEW.created_at) IS DISTINCT FROM(OLD.id,OLD.workspace_id,OLD.team_id,OLD.timezone,OLD.created_at) OR NEW.version<>OLD.version+1 THEN
   RAISE EXCEPTION 'Cycle identity and timezone are immutable; version must advance once' USING ERRCODE='23514';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER cycles_guard BEFORE INSERT OR UPDATE ON cycles FOR EACH ROW EXECUTE FUNCTION cycle_guard();
CREATE TRIGGER cycles_no_delete BEFORE DELETE ON cycles FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER cycles_no_truncate BEFORE TRUNCATE ON cycles FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
DO $$ DECLARE t text;BEGIN
 FOREACH t IN ARRAY ARRAY['cycle_snapshots','cycle_close_reports','cycle_reopen_reasons'] LOOP
  EXECUTE format('CREATE TRIGGER immutable_rows BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION audit_append_only()',t);
  EXECUTE format('CREATE TRIGGER immutable_table BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only()',t);
 END LOOP;
 EXECUTE format('ALTER FUNCTION cycle_guard() SET search_path=pg_catalog,%I,pg_temp',current_schema());
 REVOKE ALL ON FUNCTION cycle_guard() FROM PUBLIC;
END $$;
