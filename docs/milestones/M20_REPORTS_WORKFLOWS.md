# M20 Reports and connected product workflows

Status: authorized; acceptance evidence pending. Dependencies: M16, M17, M18, M19. Feature coverage: F45. [Implementation plan](../IMPLEMENTATION_PLAN.md), [requirements status](../REQUIREMENTS_STATUS.md).

## Scope and deliverables

Deliver the complete behavior named below as vertical database/API/UI slices with current server authorization, migration/rollback notes and actual runtime checks. A feature shell or sample-data screen does not satisfy a task.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M20-T01 Defined metric projections | M16, M17, M18, M19 | Implement versioned flow/planning/improvement metrics with units, windows and unknown-data semantics; F45-A1/A2. |
| M20-T02 Reports, search and recipient authorization | M20-T01 | Deliver source tables, report/download reauthorization and cross-type search with no hidden counts; F45-A3/A4. |
| M20-T03 Connected workflow verification | M20-T02 | Execute feedback-to-delivery, retro-to-improvement, incident-to-replanning and project-outcome review with real persisted links; X50–X52/X61–X65. |
| M20-T04 Full module transfer and recovery | M20-T03 | Exercise native export/import and isolated restore of all modules, private content and attachments; X59/X66 and no duplicate template run effects. |

## Validation

Use real PostgreSQL for constraints, authorization and races. Exercise duplicate requests, stale versions, deletion/restoration and revoke-during-operation paths. Check complete browser workflows, keyboard/narrow layout, both themes, loading/empty/error states and AI-disabled behavior. Reconcile emitted events, search, reports and export with source access. External effects additionally require permitted live-provider evidence; fixtures do not prove provider eligibility.

## Risks and scope guard

Shared UI and CRUD helpers must not flatten feature-specific privacy, state machines or evidence semantics. Every durable object needs a registered export/import/restore path. Do not broaden private-team access to make a cross-feature screen easier to implement.

## Completion condition

All named tasks have linked source and executed acceptance evidence in the requirements ledger, with failures resolved and no misleading feature claim. Later independent tasks may begin once their prerequisites pass; an unavailable optional provider must not block unrelated core work. The complete-product goal still requires its outstanding provider/OS checks to be reported and resolved rather than silently skipped.
