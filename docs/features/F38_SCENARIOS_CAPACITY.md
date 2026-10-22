# F38 Roadmap scenarios and capacity

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M16_PLANNING_EXPANSION](../milestones/M16_PLANNING_EXPANSION.md). Expansion coverage: X20, X21, X22, X23, X24, X63. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Compare feasible plans before changing live commitments.

## 2. Functional requirements

Approved expansion obligations: X19, X20, X21, X22, X23, X24, X63. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Snapshot at most 100 projects per scenario with base versions, date ranges and team capacity assumptions. Dependencies are finish-to-start with nonnegative lag in team working days. Store explicit working calendars and estimate units. Support manual earliest/likely/latest ranges; never infer statistical confidence. Apply at most 100 selected edits atomically after complete current-access/version validation.

## 3. Interaction patterns

Timeline and table expose identical data and date editing. Scenario comparison lists changed scope/dates/capacity, unknown estimates and overload. Apply displays an exact selected diff and rationale; rejected comparisons remain readable.

## 4. Data requirements

planning_scenarios, scenario_items, project_dependencies, team_capacity_periods, capacity_allocations and scenario_applications. Scenario snapshots are immutable revisions; rows include source IDs/base versions and typed overrides. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/roadmap; /project-dependencies; /capacity-periods; /planning-scenarios; /planning-scenarios/{id}/revisions,compare,apply. Apply requires expected scenario revision, source versions, selected edit IDs and rationale. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Scenario team membership does not grant linked project access. Only users able to edit every selected target may apply. Capacity edits require team maintainer; private leave details are never stored in aggregate capacity. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Dependency cycles, missing duration/estimates, zero capacity, mixed team point scales, source deletion, date boundaries and simultaneous apply.

## 8. Failure behavior

Any stale or unauthorized selected target rejects the entire bounded apply with authorized field errors. No live changes occur before success. An apply retry returns one recorded operation.

## 9. Performance considerations

Bound graph to 100 selected projects plus accessible adjacent edges. Use server graph validation and keyset lists. Do not hold a transaction while rendering comparison. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F05, F07, F14, F16, F17, F37; M08 and M15.

## 11. Acceptance criteria

F38-A1: Scenario edits never mutate live projects until apply succeeds.

F38-A2: Concurrent opposite dependency edges cannot create a cycle.

F38-A3: Mixed estimate units are not summed and unknown demand is not reported as zero.

F38-A4: Applying a stale or unauthorized selection produces no partial live edits; a retry cannot apply twice.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
