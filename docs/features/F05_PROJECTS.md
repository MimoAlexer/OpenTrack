# F05 Projects and project updates

Stage: MVP. Delivery: [M05_PLANNING](../milestones/M05_PLANNING.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Group related issues around a deliverable and communicate progress with context beyond individual statuses.

## 2. Functional requirements

Approved expansion obligations: X13, X16, X17. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Project name, summary, rich description, lead, planned/active/paused/completed/canceled state, dates and health. MVP has one owning team and milestones. Project updates are authored snapshots; progress is completed non-deleted issues divided by eligible scope, with canceled excluded and denominator shown.

## 3. Interaction patterns

Overview combines narrative, lead/dates/health, milestone progress and recent update; Issues tab preserves list/board filters. Completing a project with open issues requires explicit acknowledgement and does not close them.

## 4. Data requirements

projects, project_updates, milestones and issues. Project/issue team equality is a DB constraint; health is manual and distinct from numerical completion.

## 5. API requirements

Project CRUD, project updates, project issue list and progress projection. State change is version-checked; aggregate read has as_of time.

## 6. Authorization rules

Read follows owning team. Workspace contributor can edit; team maintainer manages destructive/archive actions. Private project metadata is not visible in global roadmap without access.

## 7. Edge cases

Zero-issue progress is unscoped rather than 100%; canceled issues, changed dates, archived team, deleted lead and overdue milestone. Multi-team projects require a new ACL design in M14.

## 8. Failure behavior

If progress projection is delayed, show last-updated time and compute authoritative counts on demand within query budget. Failed update preserves the typed draft.

## 9. Performance considerations

Aggregate counts via indexed filtered query initially; cache only with event invalidation and no cross-ACL totals. Update feed paginates 25.

## 10. Feature dependencies

F04 access, F06 issues, F12 editor, F14 milestones; F25 refresh.

## 11. Acceptance criteria

F05-A1: one issue cannot join a project in another team. F05-A2: completing project leaves issue states intact. F05-A3: progress numerator/denominator matches the documented filter and visible scope.

## 12. Testing strategy

Constraint and aggregate tests; project state browser flow; date ordering and zero-denominator cases; stale version and delayed projection fixtures.
