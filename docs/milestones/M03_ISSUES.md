# M03 Issue management vertical slice

Status: in progress; the [foundation evidence](../evidence/2026-10-17-foundation.md) proves only the current slice. Full task completion remains governed by [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP. Dependencies: M02. Feature coverage: F06–F10, F21–F22; core F31.

## Scope and deliverables

Create, assign, move and rediscover a persisted issue with polished list/detail/create workflows.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M03-T01 Workflow/taxonomy | M02 | Team statuses/default, priorities, labels and eligible assignee queries; retirement rules and same-team constraints. |
| M03-T02 Issue commands | M03-T01, M03-T06 | Create/read/patch/archive/delete/restore, atomic versions/activity/events, idempotency and strong ETags. |
| M03-T03 Hierarchy/relationships | M03-T02 | Parent depth/acyclic checks under team graph lock, block/related/duplicate semantics and historical key aliases; scoped key/title lookup supports pickers before full search. |
| M03-T04 List/detail/create | M03-T02, M03-T03 | Build screen states, accessible selection, properties and context-preserving deep links; simple description can precede rich editor. |
| M03-T05 Keyboard and palette | M03-T04 | Central command registry, scoped shortcuts, discoverable menu alternatives and focus restoration. |
| M03-T06 Durable event foundation | M02 | Implement commit-ordered event append and core worker receipts before T02 issue commands and later realtime consumers depend on it; include retention watermarks and durable dead-letter rules. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F06–F10/F21/F22 acceptance; double-submit, stale version, concurrent graph edges, private IDs, last-owner access change, restart persistence and keyboard create-to-edit E2E.

## Risks and scope guard

Event order, graph locking and optimistic UI are correctness-sensitive. Keep scope to a usable issue slice; board polish follows planning/view work.

## Completion condition

A user creates/assigns/edits issue, another authorized user reads it after reload, conflicts preserve drafts and denied users cannot infer content.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
