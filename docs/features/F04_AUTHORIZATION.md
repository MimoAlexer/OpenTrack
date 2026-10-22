# F04 Authorization and isolation

Stage: MVP. Delivery: [M02_IDENTITY](../milestones/M02_IDENTITY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Make the access model predictable and prevent private content leaks through secondary surfaces.

## 2. Functional requirements

Approved expansion obligations: X60, X65. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Workspace owner/admin/member/viewer roles plus private-team membership and team maintainer role. Every command, list, count, search, job, export, event and AI context path uses the same current policy. No custom per-issue ACL in MVP.

## 3. Interaction patterns

Hide inaccessible objects; show disabled known actions with a concise reason. Losing access clears the open pane and cache while preserving no server content in a new workspace.

## 4. Data requirements

workspace_auth_guards epoch and memberships; typed authorization context; API token scopes intersect current permissions. RLS enforces workspace scope and composite FKs prevent mislinked tenant rows.

## 5. API requirements

Permission capabilities returned with authorized objects are UI hints only. 404 conceals unknown/inaccessible object; 403 is reserved for a known denied operation.

## 6. Authorization rules

Apply the normative matrix in SECURITY_ARCHITECTURE.md. Admin ownership cannot bypass private-team membership. Emergency local access is explicit, time-limited and audited.

## 7. Edge cases

Revoke while user submits a mutation, leaves stream connected, downloads export, or owns an agent grant. Organization role changes cannot silently alter content access.

## 8. Failure behavior

Missing workspace context, membership lookup failure or stale epoch fails closed. No cached permission fallback on DB failure.

## 9. Performance considerations

Batch ACL joins, not per-row network checks; guard locking serializes revocation only, with short read/write transactions. Monitor contention.

## 10. Feature dependencies

F01, F02, F03 and all content features. This is a prerequisite, not a later hardening patch.

## 11. Acceptance criteria

F04-A1: denied object contributes no row, count, title, snippet or event ID. F04-A2: revoke/write race linearizes before or after revocation. F04-A3: stale worker cannot publish private content using previous authority.

## 12. Testing strategy

Matrix-driven tests for every endpoint/role, randomized cross-tenant graph fixtures, concurrent revoke/write, pool-context reuse, and two-instance stream invalidation.
