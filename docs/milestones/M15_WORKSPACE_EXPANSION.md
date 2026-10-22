# M15 Workspace customization and shared expansion primitives

Status: authorized; acceptance evidence pending. Dependencies: M08, M10. Feature coverage: F46, F47. [Implementation plan](../IMPLEMENTATION_PLAN.md), [requirements status](../REQUIREMENTS_STATUS.md).

## Scope and deliverables

Deliver the complete behavior named below as vertical database/API/UI slices with current server authorization, migration/rollback notes and actual runtime checks. A feature shell or sample-data screen does not satisfy a task.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M15-T01 Typed custom fields | M08 | Implement field definitions/options/values, validation, typed filters, migration preview and export mapping; F47-A1–A4. |
| M15-T02 Personal navigation and modules | M08 | Persist personal order/pins and team module settings; hide navigation without deleting data or weakening ACL; F46-A1. |
| M15-T03 Template and recurrence execution | M10 | Preview audience and all creations, persist per-step results, enforce unique occurrence keys and simulate rules without effects; F46-A2/A3. |
| M15-T04 Expansion history and transfer foundation | M15-T01, M15-T02 | Add owner-specific document support, event dispatch policy registry and versioned export sections; new modules must register before release. |
| M15-T05 Integration health and export proof | M15-T03, M15-T04 | Expose redacted connection/retry status and round-trip typed fields/template runs/preferences; F46-A4 and X57–X59. |

## Validation

Use real PostgreSQL for constraints, authorization and races. Exercise duplicate requests, stale versions, deletion/restoration and revoke-during-operation paths. Check complete browser workflows, keyboard/narrow layout, both themes, loading/empty/error states and AI-disabled behavior. Reconcile emitted events, search, reports and export with source access. External effects additionally require permitted live-provider evidence; fixtures do not prove provider eligibility.

## Risks and scope guard

Shared UI and CRUD helpers must not flatten feature-specific privacy, state machines or evidence semantics. Every durable object needs a registered export/import/restore path. Do not broaden private-team access to make a cross-feature screen easier to implement.

## Completion condition

All named tasks have linked source and executed acceptance evidence in the requirements ledger, with failures resolved and no misleading feature claim. Later independent tasks may begin once their prerequisites pass; an unavailable optional provider must not block unrelated core work. The complete-product goal still requires its outstanding provider/OS checks to be reported and resolved rather than silently skipped.
