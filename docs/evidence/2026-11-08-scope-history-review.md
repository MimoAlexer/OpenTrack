# Project scope history and planning review

The Scope history tab records named baselines, compares current work with an earlier agreement and shows reasons for additions, removals and postponements. A postponement preserves issue workflow state. Current source access controls membership references, links, event reasons and comparison counts. Legacy membership before migration 0031 has unknown history.

## Local controls

- The React project suites covered clearing source references, stale selection and agreement while a replacement first page was pending. They also covered dirty and pristine update context, the original retry body/key/validator, and closing expired baseline and publication attempts after history refresh.
- The PostgreSQL scope contracts exercised baseline agreement, rollback, permission changes, private moves, deleted sources, cursors, bounds and both observed orders of a concurrent issue move and re-addition. Update contracts exercised snapshots, publication races and adjacent comparisons.
- The in-process HTTP contract checked successful replay, stale preconditions and expiry for all three planning commands. The existing HTTP 409 problem code is preserved. Only matching expired planning receipts add the optional flag. Different bodies and a control project-creation endpoint retained their ordinary conflict shape.
- Historical connected Chromium, Firefox and WebKit runs of the baseline, postponement, update context and connected planning flows are recorded with their source revision in the [browser evidence](2026-11-08-planning-connected-browser.json). The [provisioning evidence](2026-11-08-scope-provisioning.md) covers migration grants.

## Scope query measurement

A disposable PostgreSQL 18.6 fixture had one agreed and current issue, 4,000 unrelated issues and 8,001 events for the reviewed project. A second project contributed 1,000 memberships, 1,000 baseline items and 1,000 events. Each query requested one item plus its lookahead. PostgreSQL analyzed the fixture tables before the restricted runtime role executed the production statement using `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)`.

| Event boundary | Earlier shared hits / reads | Revised shared hits / reads | Earlier / revised execution (ms) |
| --- | ---: | ---: | ---: |
| First page | 462 / 0 | 147 / 0 | 4.138 / 0.666 |
| Before ordinal 4,000 | 400 / 0 | 19 / 0 | 3.106 / 0.278 |

The earlier plans filtered 4,000 unrelated issues and scanned the event table. The revised plans read one membership and one baseline item from scoped indexes and two event rows from the project ordinal index. The unchanged comparison item required no latest-reason lookup. The statement uses an unnamed prepared query because generic estimates for two very uneven project and baseline sizes also led PostgreSQL to scan unrelated memberships. Revised planning times were 2.108 ms and 1.339 ms respectively, in addition to the execution times above. These single-run timings describe this fixture. Long runs of inaccessible historical sources can still require additional visibility checks to fill a page. The regression checks rows visited, allowing execution time to vary.

No migration or index change is required for these corrections. X16 remains in progress pending integrated qualification, native and manual physical-input/assistive-technology evidence. Product recovery and governance gates remain open.
