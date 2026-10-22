# M04 Editor, comments, attachments and templates

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP. Dependencies: M03. Feature coverage: F11–F13, F23.

## Scope and deliverables

Add durable discussion and rich issue context with safe uploads and conflict recovery.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M04-T01 Document schema | M03 | Restricted versioned JSON, ownership links, derived plain text and revision retention; explicit document ETag. |
| M04-T02 Editor interactions | M04-T01 | Toolbar/IME/paste/code/mention support, autosave state and two-document conflict compare. |
| M04-T03 Discussion | M04-T01, M04-T02 | Comments, edits/tombstones, mentions/subscriptions and paginated activity feed. |
| M04-T04 Upload lifecycle | M04-T01, M04-T02 | Quota reservation, stream/quarantine/validate/ready, authorized download, checksum and cleanup reconciliation. |
| M04-T05 Templates | M04-T01, M04-T02 | Versioned team defaults, preview, stale reference validation and preserve-edited-draft choice. |
| M04-T06 Failure UX | M04-T03, M04-T04, M04-T05 | Interrupted save/upload, missing attachment, retry same key, lost access and unsaved navigation treatment. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

Rich-text hostile corpus, same-document conflict, idempotent comment, quota race/disk-full, traversal and attachment hash restore; manual editor keyboard/IME and zoom.

## Risks and scope guard

Unsafe rich HTML or active file preview can undermine session security. No paid editor or object service dependency; optional antivirus honestly reported.

## Completion condition

Two users discuss a real issue with safe attachment and no silent draft loss; supported content round-trips native export format design.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).

Current team-template increment: [qualification and boundaries](../evidence/2026-10-18-templates.md). User mentions, personal subscriptions and durable private source records are covered by [mention and discussion evidence](../evidence/2026-10-18-mentions.md). Notification delivery, full IME/portable round trips, and remaining attachment/native qualification are still open.
