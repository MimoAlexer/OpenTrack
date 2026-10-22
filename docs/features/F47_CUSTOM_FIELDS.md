# F47 Typed custom fields

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M15_WORKSPACE_EXPANSION](../milestones/M15_WORKSPACE_EXPANSION.md). Expansion coverage: X56. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Support team-specific structured information without arbitrary schemas or executable filters.

## 2. Functional requirements

Approved expansion obligations: X56. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Team-maintained field definitions support text, integer, decimal, boolean, date and single/multi-select. Definitions specify entity kind, name, required flag, bounds and active/archive state. Field values are validated server-side. Type changes with existing values require previewed migration, never silent coercion.

## 3. Interaction patterns

Field editor shows type, scope, constraints and migration impact. Forms display validation inline; list filters, sorting and export preserve typed semantics. Archived fields remain readable in history.

## 4. Data requirements

custom_field_definitions, custom_field_options and typed custom_field_values keyed by concrete issue/project references. Exactly one typed value representation matches definition; multi-select options use scoped join rows. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/custom-field-definitions; /custom-field-definitions/{id}/migration-preview,archive; /issues/{id}/field-values and /projects/{id}/field-values. Filter AST accepts only known field IDs and type-compatible operators. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Maintainers edit definitions within team; normal entity-edit authority updates values. Field configuration grants no access to its entities. No field accepts secrets or arbitrary script/expression evaluation. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Required field introduced on existing work, referenced select option removed, decimal precision, field renamed, stale schema version and invalid CSV values.

## 8. Failure behavior

Return per-field validation without partial entity mutation. Migration failures retain old definition/values with resumable state.

## 9. Performance considerations

Bound definitions to 100 per team/entity kind; selective typed indexes added only for supported predicates and measured query plans. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F06, F05, F17, F24, F28; M08.

## 11. Acceptance criteria

F47-A1: Server rejects values and filter operators incompatible with the field type.

F47-A2: Definition changes cannot silently erase or coerce existing values.

F47-A3: Field filters/counts obey the same entity authorization as ordinary queries.

F47-A4: Export/import round-trips field types, options and values with explicit ID mapping.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
