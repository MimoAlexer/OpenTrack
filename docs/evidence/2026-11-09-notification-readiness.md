# Due recipient invalidation discovery

The default worker visited one registered workspace per notification-publication
stage, including idle scopes. With 40 idle workspaces, a real recipient HTTP
PATCH committed its read state but its durable version-2 invalidation remained
unpublished after 10,010 ms. The daemon completed 20 notification publication
rounds. This reproduces the connected inbox deadline without extending it.

Migration 0036 adds private workspace scheduling hints. An outbox trigger locks a
stable workspace guard before querying the earliest unpublished, unblocked row.
Its nullable due time changes atomically with the original outbox transaction.
Missing/suppressed guard or update effects fail the original operation. No source
locks are acquired after the guard, and outbox recipient RLS is unchanged.

Only the owner can read the hint globally; owner writes require the current
workspace. Runtime, worker and operator roles receive no raw hint access. The
worker receives only a fixed-path definer selector returning the next due UUID.
Its keyset cursor retains round-robin fairness between due workspaces, skipping
idle, blocked and deferred scopes. Existing publication admission remains intact.

The real daemon regression uses its default 2,000-ms idle interval, 40 actual
workspaces and a real API PATCH. With readiness discovery, publication completed
in 2,013 ms (six publication rounds), within the same ten-second deadline.

Seven PostgreSQL notification contracts pass, including recipient privacy,
repeat worker/operator grants, raw-hint denial, runtime grant-drift rejection,
suppressed INSERT/UPDATE rollback, deferred/blocked skipping, due-workspace
fairness, and concurrent final publication versus insertion. The race covers both
guard acquisition orders and insertion commit/rollback, observing actual database
blocking before release. These are local contract results; integrated three-engine
browser timing and development/CI repeat provisioning remain separate gates.

The migration is additive and preserves recipient content. Source rollback can
use the previous registry scan while leaving the private projection installed.
No destructive down-migration is supplied. Existing migrations 0034 and 0035 are
unchanged.
