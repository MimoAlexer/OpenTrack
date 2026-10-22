-- Remove previously granted destructive audit privileges from runtime grantees.
-- Keep the trigger from 0002 as defense if an operator later over-grants DML.
DO $$ DECLARE grant_role text; BEGIN
    FOR grant_role IN SELECT DISTINCT grantee FROM information_schema.role_table_grants
        WHERE table_schema=current_schema() AND table_name='audit_entries'
        AND privilege_type IN ('UPDATE','DELETE','TRUNCATE') AND grantee<>current_user
    LOOP
        EXECUTE format('REVOKE UPDATE, DELETE, TRUNCATE ON audit_entries FROM %I',grant_role);
    END LOOP;
END $$;
CREATE INDEX login_attempts_expiry ON login_attempts(window_start,account_hash);
