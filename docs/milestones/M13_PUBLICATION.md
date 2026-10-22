# M13 Human review and draft pull requests

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: Agent release. Dependencies: M12, M10. Feature coverage: F34 publication; F26 links.

## Scope and deliverables

Publish only the exact reviewed and tested artifact using a trusted external writer.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M13-T01 Review binding | M12, M10 | Digest/base/head/test/policy expiry, required reviewer independence and stale approval handling. |
| M13-T02 Clean publisher | M13-T01 | Verify bundle objects/refs and independently reproduce the approved tree from patch/base without hooks; push exact approved commit SHA outside sandbox with scoped credentials only after verification. |
| M13-T03 PR creation | M13-T02 | Concrete title/body/target preview, durable operation marker and draft PR result verification. |
| M13-T04 Reconciliation | M13-T03 | Lost push/PR response, already-created branch, moved base, canceled publication and revoked installation. |
| M13-T05 End-to-end evidence | M13-T04 | Link issue/run/PR, record independent CI status separately from runner tests, no automatic merge/deploy. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

G04 full flow, artifact/head mismatch, stale/expired approvals, permission revoke, duplicate publication and force-push prevention.

## Risks and scope guard

External effects cannot be rolled back by a database transaction. Ambiguous state must reconcile; a trusted publisher must not run repo scripts with credentials.

## Completion condition

One exact approved artifact creates at most one reconciled draft PR, accessible from its issue, with clear test/approval provenance and no merge capability.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
