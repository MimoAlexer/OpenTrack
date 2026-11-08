-- Durable recipient projections. No private title or document body is copied.
LOCK TABLE mention_intents,discussion_sources,issues IN SHARE ROW EXCLUSIVE MODE;
CREATE TABLE notification_heads (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),user_id uuid NOT NULL REFERENCES users(id),
 last_seq bigint NOT NULL DEFAULT 0 CHECK(last_seq>=0),PRIMARY KEY(workspace_id,user_id)
);
CREATE TABLE notification_preferences (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),user_id uuid NOT NULL REFERENCES users(id),
 assignments boolean NOT NULL DEFAULT true,mentions boolean NOT NULL DEFAULT true,subscribed_comments boolean NOT NULL DEFAULT true,
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(workspace_id,user_id)
);
CREATE TABLE notifications (
 workspace_id uuid NOT NULL,user_id uuid NOT NULL,id uuid NOT NULL DEFAULT gen_random_uuid(),
 inbox_seq bigint NOT NULL CHECK(inbox_seq>0),event_id uuid NOT NULL,event_seq bigint CHECK(event_seq>0),
 entity_kind text NOT NULL CHECK(entity_kind IN('issue','project')),entity_id uuid NOT NULL,
 reason text NOT NULL CHECK(reason IN('assignment','mention','subscribed_comment')),
 origin_team_id uuid NOT NULL,actor_id uuid NOT NULL REFERENCES users(id),document_id uuid,comment_id uuid,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),read_at timestamptz,archived_at timestamptz,snoozed_until timestamptz,
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),PRIMARY KEY(workspace_id,user_id,id),UNIQUE(workspace_id,id),
 UNIQUE(workspace_id,user_id,inbox_seq),UNIQUE(workspace_id,user_id,event_id,reason),
 FOREIGN KEY(workspace_id,user_id) REFERENCES notification_heads(workspace_id,user_id),
 FOREIGN KEY(workspace_id,origin_team_id) REFERENCES teams(workspace_id,id),
 FOREIGN KEY(workspace_id,document_id) REFERENCES documents(workspace_id,id),
 FOREIGN KEY(workspace_id,comment_id) REFERENCES comments(workspace_id,id),CHECK(user_id<>actor_id),
 issue_id uuid GENERATED ALWAYS AS (CASE WHEN entity_kind='issue' THEN entity_id END) STORED,
 project_id uuid GENERATED ALWAYS AS (CASE WHEN entity_kind='project' THEN entity_id END) STORED,
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),FOREIGN KEY(workspace_id,project_id) REFERENCES projects(workspace_id,id)
);
CREATE INDEX notifications_inbox ON notifications(workspace_id,user_id,inbox_seq DESC);
CREATE INDEX notifications_unread ON notifications(workspace_id,user_id,inbox_seq DESC) WHERE read_at IS NULL AND archived_at IS NULL;
CREATE TABLE notification_invalidations (
 workspace_id uuid NOT NULL,user_id uuid NOT NULL,notification_id uuid NOT NULL,notification_version bigint NOT NULL CHECK(notification_version>0),
 event_kind text NOT NULL CHECK(event_kind IN('notification.created','notification.updated')),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 published_seq bigint CHECK(published_seq>0),attempts integer NOT NULL DEFAULT 0 CHECK(attempts>=0),available_at timestamptz NOT NULL DEFAULT clock_timestamp(),blocked boolean NOT NULL DEFAULT false,
 PRIMARY KEY(workspace_id,user_id,notification_id,notification_version),FOREIGN KEY(workspace_id,user_id,notification_id) REFERENCES notifications(workspace_id,user_id,id)
);
CREATE INDEX notification_invalidations_pending ON notification_invalidations(workspace_id,available_at,notification_id,notification_version) WHERE published_seq IS NULL AND NOT blocked;
CREATE TABLE notification_sources (
 workspace_id uuid NOT NULL REFERENCES workspaces(id),id uuid NOT NULL,
 reason text NOT NULL CHECK(reason IN('assignment','mention','subscribed_comment')),
 entity_kind text NOT NULL CHECK(entity_kind IN('issue','project')),entity_id uuid NOT NULL,source_version bigint NOT NULL CHECK(source_version>0),
 origin_team_id uuid NOT NULL,actor_id uuid NOT NULL REFERENCES users(id),recipient_id uuid REFERENCES users(id),document_id uuid,comment_id uuid,
 source_transaction bigint,event_id uuid,event_seq bigint CHECK(event_seq>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),after_user_id uuid,completed boolean NOT NULL DEFAULT false,
 PRIMARY KEY(workspace_id,id),CHECK((event_id IS NULL)=(event_seq IS NULL)),
 CHECK((reason='subscribed_comment')=(recipient_id IS NULL)),
 FOREIGN KEY(workspace_id,origin_team_id) REFERENCES teams(workspace_id,id),
 FOREIGN KEY(workspace_id,document_id) REFERENCES documents(workspace_id,id),FOREIGN KEY(workspace_id,comment_id) REFERENCES comments(workspace_id,id),
 issue_id uuid GENERATED ALWAYS AS (CASE WHEN entity_kind='issue' THEN entity_id END) STORED,
 project_id uuid GENERATED ALWAYS AS (CASE WHEN entity_kind='project' THEN entity_id END) STORED,
 FOREIGN KEY(workspace_id,issue_id) REFERENCES issues(workspace_id,id),FOREIGN KEY(workspace_id,project_id) REFERENCES projects(workspace_id,id)
);
CREATE INDEX notification_sources_pending ON notification_sources(workspace_id,event_seq,id) WHERE NOT completed;
CREATE INDEX notification_sources_transaction ON notification_sources(workspace_id,source_transaction) WHERE event_seq IS NULL;
CREATE TABLE notification_backfills (
 workspace_id uuid PRIMARY KEY REFERENCES workspaces(id),job_id uuid,
 mention_upper uuid,discussion_upper uuid,after_mention uuid,after_discussion uuid,
 mentions_complete boolean NOT NULL DEFAULT false,discussions_complete boolean NOT NULL DEFAULT false,
 FOREIGN KEY(workspace_id,job_id) REFERENCES jobs(workspace_id,id)
);
DO $$ DECLARE tab text;BEGIN
 FOREACH tab IN ARRAY ARRAY['notification_heads','notification_preferences','notifications','notification_invalidations','notification_sources','notification_backfills'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',tab);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',tab);
  IF tab IN('notification_heads','notification_preferences','notifications','notification_invalidations') THEN
   EXECUTE format('CREATE POLICY recipient_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid AND (user_id=nullif(current_setting(''opentrack.user_id'',true),'''')::uuid OR current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=%L::regclass)))) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid AND (user_id=nullif(current_setting(''opentrack.user_id'',true),'''')::uuid OR current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=%L::regclass))))',tab,tab,tab);
  ELSE
   EXECUTE format('CREATE POLICY tenant_scope ON %I USING(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid) WITH CHECK(workspace_id=nullif(current_setting(''opentrack.workspace_id'',true),'''')::uuid)',tab);
  END IF;
 END LOOP;
END $$;
CREATE FUNCTION notification_identity_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_TABLE_NAME='notifications' THEN
  IF (NEW.workspace_id,NEW.user_id,NEW.id,NEW.inbox_seq,NEW.event_id,NEW.event_seq,NEW.entity_kind,NEW.entity_id,NEW.reason,NEW.origin_team_id,NEW.actor_id,NEW.document_id,NEW.comment_id,NEW.created_at)
   IS DISTINCT FROM (OLD.workspace_id,OLD.user_id,OLD.id,OLD.inbox_seq,OLD.event_id,OLD.event_seq,OLD.entity_kind,OLD.entity_id,OLD.reason,OLD.origin_team_id,OLD.actor_id,OLD.document_id,OLD.comment_id,OLD.created_at)
   OR NEW.version<>OLD.version+1 THEN RAISE EXCEPTION 'Notification identity/version is invalid' USING ERRCODE='23514';END IF;
 ELSIF TG_OP='UPDATE' THEN
  IF (NEW.workspace_id,NEW.user_id) IS DISTINCT FROM (OLD.workspace_id,OLD.user_id) OR NEW.version<>OLD.version+1 THEN RAISE EXCEPTION 'Notification preference identity/version is invalid' USING ERRCODE='23514';END IF;
 ELSIF NEW.version<>1 THEN RAISE EXCEPTION 'Notification preferences start at version one' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER notification_identity BEFORE UPDATE ON notifications FOR EACH ROW EXECUTE FUNCTION notification_identity_guard();
CREATE TRIGGER notification_preference_identity BEFORE INSERT OR UPDATE ON notification_preferences FOR EACH ROW EXECUTE FUNCTION notification_identity_guard();

CREATE FUNCTION notification_invalidation_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) AND (NEW.event_kind<>'notification.updated' OR NEW.published_seq IS NOT NULL OR NEW.attempts<>0 OR NEW.blocked) THEN RAISE EXCEPTION 'Invalid notification invalidation authority' USING ERRCODE='42501';END IF;
 IF NOT EXISTS(SELECT 1 FROM notifications WHERE workspace_id=NEW.workspace_id AND user_id=NEW.user_id AND id=NEW.notification_id AND version=NEW.notification_version) THEN RAISE EXCEPTION 'Invalidation must reference the current recipient projection' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER notification_invalidation_identity BEFORE INSERT ON notification_invalidations FOR EACH ROW EXECUTE FUNCTION notification_invalidation_guard();

-- Record actual assignment deltas, never infer them from a later issue snapshot.
CREATE FUNCTION notification_assignment_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE a uuid:=nullif(current_setting('opentrack.user_id',true),'')::uuid;n bigint;
BEGIN
 IF NEW.assignee_id IS NULL OR (TG_OP='UPDATE' AND NEW.assignee_id IS NOT DISTINCT FROM OLD.assignee_id) THEN RETURN NEW;END IF;
 IF a IS NULL OR a=NEW.assignee_id THEN RETURN NEW;END IF;
 INSERT INTO notification_sources(workspace_id,id,reason,entity_kind,entity_id,source_version,origin_team_id,actor_id,recipient_id,source_transaction)
 VALUES(NEW.workspace_id,gen_random_uuid(),'assignment','issue',NEW.id,NEW.version,NEW.team_id,a,NEW.assignee_id,txid_current());
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);RETURN NEW;
END $$;
CREATE TRIGGER notification_assignment_source AFTER INSERT OR UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION notification_assignment_source();
CREATE FUNCTION notification_discussion_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE n bigint;BEGIN
 IF TG_TABLE_NAME='mention_intents' THEN
  INSERT INTO notification_sources(workspace_id,id,reason,entity_kind,entity_id,source_version,origin_team_id,actor_id,recipient_id,document_id,comment_id,source_transaction,created_at)
  VALUES(NEW.workspace_id,NEW.id,'mention',CASE WHEN NEW.project_id IS NULL THEN 'issue' ELSE 'project' END,coalesce(NEW.issue_id,NEW.project_id),NEW.document_version,NEW.origin_team_id,NEW.actor_id,NEW.recipient_id,NEW.document_id,(SELECT comment_id FROM documents WHERE workspace_id=NEW.workspace_id AND id=NEW.document_id),txid_current(),NEW.created_at);
 ELSE
  INSERT INTO notification_sources(workspace_id,id,reason,entity_kind,entity_id,source_version,origin_team_id,actor_id,document_id,comment_id,source_transaction,created_at)
  VALUES(NEW.workspace_id,NEW.id,'subscribed_comment','issue',NEW.issue_id,NEW.document_version,NEW.origin_team_id,NEW.actor_id,NEW.document_id,NEW.comment_id,txid_current(),NEW.created_at);
 END IF;
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);RETURN NEW;
END $$;
CREATE TRIGGER notification_mention_source AFTER INSERT ON mention_intents FOR EACH ROW EXECUTE FUNCTION notification_discussion_source();
CREATE TRIGGER notification_discussion_source AFTER INSERT ON discussion_sources FOR EACH ROW EXECUTE FUNCTION notification_discussion_source();
CREATE FUNCTION notification_bind_event() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$ BEGIN
 UPDATE notification_sources s SET event_id=NEW.event_id,event_seq=NEW.seq
 WHERE s.workspace_id=NEW.workspace_id AND s.source_transaction=txid_current() AND s.event_seq IS NULL AND s.actor_id=NEW.actor_id AND (
  (s.reason='assignment' AND NEW.event_type IN('issue.created','issue.updated') AND s.entity_id=NEW.entity_id AND s.source_version=NEW.entity_version)
  OR (s.reason='mention' AND NEW.event_type='document.updated' AND s.document_id=NEW.entity_id AND s.source_version=NEW.entity_version)
  OR (s.reason IN('mention','subscribed_comment') AND NEW.event_type='comment.created' AND s.comment_id=NEW.entity_id AND s.source_version=1 AND NEW.entity_version=1)
  OR (s.reason='mention' AND NEW.event_type IN('issue.created','project.created') AND s.entity_id=NEW.entity_id AND s.source_version=1 AND NEW.entity_version=1)
 );RETURN NEW;
END $$;
CREATE TRIGGER notification_bind_event AFTER INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION notification_bind_event();
-- A live source must have the matching real event before commit. Metadata alone
-- cannot manufacture a source from an unrelated issue version or actor.
CREATE FUNCTION notification_source_event_required() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$ BEGIN
 IF NEW.source_transaction IS NOT NULL AND NOT EXISTS(SELECT 1 FROM notification_sources WHERE workspace_id=NEW.workspace_id AND id=NEW.id AND event_seq IS NOT NULL) THEN
  RAISE EXCEPTION 'Notification source requires its exact authored event' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER notification_source_event_required AFTER INSERT ON notification_sources DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION notification_source_event_required();

ALTER TABLE jobs DROP CONSTRAINT jobs_kind_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_kind_check CHECK(kind IN('workflow.status_replace','assignment.reconcile','issues.order_normalize','planning.backfill','notification.backfill'));
ALTER TABLE jobs DROP CONSTRAINT jobs_requester_shape;
ALTER TABLE jobs ADD CONSTRAINT jobs_requester_shape CHECK((kind IN('workflow.status_replace','issues.order_normalize') AND requested_by IS NOT NULL) OR (kind IN('assignment.reconcile','planning.backfill','notification.backfill') AND requested_by IS NULL));
CREATE OR REPLACE FUNCTION maintenance_job_authority() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF NEW.kind IN('assignment.reconcile','planning.backfill','notification.backfill') AND current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID))
 AND NOT has_function_privilege(current_user,'worker_lock_maintenance_context(uuid)','EXECUTE') THEN RAISE EXCEPTION 'Maintenance work requires worker authority' USING ERRCODE='42501';END IF;RETURN NEW;
END $$;
CREATE FUNCTION register_notification_consumer(w uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE saved text:=current_setting('opentrack.workspace_id',true);j uuid:=gen_random_uuid();floor bigint;n bigint;BEGIN
 PERFORM set_config('opentrack.workspace_id',w::text,true);
 SELECT purged_through_seq INTO STRICT floor FROM event_retention_state WHERE workspace_id=w;
 INSERT INTO consumer_registrations(workspace_id,consumer_name,consumer_version) VALUES(w,'notifications',1);GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version,last_seq) VALUES(w,'notifications',1,floor);GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO notification_backfills(workspace_id,job_id,mention_upper,discussion_upper)
 VALUES(w,NULL,(SELECT id FROM mention_intents WHERE workspace_id=w ORDER BY id DESC LIMIT 1),(SELECT id FROM discussion_sources WHERE workspace_id=w ORDER BY id DESC LIMIT 1));GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 PERFORM set_config('opentrack.workspace_id',coalesce(saved,''),true);
END $$;
CREATE OR REPLACE FUNCTION register_workspace_consumers() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$ BEGIN
 PERFORM register_assignment_consumer(NEW.workspace_id);PERFORM register_planning_counts_consumer(NEW.workspace_id);PERFORM register_notification_consumer(NEW.workspace_id);RETURN NEW;
END $$;
ALTER TABLE workspaces NO FORCE ROW LEVEL SECURITY;
DO $$ DECLARE w uuid;BEGIN FOR w IN SELECT id FROM workspaces ORDER BY id LOOP PERFORM register_notification_consumer(w);END LOOP;END $$;
ALTER TABLE workspaces FORCE ROW LEVEL SECURITY;

CREATE FUNCTION notification_source_access(w uuid,k text,i uuid,origin uuid,u uuid,d uuid,c uuid) RETURNS boolean LANGUAGE sql STABLE AS $$
 SELECT CASE k WHEN 'issue' THEN EXISTS(SELECT 1 FROM issues source WHERE source.workspace_id=w AND source.id=i AND source.deleted_at IS NULL AND mention_eligible(w,source.team_id,origin,u))
 WHEN 'project' THEN EXISTS(SELECT 1 FROM projects source WHERE source.workspace_id=w AND source.id=i AND mention_eligible(w,source.team_id,origin,u)) ELSE false END
 AND (d IS NULL OR EXISTS(SELECT 1 FROM documents WHERE workspace_id=w AND id=d AND deleted_at IS NULL))
 AND (c IS NULL OR EXISTS(SELECT 1 FROM comments WHERE workspace_id=w AND id=c AND deleted_at IS NULL))
$$;
CREATE FUNCTION notification_insert(w uuid,s notification_sources,u uuid) RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE prefs notification_preferences%ROWTYPE;number bigint;result uuid;n bigint;BEGIN
 IF u=s.actor_id OR NOT notification_source_access(w,s.entity_kind,s.entity_id,s.origin_team_id,u,s.document_id,s.comment_id) THEN RETURN NULL;END IF;
 -- Materialize the default to serialize a concurrent preference toggle, including
 -- its first write. Preference locks always precede recipient head locks.
 INSERT INTO notification_preferences(workspace_id,user_id) VALUES(w,u) ON CONFLICT DO NOTHING;
 SELECT * INTO STRICT prefs FROM notification_preferences WHERE workspace_id=w AND user_id=u FOR SHARE;
 IF NOT (CASE s.reason WHEN 'assignment' THEN prefs.assignments WHEN 'mention' THEN prefs.mentions ELSE prefs.subscribed_comments END) THEN RETURN NULL;END IF;
 IF s.reason='subscribed_comment' AND NOT EXISTS(SELECT 1 FROM issue_subscriptions WHERE workspace_id=w AND issue_id=s.entity_id AND user_id=u AND preference<>'unsubscribed' AND (preference='subscribed' OR source_creator OR source_assignment OR source_mention) FOR SHARE) THEN RETURN NULL;END IF;
 IF EXISTS(SELECT 1 FROM notifications WHERE workspace_id=w AND user_id=u AND event_id=coalesce(s.event_id,s.id) AND reason=s.reason) THEN RETURN NULL;END IF;
 INSERT INTO notification_heads(workspace_id,user_id,last_seq) VALUES(w,u,1) ON CONFLICT(workspace_id,user_id) DO UPDATE SET last_seq=notification_heads.last_seq+1 RETURNING last_seq INTO number;
 IF NOT FOUND THEN RAISE EXCEPTION 'Required inbox head missing';END IF;
 result:=gen_random_uuid();
 INSERT INTO notifications(workspace_id,user_id,id,inbox_seq,event_id,event_seq,entity_kind,entity_id,reason,origin_team_id,actor_id,document_id,comment_id,created_at)
 VALUES(w,u,result,number,coalesce(s.event_id,s.id),s.event_seq,s.entity_kind,s.entity_id,s.reason,s.origin_team_id,s.actor_id,s.document_id,s.comment_id,s.created_at);
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO notification_invalidations(workspace_id,user_id,notification_id,notification_version,event_kind) VALUES(w,u,result,1,'notification.created');GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);RETURN result;
END $$;
-- Append invalidations only after every recipient head lock/effect. The shared
-- stream is references only; replay resolves each ID through recipient RLS.
CREATE FUNCTION worker_flush_notification_events(w uuid,batch_limit integer) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE item notification_invalidations%ROWTYPE;pending notification_invalidations[];finish bigint;amount integer;n bigint;published bigint:=0;constraint_name text;failed boolean;admission boolean;BEGIN
 IF batch_limit IS NULL OR batch_limit<1 OR batch_limit>100 THEN RAISE EXCEPTION 'Invalid notification invalidation bound' USING ERRCODE='22023';END IF;
 IF NOT worker_lock_maintenance_context(w) THEN RETURN 0;END IF;
 -- Row locks are acquired before the stream head; effects are already durable.
 SELECT array_agg(locked) INTO pending FROM (SELECT * FROM notification_invalidations WHERE workspace_id=w AND published_seq IS NULL AND NOT blocked AND available_at<=clock_timestamp() ORDER BY available_at,notification_id,notification_version LIMIT batch_limit FOR UPDATE SKIP LOCKED) locked;
 FOREACH item IN ARRAY coalesce(pending,'{}'::notification_invalidations[]) LOOP
  failed:=false;admission:=false;
  BEGIN
   UPDATE workspace_stream_heads SET last_seq=last_seq+1 WHERE workspace_id=w RETURNING last_seq INTO finish;IF NOT FOUND THEN RAISE EXCEPTION 'Required notification stream head missing';END IF;
   INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,team_id,entity_id,entity_version,actor_kind,actor_id,mutation_id)
   VALUES(w,finish,gen_random_uuid(),item.event_kind,NULL,item.notification_id,item.notification_version,'system',NULL,gen_random_uuid());GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
   UPDATE notification_invalidations SET published_seq=finish WHERE workspace_id=w AND user_id=item.user_id AND notification_id=item.notification_id AND notification_version=item.notification_version;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
  EXCEPTION WHEN query_canceled OR OTHERS THEN
   GET STACKED DIAGNOSTICS constraint_name=CONSTRAINT_NAME;
   admission:=constraint_name='event_admission_blocked';failed:=true;
  END;
  IF failed THEN
   IF admission THEN EXIT;END IF;
   UPDATE notification_invalidations SET attempts=attempts+1,blocked=attempts+1>=5,available_at=clock_timestamp()+make_interval(secs=>least(900,power(2,least(attempts+1,9)))) WHERE workspace_id=w AND user_id=item.user_id AND notification_id=item.notification_id AND notification_version=item.notification_version;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
   EXIT;
  END IF;
  published:=published+1;
 END LOOP;
 RETURN published;
END $$;
ALTER TABLE workspace_events DROP CONSTRAINT workspace_event_actor_shape;
ALTER TABLE workspace_events ADD CONSTRAINT workspace_event_actor_shape CHECK(
 (actor_kind='account' AND actor_id IS NOT NULL AND system_job_id IS NULL)
 OR (actor_kind='local_operator' AND actor_id IS NULL AND event_type='access.changed' AND team_id IS NULL AND entity_id=workspace_id AND system_job_id IS NULL)
 OR (actor_kind='system' AND actor_id IS NULL AND ((event_type='issue.updated' AND system_job_id IS NOT NULL AND mutation_id=system_job_id) OR (event_type IN('notification.created','notification.updated') AND system_job_id IS NULL))));
CREATE OR REPLACE FUNCTION system_effect_authority() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF NEW.actor_kind='system' THEN
  IF current_user<>pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid=TG_RELID)) AND NOT has_function_privilege(current_user,'worker_lock_maintenance_context(uuid)','EXECUTE') THEN RAISE EXCEPTION 'System attribution requires maintenance authority' USING ERRCODE='42501';END IF;
  IF TG_TABLE_NAME='workspace_events' THEN
   IF NEW.event_type IN('notification.created','notification.updated') THEN
   IF NOT EXISTS(SELECT 1 FROM notifications WHERE workspace_id=NEW.workspace_id AND id=NEW.entity_id AND version>=NEW.entity_version AND (NEW.team_id IS NULL OR origin_team_id=NEW.team_id)) THEN RAISE EXCEPTION 'Notification invalidation requires its current recipient projection' USING ERRCODE='23514';END IF;RETURN NEW;
   END IF;
  END IF;
  IF NOT EXISTS(SELECT 1 FROM jobs WHERE workspace_id=NEW.workspace_id AND id=NEW.system_job_id AND kind='assignment.reconcile' AND state='running' AND lease_expires_at>clock_timestamp()) THEN RAISE EXCEPTION 'System effect requires a live maintenance job' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE FUNCTION notification_fanout_source(w uuid,source_id uuid,budget integer)
RETURNS TABLE(examined integer,created_ids uuid[],done boolean) LANGUAGE plpgsql AS $$
DECLARE s notification_sources%ROWTYPE;u uuid;result uuid;n bigint;BEGIN
 examined:=0;created_ids:='{}';done:=false;
 SELECT * INTO STRICT s FROM notification_sources WHERE workspace_id=w AND id=source_id FOR UPDATE;
 IF s.completed THEN done:=true;RETURN NEXT;RETURN;END IF;
 FOR u IN SELECT candidate FROM (
  SELECT s.recipient_id AS candidate WHERE s.recipient_id IS NOT NULL AND s.after_user_id IS NULL
  UNION ALL SELECT sub.user_id FROM issue_subscriptions sub WHERE s.reason='subscribed_comment' AND sub.workspace_id=w AND sub.issue_id=s.entity_id
   AND (s.after_user_id IS NULL OR sub.user_id>s.after_user_id) AND sub.preference<>'unsubscribed' AND (sub.preference='subscribed' OR sub.source_creator OR sub.source_assignment OR sub.source_mention)
 ) candidates ORDER BY candidate LIMIT budget LOOP
  result:=notification_insert(w,s,u);IF result IS NOT NULL THEN created_ids:=array_append(created_ids,result);END IF;
  s.after_user_id:=u;examined:=examined+1;
 END LOOP;
 done:=CASE WHEN s.recipient_id IS NOT NULL THEN s.after_user_id IS NOT NULL ELSE NOT EXISTS(SELECT 1 FROM issue_subscriptions sub WHERE sub.workspace_id=w AND sub.issue_id=s.entity_id AND (s.after_user_id IS NULL OR sub.user_id>s.after_user_id) AND sub.preference<>'unsubscribed' AND (sub.preference='subscribed' OR sub.source_creator OR sub.source_assignment OR sub.source_mention)) END;
 UPDATE notification_sources SET after_user_id=s.after_user_id,completed=done WHERE workspace_id=w AND id=source_id;
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);RETURN NEXT;
END $$;
CREATE FUNCTION worker_apply_notification_events(w uuid,batch_limit integer)
RETURNS TABLE(processed bigint,captured bigint,backfilled boolean,dead_lettered boolean) LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE cp consumer_checkpoints%ROWTYPE;b notification_backfills%ROWTYPE;m mention_intents%ROWTYPE;d discussion_sources%ROWTYPE;
 e workspace_events%ROWTYPE;source_id uuid;fanout record;dl consumer_dead_letters%ROWTYPE;n bigint;scanned integer:=0;budget integer;iterations integer:=0;
 ids uuid[]:='{}';failure_seq bigint;failure_event uuid;failure_type text;failure_schema integer;code text:='effect_failed';failed boolean:=false;letter uuid;attempt integer;j uuid;receipt consumer_receipts%ROWTYPE;
BEGIN
 processed:=0;captured:=0;backfilled:=false;dead_lettered:=false;budget:=batch_limit;
 IF batch_limit IS NULL OR batch_limit<1 OR batch_limit>100 THEN RAISE EXCEPTION 'Invalid notification fanout bound' USING ERRCODE='22023';END IF;
 IF NOT worker_lock_maintenance_context(w) THEN RETURN NEXT;RETURN;END IF;
 PERFORM 1 FROM consumer_registrations WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1 AND required AND enabled FOR SHARE;
 IF NOT FOUND THEN RETURN NEXT;RETURN;END IF;
 SELECT * INTO STRICT cp FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1 FOR UPDATE;
 failure_seq:=CASE WHEN cp.backfill_job_id IS NULL THEN 0 ELSE cp.last_seq+1 END;
 SELECT * INTO dl FROM consumer_dead_letters WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1 AND seq=failure_seq FOR UPDATE;
 IF FOUND AND (dl.state='blocked' OR (dl.state='retry_wait' AND dl.available_at>clock_timestamp())) THEN dead_lettered:=dl.state='blocked';RETURN NEXT;RETURN;END IF;
 BEGIN
  IF cp.backfill_job_id IS NULL THEN
   SELECT * INTO STRICT b FROM notification_backfills WHERE workspace_id=w FOR UPDATE;
   IF NOT b.mentions_complete THEN
    FOR m IN SELECT * FROM mention_intents WHERE workspace_id=w AND id<=b.mention_upper AND (b.after_mention IS NULL OR id>b.after_mention) ORDER BY id LIMIT batch_limit LOOP
     INSERT INTO notification_sources(workspace_id,id,reason,entity_kind,entity_id,source_version,origin_team_id,actor_id,recipient_id,document_id,comment_id,created_at)
     VALUES(w,m.id,'mention',CASE WHEN m.project_id IS NULL THEN 'issue' ELSE 'project' END,coalesce(m.issue_id,m.project_id),m.document_version,m.origin_team_id,m.actor_id,m.recipient_id,m.document_id,(SELECT comment_id FROM documents WHERE workspace_id=w AND id=m.document_id),m.created_at) ON CONFLICT DO NOTHING;
     IF NOT EXISTS(SELECT 1 FROM notification_sources WHERE workspace_id=w AND id=m.id) THEN RAISE EXCEPTION 'Required historical mention missing';END IF;
     b.after_mention:=m.id;scanned:=scanned+1;
    END LOOP;
    b.mentions_complete:=NOT EXISTS(SELECT 1 FROM mention_intents WHERE workspace_id=w AND id<=b.mention_upper AND (b.after_mention IS NULL OR id>b.after_mention));
   END IF;
   IF b.mentions_complete AND NOT b.discussions_complete THEN
    FOR d IN SELECT * FROM discussion_sources WHERE workspace_id=w AND id<=b.discussion_upper AND (b.after_discussion IS NULL OR id>b.after_discussion) ORDER BY id LIMIT batch_limit-scanned LOOP
     INSERT INTO notification_sources(workspace_id,id,reason,entity_kind,entity_id,source_version,origin_team_id,actor_id,document_id,comment_id,created_at)
     VALUES(w,d.id,'subscribed_comment','issue',d.issue_id,d.document_version,d.origin_team_id,d.actor_id,d.document_id,d.comment_id,d.created_at) ON CONFLICT DO NOTHING;
     IF NOT EXISTS(SELECT 1 FROM notification_sources WHERE workspace_id=w AND id=d.id) THEN RAISE EXCEPTION 'Required historical discussion missing';END IF;
     b.after_discussion:=d.id;scanned:=scanned+1;
    END LOOP;
    b.discussions_complete:=NOT EXISTS(SELECT 1 FROM discussion_sources WHERE workspace_id=w AND id<=b.discussion_upper AND (b.after_discussion IS NULL OR id>b.after_discussion));
   END IF;
   UPDATE notification_backfills SET after_mention=b.after_mention,after_discussion=b.after_discussion,mentions_complete=b.mentions_complete,discussions_complete=b.discussions_complete WHERE workspace_id=w;
   GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
   FOR source_id IN SELECT id FROM notification_sources WHERE workspace_id=w AND source_transaction IS NULL AND NOT completed ORDER BY id LIMIT batch_limit LOOP
    SELECT * INTO STRICT fanout FROM notification_fanout_source(w,source_id,budget);budget:=budget-fanout.examined;ids:=ids||fanout.created_ids;
    EXIT WHEN budget=0;
   END LOOP;
   captured:=cardinality(ids);backfilled:=scanned>0 OR captured>0;
   IF b.mentions_complete AND b.discussions_complete AND NOT EXISTS(SELECT 1 FROM notification_sources WHERE workspace_id=w AND source_transaction IS NULL AND NOT completed) THEN
    j:=gen_random_uuid();
    INSERT INTO jobs(workspace_id,id,kind,requested_by,state,finished_at,dedupe_key) VALUES(w,j,'notification.backfill',NULL,'succeeded',clock_timestamp(),'notifications:backfill:v1');GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    UPDATE notification_backfills SET job_id=j WHERE workspace_id=w;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    UPDATE consumer_checkpoints SET backfill_job_id=j,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    backfilled:=true;
   END IF;
  ELSE
   FOR e IN SELECT * FROM workspace_events WHERE workspace_id=w AND seq>cp.last_seq ORDER BY seq LIMIT batch_limit LOOP
    failure_seq:=e.seq;failure_event:=e.event_id;failure_type:=e.event_type;failure_schema:=e.schema_version;
    SELECT * INTO dl FROM consumer_dead_letters WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1 AND seq=e.seq FOR UPDATE;
    EXIT WHEN FOUND AND (dl.state='blocked' OR (dl.state='retry_wait' AND dl.available_at>clock_timestamp()));
    IF e.seq<>cp.last_seq+1 THEN RAISE EXCEPTION 'Notification event sequence gap';END IF;
    IF e.schema_version<>1 AND (starts_with(e.event_type,'issue.') OR starts_with(e.event_type,'document.') OR starts_with(e.event_type,'comment.') OR starts_with(e.event_type,'project.')) THEN code:='unsupported_event_schema';RAISE EXCEPTION 'Unsupported notification event schema';END IF;
    FOR source_id IN SELECT id FROM notification_sources WHERE workspace_id=w AND event_seq=e.seq AND NOT completed ORDER BY id LIMIT batch_limit LOOP
     SELECT * INTO STRICT fanout FROM notification_fanout_source(w,source_id,budget);budget:=budget-fanout.examined;ids:=ids||fanout.created_ids;
     EXIT WHEN budget=0;
    END LOOP;
    EXIT WHEN EXISTS(SELECT 1 FROM notification_sources WHERE workspace_id=w AND event_seq=e.seq AND NOT completed);
    SELECT * INTO receipt FROM consumer_receipts WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1 AND event_id=e.event_id;
    IF FOUND AND (receipt.seq<>e.seq OR receipt.state NOT IN('applied','ignored') OR receipt.job_id IS NOT NULL) THEN RAISE EXCEPTION 'Inconsistent notification receipt';END IF;
    IF receipt.event_id IS NULL THEN
     INSERT INTO consumer_receipts(workspace_id,consumer_name,consumer_version,event_id,seq,state) VALUES(w,'notifications',1,e.event_id,e.seq,CASE WHEN EXISTS(SELECT 1 FROM notification_sources WHERE workspace_id=w AND event_seq=e.seq) THEN 'applied' ELSE 'ignored' END);GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    END IF;
    UPDATE consumer_checkpoints SET last_seq=e.seq,updated_at=clock_timestamp() WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1 AND last_seq=cp.last_seq;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
    cp.last_seq:=e.seq;processed:=processed+1;
    IF dl.id IS NOT NULL AND dl.state<>'resolved' THEN
     UPDATE consumer_dead_letters SET state='resolved',resolved_at=clock_timestamp() WHERE workspace_id=w AND id=dl.id;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
     UPDATE consumer_alerts SET resolved_at=clock_timestamp() WHERE workspace_id=w AND dead_letter_id=dl.id;
    END IF;
    EXIT WHEN budget=0;
   END LOOP;
   captured:=cardinality(ids);
  END IF;
  IF failure_seq=0 AND dl.id IS NOT NULL AND dl.state<>'resolved' THEN
   UPDATE consumer_dead_letters SET state='resolved',resolved_at=clock_timestamp() WHERE workspace_id=w AND id=dl.id;GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
   UPDATE consumer_alerts SET resolved_at=clock_timestamp() WHERE workspace_id=w AND dead_letter_id=dl.id;
  END IF;

 EXCEPTION WHEN query_canceled OR OTHERS THEN failed:=true;
 END;
 IF failed THEN
  processed:=0;captured:=0;backfilled:=false;
  INSERT INTO consumer_dead_letters(workspace_id,id,consumer_name,consumer_version,seq,event_id,event_type,schema_version,attempts,state,error_code,available_at)
  VALUES(w,gen_random_uuid(),'notifications',1,failure_seq,failure_event,failure_type,failure_schema,1,'retry_wait',code,clock_timestamp()+interval '2 seconds')
  ON CONFLICT(workspace_id,consumer_name,consumer_version,seq) DO UPDATE SET attempts=consumer_dead_letters.attempts+1,state=CASE WHEN consumer_dead_letters.attempts+1>=5 THEN 'blocked' ELSE 'retry_wait' END,error_code=excluded.error_code,available_at=clock_timestamp()+make_interval(secs=>least(900,power(2,least(consumer_dead_letters.attempts+1,9)))) RETURNING id,attempts INTO letter,attempt;
  IF NOT FOUND THEN RAISE EXCEPTION 'Required notification failure record missing';END IF;
  IF attempt>=5 THEN
   INSERT INTO consumer_alerts(workspace_id,dead_letter_id) VALUES(w,letter) ON CONFLICT DO NOTHING;
   IF NOT EXISTS(SELECT 1 FROM consumer_alerts WHERE workspace_id=w AND dead_letter_id=letter) THEN RAISE EXCEPTION 'Required notification alert missing';END IF;dead_lettered:=true;
  END IF;
 END IF;RETURN NEXT;
END $$;
CREATE FUNCTION operator_notification_retry(w uuid,letter uuid,reason_text text) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE n bigint;BEGIN
 IF NOT operator_lock_consumer_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 PERFORM 1 FROM consumer_checkpoints WHERE workspace_id=w AND consumer_name='notifications' AND consumer_version=1 FOR UPDATE;
 UPDATE consumer_dead_letters SET attempts=1,state='retry_wait',available_at=clock_timestamp(),disposition='retry',reason=btrim(reason_text) WHERE workspace_id=w AND id=letter AND consumer_name='notifications' AND consumer_version=1 AND state IN('blocked','retry_wait');
 GET DIAGNOSTICS n=ROW_COUNT;IF n<>1 THEN RAISE EXCEPTION 'Notification dead letter unavailable' USING ERRCODE='P0002';END IF;
 RETURN consumer_operator_audit(w,letter,'notifications.retry',reason_text);
END $$;
CREATE FUNCTION operator_notification_retry_invalidation(w uuid,notification uuid,reason_text text) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE n bigint;BEGIN
 IF NOT operator_lock_consumer_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 UPDATE notification_invalidations SET attempts=0,blocked=false,available_at=clock_timestamp() WHERE workspace_id=w AND notification_id=notification AND published_seq IS NULL AND blocked;GET DIAGNOSTICS n=ROW_COUNT;
 IF n=0 THEN RAISE EXCEPTION 'Blocked notification invalidation unavailable' USING ERRCODE='P0002';END IF;
 RETURN consumer_operator_audit(w,notification,'notifications.invalidation_retry',reason_text);
END $$;
DO $$ DECLARE f regprocedure;r text;BEGIN
 FOR f IN SELECT p.oid::regprocedure FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND (p.proname LIKE 'notification_%' OR p.proname IN('register_notification_consumer','register_workspace_consumers','maintenance_job_authority','system_effect_authority','worker_apply_notification_events','worker_flush_notification_events','operator_notification_retry','operator_notification_retry_invalidation')) LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path=pg_catalog,%I,pg_temp',f,current_schema());EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
 FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants WHERE table_schema=current_schema() AND table_name='issues' AND grantee<>current_user LOOP
  EXECUTE format('REVOKE ALL ON notifications,notification_heads,notification_preferences,notification_invalidations,notification_sources,notification_backfills FROM %I',r);
  EXECUTE format('GRANT SELECT ON notifications,notification_heads,notification_preferences TO %I',r);
  EXECUTE format('GRANT INSERT(workspace_id,user_id,notification_id,notification_version,event_kind) ON notification_invalidations TO %I',r);
  EXECUTE format('GRANT UPDATE(read_at,archived_at,snoozed_until,version) ON notifications TO %I',r);
  EXECUTE format('GRANT INSERT(workspace_id,user_id,assignments,mentions,subscribed_comments),UPDATE(assignments,mentions,subscribed_comments,version,updated_at) ON notification_preferences TO %I',r);
 END LOOP;
END $$;
