-- Logical replay-storage admission; this does not measure heap, WAL or free disk.
ALTER TABLE workspace_stream_heads
 ADD COLUMN max_pending_events bigint NOT NULL DEFAULT 100000 CHECK(max_pending_events>0),
 ADD COLUMN max_retained_event_bytes bigint NOT NULL DEFAULT 268435456 CHECK(max_retained_event_bytes>0),
 ADD COLUMN retained_event_bytes bigint NOT NULL DEFAULT 0 CHECK(retained_event_bytes>=0);
ALTER TABLE workspace_events ADD COLUMN storage_bytes bigint NOT NULL DEFAULT 0 CHECK(storage_bytes>=0);

-- Version 1 charges a fixed reference/envelope allowance and both variable text
-- fields. Persist each charge so pruning and metadata corrections remain exact.
DO $$ DECLARE w uuid;saved text:=current_setting('opentrack.workspace_id',true);BEGIN
 FOR w IN SELECT id FROM workspaces ORDER BY id LOOP
  PERFORM set_config('opentrack.workspace_id',w::text,true);
  UPDATE workspace_events SET storage_bytes=192+octet_length(event_type)::bigint+octet_length(actor_kind)::bigint WHERE workspace_id=w;
  UPDATE workspace_stream_heads SET retained_event_bytes=(SELECT coalesce(sum(storage_bytes),0)::bigint FROM workspace_events WHERE workspace_id=w) WHERE workspace_id=w;
 END LOOP;
 PERFORM set_config('opentrack.workspace_id',coalesce(saved,''),true);
END $$;
ALTER TABLE workspace_events ALTER COLUMN storage_bytes DROP DEFAULT;

CREATE FUNCTION event_storage_charge() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND (NEW.workspace_id,NEW.seq,NEW.event_id) IS DISTINCT FROM(OLD.workspace_id,OLD.seq,OLD.event_id) THEN
  RAISE EXCEPTION 'Event stream identity is immutable' USING ERRCODE='23514',CONSTRAINT='event_stream_identity';
 END IF;
 NEW.storage_bytes:=192+octet_length(NEW.event_type)::bigint+octet_length(NEW.actor_kind)::bigint;
 RETURN NEW;
END $$;
CREATE TRIGGER event_storage_charge BEFORE INSERT OR UPDATE ON workspace_events FOR EACH ROW EXECUTE FUNCTION event_storage_charge();

CREATE FUNCTION event_storage_account() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE statement text;batch record;head workspace_stream_heads%ROWTYPE;watermark bigint;next_bytes bigint;n bigint;
BEGIN
 statement:=CASE TG_OP
  WHEN 'INSERT' THEN 'SELECT workspace_id,sum(storage_bytes)::bigint AS byte_delta,min(seq) AS first_seq,max(seq) AS last_seq FROM new_events GROUP BY workspace_id ORDER BY workspace_id'
  WHEN 'DELETE' THEN 'SELECT workspace_id,-sum(storage_bytes)::bigint AS byte_delta FROM old_events GROUP BY workspace_id ORDER BY workspace_id'
  ELSE 'SELECT n.workspace_id,(n.bytes-o.bytes)::bigint AS byte_delta FROM (SELECT workspace_id,sum(storage_bytes) AS bytes FROM new_events GROUP BY workspace_id) n JOIN (SELECT workspace_id,sum(storage_bytes) AS bytes FROM old_events GROUP BY workspace_id) o USING(workspace_id) ORDER BY n.workspace_id'
 END;
 -- Transition tables acquire every event-row lock before the stream head. In
 -- particular, pruning must not lock the head between individual row deletes.
 FOR batch IN EXECUTE statement LOOP
  IF batch.workspace_id IS DISTINCT FROM nullif(current_setting('opentrack.workspace_id',true),'')::uuid THEN
   RAISE EXCEPTION 'Event workspace scope unavailable' USING ERRCODE='42501';
  END IF;
  SELECT * INTO STRICT head FROM workspace_stream_heads WHERE workspace_id=batch.workspace_id FOR UPDATE;
  IF TG_OP='INSERT' THEN
   IF batch.first_seq<1 OR batch.last_seq>head.last_seq THEN
    RAISE EXCEPTION 'Event sequence was not reserved' USING ERRCODE='23514',CONSTRAINT='event_sequence_unreserved';
   END IF;
   -- Never lock checkpoints after the head: retention takes those locks first.
   -- Disabled required consumers keep their obligation and admission watermark.
   IF NOT EXISTS(SELECT 1 FROM consumer_registrations WHERE workspace_id=batch.workspace_id AND consumer_name='assignment_cleanup' AND consumer_version=1 AND required)
   OR EXISTS(SELECT 1 FROM consumer_registrations r LEFT JOIN consumer_checkpoints c USING(workspace_id,consumer_name,consumer_version) WHERE r.workspace_id=batch.workspace_id AND r.required AND (c.workspace_id IS NULL OR c.last_seq>=batch.first_seq)) THEN
    RAISE EXCEPTION 'Required consumer state unavailable; repair before appending events' USING ERRCODE='P0001',CONSTRAINT='event_admission_blocked';
   END IF;
   SELECT min(c.last_seq) INTO watermark FROM consumer_registrations r JOIN consumer_checkpoints c USING(workspace_id,consumer_name,consumer_version) WHERE r.workspace_id=batch.workspace_id AND r.required;
   IF watermark IS NULL OR head.last_seq-watermark>head.max_pending_events THEN
    RAISE EXCEPTION 'Required consumer backlog reached its configured event limit' USING ERRCODE='P0001',CONSTRAINT='event_admission_blocked';
   END IF;
  END IF;
  next_bytes:=head.retained_event_bytes+batch.byte_delta;
  IF next_bytes<0 THEN
   RAISE EXCEPTION 'Retained event accounting is inconsistent' USING ERRCODE='23514',CONSTRAINT='event_storage_accounting';
  END IF;
  IF batch.byte_delta>0 AND next_bytes>head.max_retained_event_bytes THEN
   RAISE EXCEPTION 'Retained events reached the configured logical storage limit' USING ERRCODE='P0001',CONSTRAINT='event_admission_blocked';
  END IF;
  IF batch.byte_delta<>0 THEN
   UPDATE workspace_stream_heads SET retained_event_bytes=next_bytes WHERE workspace_id=batch.workspace_id;
   GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
  END IF;
 END LOOP;
 RETURN NULL;
END $$;
CREATE TRIGGER event_storage_insert AFTER INSERT ON workspace_events REFERENCING NEW TABLE AS new_events FOR EACH STATEMENT EXECUTE FUNCTION event_storage_account();
CREATE TRIGGER event_storage_update AFTER UPDATE ON workspace_events REFERENCING OLD TABLE AS old_events NEW TABLE AS new_events FOR EACH STATEMENT EXECUTE FUNCTION event_storage_account();
CREATE TRIGGER event_storage_delete AFTER DELETE ON workspace_events REFERENCING OLD TABLE AS old_events FOR EACH STATEMENT EXECUTE FUNCTION event_storage_account();

CREATE FUNCTION operator_event_admission_status(w uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE snapshot record;reason text;pending bigint;remaining bigint;
BEGIN
 IF NOT worker_lock_maintenance_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 SELECT h.last_seq,h.max_pending_events,h.max_retained_event_bytes,h.retained_event_bytes,min(c.last_seq) AS watermark,
 coalesce(bool_or(c.workspace_id IS NULL OR c.last_seq>h.last_seq),true)
 OR NOT coalesce(bool_or(r.consumer_name='assignment_cleanup' AND r.consumer_version=1),false) AS consumer_unavailable
 INTO STRICT snapshot FROM workspace_stream_heads h
 LEFT JOIN consumer_registrations r ON r.workspace_id=h.workspace_id AND r.required
 LEFT JOIN consumer_checkpoints c ON c.workspace_id=r.workspace_id AND c.consumer_name=r.consumer_name AND c.consumer_version=r.consumer_version
 WHERE h.workspace_id=w GROUP BY h.workspace_id;
 pending:=snapshot.last_seq-snapshot.watermark;
 remaining:=greatest(0,snapshot.max_retained_event_bytes-snapshot.retained_event_bytes);
 IF snapshot.consumer_unavailable OR snapshot.watermark IS NULL THEN
  reason:='required_consumer_unavailable';
 ELSIF pending>=snapshot.max_pending_events THEN reason:='pending_event_limit';
 ELSIF remaining<199 THEN reason:='retained_event_byte_limit';
 END IF;
 -- Threshold status uses the smallest possible charge, not a proposed batch.
 RETURN jsonb_build_object('max_pending_events',snapshot.max_pending_events::text,'max_retained_event_bytes',snapshot.max_retained_event_bytes::text,'retained_event_bytes',snapshot.retained_event_bytes::text,'remaining_event_bytes',remaining::text,'minimum_event_bytes','199','pending_events',pending::text,'admission_blocked',reason IS NOT NULL,'admission_reason',reason,'accounting','logical_v1');
END $$;

CREATE FUNCTION operator_event_admission_limits(w uuid,max_pending bigint,max_bytes bigint,reason_text text) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE h workspace_stream_heads%ROWTYPE;correlation uuid:=gen_random_uuid();n bigint;
BEGIN
 IF max_pending IS NULL OR max_pending<1 OR max_bytes IS NULL OR max_bytes<1 THEN
  RAISE EXCEPTION 'Event admission limits must be positive' USING ERRCODE='22023';
 END IF;
 IF reason_text IS NULL OR length(btrim(reason_text)) NOT BETWEEN 1 AND 500 OR reason_text~'[[:cntrl:]]' THEN
  RAISE EXCEPTION 'A bounded reason is required' USING ERRCODE='22023';
 END IF;
 IF NOT operator_lock_consumer_context(w) THEN RAISE EXCEPTION 'Workspace unavailable' USING ERRCODE='P0002';END IF;
 SELECT * INTO STRICT h FROM workspace_stream_heads WHERE workspace_id=w;
 INSERT INTO audit_entries(id,workspace_id,actor_kind,operator_identity,action,target_id,correlation_id,metadata)
 VALUES(correlation,w,'local_operator',session_user,'events.admission_limits_changed',w,correlation,jsonb_build_object('before',jsonb_build_object('max_pending_events',h.max_pending_events::text,'max_retained_event_bytes',h.max_retained_event_bytes::text),'after',jsonb_build_object('max_pending_events',max_pending::text,'max_retained_event_bytes',max_bytes::text),'reason',btrim(reason_text)));
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 INSERT INTO consumer_operations(workspace_id,id,action,through_seq,affected_count,reason,operator_identity)
 VALUES(w,correlation,'events.admission_limits_changed',h.last_seq,1,btrim(reason_text),session_user);
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 UPDATE workspace_stream_heads SET max_pending_events=max_pending,max_retained_event_bytes=max_bytes WHERE workspace_id=w;
 GET DIAGNOSTICS n=ROW_COUNT;PERFORM consumer_required(n);
 RETURN correlation;
END $$;

DO $$ DECLARE f regprocedure;r record;columns text;BEGIN
 FOR f IN SELECT p.oid::regprocedure FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proname IN('event_storage_charge','event_storage_account','operator_event_admission_status','operator_event_admission_limits') LOOP
  EXECUTE format('ALTER FUNCTION %s SET search_path TO pg_catalog,%I,pg_temp',f,current_schema());
  EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC',f);
 END LOOP;
 SELECT string_agg(quote_ident(attname),',' ORDER BY attnum) INTO columns FROM pg_attribute WHERE attrelid='workspace_stream_heads'::regclass AND attnum>0 AND NOT attisdropped;
 FOR r IN SELECT DISTINCT role.rolname,has_any_column_privilege(role.oid,'workspace_stream_heads'::regclass,'INSERT') AS can_insert,has_column_privilege(role.oid,'workspace_stream_heads'::regclass,'last_seq','UPDATE') AS can_update_seq
 FROM pg_roles role WHERE role.oid<>(SELECT relowner FROM pg_class WHERE oid='workspace_stream_heads'::regclass)
 AND (role.oid IN(SELECT acl.grantee FROM pg_class c CROSS JOIN LATERAL aclexplode(c.relacl) acl WHERE c.oid='workspace_stream_heads'::regclass)
 OR role.oid IN(SELECT acl.grantee FROM pg_attribute a CROSS JOIN LATERAL aclexplode(a.attacl) acl WHERE a.attrelid='workspace_stream_heads'::regclass)) LOOP
  EXECUTE format('REVOKE INSERT,DELETE,UPDATE,TRUNCATE,TRIGGER ON workspace_stream_heads FROM %I',r.rolname);
  EXECUTE format('REVOKE INSERT(%1$s),UPDATE(%1$s) ON workspace_stream_heads FROM %2$I',columns,r.rolname);
  IF r.can_insert THEN EXECUTE format('GRANT INSERT(workspace_id) ON workspace_stream_heads TO %I',r.rolname);END IF;
  IF r.can_update_seq THEN EXECUTE format('GRANT UPDATE(last_seq) ON workspace_stream_heads TO %I',r.rolname);END IF;
 END LOOP;
 REVOKE INSERT,DELETE,UPDATE,TRUNCATE,TRIGGER ON workspace_stream_heads FROM PUBLIC;
 EXECUTE format('REVOKE INSERT(%1$s),UPDATE(%1$s) ON workspace_stream_heads FROM PUBLIC',columns);
END $$;
