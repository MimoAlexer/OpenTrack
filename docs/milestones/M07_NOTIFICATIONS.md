# M07 Notification delivery and inbox

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP. Dependencies: M06. Feature coverage: F19–F20.

## Scope and deliverables

Provide actionable personal updates with durable in-app delivery and optional email.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M07-T01 Fan-out consumer | M06 | Dedupe recipient/event/reason, preference/subscription logic, reauthorize before delayed dispatch. |
| M07-T02 Delivery pipeline | M07-T01 | Independent channel state, bounded retries, stable email Message-ID and optional SMTP settings. |
| M07-T03 Inbox operations | M07-T01 | Read/archive/snooze and snapshot-bounded mark-all with current source access. |
| M07-T04 Inbox UI | M07-T03 | Two-pane preview, badge, next-item focus, timezone snooze and unavailable-source state. |
| M07-T05 Recovery/fairness | M07-T02, M07-T04 | Crash after fan-out/SMTP acceptance, duplicate events, worker backlog and per-workspace budget. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F19/F20 acceptance; no-SMTP core scenario, revoked recipient, concurrent arrival/mark-all, fake clock snooze and keyboard processing.

## Risks and scope guard

Email cannot guarantee exactly-once acceptance; represent uncertainty and keep durable in-app truth. Avoid notification storms.

## Completion condition

Assignment/mention yields one durable inbox reason, processing is accessible, and worker/mail failures do not block issue writes.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
