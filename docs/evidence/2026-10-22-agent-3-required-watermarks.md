# Agent 3 required-consumer watermark qualification

22 October 2026. Issue #19 remains in progress. Baseline: integrated main d01fb0c, including accepted schema 0025. No production behavior or migration changes.

`multiple_consumer_watermarks_preserve_required_work_and_storage_accounting` exercises the real PostgreSQL admission/retention functions with an additional required registration and an optional registration:

- The slowest required checkpoint pins admission and retention even when that consumer is disabled.
- A missing required checkpoint refuses the whole write and preserves retry state and retained bytes.
- Missing required backfill intent pins retention while admission continues to use the actual checkpoint; the two rules are tested separately.
- Optional missing and zero checkpoints do not contribute to required admission or retention.
- Rejected commands remain retryable with the same key. Assignment capture does not advance another consumer's checkpoint or reclaim its pending replay data.
- One-row pruning advances only the protected floor and releases exactly the stored logical charge after every required checkpoint permits it.

The second registration/checkpoint states are synthetic fixture metadata, not a second implemented consumer. Its existing job reference satisfies the backfill FK only; no notification, search or other projection effect is claimed.

Executed in the assigned WSL/isolated PostgreSQL environment under the shared verification lock:

```sh
cargo fmt --all --check
cargo clippy --locked -p opentrack-app --test event_admission_contract -- -D warnings
cargo test --locked -p opentrack-app --test event_admission_contract -- --test-threads=2
```

Formatting and strict lint passed. All 7 admission cases passed in 16.43s after recovery of an unexpectedly stopped private cluster. The preceding run failed all 7 before fixture setup with migration-owner PoolTimedOut; assigned-port readiness and pg_ctl status confirmed the server was stopped. Ordinary start recovered the existing cluster without setup/reset/delete, role changes or peer-service access. PostgreSQL recorded interrupted/improper shutdown recovery; no global cause or whole-product recovery acceptance is inferred. No assertion or timeout was weakened.

Remaining #19 scope includes actual deployed operator/retention qualification, additional real consumers and broader operations. This fixture does not measure physical disk/WAL capacity, satisfy M06/M07 dependencies or close #19. No database rollback is needed for this test-only increment.
