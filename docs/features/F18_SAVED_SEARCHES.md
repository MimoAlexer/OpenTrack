# F18 Saved searches

Stage: MVP. Delivery: [M06_DISCOVERY](../milestones/M06_DISCOVERY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Allow a person to revisit a useful text query without rebuilding its filters.

## 2. Functional requirements

Approved expansion obligations: X49. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Private named searches combine query text and filter AST; favorite/unfavorite, rename/delete, explicit run. Shared searchable views use F17 rather than duplicating ACL. Scheduled search notifications are later automation work.

## 3. Interaction patterns

Search results offer Save search; sidebar Favorites stores shortcut. Editing an active query marks it unsaved until explicit save.

## 4. Data requirements

saved_searches with owner, query<=256 characters, filter schema version and stable UUID. Favorites reference the saved object, not copied query text.

## 5. API requirements

Owner-scoped CRUD, execute through ordinary search. No endpoint returns another user search names even when results would be public.

## 6. Authorization rules

Owner-only. On member removal, remove access immediately and delete according to personal-data policy.

## 7. Edge cases

Deleted filter target, renamed team, empty query, same search name, changes in result permissions and query schema upgrade.

## 8. Failure behavior

Failed execution preserves saved definition and shows recoverable error. Invalid migrated filters require user repair rather than broadening.

## 9. Performance considerations

At most 100 saved searches/user/workspace initially, paginated management. Execution shares search timeout and result limit.

## 10. Feature dependencies

F17 typed filters, F24 search, F04 user scope, F21 navigation.

## 11. Acceptance criteria

F18-A1: saved query round-trips filters and returns current authorized results. F18-A2: another user cannot list or execute its private definition. F18-A3: renaming preserves favorite link.

## 12. Testing strategy

Owner ACL, schema version migration, Unicode query persistence, invalid predicate, and save/reopen/delete browser flow.
