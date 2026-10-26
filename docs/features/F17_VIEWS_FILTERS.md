# F17 Views and filters

Stage: MVP. Delivery: [M06_DISCOVERY](../milestones/M06_DISCOVERY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Let people see the same work through useful lenses without duplicating issues or requiring a reporting language.

## 2. Functional requirements

Approved expansion obligations: X01, X07, X11, X49, X55. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

List/board, typed filters, group/sort, visible columns, personal/team/workspace saved views. Maximum filter depth 4 and 50 predicates. Manual rank enabled only in explicit manual sort; board status moves are distinct from ordering.

## 3. Interaction patterns

Filter chips show active conditions, clear and save actions; URL carries a versioned encoded view state without secrets. Unsaved overrides are labeled and do not overwrite shared defaults.

Structured display/filter state uses the URL fragment and the [body-based query transport](../API_DESIGN.md#pagination-filters-and-search); it is validated before execution. Invalid state requires explicit repair or clearing rather than a silent unfiltered reload. This is the M06 target, not current flat-filter acceptance.

## 4. Data requirements

saved_views.filter_ast, version, layout and sort; per-user preferences separate from shared view definition. No raw SQL or executable expressions.

## 5. API requirements

View CRUD plus issue listing with same validated filter AST. Shared view update requires If-Match; bulk actions must explicitly choose loaded selection or an authorized server query snapshot.

## 6. Authorization rules

Personal view owner edits; team maintainers edit team defaults; workspace admins edit workspace defaults. View visibility never grants object access.

## 7. Edge cases

Deleted labels/statuses, stale schema version, inaccessible team filter, no results, changing sort during pending mutation and huge select-all scope.

## 8. Failure behavior

Invalid saved predicate is visibly disabled with repair path, not broadened silently. Failed save preserves local overrides and previous shared version.

## 9. Performance considerations

Keyset page 50; counts optional; supported composite indexes only. Filter typing debounced 150 ms and obsolete queries canceled.

## 10. Feature dependencies

F06, F08/F09/F10/F15, F04, F24 search.

## 11. Acceptance criteria

F17-A1: shared view produces recipient-authorized results. F17-A2: URL reload preserves display/filter state. F17-A3: invalid predicate cannot silently expose broader results or run SQL.

## 12. Testing strategy

AST fuzz/property tests, SQL binding verification, private-label/deleted-status cases, board/list parity and URL/back-button E2E.
