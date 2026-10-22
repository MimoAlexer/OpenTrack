CREATE TABLE user_preferences (
 workspace_id uuid NOT NULL,user_id uuid NOT NULL,
 shortcut_mode text NOT NULL CHECK(shortcut_mode IN('enabled','disabled')),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,user_id),FOREIGN KEY(workspace_id,user_id) REFERENCES workspace_members(workspace_id,user_id)
);
CREATE TABLE user_preference_events (
 workspace_id uuid NOT NULL,id uuid NOT NULL,user_id uuid NOT NULL,
 preference_version bigint NOT NULL CHECK(preference_version>0),shortcut_mode text NOT NULL CHECK(shortcut_mode IN('enabled','disabled')),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(workspace_id,id),
 UNIQUE(workspace_id,user_id,preference_version),FOREIGN KEY(workspace_id,user_id) REFERENCES user_preferences(workspace_id,user_id)
);
DO $$ DECLARE tab text;BEGIN
 FOREACH tab IN ARRAY ARRAY['user_preferences','user_preference_events'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY personal_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid AND user_id=nullif(current_setting(''opentrack.user_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid AND user_id=nullif(current_setting(''opentrack.user_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;
CREATE FUNCTION user_preferences_identity_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='INSERT' AND NEW.version<>1 THEN RAISE EXCEPTION 'Initial preference version must be one' USING ERRCODE='23514';END IF;
 IF TG_OP='UPDATE' AND ((NEW.workspace_id,NEW.user_id,NEW.created_at) IS DISTINCT FROM(OLD.workspace_id,OLD.user_id,OLD.created_at) OR NEW.version<>OLD.version+1) THEN RAISE EXCEPTION 'Preference identity/version is invalid' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER user_preferences_identity BEFORE INSERT OR UPDATE ON user_preferences FOR EACH ROW EXECUTE FUNCTION user_preferences_identity_guard();
CREATE TRIGGER user_preferences_no_delete BEFORE DELETE ON user_preferences FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER user_preferences_no_truncate BEFORE TRUNCATE ON user_preferences FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER user_preference_events_immutable BEFORE UPDATE OR DELETE ON user_preference_events FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER user_preference_events_no_truncate BEFORE TRUNCATE ON user_preference_events FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
DO $$ DECLARE r record;BEGIN
 EXECUTE format('ALTER FUNCTION user_preferences_identity_guard() SET search_path=pg_catalog,%I,pg_temp',current_schema());
 REVOKE ALL ON FUNCTION user_preferences_identity_guard() FROM PUBLIC;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name='issues' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE ALL ON user_preferences,user_preference_events FROM %I',r.grantee);
  EXECUTE format('GRANT SELECT,INSERT ON user_preferences,user_preference_events TO %I',r.grantee);
  EXECUTE format('GRANT UPDATE(shortcut_mode,version,updated_at) ON user_preferences TO %I',r.grantee);
 END LOOP;
END $$;
