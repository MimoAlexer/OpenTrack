# Migration-27 test fixture compatibility

25 October 2026. Base `8a5c25f14df0f13cd5bd8560851598e05378e476`.

Current-schema worker, workflow, graph-capacity, cycle-history and deletion-fanout fixtures now provide finite explicit ranks. Their existing issue numbers, counts, invalid-field checks, fanout/capacity, history and race workloads are preserved. Assignment cleanup shares its seed with a genuine migration-14 driver, so that branch retains the original rank-free SQL while current fixtures provide rank.

Deletion's migration-13 setup and the migration-22 cycle upgrade use an explicit historical fixture helper instead of calling current issue creation against an obsolete schema. The helper is limited to those two drivers and preserves normalized request/key hashes, the original create acknowledgement, initial body projection, parent versions and account activity/events. Ordinary current fixtures still call the real application. The deletion replay and original preservation/history assertions pass. Full snapshot expectations add only the specified backfilled rank and initial ordering generation; they do not discard original fields.

The specified migration-11/10/15/25/26 legacy insert files and existing API contracts are unchanged. Original assertions and timeout lines remain. Touched panic wrappers retain failed schemas before normal success cleanup.

All **14 cases** pass in one focused serial batch across the six touched targets: deletion **1**, assignment cleanup **3**, cycle races **7**, worker **1**, workflow **1** and graph **1**. Formatting, compilation and strict focused Clippy pass. The selected workloads include the exact deletion upgrade, idempotent replay, 105-child fanout, 10001-edge bound, assignment backfill and fifty 10000-issue cycle snapshots. No global suite was repeated. All six test processes are reaped, PostgreSQL is stopped and the original data remain retained; successful fresh schemas use normal cleanup.

The [structured evidence](2026-10-25-rank-fixture-compatibility.json) records exact hashes and scope. The private index `$OPENTRACK_DEV_STATE/rank-fixture-compatibility/artifact-index.json` has SHA-256 `a24c543cc1d69d6f4f70ed694cd8e86daed17ffec87f23c043ba6e18238ee5d4`. Historical seeding models persisted upgrade inputs, not creation authorization on obsolete schemas. Production, migrations, defaults/checksums and generated files are unchanged; integration, connected UI and wider product qualification remain separate.
