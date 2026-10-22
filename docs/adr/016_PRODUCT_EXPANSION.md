# ADR-016: integrate the approved product expansion without weakening content boundaries

Status: accepted implementation design; runtime evidence pending. Date: 17 October 2026. Related: [product expansion](../design/PRODUCT_EXPANSION.md), [database](../DATABASE_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md), [plan](../IMPLEMENTATION_PLAN.md).

## Problem

The user approved a broad product including retros, initiatives, roadmap scenarios, capacity, decisions, feedback, releases, incidents and reports. The old Phase 0 baseline did not define these objects or their cross-feature permissions. A broad container must not make private linked content visible, and a private retro cannot rely on ordinary team visibility alone.

## Alternatives

Keep only the 36-feature baseline and treat new pages as mockups: fails the authorized scope. Replace every task/project with a new umbrella object: conflicts with the user's preference for the familiar workspace. Give every object arbitrary ACLs: expands security complexity and contradicts the stable team-owned baseline. Use copied snapshots without source checks: exposes revoked/private content. Apply large scenario changes as an opaque best-effort batch: risks partial commitments.

## Decision

Preserve F01–F36 and M01–M14; add F37–F47, X01–X66 and M15–M21. Keep projects owned by one team and use independently authorized links for initiatives/releases/scenarios/reports. A retro narrows team access with explicit participants; summary readers receive a distinct representation. Hidden authors are not anonymous to the operator. Keep source references in immutable scenario revisions and reauthorize source rows on every read/export. Bound scenario apply to 100 selected projects and commit atomically after current permission/version checks. Use one workspace planning lock for cross-project graph changes in the canonical lock order. All modules remain inside the existing Rust/PostgreSQL core; AI is not required.

## Consequences

More endpoint families and durable schemas require explicit transfer, search, event, retention and restore coverage. Some aggregate views intentionally omit inaccessible sources without hidden counts. A member may retain access to a scenario header but lose access to some snapshot rows; UI must explain restricted scope generically. Larger planning changes must be split explicitly. Retros need specialized representations instead of generic document access. Complete scope includes new failure-path and privacy tests, not just CRUD pages.

## Reconsideration and validation

Reconsider the 100-item atomic bound only after measured contention and a reviewed larger operation model. Multi-team ownership and truly anonymous retros need separate ADRs and threat/retention designs. Validate G06–G10, source-revocation races, vote budgets, snapshot/export leakage and full restore. Requirements status remains unverified until evidence is linked; this ADR does not establish implemented security or scheduling correctness.
