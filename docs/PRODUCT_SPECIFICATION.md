# Product specification

Status: implementation authorized for the complete baseline and approved expansion; acceptance evidence is tracked in [REQUIREMENTS_STATUS](REQUIREMENTS_STATUS.md). [Vision](PRODUCT_VISION.md), [feature index](FEATURE_MATRIX.md), [architecture](ARCHITECTURE.md), [screens](design/SCREEN_SPECIFICATIONS.md), [roadmap](ROADMAP.md).

## Scope and navigation

The complete functional specification is split into 47 feature documents under features/ rather than one unmanageable file. Each has exactly the twelve requested dimensions and named acceptance criteria. FEATURE_MATRIX.md is the traceability index. This document owns shared terminology, release scope and cross-feature semantics; it does not replace those detailed specifications.

## Shared product vocabulary

Organization groups workspace administration. Workspace is the content tenant. Team owns issues/workflow and may be workspace-visible or private. Project groups one team's issues around an outcome in MVP. Milestone partitions project work. Cycle assigns a team planning interval without changing issue lifecycle. Issue has one human owner at most; an agent run is a separate bounded delegation. View is a query/display definition, not a new copy of work or an access grant. Activity explains product changes; audit records restricted security evidence. Notification is a durable recipient record; toast is transient UI feedback.

A workspace member may contribute to workspace-visible teams; private teams additionally require explicit team membership. Viewer remains read-only. Team maintainer manages workflow/members within team scope. Workspace admin/owner does not silently bypass private content access. All public API and worker operations use these rules. The security matrix is canonical.

## Core product contract

- Users can install, authenticate locally, create workspaces/teams/issues, discuss/attach files, plan projects/cycles, filter/search, receive in-app updates, export and restore without internet, AI, email, GitHub or Redis.
- Each issue has stable UUID, team-qualified readable key, one current status, optional assignee, one optional project/milestone/cycle and one optional same-team parent. Title 1–256 characters; body bounded structured JSON. Priorities 0 unset/1 urgent/2 high/3 normal/4 low, with unset sorted last.
- Current issue workflow categories are triage/backlog/unstarted/started/completed/canceled. Custom names map to these categories. Completing a parent/project/cycle does not silently complete child issues.
- Project visibility follows one owning team. Cross-team projects, guests and per-issue ACL are deferred together, so inconsistent partial privacy models do not appear early.
- Archive hides from normal active views; delete is a 30-day recoverable tombstone; purge is explicit scheduled cleanup. Historical identity is preserved/pseudonymized where needed. Backups expire independently.
- Human edits use versions and visible conflict recovery. Realtime keeps entity state current but does not claim collaborative character-level editing or offline mutation queues.
- Every external effect has a persisted intent and a recovery path. Successful core writes do not depend on provider availability. Retry safety is stated rather than assumed.

## Golden user journeys and failure expectations

Individual: create personal workspace/team, capture issue with C, prioritize/assign to self, find via search and complete. No organization-directory or AI onboarding required.

Team: invite colleague, create project/milestone, add issues and cycle, comment/mention, process inbox, move issue status and view progress. Private-team member removal takes effect across all surfaces. If two users edit, one sees a recoverable conflict instead of losing work.

Operator: validate config, start release, bootstrap owner, back up DB/blobs/keys, upgrade with explicit migration, restore in isolation and only then enable external dispatch. An unhealthy optional component has a bounded degraded state.

Agent supervisor, after MVP: select issue/repo/base/provider/grant, review permissions/budget, run in isolation, inspect patch and exact-SHA tests, approve optional draft PR publication. Provider completion, test success, approved artifact, published PR and merged deployment are distinct outcomes.

External assistant, later: connect a scoped OpenTrack tool endpoint through supported authorization, retrieve accessible work and propose an update. It cannot inherit a chat provider token as an OpenTrack session or bypass repository grants.

## Release scope and non-goals

MVP includes F01–F15 (basic planning), F17–F25, F28–F30 and the browser API portion of F31. F16 advanced roadmap, F26 Git, F27 automation, external F31/F32, F33–F35 AI/agents and F36 desktop follow their plans. Mandatory cloud services, payment/billing systems, a general team-chat replacement, public anonymous projects, unbounded scripting, automatic merge/deploy and full offline writes are outside this baseline.

Feature launch requires its explicit acceptance tests plus the cross-cutting definition of done. Quantitative performance is a design target until measured; official provider documentation is capability evidence until a real permitted account/runtime test verifies the chosen route. Open questions have default behavior and milestone gates, so unresolved optional provider decisions do not prevent a coherent core implementation.

## Approved complete-product scope

The explicit implementation goal includes every F01–F36 acceptance criterion, F37–F47 and X01–X66. The older MVP/post-MVP labels describe delivery order, not excluded goal scope. AI is optional for an installation at runtime, but provider/agent implementation and its verification remain in the authorized product. Projects retain a single owning team; broader coordination uses authorized links rather than silently changing project ownership. The familiar Issues/Projects/Cycles workspace remains central; the discarded Tracks-centric experiment is not a product requirement.

| Added object | Product meaning and lifecycle |
| --- | --- |
| Initiative / outcome | An owning team's objective links projects; observations are sourced values; outcome review may be achieved, missed or inconclusive independently of project delivery. |
| Planning scenario | A versioned proposal captures source versions, dates and capacity assumptions. Editing it never changes live work; bounded reviewed apply is a distinct atomic command. |
| Capacity period | Team working dates, available effort and reserved support in an explicit unit. Missing estimates are unknown and story points from different teams are not additive. |
| Routine / focus | Focus and drafts are private preferences. Check-ins, agendas and agreements coordinate a team; they link existing tasks rather than creating second owners. |
| Retrospective | Team-scoped session with explicit participants, facilitator, phase, privacy and retention. Raw cards/votes and published summary have separate audiences. Actions are ordinary issues plus an independently reviewed success condition. |
| Decision / knowledge | Decisions preserve alternatives and rationale when superseded. Notes and playbooks have owners and review dates; a date is not evidence that review happened. |
| Feedback | Original reports survive triage/merge. Contact data is segregated and restricted. Evidence supports a priority decision without making it automatically. |
| Release / review | A release groups work across cycles and tracks readiness. Reviews bind an exact version/digest. Ready, approved, published and deployed are distinct facts. |
| Incident | Attributable timeline and response work; corrections preserve originals. Response closure, post-incident review and corrective action verification are separate outcomes. |
| Report / template / custom field | Reports expose definitions and authorized source rows; templates preview bounded creations and retry safely; typed custom fields never execute expressions. |

F37–F47 and the appended database/API/security sections define these objects. X01–X66 also extend existing features: personal work, triage, batch preview, issue templates, scope history, cycle planning, recurrence, navigation, automation preview, integration health and complete transfer must be implemented even where they add no new feature number. [IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md) selects dependency-ready slices; [REQUIREMENTS_STATUS](REQUIREMENTS_STATUS.md) is the completion ledger.

Required connected journeys include retro → improvement issue → next cycle → observed result; feedback → issue/project → reviewed release; incident → capacity adjustment proposal → approved plan → corrective verification; and completed project → measured outcome → follow-up decision. Optional AI can assist with source-linked drafts but cannot silently vote, publish summaries, change commitments or assert success.
