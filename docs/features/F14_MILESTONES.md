# F14 Project milestones

Stage: MVP. Delivery: [M05_PLANNING](../milestones/M05_PLANNING.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Give a project meaningful intermediate outcomes and make remaining scope visible.

## 2. Functional requirements

Approved expansion obligations: X13. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Create/reorder/rename milestones with target dates; assign an issue only to a milestone of its own project. Progress counts completed eligible issues; manual milestone completion is separate from inferred issue percentage.

## 3. Interaction patterns

Project overview shows chronological milestone sections and scope count. Issue property picker filters to current project. Removing a project prompts clearing its milestone.

## 4. Data requirements

Milestones plus the project_issues project/milestone composite FK; project membership is not duplicated on issues. Rank updates and project version protect ordering; state planned/completed and completed_at for explicit manual completion. The implemented model, API, denominator and migration contract are described in [Milestones](../MILESTONES.md). Planning screen and keyboard/date-label acceptance remain separate M05-T04 work.

## 5. API requirements

Project milestone CRUD/reorder and issue patch. Delete offers detach issues then remove; never cascades issue deletion.

## 6. Authorization rules

Project/team contributors assign issues; project lead or team maintainer manages milestone structure. Readers see only accessible project content.

## 7. Edge cases

Issue project change, empty milestone, overdue date, two reorders, deletion during assignment and archived project.

## 8. Failure behavior

Conflict refetches current order and preserves intended move. Retiring milestone rejects new assignments until detach completes.

## 9. Performance considerations

One aggregate query per project for counts; cap displayed milestones 50 then paginate/expand. No per-milestone N+1.

## 10. Feature dependencies

F05 projects, F06 issues, F04 access.

## 11. Acceptance criteria

F14-A1: milestone from another project is rejected at DB and API. F14-A2: deletion detaches without removing issues. F14-A3: empty scope displays no invented completion percentage.

## 12. Testing strategy

Composite-FK tests, simultaneous delete/assign, count parity, keyboard reorder and date-label usability checks.
