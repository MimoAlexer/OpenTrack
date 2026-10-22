# F30 Audit logging

Stage: MVP. Delivery: [M02_IDENTITY](../milestones/M02_IDENTITY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Provide restricted evidence of security-sensitive changes and external actions, separate from everyday issue history.

## 2. Functional requirements

Approved expansion obligations: X60. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Record login/recovery, role/privacy/integration/key changes, export, administrative actions, approval, runner grant and external publication. Actor, target, decision, request/correlation ID, timestamp and reason; redact secrets and content. Default 365-day retention.

## 3. Interaction patterns

Admin audit table filters action/actor/date with safe target label. Export requires authorization and itself creates an audit event. Corrections append records; UI never edits history.

## 4. Data requirements

audit_entries append-only to runtime role; workspace_id nullable only for true instance/identity events. Source IP has explicit retention minimization. Optional external signed sink is later, not a tamper-proof claim.

## 5. API requirements

Authorized cursor read/export; no public create/update/delete. Domain use case appends required audit within same transaction as privileged mutation.

## 6. Authorization rules

Workspace admin sees workspace security metadata; instance audit requires operator capability. Private issue bodies and secrets never enter this surface.

## 7. Edge cases

Actor deleted, denied request before workspace resolved, audit insert failure, clock skew, retention purge and DB administrator tampering.

## 8. Failure behavior

For privileged mutation, required audit write failure aborts mutation. Authentication failure logging uses bounded fallback to avoid recursive outage; security event loss raises alert.

## 9. Performance considerations

Index workspace/time and action; batch retention, partition only if measured. Avoid logging every keystroke/read as default.

## 10. Feature dependencies

F03/F04, all privileged operations, observability.

## 11. Acceptance criteria

F30-A1: role change and audit are atomic. F30-A2: token/password content absent from exports/logs. F30-A3: runtime role cannot rewrite previous audit. F30-A4: authorized retention is itself recorded.

## 12. Testing strategy

DB privilege tests, audit failure injection, redaction canaries, actor anonymization, filter pagination and retention boundary checks.
