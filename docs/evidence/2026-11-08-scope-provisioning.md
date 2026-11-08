# Scope provisioning grant verification

Executed locally on 8 November 2026 with PostgreSQL 18.6, using only the idle isolated cluster on port 55672. No application listener was started.

The development `scripts/dev-db.sh setup` path was rerun against a schema containing migration 0031. Separately, `scripts/ci-db.py` created unique disposable CI identities and a test database; the actual migrator applied the current migrations, then CI provisioning ran again. Catalog checks before and after reprovisioning matched the policy below. The disposable CI database and its four roles were removed afterward.

| Runtime permission | Development reprovisioning | CI migration / reprovisioning |
| --- | --- | --- |
| Scope-head SELECT | Allowed | Allowed / allowed |
| Scope-head INSERT, table UPDATE, identity-column UPDATE, DELETE | Denied | Denied / denied |
| Scope-head revision-column UPDATE | Allowed | Allowed / allowed |
| Scope-event SELECT | Allowed | Allowed / allowed |
| Scope-event INSERT, UPDATE, DELETE | Denied | Denied / denied |
| Baseline INSERT and baseline-item INSERT | Allowed | Allowed / allowed |

Checks used `has_table_privilege` and `has_column_privilege`; no forged-history data mutation was attempted. `bash -n scripts/dev-db.sh`, Python compilation of `scripts/ci-db.py`, and Git whitespace checks passed. These results qualify the provisioning paths and their positive application grants; they do not substitute for native, recovery or complete-product qualification.
