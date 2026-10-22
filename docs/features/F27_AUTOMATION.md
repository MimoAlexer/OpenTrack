# F27 Automation rules

Stage: Post-MVP. Delivery: [M10_AUTOMATION](../milestones/M10_AUTOMATION.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Automate repetitive tracking actions with understandable triggers and bounded effects.

## 2. Functional requirements

Approved expansion obligations: X12, X41, X57. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Rules have event trigger, typed condition AST and allowlisted actions: set status/priority/assignee/labels, create notification, later webhook. No arbitrary script/shell/SQL. Dry-run shows matched records/effects. Max chain depth 5, per-rule rate caps and causation IDs prevent loops.

## 3. Interaction patterns

Rule builder uses readable conditions and an effect preview. Enable after sample dry-run; execution history shows matched/skipped/failed and current version. Pause immediately stops new effects.

## 4. Data requirements

automation_rules and executions keyed by rule/version/event; creator/granted team capability retained; action receipt records causal root.

## 5. API requirements

CRUD/enable/pause/simulate and execution read. Each actual action reuses ordinary commands with idempotency and current authorization.

## 6. Authorization rules

Team maintainer/admin owns rule; effective rights are explicit service grant intersected with scope, not a perpetual copy of creator authority. Revoke grant disables rule.

## 7. Edge cases

Rule triggers itself, two rules loop, target status deleted, creator removed, edit while queued, event replay after retention and bulk storm.

## 8. Failure behavior

Execution stores rule version snapshot, failed action and retryability. Stop loop/depth violations with visible diagnostic; do not continue remaining dependent effects after prerequisite failure.

## 9. Performance considerations

Core job queue has per-workspace fairness; max 100 actions/event and bounded fan-out. No user rule on request transaction path.

## 10. Feature dependencies

F25 durable events, F04, F30, F08/F09/F10; F32 for outbound notifications.

## 11. Acceptance criteria

F27-A1: replay causes no duplicate rule effect. F27-A2: cyclic rules stop by causation/depth limit. F27-A3: simulate has zero writes. F27-A4: revoked scope blocks queued actions.

## 12. Testing strategy

Rule AST property tests, causation loop fixtures, version/revocation races, job crash recovery, rate-cap load tests and builder preview E2E.
