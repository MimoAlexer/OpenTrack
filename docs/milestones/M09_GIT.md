# M09 GitHub repository and pull request links

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: Integration release. Dependencies: M08. Feature coverage: F26; inbound F32.

## Scope and deliverables

Connect existing code review to issues using least-privilege GitHub App access.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M09-T01 App installation | M08 | Register/document operator-created GitHub App, encrypted installation credentials and selected repository grants. |
| M09-T02 Webhook ingestion | M09-T01 | Raw-body HMAC, durable inbox, delivery/hash dedupe, ACK before async processing. |
| M09-T03 PR projection | M09-T02 | Link issue keys, persist revision/check/state, reconcile out-of-order hooks and provider outages. |
| M09-T04 Integration UI | M09-T03 | Connection/repo permission/sync status, issue PR cards and branch naming helper. |
| M09-T05 Revocation/recovery | M09-T04 | Installation removed, repo renamed, private fork, rate limits and missed-hook reconciliation. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F26 and inbound F32 acceptance; fixture suite plus an explicitly authorized real sandbox repository experiment during implementation.

## Risks and scope guard

Provider app registration/external reachability is an operator prerequisite. Do not request Git write/workflow permissions just for read linking.

## Completion condition

PR lifecycle appears accurately on authorized issue and forged/duplicate/out-of-order events cannot corrupt status.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
