CREATE TABLE users (
    id uuid PRIMARY KEY,
    email text NOT NULL CHECK (email = lower(email)),
    display_name text NOT NULL CHECK (length(display_name) BETWEEN 1 AND 100),
    password_hash text NOT NULL,
    disabled_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(email)
);
CREATE TABLE sessions (
    token_hash text PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id),
    csrf_token text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE login_attempts (
    account_hash text PRIMARY KEY,
    attempts integer NOT NULL DEFAULT 0,
    window_start timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE organizations (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    slug text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE workspace_members (
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    user_id uuid NOT NULL REFERENCES users(id),
    role text NOT NULL CHECK (role IN ('owner','admin','member','viewer')),
    active boolean NOT NULL DEFAULT true,
    PRIMARY KEY(workspace_id, user_id)
);
CREATE INDEX workspace_members_user ON workspace_members(user_id, workspace_id);
CREATE TABLE workspace_auth_guards (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces(id),
    epoch bigint NOT NULL DEFAULT 1
);
CREATE TABLE teams (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    key text NOT NULL CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}$'),
    private boolean NOT NULL DEFAULT false,
    next_issue_number bigint NOT NULL DEFAULT 1,
    UNIQUE(workspace_id,id),
    UNIQUE(workspace_id,key)
);
CREATE TABLE team_members (
    workspace_id uuid NOT NULL,
    team_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role text NOT NULL DEFAULT 'contributor' CHECK (role IN ('maintainer','contributor')),
    PRIMARY KEY(workspace_id,team_id,user_id),
    FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),
    FOREIGN KEY(workspace_id,user_id) REFERENCES workspace_members(workspace_id,user_id)
);
CREATE TABLE workflow_statuses (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL,
    team_id uuid NOT NULL,
    name text NOT NULL,
    category text NOT NULL CHECK (category IN ('triage','backlog','unstarted','started','completed','canceled')),
    color text NOT NULL,
    position integer NOT NULL,
    is_default boolean NOT NULL DEFAULT false,
    UNIQUE(workspace_id,team_id,id),
    UNIQUE(workspace_id,team_id,name),
    FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id)
);
CREATE UNIQUE INDEX status_one_default ON workflow_statuses(workspace_id,team_id) WHERE is_default;
CREATE TABLE issues (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL,
    team_id uuid NOT NULL,
    created_by_user_id uuid NOT NULL,
    number bigint NOT NULL CHECK(number > 0),
    title text NOT NULL CHECK(length(title) BETWEEN 1 AND 256),
    description text NOT NULL DEFAULT '' CHECK(octet_length(description) <= 262144),
    status_id uuid NOT NULL,
    priority smallint NOT NULL DEFAULT 0 CHECK(priority BETWEEN 0 AND 4),
    assignee_id uuid,
    estimate integer CHECK(estimate BETWEEN 0 AND 1000),
    due_date date,
    version bigint NOT NULL DEFAULT 1,
    archived_at timestamptz,
    deleted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(workspace_id,id),
    UNIQUE(workspace_id,team_id,number),
    FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id),
    FOREIGN KEY(workspace_id,team_id,status_id) REFERENCES workflow_statuses(workspace_id,team_id,id),
    FOREIGN KEY(workspace_id,created_by_user_id) REFERENCES workspace_members(workspace_id,user_id),
    FOREIGN KEY(workspace_id,assignee_id) REFERENCES workspace_members(workspace_id,user_id)
);
CREATE INDEX issues_listing ON issues(workspace_id,created_at DESC,id DESC) WHERE deleted_at IS NULL;
CREATE INDEX issues_team_status ON issues(workspace_id,team_id,status_id) WHERE deleted_at IS NULL;
CREATE INDEX issues_assignee ON issues(workspace_id,assignee_id) WHERE deleted_at IS NULL;
CREATE TABLE workspace_stream_heads (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces(id),
    last_seq bigint NOT NULL DEFAULT 0
);
CREATE TABLE workspace_events (
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    seq bigint NOT NULL,
    event_id uuid NOT NULL UNIQUE,
    event_type text NOT NULL,
    team_id uuid,
    entity_id uuid NOT NULL,
    entity_version bigint NOT NULL,
    actor_id uuid NOT NULL REFERENCES users(id),
    mutation_id uuid NOT NULL,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(workspace_id,seq),
    FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id)
);
CREATE TABLE activity_entries (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL,
    team_id uuid NOT NULL,
    entity_id uuid NOT NULL,
    actor_id uuid NOT NULL REFERENCES users(id),
    event_kind text NOT NULL,
    changes jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(workspace_id,team_id) REFERENCES teams(workspace_id,id)
);
CREATE INDEX activity_entity ON activity_entries(workspace_id,entity_id,created_at,id);
CREATE TABLE audit_entries (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    actor_id uuid NOT NULL REFERENCES users(id),
    action text NOT NULL,
    target_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE idempotency_records (
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    principal_id uuid NOT NULL REFERENCES users(id),
    route text NOT NULL,
    key_hash text NOT NULL,
    request_hash text NOT NULL,
    response_body jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now()+interval '24 hours',
    PRIMARY KEY(workspace_id,principal_id,route,key_hash)
);

-- Transaction-local context fails closed when missing. The runtime must not own tables.
DO $$ DECLARE tab text; BEGIN
  FOREACH tab IN ARRAY ARRAY['workspace_members','workspace_auth_guards','teams','team_members','workflow_statuses','issues','workspace_stream_heads','workspace_events','activity_entries','audit_entries','idempotency_records'] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', tab);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', tab);
    EXECUTE format('CREATE POLICY tenant_scope ON %I USING (workspace_id = nullif(current_setting(''opentrack.workspace_id'', true),'''')::uuid) WITH CHECK (workspace_id = nullif(current_setting(''opentrack.workspace_id'', true),'''')::uuid)',tab);
  END LOOP;
END $$;
-- Session workspace discovery can see only the principal's own memberships.
CREATE POLICY own_memberships ON workspace_members FOR SELECT USING (user_id = nullif(current_setting('opentrack.user_id',true),'')::uuid);
ALTER TABLE workspaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspaces FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_scope ON workspaces USING (id = nullif(current_setting('opentrack.workspace_id',true),'')::uuid OR EXISTS (SELECT 1 FROM workspace_members m WHERE m.workspace_id=workspaces.id AND m.user_id=nullif(current_setting('opentrack.user_id',true),'')::uuid AND m.active)) WITH CHECK (id = nullif(current_setting('opentrack.workspace_id',true),'')::uuid);
