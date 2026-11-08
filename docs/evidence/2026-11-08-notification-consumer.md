# Notification consumer qualification — 8 November 2026

## Environment and scope

This focused branch starts from `8a5df12` and includes the corrected private project-note validator and scoped database provisioning base (`fd4aa2d`, `e668f2a`). Checks used an explicitly isolated local PostgreSQL 18 database, restricted runtime/worker/operator credentials, fresh random per-test schemas, two Cargo build jobs and a disk-backed build target. The original development database and build were untouched. This branch qualifies migrations through `0031` plus notification migration `0034`; filters `0032` and realtime `0033` require their integrated checks.

The increment implements in-app assignment/mention/subscribed-comment sources, a required bounded consumer, recipient heads/preferences/state storage, a private invalidation outbox and audited operator retry commands. Source metadata remains references only.

## Executed evidence

The following app contract command passed **29 tests**: event admission 7, event consumer 5, notification consumer 12 and planning consumer 5.

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo test -p opentrack-app --test notification_consumer_contract --test event_consumer_contract --test event_admission_contract --test planning_counts_consumer_contract -- --test-threads=1
```

The notification cases cover actual restricted App commits; exact source actor/entity/version binding; newly assigned restoration; added issue/comment/project mentions; no self-notification; event/reason dedupe; preference and explicit unsubscribe behavior; observed database-lock waits for preference/unsubscribe before dispatch; current/origin-team access and account disablement; 205-recipient fanout across persisted batches and worker restart; rollback of inbox rows/heads/outbox with a suppressed receipt; progress preceding a later poison event; five-attempt blocking and operator recovery; saturated event admission with durable inbox/outbox preservation; and denial of runtime forged system events.

The schema-31 upgrade fixture authors 102 mention intents through App mutations plus a discussion source. An owner fixture removes the prior replay and records its purge boundary. After migration, the first historical tick captures exactly 100 recipients without completing its backfill marker; restart finishes the remaining source work with no invented assignment notification. This is a controlled historical-source recovery check, rather than a deployment backup/restore rehearsal.

An additional fresh targeted outbox recovery run executed the production `deploy/grant-operator.sql` script before using the restricted Operator status/retry wrappers, and passed. Both worker and operator grant scripts are therefore exercised by real database paths. The final legacy-pending partial index is included in that migration run.

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo test -p opentrack-app --test notification_consumer_contract invalidation_failures_block_and_operator_retry_preserves_inbox -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo test -p opentrack-app --test assignment_cleanup_contract -- --test-threads=1
```

The assignment cleanup suite passed **3 tests**, including actual system-attributed cleanup, required-effect rollback and the historical schema-14 upgrade. Its bulk owner-seeded historical fixture explicitly clears human actor context; actual App mutation paths retain the authored-source guard.

The full worker daemon suite initially passed 11 cases and exposed two legacy fixture assumptions: a fixed job count omitted the new truthful notification backfill marker, and cached effects expected five maintenance selectors instead of seven. Both corrected cases passed targeted reruns. The remaining 11 cases already covered one-pass execution, shutdown, private worker authority, default dispatch and bounded discovery. No worker budget or timing threshold was relaxed.

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo test -p opentrack-server --test worker_daemon_contract -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo test -p opentrack-server --test worker_daemon_contract daemon_continues_healthy_workspace_work_after_capture_error -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo test -p opentrack-server --test worker_daemon_contract four_claim_quantum_preserves_two_workspace_fifo_fairness_and_stage_rotation -- --test-threads=1
```

Recovery CLI validation passed its focused parser test. All-target app/server Clippy with warnings denied, formatting and whitespace checks passed. Fresh PostgreSQL application of all branch migrations was also executed in a transaction and rolled back successfully before the behavioral runs.

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo test -p opentrack-server --bin opentrack-server config::tests::notification_recovery_commands_require_scoped_bounded_targets -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=work/target cargo clippy -p opentrack-app -p opentrack-server --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## Remaining acceptance

Separate evidence is required for API/UI integration, recipient-only live realtime dispatch, source redaction rendering, snapshot mark-all, snooze interaction, keyboard/focus behavior, native clients and assistive technology. SMTP, email and digest delivery are outside this increment. These checks do not declare F19, F20, M07 or the complete product finished. Apply matching grants after migration; rollback uses a matched database backup and executable as described in [notification operations](../NOTIFICATIONS.md).
