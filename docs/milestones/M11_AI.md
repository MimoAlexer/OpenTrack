# M11 Optional provider connections and conversations

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: AI preview. Dependencies: M08; M10 for write-tool integration. Feature coverage: F33, F35.

## Scope and deliverables

Add useful provider-neutral assistance while keeping the core independent and billing/auth explicit.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M11-T01 Current evidence gate | M08 | Recheck official APIs/policies; resolve eligible auth modes, model discovery and exact adapter schemas. Q01 plan-use remains disabled if unresolved. |
| M11-T02 Connection vault | M11-T01 | Personal/shared-service grants, encryption, refresh fencing, revocation and redacted connection status. |
| M11-T03 Inference adapters | M11-T02 | OpenAI API, Anthropic API and one local provider; normalized bounded streams/errors/usage/cancellation. |
| M11-T04 Conversation UI | M11-T03 | Explicit context/model/account, private history, partial status, stop/retry and unknown usage. |
| M11-T05 Tools/budgets | M11-T04; M10 additionally for write tools | Read tools first; structured reviewed write proposals, call dedupe and cost/data-destination consent. |
| M11-T06 Optional SIWC spike | M11-T02, M11-T03 and Q01; optional | Eligible local companion flow only after Q01; prove loopback pairing/refresh ownership without importing native credentials. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F33/F35 acceptance, fake-provider streaming/error/rotation suite, AI-disabled G05, redaction canaries and opt-in real-account test per claimed mode.

## Risks and scope guard

Provider policy drift and cost surprises. Never block core release on SIWC or offer Claude token intermediation; avoid hardcoded model entitlement.

## Completion condition

OpenAI API, Anthropic API and one local route pass the defined fixtures and permitted real-account checks with clear permissions/cost and honest interruption states. Claim support only for validated modes; unverified modes remain disabled or explicitly experimental, and missing required adapter evidence leaves that portion incomplete. SIWC is optional and does not block the API/local release.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
