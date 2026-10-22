# F15 Cycles and iterations

Stage: MVP explicit; recurrence later. Delivery: [M05_PLANNING](../milestones/M05_PLANNING.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Plan a bounded interval of team work and understand scope changes without altering the issue workflow.

## 2. Functional requirements

Approved expansion obligations: X25, X26, X36. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Explicit team cycles with local start date and exclusive end date, no overlap, planned/active/closed state. One cycle per issue. On close, show unfinished items and explicit carry-forward choice; default leaves them in closed-cycle history until user moves them. Automatic recurrence is M14.

## 3. Interaction patterns

Team cycle list opens scope, progress and change history. Date picker displays timezone. Close preview separates completed, canceled and unfinished work and asks which items to move.

## 4. Data requirements

cycles, issues.cycle_id, daily cycle_snapshots with metric definition version. Exclusion constraint prevents overlap; date semantics avoid DST-duration ambiguity.

## 5. API requirements

Cycle CRUD, issue assignment, close command with expected versions and optional destination cycle. Return per-item conflicts on carry-forward.

## 6. Authorization rules

Team maintainer manages dates/closure; contributors plan accessible issues. A closed cycle is history; corrections require explicit reopen and audit.

## 7. Edge cases

DST/weekends, timezone changed later, overlapping date edits, late completion, scope moved after closure and recurring job fired twice in future.

## 8. Failure behavior

Cycle close commits state once; carry-forward records a resumable operation with item results. Failed chart generation shows scope table and last-computed date.

## 9. Performance considerations

Daily snapshots rebuilt from activity within bounded windows; indexed cycle/status counts. Recurrence scheduling later uses unique team/date job keys.

## 10. Feature dependencies

F02, F06, F08, F30, F25.

## 11. Acceptance criteria

F15-A1: overlapping cycles cannot commit concurrently. F15-A2: closing does not silently close or move issues. F15-A3: dates around DST retain intended local calendar boundaries.

## 12. Testing strategy

Exclusion/date property tests, timezone fixtures, concurrent close/edit, carry-forward retry and accessible table equivalent for charts.
