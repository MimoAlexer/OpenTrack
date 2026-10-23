# Agent 3: redacted worker timing diagnostics

The default-profile browser receipts contain 35–44-second queue delays with short successful attempts, but no retained stage timeline or executable/configuration provenance. The isolated 34-workspace probe produced shorter 6–8-second waits, mostly overlapping idle sleeps. Neither establishes the cause of the original delay.

The foreground worker now records its effective poll interval, connection limit and lease duration in its existing startup message. Opt-in debug records identify each completed stage, monotonic elapsed milliseconds, the existing stage activity decision (`worked`) and a returned error (`failed`). These flags are not committed-effect receipts: handled blocked/dead-letter outcomes differ from a stage error, and durable consumer/job records remain authoritative. Idle-start and idle-completion records distinguish configured polling from stage time. Enable the existing logging filter with `RUST_LOG=info,opentrack_server::worker=debug` for a bounded diagnostic run.

These fields contain no workspace/job/entity identifiers, credentials, database errors, connection URLs or filesystem paths. Stage elapsed time includes its waits, queries and I/O; it does not separate database execution from pool wait. Clock reads and synchronous debug logging add measurement overhead; a stage excludes its own completion emission, while later measurements can reflect prior logging. PostgreSQL/wall-clock correlation remains distinct from monotonic durations. Shutdown can finish an in-flight stage without its completion record, or interrupt an idle wait. A missing completion record does not establish a failed effect; durable receipts and the existing shutdown contract remain authoritative.

This increment changes diagnostics only. It does not change discovery queries, scheduling, defaults, authorization, fencing, retry policy, shutdown bounds or schema. It is not a fix or performance acceptance for the original queue delay, a general fairness benchmark or M06 readiness. The scan-continuation candidate remains unapplied while its resource/liveness policy is unresolved.

Local verification on base `590725e9e6332f6fe06b20dd870eed55a1e63ab2` with these diagnostic changes passed `cargo fmt --all --check`, the real PostgreSQL-backed `worker_daemon_contract` (1/1, 3.44 seconds), and `cargo clippy --locked -p opentrack-server --all-targets -- -D warnings`. The daemon contract retains the poisoned/healthy workspace, authority, exact receipts, retention bytes, fencing, output redaction and owned-process shutdown checks. It additionally validates effective settings, strict diagnostic field allowlists, every stage, error/progress outcomes and a completed idle wait. These are local checks; hosted checks and exact-head integration review remain separate.

A separate local exploratory fixture reproduced a default-profile delay with 34 registered workspaces and a target first in discovery order. Normal commands created seven older assignment-maintenance intents through team privacy changes, then queued a workflow job before starting the daemon. Three teams retained live owner-assigned issues; setup work was drained first. The actual startup log confirmed the unchanged 2000ms interval, five connections and 60-second lease. No job, lease, epoch or checkpoint rows were patched to manufacture the backlog.

| Observation | Measured result |
| --- | ---: |
| Workflow queue time | 43,055.087ms |
| Single successful workflow attempt | 23.592ms |
| Completed idle waits | 21, totaling 42,027.484637ms |
| Completed stage calls | 150, totaling 924.527829ms |
| Longest stage call | 50.958006ms |
| Allowlisted trace events | 195, zero invalid or omitted |
| Existing 30-second completion criterion | **Failed** |

The exploratory test exited unsuccessfully on that unchanged criterion after recording eventual success, exact authorized effects/fencing/replay and clean owned-worker shutdown. Its 75-second observation bound did not replace the 30-second criterion. Queue timing includes daemon startup; stage and idle wall durations include measurement overhead and are not CPU measurements. Seven durable capture receipts and unchanged eligible assignments were retained. Assignment attempts can remain open across queued batch resumptions, so their wall spans do not measure continuous effect execution.

The reproduction used the diagnostic worker source SHA-256 `ab46ebe494adfad991f206d1afa6c0e5b55d10bf993d1b8c2fe5533b9d53d2f5`, exploratory fixture `1c14f1dc2191475535b686b2d2b91020484faac076f9f467f2cf64a372c52be7` and driver `8da917755ed5327b7b031f26540e69b6a498fd747d474d2a0da0a0089dca27b5`. The post-run local debug worker executable SHA-256 was `5748caf466bc31cfe03f0d15b0c7941335876e5a5e42d3233f73f05fc2899a56`. Exploratory fixtures and sanitized receipts are retained locally; they are not a new CI performance gate. This demonstrates current idle amplification under a legitimate backlog, while the exact historical browser-run cause, executable provenance, general fairness and p95 performance remain unproved. No migration or data rollback is needed for the diagnostic change; reverting it removes only the added records and checks.
