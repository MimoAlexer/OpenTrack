# M18 Decisions, feedback and maintained knowledge

Status: authorized; acceptance evidence pending. Dependencies: M15, M16. Feature coverage: F41, F42. [Implementation plan](../IMPLEMENTATION_PLAN.md), [requirements status](../REQUIREMENTS_STATUS.md).

## Scope and deliverables

Deliver the complete behavior named below as vertical database/API/UI slices with current server authorization, migration/rollback notes and actual runtime checks. A feature shell or sample-data screen does not satisfy a task.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M18-T01 Decisions and knowledge lifecycle | M15 | Implement versioned rationale, cycle-safe supersession, owner/review dates and source-safe indexing; F41-A1/A2/A4. |
| M18-T02 Playbook execution | M18-T01 | Compose bounded template steps and resumable per-step creation with exact preview; F41-A3. |
| M18-T03 Feedback and contact privacy | M16, M18-T01 | Preserve original sources, segregate contact fields, support attributable triage/merge and prioritization; F42-A1–A4. |
| M18-T04 Delivery linkage and recovery | M18-T02, M18-T03 | Connect feedback to issue/project, redact revoked evidence, exercise export/restore and malformed import; X37–X42. |

## Validation

Use real PostgreSQL for constraints, authorization and races. Exercise duplicate requests, stale versions, deletion/restoration and revoke-during-operation paths. Check complete browser workflows, keyboard/narrow layout, both themes, loading/empty/error states and AI-disabled behavior. Reconcile emitted events, search, reports and export with source access. External effects additionally require permitted live-provider evidence; fixtures do not prove provider eligibility.

## Risks and scope guard

Shared UI and CRUD helpers must not flatten feature-specific privacy, state machines or evidence semantics. Every durable object needs a registered export/import/restore path. Do not broaden private-team access to make a cross-feature screen easier to implement.

## Completion condition

All named tasks have linked source and executed acceptance evidence in the requirements ledger, with failures resolved and no misleading feature claim. Later independent tasks may begin once their prerequisites pass; an unavailable optional provider must not block unrelated core work. The complete-product goal still requires its outstanding provider/OS checks to be reported and resolved rather than silently skipped.
