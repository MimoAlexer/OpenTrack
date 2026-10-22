# F10 Assignees and ownership

Stage: MVP. Delivery: [M03_ISSUES](../milestones/M03_ISSUES.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Make one human accountable for an issue or project while allowing agent work to be delegated separately.

## 2. Functional requirements

Approved expansion obligations: X01, X10. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

One optional human assignee per issue, one project lead, subscribers independent of assignment. Assignment requires current access. Agent delegation records a run and accountable requester, not a fake human member.

## 3. Interaction patterns

Assignee picker includes Unassigned and Assign to me. Avatar has accessible name; no assumption that an image conveys ownership. My work defaults to open assigned issues.

## 4. Data requirements

issues.assignee_user_id, projects.lead_user_id, issue_subscriptions and membership lifecycle. Activity records assignment source and previous/current actor IDs with safe display.

## 5. API requirements

Issue/project patch plus eligible-assignee lookup filtered to target access. Batch assignment preserves per-item version errors.

## 6. Authorization rules

Contributors can assign eligible members in their writable team. Viewer cannot assign. A provider connection or agent machine cannot be chosen as a human assignee.

## 7. Edge cases

User deactivates or loses private-team access during assignment; no eligible users; agent run outlives requester membership.

## 8. Failure behavior

Conflict/error keeps selection draft; deactivation clears open assignments with activity and cancels now-invalid grants. History retains anonymized identity when required.

## 9. Performance considerations

Index assignee and status for My work; eligible picker max 20 results, debounced. Cleanup is batched and immediate access denial does not await cleanup.

## 10. Feature dependencies

F02, F04, F06, F19 notifications, F34 agents.

## 11. Acceptance criteria

F10-A1: cannot assign a private-team issue to a nonmember. F10-A2: assignment sends one notification. F10-A3: removing access blocks the assignee even while cleanup is pending.

## 12. Testing strategy

Assignment/revocation concurrency test, eligibility filter tests, notification dedupe and keyboard assignment flow.
