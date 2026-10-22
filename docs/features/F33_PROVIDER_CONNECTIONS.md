# F33 AI provider connections

Stage: Optional post-MVP. Delivery: [M11_AI](../milestones/M11_AI.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Let users choose eligible hosted or local inference without making AI a requirement for tracking work.

## 2. Functional requirements

Approved expansion obligations: X53. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Provider-neutral capabilities, personal API-key connection, optional explicitly shared service API key, local endpoint allowlist and current connection/model status. ChatGPT plan usage is a distinct gated SIWC mode; Claude subscription token collection is not supported.

## 3. Interaction patterns

Settings separates OpenTrack login, AI identity, billing source and coding runtime. Connection wizard lists data sent, supported features and verification state. Disabled AI leaves core UI complete.

## 4. Data requirements

provider_connections, provider_grants, encrypted_credentials and usage. Identity tuple includes provider issuer/subject/client registration, not email alone; personal subscription grants cannot pool users.

## 5. API requirements

Create/verify/revoke/list models and capabilities; explicit connection per request. Runtime probes establish access only for that request, not universal future entitlement.

## 6. Authorization rules

User owns personal credentials; sharing service keys requires admin grant and budget. Revoke stops new requests and cancels in-flight work where supported.

## 7. Edge cases

Expired/revoked token, rotated refresh token, account/workspace switch, missing model/tool, offline local host and ambiguous subscription eligibility.

## 8. Failure behavior

Normalize auth/rate/quota/unavailable/unsupported/interrupted errors. No silent switch to paid API or another provider; ask through explicit UI selection with cost/data implications.

## 9. Performance considerations

Capability cache TTL 5 min with manual refresh; per-user concurrency 2 and request budget. Provider calls never occupy a DB transaction.

## 10. Feature dependencies

F03/F04, F30, provider research, encrypted secrets; F35 conversation consumer.

## 11. Acceptance criteria

F33-A1: all core functions pass with AI disabled. F33-A2: revoked connection cannot run new inference. F33-A3: unsupported feature is hidden/disabled clearly. F33-A4: subscription mode is not presented as general API credit.

## 12. Testing strategy

Fake-provider stream/auth/rate fixtures, credential redaction/rotation tests, capability downgrade, private connection ACL, opt-in real-account smoke test per supported mode in M11.
