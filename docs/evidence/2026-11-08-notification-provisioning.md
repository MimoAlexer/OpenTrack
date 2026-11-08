# Notification grants survive repeated provisioning

Executed on 8 November 2026 against a dedicated local PostgreSQL 18 helper cluster on loopback port 55720. Its data and credentials were separate from the integration cluster, and the PostgreSQL runtime was reused read-only. The API/worker probe starts no listener or job. Only provisioning helpers, the catalog probe and this evidence record changed.

Both development setup and CI bootstrap were first run against a fresh helper database, followed by the actual application migration sequence through `0031` plus `0034` and matching worker/operator grants. Before the correction, an actual provisioning rerun in each profile changed the probe to:

```json
{"app_connect":false,"notification_grants":false,"worker_connect":true,"workflow_grants":false}
```

Independent catalog checks also found that private notification sources were accessible, inbox heads were writable, recipient notification INSERT was allowed, and the private invalidation outbox was readable. This red check occurred before changing either provisioning helper. The App constructor correctly rejected the serving credential; its authority checks were retained.

Both helpers now conditionally revoke access to `notification_sources`/`notification_backfills` and restore the precise migration-0034 grants after their broad table grant. Notifications permit recipient state/version updates, heads remain read-only, preferences retain their defined INSERT/UPDATE columns, and invalidations retain only their defined INSERT columns. Required reads and writes for existing subscriptions/templates/planning remain available.

The read-only probe checks all six notification relations when the notification schema is present. It verifies forced RLS and every column's SELECT, INSERT, UPDATE and REFERENCES privileges, as well as denial of DELETE/TRUNCATE/TRIGGER authority. Its catalog check also runs after a rejected App constructor, making the regression observable without accessing source content.

After the fix, development setup was rerun twice and CI bootstrap was rerun twice, with matching CI worker/operator grants reapplied. Every one of the four post-migration repeats produced:

```json
{"app_connect":true,"notification_grants":true,"worker_connect":true,"workflow_grants":true}
```

A read-only comparison of every successful migration checksum against the branch's migration files passed for both databases after the repeats. Migration ledgers were not edited. No integration-cluster credentials were changed, and the dedicated helper cluster was stopped after qualification.

Validation passed:

- Actual development and CI bootstrap/migration/reprovisioning paths with the red and repeated green catalog/constructor results above.
- `cargo run -q -p opentrack-app --example check_provisioned_connections -- --read-only-local-role-check` with explicit private helper credentials.
- `cargo clippy -p opentrack-app --example check_provisioned_connections -- -D warnings`.
- `scripts/test-ci-scope.py` (14 tests), `scripts/test-dev-worker-credentials.py` (3 tests), and `scripts/test-dev-operator-credentials.py` (3 tests).
- Shell syntax, Python compilation, formatting and whitespace checks.

This record qualifies post-0034 provisioning ACL preservation. It adds no consumer, API, realtime or inbox behavior and does not repeat or broaden the separate notification-consumer acceptance claim.
