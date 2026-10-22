# Team cycles

A cycle belongs to one team. Maintainers create and manage it; current contributors may explicitly plan accessible, active issues. Workspace viewers can read the same visible team projection. Every command rechecks current workspace, team and session access under authorization guards. Private cycles, scope counts, snapshots and reports stay behind that same team boundary.

## Calendar and lifecycle

Dates are local calendar dates in an explicitly selected IANA timezone. The end is exclusive. A cycle spans 1–366 calendar days. The captured timezone is immutable; later changes to account or workspace preferences cannot reinterpret its history. The API also returns the corresponding UTC boundary instants. A local spring-transition day can span 23 hours and an autumn-transition day 25 hours; date arithmetic never assumes 24-hour days.

PostgreSQL enforces nonoverlapping `daterange(start_date,end_date_exclusive,'[)')` values per workspace/team, including closed cycles. Adjacent intervals are valid. Team serialization gives friendly conflict handling, while the exclusion constraint also rejects competing direct inserts and date edits.

Creation produces `planned`; opening is explicit and captures the opening scope. Opening and closing do not depend on the current clock, allowing reviewed historical intervals. An unused planned cycle can be tombstoned with its current ETag, releasing its dates. Opened cycles retain their history. Closed cycles reject ordinary edits or scope changes. Reopening is a separate maintainer action with a required reason, an immutable reason record and a fresh snapshot. Earlier close reports remain unchanged.

Each issue has at most one cycle membership. Assignment requires both the cycle ETag and the issue ETag. Cross-team membership is rejected by scoped foreign keys. A linked issue must be explicitly detached before moving teams. Neither assignment nor closure changes workflow status.

## Scope, snapshots and close review

The current scope table contains issue IDs, expected versions, workflow categories, estimates and explicit exclusion flags. Deleted and archived issues are excluded from counts. Scope count includes completed, canceled and unfinished issues; the progress denominator is `scope_count - canceled_count`, with zero meaning unscoped. Estimate total excludes canceled, archived and deleted work. All metrics use definition version 1 and an authoritative calculation timestamp.

Opening, reopening, active scope changes, closing and successful carry-forward append immutable snapshots. The explicit daily capture endpoint records at most one `daily` snapshot per cycle/local date; repeated capture returns the original record. The caller or a later scheduling service invokes capture: this increment does not install an automatic daily timer or recurrence scheduler. Additional lifecycle/scope snapshots on the same day retain changes without silently rewriting the daily baseline. Snapshot history is cursor-paginated and reauthorized on every request. Its distinct `CycleSnapshotPoint` DTO contains only metrics and observation metadata; PostgreSQL aggregates currently authorized source facts inside a bounded query instead of sending historical item arrays into application/client memory. Current scope and close reports retain the reviewed item table.

Close preview separates completed, canceled, unfinished and excluded items. Its review token binds the cycle representation and every scoped issue version/category/exclusion/estimate. Closing requires that token, the current cycle ETag and an idempotency key. An intervening issue edit invalidates the preview even if it leaves counts unchanged. The state, immutable snapshot, immutable report, receipt, audit and event commit together. Missing mandatory effects roll everything back. Closing leaves all issue statuses and memberships unchanged. The cycle exposes `latest_report_id` for reopening a saved report after a fresh client session.

## Explicit carry-forward and recovery

Carry-forward is a separate reviewed command against a close report. Select 1–100 unfinished, nonexcluded issue IDs from that report and supply their current numeric versions, source cycle ETag, destination cycle ETag and an idempotency key. The destination must be a later planned/open cycle in the same team. Cycle scope is bounded to 10,000 issues.

The durable operation records one result for each selected issue: `moved`, `version_conflict`, `scope_conflict`, `no_longer_unfinished`, `destination_full` or `unavailable`. A conflict never moves or changes the workflow of that issue. Successful items commit once, and the immutable close report still reflects the original closed scope. Same-key retries recover the stored operation instead of moving work again.

An operation with unresolved results is `needs_review`. Fetch current issue versions and destination state, review the differences, then resume only those unresolved IDs using the operation ETag and a new idempotency key. Prior successes cannot be submitted again. Each resumption retains all prior results, replaces explicitly retried results and advances the operation version. An operation becomes `complete` only when all originally selected items moved. Reopening/reclosing the source invalidates carry-forward against an earlier close report.

## Migration and verification

Migration 0024 adds cycles, single-membership joins, immutable scope/close/reopen records and durable carry operations with forced workspace RLS and scoped foreign keys. It installs trusted PostgreSQL `btree_gist` in the separate migration-owned `opentrack_extensions` schema, with a database-wide transaction advisory lock before extension creation. PUBLIC schema access is revoked; ordinary runtime and worker roles receive neither USAGE nor CREATE there. Existing namespace and extension ownership are checked. A preinstalled extension outside that schema requires explicit database-operator relocation before upgrade; the migrator fails closed rather than adopting foreign-owned extension objects. The migration role needs database CREATE privilege for that first installation; the runtime role gets no extension or migration authority. PostgreSQL 18 distributions must include the extension (the configured official image does). All application test tables use generated isolated schemas.

Apply after 0023 before starting the matching API. Earlier migrations remain immutable. No destructive down migration is provided: rollback uses a quiesced pre-upgrade database backup and matching binaries. The upgrade contract preserves existing issue data. API and real PostgreSQL tests are in `crates/opentrack-api/tests/cycles_contract.rs` and `crates/opentrack-app/tests/cycles_races.rs`; executed results are recorded with the PR. Cycle screens and automatic recurrence remain separate tasks.

Historical snapshot, close-report and carry-operation reads (including idempotent receipt replay) reauthorize every issue against current issue/team access as well as the recorded cycle team. Inaccessible issue IDs and facts are omitted, displayed counts/estimates are recomputed from visible source facts, and no hidden-item count is returned. The projection indicates `redacted` when incomplete; immutable database facts remain unchanged for authorized recovery.

A redacted carry operation projects state `restricted` rather than inferring completion from its remaining visible results. Its durable internal state remains unchanged; fetch an authorized complete projection before treating the operation as fully reconciled.

The current issue table is separately cursor-paginated at `/{id}/issues`, returning live authorized titles, workflow metadata, estimates, versions and issue ETags. Desktop responses retain the default 1 MiB limit except exact single-scope, close-preview/report reads and single close/daily-capture receipts, which permit 4 MiB. Metric history and current issue pages retain the default cap. Qualification covers 50 historical snapshots with 10,000 issues each, a full 10,000-item current scope and a 100-row live issue page under unchanged database statement limits.
