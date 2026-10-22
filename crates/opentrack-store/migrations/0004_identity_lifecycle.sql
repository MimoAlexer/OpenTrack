-- Additive identity lifecycle. Existing ownership, issue keys and authorship survive.
ALTER TABLE sessions ADD COLUMN auth_time timestamptz;
UPDATE sessions SET auth_time=created_at;
ALTER TABLE sessions ALTER COLUMN auth_time SET NOT NULL;
ALTER TABLE sessions ALTER COLUMN auth_time SET DEFAULT now();
ALTER TABLE workspaces ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK(version>0), ADD COLUMN archived_at timestamptz;
ALTER TABLE workspaces DROP CONSTRAINT workspaces_slug_key;
ALTER TABLE workspaces ADD CONSTRAINT workspace_organization_slug UNIQUE(organization_id,slug);
ALTER TABLE teams ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK(version>0), ADD COLUMN archived_at timestamptz;
ALTER TABLE workspace_members ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK(version>0);
ALTER TABLE team_members ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK(version>0);
CREATE TABLE organization_members (
 organization_id uuid NOT NULL REFERENCES organizations(id), user_id uuid NOT NULL REFERENCES users(id),
 role text NOT NULL CHECK(role IN ('owner','admin')), PRIMARY KEY(organization_id,user_id)
);
INSERT INTO organization_members(organization_id,user_id,role) SELECT id,created_by,'owner' FROM organizations;
ALTER TABLE organization_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE organization_members FORCE ROW LEVEL SECURITY;
CREATE POLICY own_organization_membership ON organization_members USING(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid) WITH CHECK(user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid);
CREATE TABLE organization_idempotency_records (
 organization_id uuid NOT NULL REFERENCES organizations(id), principal_id uuid NOT NULL REFERENCES users(id),
 route text NOT NULL, key_hash text NOT NULL, request_hash text NOT NULL, response_body jsonb,
 expires_at timestamptz NOT NULL DEFAULT now()+interval '24 hours', PRIMARY KEY(organization_id,principal_id,route,key_hash)
);
ALTER TABLE organization_idempotency_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE organization_idempotency_records FORCE ROW LEVEL SECURITY;
CREATE POLICY own_organization_retry ON organization_idempotency_records USING(principal_id=nullif(current_setting('opentrack.user_id',true),'')::uuid) WITH CHECK(principal_id=nullif(current_setting('opentrack.user_id',true),'')::uuid);
CREATE TABLE team_prefixes (
 workspace_id uuid NOT NULL, prefix text NOT NULL, team_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,prefix), FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id)
);
-- RLS must be installed after backfilling under the migrator, whose context is absent.
ALTER TABLE teams NO FORCE ROW LEVEL SECURITY;
INSERT INTO team_prefixes(workspace_id,prefix,team_id) SELECT workspace_id,key,id FROM teams;
ALTER TABLE teams FORCE ROW LEVEL SECURITY;
CREATE TABLE invitations (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id),
 email text NOT NULL CHECK(email=lower(email)), email_hash text NOT NULL,
 role text NOT NULL CHECK(role IN ('admin','member','viewer')), token_hash text NOT NULL UNIQUE,
 invited_by uuid NOT NULL, expires_at timestamptz NOT NULL, accepted_at timestamptz, revoked_at timestamptz,
 version bigint NOT NULL DEFAULT 1 CHECK(version>0), created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(workspace_id,id), FOREIGN KEY(workspace_id,invited_by) REFERENCES workspace_members(workspace_id,user_id),
 CHECK(NOT (accepted_at IS NOT NULL AND revoked_at IS NOT NULL))
);
CREATE UNIQUE INDEX invitation_one_pending ON invitations(workspace_id,email_hash) WHERE accepted_at IS NULL AND revoked_at IS NULL;
CREATE INDEX invitation_directory ON invitations(workspace_id,id);
CREATE TABLE invitation_teams (
 workspace_id uuid NOT NULL, invitation_id uuid NOT NULL, team_id uuid NOT NULL,
 PRIMARY KEY(workspace_id,invitation_id,team_id),
 FOREIGN KEY(workspace_id,invitation_id) REFERENCES invitations(workspace_id,id),
 FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id)
);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['team_prefixes','invitations','invitation_teams'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
 END LOOP;
END $$;
-- Guard lock linearizes all application access changes. This deferred constraint
-- also rejects direct SQL that would leave a workspace without an active owner.
CREATE FUNCTION enforce_workspace_owner() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE target uuid; BEGIN
 target := COALESCE(NEW.workspace_id,OLD.workspace_id);
 PERFORM 1 FROM workspace_auth_guards WHERE workspace_id=target FOR UPDATE;
 IF EXISTS(SELECT 1 FROM workspaces WHERE id=target) AND NOT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id=target AND active AND role='owner') THEN
  RAISE EXCEPTION 'A workspace must retain an active owner' USING ERRCODE='23514';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workspace_retains_owner AFTER INSERT OR UPDATE OR DELETE ON workspace_members DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_workspace_owner();
