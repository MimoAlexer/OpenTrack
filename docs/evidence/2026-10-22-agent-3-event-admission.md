# Agent 3 durable-event admission increment

22 October 2026. Branch `codex/goal-03-discovery-notifications`, synchronized with main `f224100` (documentation-only changes since the tested implementation baseline). Issue #19 remains in progress. This record covers the additive 0025 increment, not completion of M03 or the complete product.

## Behavior

All committed replay event inserts share PostgreSQL admission enforcement, including issue batches, identity changes and worker effects. Each workspace defaults to at most 100,000 events beyond the slowest required consumer checkpoint and 268,435,456 logical retained event bytes. Disabled required consumers retain their obligation; missing required registration/checkpoint state refuses new events. A refused event rolls back its domain write, activity, audit, retry receipt and stream-head reservation. The API uses its existing retryable 503 response; it does not silently replay an uncertain write.

Logical accounting version 1 charges 192 bytes per event envelope plus the UTF-8 byte lengths of event_type and actor_kind. The stored per-event charge makes prune and metadata correction deltas exact within this model. This is a bounded replay-log budget, not a measurement of PostgreSQL heap/index/WAL size or free disk. Physical disk monitoring and whole-installation storage qualification remain required.

Capture/checkpoint advancement, bounded safe retention and reasoned operator limit changes do not append replay events and remain available under pressure. Domain changes that require events, including access changes and event-emitting cleanup batches, can be refused until capacity is restored. No event family bypasses admission. Existing watermark/dead-letter protections are retained; no pruning of uncaptured required work is authorized.

## Operation and migration

Use the separate restricted operator role:

```sh
opentrack-server operator-consumer-status --workspace WORKSPACE_UUID
opentrack-server operator-event-limits --workspace WORKSPACE_UUID --max-pending-events 100000 --max-event-bytes 268435456 --reason 'Capacity reviewed'
```

The status includes redacted admission counters/limits and required-consumer state. It is not an estimate of a particular future batch's size. Inspect consumer lag/dead letters first; repair/capture or safely prune before considering a reasoned capacity increase. Limit changes preserve before/after values and a reason in mandatory audit records.

Migration 0025 adds limits/accounting and admission triggers, backfills existing scoped events, and restricts serving credentials to head creation and sequence assignment. It changes neither an applied historical migration nor existing event identities. Apply migrations and matching restricted operator grants before starting the new binary. Rollback requires the matching pre-upgrade database backup and compatible application binary; removing triggers or lowering a ledger version is not a rollback.

## Executed verification

The existing baseline event-consumer contract passed all four real PostgreSQL tests on the isolated Agent 3 database before this increment: bounded capture/receipt/checkpoint fencing, poison disposition/disabled-consumer/seven-day retention, mandatory-effect rollback/credential separation, and statement deadline/durable retry.

The corrected focused run passed 12 real PostgreSQL tests: six new admission contracts, two new ordering contracts and the four existing consumer contracts. They cover atomic issue/identity rollback, same-key retry, disabled and missing/ahead consumer state, pruning/recovery, mandatory operator audit, restricted grants, schema-24 backfill/readiness and observed concurrent lock/cap boundaries. Commands: `cargo test --locked -p opentrack-app --test event_order_contract --test event_admission_contract --test event_consumer_contract -- --test-threads=2`, under the shared verification lock through the assigned WSL environment. Linux helper suites also passed 3 operator credential, 3 worker credential and 14 CI-scope cases. Formatting and strict all-target Clippy passed on the final source, including the new HTTP fixture. The new router/PostgreSQL test also passed: it verifies generic retryable 503 responses for issue and identity writes, no partial state, restored-capacity same-key retry and committed replay. The full Rust workspace suite is still running at publication; final PR check results remain required before merge. Frontend/browser/native/container acceptance is not inferred from these local backend results.

Initial failures and corrections: four admission tests first assumed setup appended an event; setup actually leaves an empty stream. The fixture now creates a real issue, and all six admission cases pass without relaxing their assertions. Unix credential helpers initially failed on Windows because fcntl is unavailable; the Linux rerun passed. The first full test launch stopped on a CRLF in the ignored local check script; the script now uses LF. Inspection also corrected the scoped migration backfill, protected repeat-provisioning/head grants and future-checkpoint validation before this passing focused run. No failed run is reported as acceptance.

## Remaining scope

The issue remains open for later consumers, broader administrative surfaces and full current-authority/retention qualification. This increment does not implement search, SSE, notifications/email, reports or physical disk monitoring. Native, browser, assistive-technology and complete-product gates remain separate.
