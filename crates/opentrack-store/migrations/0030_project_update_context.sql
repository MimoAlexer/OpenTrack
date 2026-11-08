-- Existing updates did not capture dates or reviewed notes. Preserve that unknown
-- history rather than backfilling today's project values into old publications.
ALTER TABLE project_updates
 ADD COLUMN context_captured boolean NOT NULL DEFAULT false,
 ADD COLUMN snapshot_start_date date,
 ADD COLUMN snapshot_target_date date,
 ADD COLUMN notes jsonb,
 ADD CONSTRAINT project_update_snapshot_dates CHECK(
  snapshot_start_date IS NULL OR snapshot_target_date IS NULL OR snapshot_start_date<=snapshot_target_date
 ),
 ADD CONSTRAINT project_update_unknown_context CHECK(
  context_captured OR (snapshot_start_date IS NULL AND snapshot_target_date IS NULL AND notes IS NULL)
 );

-- A private invoker trigger validates API inserts without granting callers a
-- general callable helper that would violate the worker capability boundary.
CREATE FUNCTION project_update_notes_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE value jsonb:=NEW.notes; kind text; item jsonb; seen text[]:=ARRAY[]::text[]; title text; detail text; note_id text;
BEGIN
 IF value IS NULL THEN RETURN NEW; END IF;
 <<validate_notes>>
 BEGIN
 IF jsonb_typeof(value)<>'object' OR NOT value ?& ARRAY['risks','decisions']
    OR value-'risks'-'decisions'<>'{}'::jsonb THEN EXIT validate_notes; END IF;
 FOREACH kind IN ARRAY ARRAY['risks','decisions'] LOOP
  IF jsonb_typeof(value->kind)<>'array' THEN EXIT validate_notes; END IF;
  IF jsonb_array_length(value->kind)>6 THEN EXIT validate_notes; END IF;
  FOR item IN SELECT jsonb_array_elements(value->kind) LOOP
   IF jsonb_typeof(item)<>'object' OR NOT item ?& ARRAY['id','title','detail','status']
      OR item-'id'-'title'-'detail'-'status'<>'{}'::jsonb THEN EXIT validate_notes; END IF;
   IF jsonb_typeof(item->'id')<>'string' OR jsonb_typeof(item->'title')<>'string'
      OR jsonb_typeof(item->'detail')<>'string' OR jsonb_typeof(item->'status')<>'string' THEN EXIT validate_notes; END IF;
   note_id:=item->>'id'; title:=item->>'title'; detail:=item->>'detail';
   IF note_id !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      OR note_id=ANY(seen) THEN EXIT validate_notes; END IF;
   seen:=array_append(seen,note_id);
   IF length(title) NOT BETWEEN 1 AND 160 OR title<>btrim(title) OR title ~ '[[:cntrl:]]'
      OR length(detail)>1000 OR detail<>btrim(detail) OR translate(detail,E'\n\r\t','') ~ '[[:cntrl:]]' THEN EXIT validate_notes; END IF;
   IF kind='risks' AND item->>'status' NOT IN('open','mitigated','closed') THEN EXIT validate_notes; END IF;
   IF kind='decisions' AND item->>'status' NOT IN('needed','decided','deferred') THEN EXIT validate_notes; END IF;
  END LOOP;
 END LOOP;
 RETURN NEW;
 END validate_notes;
 RAISE EXCEPTION 'Invalid project update notes' USING ERRCODE='23514';
END $$;
REVOKE ALL ON FUNCTION project_update_notes_guard() FROM PUBLIC;
DO $$ BEGIN
 EXECUTE format('ALTER FUNCTION project_update_notes_guard() SET search_path=pg_catalog,%I,pg_temp',current_schema());
END $$;
CREATE TRIGGER project_update_notes_shape BEFORE INSERT ON project_updates
 FOR EACH ROW EXECUTE FUNCTION project_update_notes_guard();
-- The existing append-only UPDATE/DELETE/TRUNCATE triggers cover these columns too.
-- The existing feed index also bounds each preceding-snapshot lookup.
