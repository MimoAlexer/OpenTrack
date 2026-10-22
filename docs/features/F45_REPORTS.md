# F45 Flow and planning reports

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M20_REPORTS_WORKFLOWS](../milestones/M20_REPORTS_WORKFLOWS.md). Expansion coverage: X50, X51, X52, X61, X64, X65. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Explain team progress with definitions and inspectable source data.

## 2. Functional requirements

Approved expansion obligations: X50, X51, X52, X61, X64, X65. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Version report definitions for lead time, waiting time, active work, scope added/removed and improvement action outcomes. Record date window, timezone, as-of time, unit and excluded/unknown data. Reports link authorized source rows; do not rank individuals or infer absent values. Retro aggregates exclude private raw content by default.

## 3. Interaction patterns

Charts have matching tables and definition popovers. Users can inspect contributing rows and distinguish stale projection results. Search spans enabled permitted object types without hidden counts.

## 4. Data requirements

report_definitions, report_runs and report_snapshots; projections rebuild from retained product history. Snapshot access is re-evaluated at read/export time; no broad shared cache keyed only by report ID. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/reports/definitions; /reports/runs; /reports/runs/{id}/rows,export; extended /search types. Asynchronous runs persist requester scope and reauthorize on execution/download. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Current content access on every source row. Source revocation invalidates snapshots/caches for affected principal. Private retros and contact data are excluded unless explicit permitted report definitions support them. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Incomplete history, reopened issues, deleted rows, timezone changes, skewed teams, late corrections and revoked permissions during export.

## 8. Failure behavior

Failed projection returns last successful timestamp and a stale marker or a usable source table; never fabricate a zero series.

## 9. Performance considerations

Bound date windows to 366 days initially; queue costly reports with workspace quotas; paginate source rows. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F11, F15, F17, F24, F25, F28, F37–F44; M16–M19.

## 11. Acceptance criteria

F45-A1: Every displayed metric exposes definition, time window, units and contributing authorized rows.

F45-A2: Unknown or missing history is not converted into zero or false precision.

F45-A3: Revoked source access prevents stale report snapshots from leaking content or hidden counts.

F45-A4: Private retro cards and contact data do not enter default reports, search summaries or exports.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
