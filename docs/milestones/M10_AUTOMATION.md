# M10 Public tokens, outbound webhooks and rules

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: Integration release. Dependencies: M09. Feature coverage: F27, F31–F32.

## Scope and deliverables

Expose safe extension points and bounded rules without arbitrary code execution.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M10-T01 Token lifecycle | M09 | One-time display, scoped hashed tokens, expiry/revocation and published API examples. |
| M10-T02 Outbound webhook | M10-T01 | Signed payload/event ID/timestamp, SSRF policy, secret rotation, retry/dead-letter and diagnostics. |
| M10-T03 Rule engine | M10-T01 | Versioned typed conditions/actions, causation/depth 5, scoped service grants and receipt dedupe. |
| M10-T04 Builder/simulation | M10-T03 | Readable preview and no-effect dry-run, execution history and immediate pause. |
| M10-T05 Abuse/recovery | M10-T02, M10-T04 | Webhook DNS rebinding, rule loops, revoked creator/grant, storm fairness and uncertain delivery. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F27/F31/F32 acceptance, consumer contract compatibility, scope matrix and external fixture-server evidence.

## Risks and scope guard

Rules can amplify a small event into overload; enforce per-event/per-workspace budgets. Generic OAuth/MCP server is deferred, not smuggled into tokens.

## Completion condition

Clients use stable contracts, webhooks report honest at-least-once status, rules cannot expand access or loop indefinitely.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
