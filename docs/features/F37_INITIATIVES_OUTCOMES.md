# F37 Initiatives and outcome review

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M16_PLANNING_EXPANSION](../milestones/M16_PLANNING_EXPANSION.md). Expansion coverage: X14, X15, X18. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Connect projects to an objective and evaluate whether delivery achieved it.

## 2. Functional requirements

Approved expansion obligations: X14, X15, X18, X39. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

An initiative has an owning team, lead, objective, state and review date. Link accessible projects without moving issues. Record outcome baseline, target, unit, source, observation date and measured/estimated status. Close with an outcome review; permit an honest inconclusive result.

## 3. Interaction patterns

List and detail show objective, permitted projects, observations and decisions. Missing observations show unknown, never zero. A project completion offers an outcome review without automatically succeeding its objective.

## 4. Data requirements

initiatives, initiative_projects, outcome_measures, outcome_observations and outcome_reviews. Scoped links retain source project IDs; no copied private titles in initiative rows. Observation corrections supersede immutable values. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/initiatives CRUD; /initiatives/{id}/projects; /outcome-measures; /outcome-measures/{id}/observations; /initiatives/{id}/reviews. Existing versions and idempotency rules apply. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Initiative team access is necessary. Each project or evidence link requires independent current read access; its existence and count are omitted otherwise. Team contributors edit; the lead or maintainer closes a review. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Project access revoked after linking; deleted evidence source; mixed units; zero baseline; project delivery without outcome observation.

## 8. Failure behavior

Stale review retains draft. Failed observation import does not alter earlier values. An inaccessible link cannot be reconstructed through aggregate counts.

## 9. Performance considerations

Page projects and observations; compute aggregates only over authorized rows with documented units. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F05, F11, F14, F24; M08 and M15.

## 11. Acceptance criteria

F37-A1: An initiative links same-workspace projects without granting project access or exposing hidden counts.

F37-A2: A measured observation requires a source, unit and observation date; absent values remain unknown.

F37-A3: Closing delivered work cannot automatically mark its objective achieved.

F37-A4: Concurrent review edits preserve the rejected draft and return a version conflict.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
