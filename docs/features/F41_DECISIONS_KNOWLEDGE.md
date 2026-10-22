# F41 Decisions and maintained knowledge

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M18_DECISIONS_FEEDBACK](../milestones/M18_DECISIONS_FEEDBACK.md). Expansion coverage: X28, X29, X37, X40, X41, X42. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Keep rationale and operational guidance connected to the work they explain.

## 2. Functional requirements

Approved expansion obligations: X28, X29, X37, X40, X41, X42, X62. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Decision records contain question, options, rationale, owner, state and review trigger. Superseding preserves original text/history and links the new decision. Knowledge notes have owner/review dates and current/superseded status. Playbooks instantiate linked template steps with a preview and per-object result.

## 3. Interaction patterns

Show the current decision and its predecessor chain; highlight stale guidance when opening or linking it. Notes reuse the rich editor. Playbook preview lists generated items and audience.

## 4. Data requirements

decision_records, decision_links, knowledge_notes, playbooks, playbook_steps and playbook_runs. Documents gain concrete decision/note owner FKs; no unrestricted polymorphic content ownership. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/decisions; /decisions/{id}/supersede,links; /knowledge-notes; /playbooks; /playbooks/{id}/preview,instantiate; /playbook-runs/{id}. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Team-scoped content and independent access on links. Author/maintainer edits records. A reviewer can request revision but cannot silently replace rationale. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Supersession cycle; stale playbook references; revoked linked evidence; partially failed multi-item creation.

## 8. Failure behavior

Supersession command rejects cycles under the graph lock. Playbook runs retain per-step idempotency/results so retry does not create duplicates.

## 9. Performance considerations

Bound decision chain depth to 32 and playbook to 100 steps; index team/state/review date. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F06, F11, F12, F13, F23, F24, F27; M08, M10, M15.

## 11. Acceptance criteria

F41-A1: Superseding a decision preserves its historical rationale and cannot form a cycle.

F41-A2: Knowledge review dates are visible at point of use and never imply the content was reviewed.

F41-A3: Playbook retry after partial failure creates each intended task at most once.

F41-A4: Search/export of notes and decisions uses current team and linked-evidence authorization.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
