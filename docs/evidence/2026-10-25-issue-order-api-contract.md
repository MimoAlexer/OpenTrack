# Manual issue-order backend qualification

25 October 2026. Private code head `8a5c25f14df0f13cd5bd8560851598e05378e476`.
[Machine receipt](2026-10-25-issue-order-api-contract.json) binds the exact source,
private logs, original failures and limits.

This is the retained qualification of that earlier source. Subsequent review found
corrupted Unicode literals and omitted ETag header declarations; the
[corrective qualification](2026-10-25-issue-order-review-corrections.md) records
their repair, the full domain suite and the new conditional HTTP checks. Statements
below about the earlier run do not establish a full domain-suite or 304 runtime pass.

The later [expired-success replay regression](2026-10-25-expired-order-replay.json)
extends one existing case without changing production behavior. After a successful
position and subsequent moves, replay of the original key/body/precondition against
its expired stored receipt returns ordinary409 conflict. It does not assert that
the original request was unapplied; the original acknowledgement, complete receipt
row and current domain/history snapshot remain intact. The one strengthened case,
formatting and strict affected Clippy pass; the other eight cases were not rerun.

All nine current-head HTTP-router/PostgreSQL cases pass in one final run. They
exercise source/version ordering, rank injection rejection, full-group adjacency,
idempotent replay and no-op effects, manual cursor binding, inline500 and durable501
maintenance, required-event rollback, partial-apply access loss and manager takeover,
authorization-bound progress/ETags, and ordinary edit/maintenance exclusion. The
fresh HTTP fixture now mirrors the migration's staging/progress restrictions and
receives only the scoped inline execution capability. Its startup and permission
invariants are executed. Required maintenance audits are checked separately from
Issue history; they are not hidden by dropping all audit assertions.

Four pure ordering tests, strict affected API Clippy, the new native method/UUID
route test and all published-operation route parity pass. The complete native core
suite passes50/50 and strict native Clippy; five schema startup regressions also
pass without repairing metadata or implicitly upgrading historical fixtures.
These are separate from native GUI or physical-input acceptance. All workspace test
targets compile; this is not a full workspace runtime pass. Generated OpenAPI has
only the expected three new ordering endpoints, manual-page response/types and the
previously implemented optional team query. The compiled UI runtime is frozen and
source-bound; its connected manual-order flows are still being implemented.

The original missing-route baseline, first seven-case failure, missing SQL status
join and overbroad intent-audit snapshot diagnosis remain retained. Earlier
migration/test failures and matching isolated schemas have not been overwritten.
Migration27 remains unpublished and was applied only to fresh test schemas. The
retained application database is unchanged. No source rollback against an unmatched
database or checksum rewrite is qualified.

This does not close #30 or #31. Connected UI, complete legacy-fixture/runtime
compatibility, native GUI and physical/assistive input, measured performance,
all four final hosted checks, independent final review and integrated-main
acceptance remain separate requirements.
