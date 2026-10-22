# M21 Complete-product qualification

Status: authorized; acceptance evidence pending. Dependencies: M13, M14, M20. Feature coverage: F01–F47. [Implementation plan](../IMPLEMENTATION_PLAN.md), [requirements status](../REQUIREMENTS_STATUS.md).

## Scope and deliverables

Deliver the complete behavior named below as vertical database/API/UI slices with current server authorization, migration/rollback notes and actual runtime checks. A feature shell or sample-data screen does not satisfy a task.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M21-T01 Requirement audit | M13, M14, M20 | Resolve every original/new feature criterion and X01–X66 against implemented source and executed evidence; no unsupported complete status. |
| M21-T02 Security and recovery qualification | M21-T01 | Run negative access, concurrent revocation, restore, migration, secret handling and runner adversarial suites; resolve failures. |
| M21-T03 UI, accessibility and performance | M21-T02 | Test actual browser/desktop journeys, both themes, narrow/zoomed layouts, keyboard/assistive technology and realistic dataset budgets. |
| M21-T04 Distribution and documentation | M21-T03 | Verify clean installation, upgrade and rollback/recovery, reproducible packaging, operator/contributor docs and release provenance. |
| M21-T05 Completion disposition | M21-T04 | Record remaining provider/OS/account evidence honestly. Claim full completion only if all required rows and release gates are verified; otherwise report exact blocked/remaining rows and continue independent work. |

## Validation

Use real PostgreSQL for constraints, authorization and races. Exercise duplicate requests, stale versions, deletion/restoration and revoke-during-operation paths. Check complete browser workflows, keyboard/narrow layout, both themes, loading/empty/error states and AI-disabled behavior. Reconcile emitted events, search, reports and export with source access. External effects additionally require permitted live-provider evidence; fixtures do not prove provider eligibility.

## Risks and scope guard

Shared UI and CRUD helpers must not flatten feature-specific privacy, state machines or evidence semantics. Every durable object needs a registered export/import/restore path. Do not broaden private-team access to make a cross-feature screen easier to implement.

## Completion condition

All named tasks have linked source and executed acceptance evidence in the requirements ledger, with failures resolved and no misleading feature claim. Later independent tasks may begin once their prerequisites pass; an unavailable optional provider must not block unrelated core work. The complete-product goal still requires its outstanding provider/OS checks to be reported and resolved rather than silently skipped.
