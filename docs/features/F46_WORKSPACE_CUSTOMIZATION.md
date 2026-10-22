# F46 Workspace modules and workflow templates

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M15_WORKSPACE_EXPANSION](../milestones/M15_WORKSPACE_EXPANSION.md). Expansion coverage: X09, X11, X12, X41, X55, X57, X58, X59, X66. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Make product breadth configurable while keeping work portable and consistent.

## 2. Functional requirements

Approved expansion obligations: X09, X11, X12, X41, X55, X57, X58, X59, X66. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Allow personal pin/order preferences and team module visibility. Disabling a module hides navigation but does not delete data or bypass authorization. Cross-feature templates preview every object, audience and link before creating a bounded run. Recurrence uses unique occurrence keys. Automation simulations are read-only. Integration status exposes last success/error/retry without secrets.

## 3. Interaction patterns

Module configuration names data effects accurately. Personal navigation can be reset. Template run preview and result expose partial failures. Integration health offers only safe eligible retries.

## 4. Data requirements

module_preferences, navigation_preferences, workflow_templates, workflow_template_steps, template_runs and template_run_items. Recurrence schedules attach to existing rules with timezone and unique occurrence keys. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/module-preferences; /me/navigation; /workflow-templates; /workflow-templates/{id}/preview,instantiate; /template-runs; /integration-health. Native transfer schema includes all enabled and disabled module data permitted by export scope. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Personal navigation is user-only; team module settings/templates require maintainer. Template instantiation revalidates every destination; hidden modules are not a security boundary. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Disabled module with links, stale template references, DST recurrence, repeated retries, partial export and secret-bearing integration errors.

## 8. Failure behavior

Persist per-step results and idempotency; resume only missing steps. A failed import/instantiate never reports the entire batch as successful.

## 9. Performance considerations

Limit templates/batches to 100 steps/items and recurrence lookahead to 90 days; bounded dispatch jobs. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F17, F21, F23, F27, F28, F29, F32; M08 and M10.

## 11. Acceptance criteria

F46-A1: Changing module visibility does not delete data, widen permissions or break canonical links.

F46-A2: Preview lists every planned creation and audience; retry after partial failure does not duplicate completed steps.

F46-A3: Recurring work creates one item per schedule occurrence across retries and DST boundaries.

F46-A4: Full authorized export/import preserves expansion objects and links, including hidden-module data, without exporting secrets.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
