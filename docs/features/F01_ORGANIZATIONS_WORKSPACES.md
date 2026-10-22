# F01 Organizations and workspaces

Stage: MVP. Delivery: [M02_IDENTITY](../milestones/M02_IDENTITY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Give individuals a low-friction private home and teams a clear content boundary independent of deployment ownership.

## 2. Functional requirements

Approved expansion obligations: X60. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Bootstrap one organization and workspace; create additional workspaces, rename and archive them. Organization owners administer the container without implicit access to private workspace content. Workspace deletion is a 30-day recoverable tombstone followed by audited purge.

## 3. Interaction patterns

First-run flow asks workspace name and team name, then opens an empty issue list. Switcher shows only authorized workspaces, recent first; switching clears old query/stream state before displaying the new workspace.

## 4. Data requirements

organizations, organization_members, workspaces, workspace_members, workspace_auth_guards. Slugs are unique in their documented scope; UUID links remain stable across rename.

## 5. API requirements

Organization/workspace CRUD and switch/bootstrap endpoints; deletion returns a job with impact summary. The API never accepts a client assertion of current workspace membership.

## 6. Authorization rules

Organization owner can create/manage workspace containers. Workspace owner manages content membership and deletion. Active membership is required to open a workspace; private teams retain their own access requirement.

## 7. Edge cases

Prevent deleting the last recovery owner; handle duplicate slugs, disabled users, archived workspaces and same display names. Cross-workspace object transfer is export/import, not an ID rewrite.

## 8. Failure behavior

An unavailable workspace leaves the switcher and retry path usable. A failed purge stays tombstoned with a retryable operator job; no partial restoration claim.

## 9. Performance considerations

Switcher list limit 100 with search after that; bootstrap <=100 KiB excluding initial issue page. Workspace creation is one short transaction with defaults.

## 10. Feature dependencies

F02 users/teams, F03 login, F04 authorization, F30 audit. Foundation provides scoped transaction helpers.

## 11. Acceptance criteria

F01-A1: create a workspace and use its defaults without external services. F01-A2: a user in A cannot infer B through IDs/search. F01-A3: rename preserves links and switch clears cached A content.

## 12. Testing strategy

Real-DB tenant FK/RLS tests; two-account browser switch test; last-owner and concurrent creation cases; crash/retry purge job and restore-window tests.
