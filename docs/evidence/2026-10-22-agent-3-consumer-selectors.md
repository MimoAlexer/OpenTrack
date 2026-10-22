# Agent 3 cross-workspace event selector qualification

22 October 2026. Issue #19 remains in progress. Baseline: integrated main 0781476, including accepted schema 0025 and the required-consumer watermark matrix. No production behavior or migration changes.

`selectors_isolate_poisoned_workspaces_and_reclaim_exact_bytes` exercises the real restricted-worker `capture_next` and `retain_next` paths across two registered workspaces:

- An unsupported event reaches the existing fifth-attempt blocked dead-letter state without advancing its required checkpoint.
- The next workspace still captures its two events atomically with exact ignored receipts; repeated cursor traversal neither duplicates effects nor changes the poisoned workspace.
- Exhausting each selector resets its cursor, allowing the next traversal to begin again in UUID order.
- Independent one-row retention traversals preserve the poisoned event and reclaim only the healthy workspace's captured, aged prefix.
- Every deletion advances the exact scoped floor and releases the stored logical charge. The stream counter equals the durable row sum and the accounting formula throughout.

Setup events are captured, drained, aged and safely pruned before the isolated scenario. The poison uses the existing assignment consumer's unsupported-schema path; no additional consumer or delivery effect is implemented. Capture and retention cursors are separate in this test, as in the server.

Executed in the assigned WSL/PostgreSQL environment under the shared verification lock:

```sh
cargo test --locked -p opentrack-app --test event_consumer_contract -- --test-threads=1
cargo clippy --locked -p opentrack-app --test event_consumer_contract -- -D warnings
cargo fmt --all --check
```

All 5 consumer cases passed in 15.63s, including the new selector case. Strict targeted Clippy and formatting passed. An earlier isolated preparation run also passed; its temporary source append was restored before synchronizing to integrated main.

This proves the existing selector paths against real PostgreSQL, not an entire running worker daemon's fairness or every future consumer. Actual deployed operator/admission/retention qualification, broader operations and later consumers remain open. No physical disk/WAL measurement, M06/M07 readiness, product completion or issue closure is claimed. This test-only increment needs no database rollback.
