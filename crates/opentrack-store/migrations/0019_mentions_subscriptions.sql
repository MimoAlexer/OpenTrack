-- Private durable sources are independent of expiring workspace replay events.
-- No delivery is performed, and no source is pruned before a qualified M07 consumer.
CREATE TABLE issue_subscriptions (
 workspace_id uuid NOT NULL,issue_id uuid NOT NULL,user_id uuid NOT NULL REFERENCES users(id),
 preference text NOT NULL DEFAULT 'automatic' CHECK(preference IN('automatic','subscribed','unsubscribed')),
 source_creator boolean NOT NULL DEFAULT false,source_assignment boolean NOT NULL DEFAULT false,source_mention boolean NOT NULL DEFAULT false,
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,issue_id,user_id),FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id)
);
CREATE TABLE subscription_events (
 workspace_id uuid NOT NULL,id uuid NOT NULL,user_id uuid NOT NULL REFERENCES users(id),issue_id uuid NOT NULL,
 subscription_version bigint NOT NULL,preference text NOT NULL CHECK(preference IN('automatic','subscribed','unsubscribed')),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(workspace_id,id),
 UNIQUE(workspace_id,issue_id,user_id,subscription_version),FOREIGN KEY(workspace_id,issue_id,user_id) REFERENCES issue_subscriptions(workspace_id,issue_id,user_id)
);
CREATE TABLE document_mentions (
 workspace_id uuid NOT NULL,document_id uuid NOT NULL,user_id uuid NOT NULL REFERENCES users(id),
 PRIMARY KEY(workspace_id,document_id,user_id),FOREIGN KEY(workspace_id,document_id) REFERENCES documents(workspace_id,id)
);
CREATE TABLE mention_intents (
 workspace_id uuid NOT NULL,id uuid NOT NULL,issue_id uuid NOT NULL,document_id uuid NOT NULL,document_version bigint NOT NULL CHECK(document_version>0),
 origin_team_id uuid NOT NULL,recipient_id uuid NOT NULL REFERENCES users(id),actor_id uuid NOT NULL REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(workspace_id,id),
 UNIQUE(workspace_id,document_id,document_version,recipient_id),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),FOREIGN KEY(workspace_id,document_id) REFERENCES documents(workspace_id,id),
 FOREIGN KEY(workspace_id,origin_team_id) REFERENCES teams(workspace_id,id)
);
CREATE TABLE discussion_sources (
 workspace_id uuid NOT NULL,id uuid NOT NULL,issue_id uuid NOT NULL,comment_id uuid NOT NULL,document_id uuid NOT NULL,document_version bigint NOT NULL,
 origin_team_id uuid NOT NULL,actor_id uuid NOT NULL REFERENCES users(id),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(workspace_id,id),UNIQUE(workspace_id,comment_id),
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),FOREIGN KEY(workspace_id,comment_id) REFERENCES comments(workspace_id,id),
 FOREIGN KEY(workspace_id,document_id) REFERENCES documents(workspace_id,id),FOREIGN KEY(workspace_id,origin_team_id) REFERENCES teams(workspace_id,id)
);
DO $$ DECLARE tab text; BEGIN
 FOREACH tab IN ARRAY ARRAY['issue_subscriptions','subscription_events','document_mentions','mention_intents','discussion_sources'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  IF tab IN('issue_subscriptions','subscription_events') THEN
   EXECUTE format('CREATE POLICY private_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid AND (user_id=nullif(current_setting(''opentrack.user_id'',true),'''')::uuid OR current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=%L::regclass)))) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid AND (user_id=nullif(current_setting(''opentrack.user_id'',true),'''')::uuid OR current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=%L::regclass))))',tab,tab,tab);
  ELSE
   EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
  END IF;
 END LOOP;
END $$;
CREATE FUNCTION mention_eligible(w uuid,t uuid,origin uuid,u uuid) RETURNS boolean LANGUAGE sql STABLE AS $$
 SELECT w=nullif(current_setting('opentrack.workspace_id',true),'')::uuid AND EXISTS(
  SELECT 1 FROM users usr JOIN workspace_members m ON m.user_id=usr.id AND m.workspace_id=w AND m.active
  JOIN teams target ON target.workspace_id=w AND target.id=t JOIN teams historical ON historical.workspace_id=w AND historical.id=origin
  WHERE usr.id=u AND usr.disabled_at IS NULL
  AND (NOT target.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=w AND tm.team_id=t AND tm.user_id=u))
  AND (NOT historical.private OR EXISTS(SELECT 1 FROM team_members tm WHERE tm.workspace_id=w AND tm.team_id=origin AND tm.user_id=u)))
$$;
CREATE FUNCTION subscription_identity_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND ((NEW.workspace_id,NEW.issue_id,NEW.user_id,NEW.created_at) IS DISTINCT FROM(OLD.workspace_id,OLD.issue_id,OLD.user_id,OLD.created_at) OR NEW.version<>OLD.version+1) THEN RAISE EXCEPTION 'Subscription identity/version is invalid' USING ERRCODE='23514';END IF;
 IF current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) THEN
  IF NEW.user_id IS DISTINCT FROM nullif(current_setting('opentrack.user_id',true),'')::uuid THEN RAISE EXCEPTION 'Subscriptions are personal' USING ERRCODE='42501';END IF;
  IF (TG_OP='INSERT' AND (NEW.source_creator OR NEW.source_assignment OR NEW.source_mention)) OR (TG_OP='UPDATE' AND (NEW.source_creator,NEW.source_assignment,NEW.source_mention) IS DISTINCT FROM(OLD.source_creator,OLD.source_assignment,OLD.source_mention)) THEN RAISE EXCEPTION 'Automatic sources require a real source mutation' USING ERRCODE='42501';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER subscription_identity BEFORE INSERT OR UPDATE ON issue_subscriptions FOR EACH ROW EXECUTE FUNCTION subscription_identity_guard();
CREATE TRIGGER subscriptions_no_delete BEFORE DELETE ON issue_subscriptions FOR EACH ROW EXECUTE FUNCTION audit_append_only();
CREATE TRIGGER subscriptions_no_truncate BEFORE TRUNCATE ON issue_subscriptions FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE FUNCTION subscription_auto(w uuid,i uuid,u uuid,kind text) RETURNS void LANGUAGE plpgsql AS $$
DECLARE n bigint;existing issue_subscriptions%ROWTYPE;
BEGIN
 IF kind NOT IN('creator','assignment','mention') THEN RAISE EXCEPTION 'Unknown subscription source';END IF;
 SELECT * INTO existing FROM issue_subscriptions WHERE workspace_id=w AND issue_id=i AND user_id=u FOR UPDATE;
 IF FOUND AND (CASE kind WHEN 'creator' THEN existing.source_creator WHEN 'assignment' THEN existing.source_assignment ELSE existing.source_mention END) THEN RETURN;END IF;
 INSERT INTO issue_subscriptions(workspace_id,issue_id,user_id,source_creator,source_assignment,source_mention)
 VALUES(w,i,u,kind='creator',kind='assignment',kind='mention') ON CONFLICT(workspace_id,issue_id,user_id) DO UPDATE SET
 source_creator=issue_subscriptions.source_creator OR EXCLUDED.source_creator,source_assignment=issue_subscriptions.source_assignment OR EXCLUDED.source_assignment,source_mention=issue_subscriptions.source_mention OR EXCLUDED.source_mention,
 version=issue_subscriptions.version+1,updated_at=clock_timestamp();
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required automatic subscription missing';END IF;
END $$;
CREATE FUNCTION document_mention_sync() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE parent issues%ROWTYPE;origin uuid;ids uuid[];old_ids uuid[];u uuid;node jsonb;n bigint;expected bigint;
BEGIN
 IF NEW.issue_id IS NOT NULL THEN SELECT * INTO STRICT parent FROM issues WHERE workspace_id=NEW.workspace_id AND id=NEW.issue_id;origin:=parent.team_id;
 ELSE SELECT i.* INTO STRICT parent FROM comments c JOIN issues i ON i.workspace_id=c.workspace_id AND i.id=c.issue_id WHERE c.workspace_id=NEW.workspace_id AND c.id=NEW.comment_id;
 SELECT origin_team_id INTO STRICT origin FROM comments WHERE workspace_id=NEW.workspace_id AND id=NEW.comment_id;END IF;
 -- The App holds actor -> workspace SHARE -> team -> issue/document locks.
 -- No recipient account lock is taken after that workspace serialization point.
 ids:='{}';
 FOR node IN SELECT value FROM jsonb_path_query(NEW.body,'$.** ? (@.type == "userMention")') AS mentions(value) LOOP
  IF jsonb_typeof(node->'attrs') IS DISTINCT FROM 'object' THEN RAISE EXCEPTION 'Mention requires attributes' USING ERRCODE='23514';END IF;
  IF (SELECT count(*) FROM jsonb_object_keys(node->'attrs'))<>1 OR NOT (node->'attrs' ? 'id') OR jsonb_typeof(node->'attrs'->'id') IS DISTINCT FROM 'string' THEN RAISE EXCEPTION 'Mention requires an ID-only node' USING ERRCODE='23514';END IF;
  BEGIN u:=(node->'attrs'->>'id')::uuid;EXCEPTION WHEN invalid_text_representation THEN RAISE EXCEPTION 'Invalid mention user ID' USING ERRCODE='23514';END;
  IF (node->'attrs'->>'id')<>u::text THEN RAISE EXCEPTION 'Mention ID must be canonical' USING ERRCODE='23514';END IF;
  IF NOT u=ANY(ids) THEN ids:=array_append(ids,u);END IF;
 END LOOP;
 IF cardinality(ids)>50 THEN RAISE EXCEPTION 'At most 50 distinct mentions' USING ERRCODE='23514';END IF;
 SELECT coalesce(array_agg(user_id),'{}') INTO old_ids FROM document_mentions WHERE workspace_id=NEW.workspace_id AND document_id=NEW.id;
 FOREACH u IN ARRAY ids LOOP
  IF NOT u=ANY(old_ids) AND NOT mention_eligible(NEW.workspace_id,parent.team_id,origin,u) THEN RAISE EXCEPTION 'A newly mentioned user cannot read this document' USING ERRCODE='23514';END IF;
 END LOOP;
 SELECT count(*) INTO expected FROM unnest(old_ids) x WHERE NOT x=ANY(ids);
 DELETE FROM document_mentions WHERE workspace_id=NEW.workspace_id AND document_id=NEW.id AND NOT user_id=ANY(ids);
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>expected THEN RAISE EXCEPTION 'Required removed mention links missing';END IF;
 FOREACH u IN ARRAY ids LOOP
  IF NOT u=ANY(old_ids) THEN
   INSERT INTO document_mentions(workspace_id,document_id,user_id) VALUES(NEW.workspace_id,NEW.id,u);
   GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required mention link missing';END IF;
   PERFORM subscription_auto(NEW.workspace_id,parent.id,u,'mention');
   IF u<>NEW.updated_by_user_id THEN
    INSERT INTO mention_intents(workspace_id,id,issue_id,document_id,document_version,origin_team_id,recipient_id,actor_id)
    VALUES(NEW.workspace_id,gen_random_uuid(),parent.id,NEW.id,NEW.version,origin,u,NEW.updated_by_user_id);
    GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required mention intent missing';END IF;
   END IF;
  END IF;
 END LOOP;
 IF TG_OP='INSERT' AND NEW.comment_id IS NOT NULL THEN
  INSERT INTO discussion_sources(workspace_id,id,issue_id,comment_id,document_id,document_version,origin_team_id,actor_id)
  VALUES(NEW.workspace_id,gen_random_uuid(),parent.id,NEW.comment_id,NEW.id,NEW.version,origin,NEW.updated_by_user_id);
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Required discussion source missing';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER documents_mention_sync AFTER INSERT OR UPDATE ON documents FOR EACH ROW EXECUTE FUNCTION document_mention_sync();
CREATE FUNCTION issue_subscription_sync() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
BEGIN
 IF TG_OP='UPDATE' AND (NEW.assignee_id IS NOT DISTINCT FROM OLD.assignee_id OR NEW.assignee_id IS NULL) THEN RETURN NEW;END IF;
 IF TG_OP='INSERT' AND mention_eligible(NEW.workspace_id,NEW.team_id,NEW.team_id,NEW.created_by_user_id) THEN PERFORM subscription_auto(NEW.workspace_id,NEW.id,NEW.created_by_user_id,'creator');END IF;
 IF NEW.assignee_id IS NOT NULL AND mention_eligible(NEW.workspace_id,NEW.team_id,NEW.team_id,NEW.assignee_id) THEN PERFORM subscription_auto(NEW.workspace_id,NEW.id,NEW.assignee_id,'assignment');END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER issues_subscription_sync AFTER INSERT OR UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION issue_subscription_sync();
-- Deterministic eligible creator/assignee backfill creates no retroactive intents.
DO $$ DECLARE w uuid;expected bigint;n bigint;BEGIN
 FOR w IN SELECT id FROM workspaces ORDER BY id LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  SELECT count(*) INTO expected FROM (SELECT i.id,u FROM issues i CROSS JOIN LATERAL unnest(ARRAY[i.created_by_user_id,i.assignee_id]) u WHERE i.workspace_id=w AND i.deleted_at IS NULL AND u IS NOT NULL AND mention_eligible(w,i.team_id,i.team_id,u) GROUP BY i.id,u) x;
  INSERT INTO issue_subscriptions(workspace_id,issue_id,user_id,source_creator,source_assignment)
  SELECT w,i.id,u,coalesce(bool_or(u=i.created_by_user_id),false),coalesce(bool_or(u=i.assignee_id),false) FROM issues i CROSS JOIN LATERAL unnest(ARRAY[i.created_by_user_id,i.assignee_id]) u
  WHERE i.workspace_id=w AND i.deleted_at IS NULL AND u IS NOT NULL AND mention_eligible(w,i.team_id,i.team_id,u) GROUP BY i.id,u;
  GET DIAGNOSTICS n=ROW_COUNT;IF n<>expected THEN RAISE EXCEPTION 'Subscription backfill incomplete';END IF;
 END LOOP;PERFORM set_config('opentrack.workspace_id','',true);
END $$;
DO $$ DECLARE tab text;p regprocedure;r record;BEGIN
 FOREACH tab IN ARRAY ARRAY['subscription_events','mention_intents','discussion_sources'] LOOP
  EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION audit_append_only()',tab||'_immutable',tab);
  EXECUTE format('CREATE TRIGGER %I BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only()',tab||'_no_truncate',tab);
 END LOOP;
 FOREACH p IN ARRAY ARRAY['mention_eligible(uuid,uuid,uuid,uuid)'::regprocedure,'subscription_identity_guard()'::regprocedure,'subscription_auto(uuid,uuid,uuid,text)'::regprocedure,'document_mention_sync()'::regprocedure,'issue_subscription_sync()'::regprocedure] LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',p,current_schema());EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',p);
 END LOOP;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name='issues' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE ALL ON document_mentions,mention_intents,discussion_sources,subscription_events,issue_subscriptions FROM %I',r.grantee);
  EXECUTE format('GRANT SELECT,INSERT ON issue_subscriptions,subscription_events TO %I',r.grantee);
  EXECUTE format('GRANT UPDATE(preference,version,updated_at) ON issue_subscriptions TO %I',r.grantee);
  EXECUTE format('GRANT EXECUTE ON FUNCTION mention_eligible(uuid,uuid,uuid,uuid) TO %I',r.grantee);
 END LOOP;
END $$;
