# M17 Retrospectives, personal planning and team routines

Status: authorized; acceptance evidence pending. Dependencies: M15. Feature coverage: F39, F40. [Implementation plan](../IMPLEMENTATION_PLAN.md), [requirements status](../REQUIREMENTS_STATUS.md).

## Scope and deliverables

Deliver the complete behavior named below as vertical database/API/UI slices with current server authorization, migration/rollback notes and actual runtime checks. A feature shell or sample-data screen does not satisfy a task.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M17-T01 Personal work and reminders | M15 | Persist focus/order/drafts and due reminders with recipient-only ACL and duplicate-safe dispatch; F39-A1–A3. |
| M17-T02 Team routines and calendar | M17-T01 | Implement check-ins, agendas, agreements and date-consistent cycle/calendar projections; F39-A4 and X25–X30. |
| M17-T03 Private retro collection | M17-T02 | Implement session/participant policy, private drafts, named/author-hidden cards, reveal and grouping; F40-A2. |
| M17-T04 Voting and discussion | M17-T03 | Enforce per-user budget, frozen rounds, facilitator transitions and keyboard discussion queue; F40-A1. |
| M17-T05 Actions and follow-through | M17-T04 | Preview broader audiences, create idempotent owned actions, publish separate summary and review prior action outcomes; F40-A3/A4 and X31–X36. |

## Validation

Use real PostgreSQL for constraints, authorization and races. Exercise duplicate requests, stale versions, deletion/restoration and revoke-during-operation paths. Check complete browser workflows, keyboard/narrow layout, both themes, loading/empty/error states and AI-disabled behavior. Reconcile emitted events, search, reports and export with source access. External effects additionally require permitted live-provider evidence; fixtures do not prove provider eligibility.

## Risks and scope guard

Shared UI and CRUD helpers must not flatten feature-specific privacy, state machines or evidence semantics. Every durable object needs a registered export/import/restore path. Do not broaden private-team access to make a cross-feature screen easier to implement.

## Completion condition

All named tasks have linked source and executed acceptance evidence in the requirements ledger, with failures resolved and no misleading feature claim. Later independent tasks may begin once their prerequisites pass; an unavailable optional provider must not block unrelated core work. The complete-product goal still requires its outstanding provider/OS checks to be reported and resolved rather than silently skipped.
