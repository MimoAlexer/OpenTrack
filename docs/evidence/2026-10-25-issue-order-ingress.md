# Additional issue-order ingress contracts

25 October 2026. Base `8a5c25f14df0f13cd5bd8560851598e05378e476`.

The new `issue_order_ingress_contract` target passes **3/3** real PostgreSQL/HTTP-router cases after review of the existing nine-case coverage. The existing contracts and worker suite were not rerun or changed.

- Actual team moves from and into a guarded status group return typed maintenance 409 with confirmed nonapplication. Exact snapshots preserve issues, full-precision ranks, team counters, aliases, documents/revisions, history, events, links, jobs, progress and receipts. A move into another unguarded status of the same team succeeds.
- For both completed and canceled issues, an ineligible historical assignee permits an explicit same-status position no-op but blocks reopening with 422. The no-op preserves domain state and writes only its checked acknowledgement. Explicitly clearing the assignee through the normal API permits a fresh reopen while preserving the document.
- A legacy description write combined with a status change or archive request rolls back completely when maintenance rejects the structural change. A description edit with the unchanged status succeeds during maintenance, advancing issue/document versions once, adding one revision/activity entry and exactly two correctly attributed events under one mutation. The guard and progress remain unchanged.

The new target uses the unchanged shared application harness, current migration 27, the existing four-role configuration and fresh schemas. Its owner fixture prepares valid queued maintenance rows and a historical assignee role change; the measured commands use the restricted serving role. All audits are included in rollback comparisons. The no-op comparison excludes only idempotency rows and checks its one successful acknowledgement separately.

Formatting, compilation and strict focused Clippy pass. The single runtime run reports **3 passed, 0 failed, 0 ignored, 0 filtered out**. Successful fresh schemas use normal harness cleanup; the original cluster/data remain retained and PostgreSQL is stopped. Production, existing tests, shared fixtures, migrations, generated contracts and UI remain unchanged.

The [structured evidence](2026-10-25-issue-order-ingress.json) records exact scope and hashes. The private index is `$OPENTRACK_DEV_STATE/issue-order-ingress/artifact-index.json`, SHA-256 `489c96c134a1c99d4db7331c8ab8ae8273cb379013a7ec49ac7cd5e13893a1ec`. These results add three focused ingress cases; they do not aggregate prior runs into a new full-suite claim or qualify connected UI, hosted integration, worker administration, native/physical/assistive/performance or the whole product. There is no migration or runtime rollback change.
