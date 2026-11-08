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

CREATE FUNCTION project_update_notes_valid(value jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE STRICT AS $$
DECLARE kind text; item jsonb; seen text[]:=ARRAY[]::text[]; title text; detail text; note_id text;
BEGIN
 IF jsonb_typeof(value)<>'object' OR NOT value ?& ARRAY['risks','decisions']
    OR value-'risks'-'decisions'<>'{}'::jsonb THEN RETURN false; END IF;
 FOREACH kind IN ARRAY ARRAY['risks','decisions'] LOOP
  IF jsonb_typeof(value->kind)<>'array' THEN RETURN false; END IF;
  IF jsonb_array_length(value->kind)>6 THEN RETURN false; END IF;
  FOR item IN SELECT jsonb_array_elements(value->kind) LOOP
   IF jsonb_typeof(item)<>'object' OR NOT item ?& ARRAY['id','title','detail','status']
      OR item-'id'-'title'-'detail'-'status'<>'{}'::jsonb THEN RETURN false; END IF;
   IF jsonb_typeof(item->'id')<>'string' OR jsonb_typeof(item->'title')<>'string'
      OR jsonb_typeof(item->'detail')<>'string' OR jsonb_typeof(item->'status')<>'string' THEN RETURN false; END IF;
   note_id:=item->>'id'; title:=item->>'title'; detail:=item->>'detail';
   IF note_id !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      OR note_id=ANY(seen) THEN RETURN false; END IF;
   seen:=array_append(seen,note_id);
   IF length(title) NOT BETWEEN 1 AND 160 OR title<>btrim(title) OR title ~ '[[:cntrl:]]'
      OR length(detail)>1000 OR detail<>btrim(detail) OR translate(detail,E'\n\r\t','') ~ '[[:cntrl:]]' THEN RETURN false; END IF;
   IF kind='risks' AND item->>'status' NOT IN('open','mitigated','closed') THEN RETURN false; END IF;
   IF kind='decisions' AND item->>'status' NOT IN('needed','decided','deferred') THEN RETURN false; END IF;
  END LOOP;
 END LOOP;
 RETURN true;
END $$;
ALTER TABLE project_updates ADD CONSTRAINT project_update_notes_shape
 CHECK(notes IS NULL OR project_update_notes_valid(notes));
-- The existing append-only UPDATE/DELETE/TRUNCATE triggers cover these columns too.
-- The existing feed index also bounds each preceding-snapshot lookup.
