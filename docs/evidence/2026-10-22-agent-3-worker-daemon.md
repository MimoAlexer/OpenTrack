# Agent 3 worker daemon error isolation

22 October 2026. Issue #19 remains in progress. Baseline: integrated main 8e3186e, including schema 0025, selector qualification and the merged frontend qualification. No production code, dependency or migration changes.

`daemon_continues_healthy_workspace_work_after_capture_error` starts the real compiled worker daemon with its restricted PostgreSQL credential and a clean environment. Two fresh workspaces first complete backfill and prune their setup events, isolating the subsequent scenario.

- The lower workspace receives an unsupported event. A fixture-only trigger suppresses its mandatory dead-letter insert, forcing a real capture-stage error rather than a successful blocked-consumer result.
- The daemon emits the exact generic stage warning and remains alive. Its output is checked for credential, private directory and workspace disclosure.
- The healthy workspace captures a supported access-change event with its exact durable receipt and reconciliation job. That job succeeds while the fault remains installed.
- Retention removes exactly the healthy workspace's one aged event, records the exact floor/count and releases its stored logical charge. The counter equals the durable event sum and accounting formula.
- The selected poisoned-workspace snapshot, including its checkpoint, event, charge, jobs, receipts and operations, remains unchanged.
- The owned child exits successfully after SIGTERM within the existing graceful-shutdown allowance. Panic cleanup kills and reaps only that child; its private fixture directory is removed.

The test uses an isolated disposable schema, private directory and log. The injected trigger is confined to that schema and is removed by normal fixture cleanup. No API or operator credential is given to the daemon.

Executed in the assigned WSL/PostgreSQL environment under the shared verification lock:

```sh
cargo test --locked -p opentrack-server --test worker_daemon_contract -- --test-threads=1
cargo clippy --locked -p opentrack-server --test worker_daemon_contract -- -D warnings
cargo fmt --all --check
```

The final named target passed 1/1 in 3.05s; strict targeted Clippy and formatting passed. Earlier ignored preparation passed 1/1 in 2.22s, with its exact temporary target removed and owned process-group cleanup completed before promotion.

This proves existing daemon error isolation and bounded healthy-workspace progress in this scenario. It does not qualify general throughput/fairness, notification delivery, SMTP uncertainty, later consumers, physical storage capacity or the whole product. Deployed admission/operator qualification remains separate. Issue #19 stays open; no database rollback is needed for this test-only increment.
