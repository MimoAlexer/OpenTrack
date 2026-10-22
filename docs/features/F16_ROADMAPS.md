# F16 Roadmaps

Stage: Basic project timeline after MVP; advanced later. Delivery: [M14_ADVANCED](../milestones/M14_ADVANCED.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Communicate project timing and dependency risk across accessible teams without turning forecasts into false commitments.

## 2. Functional requirements

Approved expansion obligations: X19, X20, X63. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Initial roadmap is a project list grouped by target month with dates/health/lead; advanced timeline adds zoom and milestone markers. Cross-project dependencies, initiatives, scenarios and capacity are now specified in [F37](F37_INITIATIVES_OUTCOMES.md), [F38](F38_SCENARIOS_CAPACITY.md) and the canonical data/API/security contracts; M16 delivers that expansion. No critical-path or capacity claim without an explicit scheduling model.

## 3. Interaction patterns

List is the accessible default; timeline is an alternate view with keyboard date editing. Undated projects live in a separate group. Moving a bar previews date changes and commits on confirmation.

## 4. Data requirements

Existing project/milestone dates and saved view filters suffice for first projection. The approved expansion uses the additive entities and constraints in ADR-016 and DATABASE_DESIGN; migrations land with their actual vertical slices, not empty speculative scaffolding.

## 5. API requirements

Authorized project projection with date range and group; updates use existing project endpoint/version. Aggregate counts use only accessible projects.

## 6. Authorization rules

Any reader can view accessible project timeline; only project editors change dates. Shared roadmap link does not widen team membership.

## 7. Edge cases

Private projects omitted from gaps/counts, missing dates, target before start, multi-timezone teams and concurrent bar move.

## 8. Failure behavior

Timeline failure retains usable project list. Conflicting date drag shows current dates and offers explicit retry, not silent overwrite.

## 9. Performance considerations

Limit timeline window and max 500 rendered bars via virtualization; project list paginates. Measure accessibility/performance before complex dependency visualization.

## 10. Feature dependencies

F05, F14, F17, F04; M08 production baseline first.

## 11. Acceptance criteria

F16-A1: timeline and list show the same authorized scope. F16-A2: keyboard user edits dates without dragging. F16-A3: undated/empty projects remain discoverable.

## 12. Testing strategy

Date/filter projection tests, screen-reader list equivalence, large timeline performance, privacy count tests and pointer/keyboard date edit E2E.
