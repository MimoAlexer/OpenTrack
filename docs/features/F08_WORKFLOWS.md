# F08 Status workflows

Stage: MVP. Delivery: [M03_ISSUES](../milestones/M03_ISSUES.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Give each team a clear lifecycle while keeping reporting categories comparable across custom status names.

## 2. Functional requirements

Approved expansion obligations: X03, X10. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Ordered statuses map to fixed categories triage/backlog/unstarted/started/completed/canceled. Exactly one active default. Teams may rename/reorder statuses; deleting a used status requires a replacement in the same team.

## 3. Interaction patterns

Settings shows ordered category groups and previews affected issue count. Board columns follow this order. Status picker displays icon, name and category without color-only meaning.

## 4. Data requirements

workflow_statuses, team default and issue status FK. Archived status may remain historical but cannot receive new issues.

## 5. API requirements

Status CRUD/reorder/replace with expected team/workflow version. Replacement updates affected issues/events in bounded maintenance batches while blocking assignment to retiring status.

## 6. Authorization rules

Team maintainer or workspace admin with team access changes workflow. Contributors choose available statuses on issues.

## 7. Edge cases

Last default removal, all statuses archived, rename collision, replacement with different semantic category and active automation referring to old status.

## 8. Failure behavior

Workflow change fails validation before altering issues; a long replacement has resumable progress and a retiring marker. Never leave an issue without status.

## 9. Performance considerations

Cache workflow per team/version and invalidate on change. Replacement batches 100 and explicit progress avoid a huge lock transaction.

## 10. Feature dependencies

F02 teams, F04, F06; F27 rules revalidate references.

## 11. Acceptance criteria

F08-A1: a new issue always receives one active default. F08-A2: used status cannot vanish without mapping. F08-A3: rename leaves stable IDs and existing issue links intact.

## 12. Testing strategy

Unique-default DB tests, concurrent status retirement/create, category aggregate tests, board/picker order and accessible label verification.
