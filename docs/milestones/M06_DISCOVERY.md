# M06 Search, saved views and realtime

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP. Dependencies: M05. Feature coverage: F17–F18, F24–F25.

## Scope and deliverables

Make large accessible datasets searchable and keep multiple clients coherent.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M06-T01 Filter contract | M05 | Versioned AST compiler, supported indexed query shapes, keyset cursors and URL state. |
| M06-T02 Views/search | M06-T01 | Shared/personal views, private saved searches, exact key/weighted FTS/trigram and safe snippets. |
| M06-T03 SSE replay | M05 | Snapshot watermark, per-node tail, NOTIFY hint/poll safety, authorized checkpoint and expired-cursor reset. |
| M06-T04 Optimistic state | M06-T03 | Per-entity mutation serialization, event/response dedupe, conflict drafts and scoped cache purge. |
| M06-T05 Discovery UX | M06-T02, M06-T04 | Search page/palette results, favorites, empty/slow/error state and virtualized large lists. |
| M06-T06 Multi-client proof | M06-T03, M06-T04, M06-T05 | Exercise two API nodes, reverse commit timing, lost listener, slow subscriber and member revocation. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F17/F18/F24/F25 acceptance; relevance/ACL corpus,100k issue benchmark, two-client conflict/reconnect and queue-bound tests.

## Risks and scope guard

Search and events leak data when treated as separate authorization systems. Keep ACL in authoritative queries and payloads minimal.

## Completion condition

Known work is found within budgets and two clients converge after disconnect without skipping a committed event or exposing private content.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
