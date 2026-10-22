-- Existing definitions keep literal brace semantics and source digests.
ALTER TABLE issue_templates ADD COLUMN title_mode text NOT NULL DEFAULT 'literal'
 CHECK(title_mode IN ('literal','pattern'));
