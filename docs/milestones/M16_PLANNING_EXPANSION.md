# M16 Initiatives, roadmap scenarios and capacity

Status: authorized; acceptance evidence pending. Dependencies: M15. Feature coverage: F16, F37, F38. [Implementation plan](../IMPLEMENTATION_PLAN.md), [requirements status](../REQUIREMENTS_STATUS.md).

## Scope and deliverables

Deliver the complete behavior named below as vertical database/API/UI slices with current server authorization, migration/rollback notes and actual runtime checks. A feature shell or sample-data screen does not satisfy a task.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M16-T01 Initiatives and outcomes | M15 | Implement objective/project links, observations and outcome review with independent source ACL; F37-A1–A4. |
| M16-T02 Capacity and dependency model | M16-T01 | Implement working calendars, explicit estimate units and atomic cycle-safe finish-to-start dependencies; F38-A2/A3. |
| M16-T03 Scenario comparison and apply | M16-T02 | Persist immutable snapshot revisions, compare authorized source changes and atomically apply selected bounded edits; F38-A1/A4. |
| M16-T04 Roadmap interaction and privacy | M16-T03 | Deliver timeline and equivalent editable table, undated queue, source revocation and stale-plan recovery; F16 acceptance and X19–X24/X63. |

## Validation

Use real PostgreSQL for constraints, authorization and races. Exercise duplicate requests, stale versions, deletion/restoration and revoke-during-operation paths. Check complete browser workflows, keyboard/narrow layout, both themes, loading/empty/error states and AI-disabled behavior. Reconcile emitted events, search, reports and export with source access. External effects additionally require permitted live-provider evidence; fixtures do not prove provider eligibility.

## Risks and scope guard

Shared UI and CRUD helpers must not flatten feature-specific privacy, state machines or evidence semantics. Every durable object needs a registered export/import/restore path. Do not broaden private-team access to make a cross-feature screen easier to implement.

## Completion condition

All named tasks have linked source and executed acceptance evidence in the requirements ledger, with failures resolved and no misleading feature claim. Later independent tasks may begin once their prerequisites pass; an unavailable optional provider must not block unrelated core work. The complete-product goal still requires its outstanding provider/OS checks to be reported and resolved rather than silently skipped.
