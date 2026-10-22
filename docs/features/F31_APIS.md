# F31 Public and internal APIs

Stage: MVP core; scoped token release in M10. Delivery: [M10_AUTOMATION](../milestones/M10_AUTOMATION.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Allow integrations and future clients to use stable contracts without privileged shortcuts.

## 2. Functional requirements

Browser consumes /api/v1 from M03; documented OpenAPI and authenticated developer tokens are released in M10 after scope/rate review. Internal runner API uses separate machine audience. Public means documented, not anonymous.

## 3. Interaction patterns

Developer settings shows token name/scope/expiry/last-use and one-time secret display. API docs show errors, pagination, ETags, idempotency and limits with safe examples.

## 4. Data requirements

api_tokens hashed, idempotency_records, request metadata and generated OpenAPI JSON artifact later. No bearer token stored in browser localStorage.

## 5. API requirements

CRUD/action inventory is API_DESIGN.md. Stable problem+json errors, additive v1 policy, scoped token management and capability discovery.

## 6. Authorization rules

Token scopes intersect live user membership; no token can expand private-team access. Runner token cannot call ordinary user admin endpoints.

## 7. Edge cases

Token expiry midstream, stale version, reused key/body mismatch, unknown fields, oversized page and mixed client/server versions.

## 8. Failure behavior

Use precise 401/403/404/409/412/428/429/503; Retry-After where applicable. Never treat malformed request as a successful no-op.

## 9. Performance considerations

Rate budget 300 requests/min/user/workspace, burst 60; resource limits and per-tenant fairness. Contract generation cannot add network overhead to core requests.

## 10. Feature dependencies

F03/F04, F06 core resources, F30, M01 schema conventions.

## 11. Acceptance criteria

F31-A1: generated TS client matches server schema. F31-A2: token cannot outlive revoked membership. F31-A3: same idempotency key/different body yields 409. F31-A4: internal audience cannot authenticate as user.

## 12. Testing strategy

OpenAPI diff/consumer contracts, scope matrix, rate/fuzz tests, token lifecycle and compatibility test with previous supported client.
