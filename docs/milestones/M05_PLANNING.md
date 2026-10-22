# M05 Projects, milestones, cycles and board

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP. Dependencies: M04. Feature coverage: F05, F14–F15; board foundation F17.

## Scope and deliverables

Deliver a complete planning loop on top of working issues.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M05-T01 Project lifecycle | M04 | Single owning team, lead/dates/health/narrative, explicit state transitions and update snapshots. |
| M05-T02 Milestone model | M05-T01 | Same-project issue constraint, order/state, detach behavior and progress calculation with documented denominator. |
| M05-T03 Cycle semantics | M04 | Local dates/exclusive end, no-overlap constraint, scope snapshots and explicit close/carry-forward report. |
| M05-T04 Planning screens | M05-T01, M05-T02, M05-T03 | Project overview/issues/updates, milestone groups and cycle list/detail with accessible chart table. |
| M05-T05 Board movement | M05-T01, M05-T03 | Status columns and versioned move/reorder, pointer/keyboard equivalence, stale drop rollback and bounded rendering. |
| M05-T06 Reporting parity | M05-T04, M05-T05 | Counts match ACL and exclusion rules; zero scope and projection lag are clear. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F05/F14/F15 acceptance; project/team/milestone FK, DST/overlap/concurrent close, board keyboard move and same-query list/board parity.

## Risks and scope guard

Cross-team project scope would multiply ACL complexity; defer it. Drag library adoption depends on accessibility and maintenance spike, not appearance.

## Completion condition

A user plans a project/milestone/cycle, moves work and closes interval without silent issue state changes; private counts remain private.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
