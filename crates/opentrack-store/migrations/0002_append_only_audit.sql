-- Runtime identities may append audit records, but cannot rewrite their history.
-- Retention will use a dedicated audited maintenance capability; ordinary DML
-- must never bypass this boundary, including when grants are configured broadly.
CREATE FUNCTION audit_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Audit entries are append-only' USING ERRCODE = '42501';
END;
$$;
CREATE TRIGGER audit_no_rewrite BEFORE UPDATE OR DELETE ON audit_entries
    FOR EACH ROW EXECUTE FUNCTION audit_append_only();
