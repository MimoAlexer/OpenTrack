# M19 Releases, reviews and incidents

Status: authorized; acceptance evidence pending. Dependencies: M09, M10, M17, M18. Feature coverage: F43, F44. [Implementation plan](../IMPLEMENTATION_PLAN.md), [requirements status](../REQUIREMENTS_STATUS.md).

## Scope and deliverables

Deliver the complete behavior named below as vertical database/API/UI slices with current server authorization, migration/rollback notes and actual runtime checks. A feature shell or sample-data screen does not satisfy a task.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M19-T01 Release scope and readiness | M09, M10, M18 | Create release scope, conditions and evidence with historical cycle preservation; F43-A2/A4. |
| M19-T02 Review and publication | M19-T01 | Bind decisions to exact target version/digest; publish authorized notes through reconciled intent; F43-A1/A3. Agent artifact path also requires M13. |
| M19-T03 Incident response and correction | M17, M18 | Implement response tasks, immutable timeline/corrections and explicit response/review states; F44-A1/A2. |
| M19-T04 Post-incident learning | M19-T03 | Create audience-safe corrective actions and explicit capacity/scenario proposals; F44-A3/A4. |
| M19-T05 Release and incident qualification | M19-T02, M19-T04 | Exercise stale review, lost publication response, linked-source revocation and complete incident-to-planning workflow; X43–X48/X62. |

## Validation

Use real PostgreSQL for constraints, authorization and races. Exercise duplicate requests, stale versions, deletion/restoration and revoke-during-operation paths. Check complete browser workflows, keyboard/narrow layout, both themes, loading/empty/error states and AI-disabled behavior. Reconcile emitted events, search, reports and export with source access. External effects additionally require permitted live-provider evidence; fixtures do not prove provider eligibility.

## Risks and scope guard

Shared UI and CRUD helpers must not flatten feature-specific privacy, state machines or evidence semantics. Every durable object needs a registered export/import/restore path. Do not broaden private-team access to make a cross-feature screen easier to implement.

## Completion condition

All named tasks have linked source and executed acceptance evidence in the requirements ledger, with failures resolved and no misleading feature claim. Later independent tasks may begin once their prerequisites pass; an unavailable optional provider must not block unrelated core work. The complete-product goal still requires its outstanding provider/OS checks to be reported and resolved rather than silently skipped.
