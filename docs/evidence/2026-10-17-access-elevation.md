# Access elevation checkpoint — 17 October 2026

This increment follows recovery checkpoint `fcc273e`. It closes targeted access-elevation session invalidation in the current identity surface. The complete-product goal and unfinished M02 requirements remain open.

## Behavior

Active workspace role increases (`viewer < member < admin < owner`), membership reactivation, new private-team access and new maintainer rights invalidate the affected account’s existing active sessions. Unchanged access and reductions do not cause global sign-out; a contributor row in an already-public team grants no additional capability. Creating one’s own workspace/team and changing archive/privacy publication use their existing permission and epoch rules. Disabled accounts cannot receive member updates.

The acting administrator keeps their session when changing another person. Self-elevation returns the successful entity, ETag and normal status, together with a clearing cookie and `X-OpenTrack-Session-Invalidated: true`. The browser removes authenticated views, reports **Access updated. Sign in again.**, and stops follow-up refreshes. This is a committed result, not a failed write to repeat. A response from an older login cannot expire the newer UI session. If navigation merely closed a view within the same session, an observed committed invalidation still clears the account; this distinction was fixed during independent review.

An existing account accepting an invitation keeps exactly its newly rotated cookie and CSRF token. The prior authentication time and absolute expiry carry forward unchanged. Its other active sessions are invalidated. New-account acceptance creates only its initial session.

## Transactions and history

The store sets transaction-local context, then locks distinct actor/target account IDs in ascending UUID order: actor shared, target exclusive (`FOR NO KEY UPDATE`), or directly exclusive for self-change. Workspace guards follow. Authorization, recent authentication, current target state and preconditions are checked while these guards are held. No shared-to-exclusive upgrade or later account lock is introduced.

Membership, session invalidation, mandatory audit, epoch and durable event commit together. Account security `user_id` identifies the affected account. Revocation records share a correlation UUID with the workspace audit ID and event mutation ID; the workspace audit identifies the actual person who changed access. The helper temporarily selects the locked target’s account-audit RLS context and restores the actor before writing workspace effects. Existing FORCE RLS and append-only constraints remain.

No new migration is introduced; migrations 0001–0006 remain unchanged. Returning to the earlier binary would restore its earlier behavior for future grants but would not revive sessions already revoked by this version. No downgrade qualification is claimed.

## Verification

Environment: Linux, Rust 1.98.1, PostgreSQL 18.6 at isolated loopback port 55432, Node 22.22.1. Fresh schemas in the separate test database use restricted runtime credentials and a separate migration owner.

Focused results already executed:

- `cargo test --locked -p opentrack-app --test access_elevation_contract`: passed, 59.13 seconds. Covers the grant predicate matrix, multiple old cookies and cached actors across workspaces, caller/unrelated-account preservation, correlated audit, disabled/stale/forbidden/recent-auth denials, both account- and workspace-audit failure rollbacks, invitation lifetime preservation, and pooled context cleanup.
- The same real-database contract observes PostgreSQL lock blockers for login/elevation and write/elevation in both orders. A prior write commits before later revocation; a cached actor queued after elevation is rejected. Reciprocal cross-user grants complete with one success and one now-unauthorized actor instead of a lock cycle.
- Existing identity PostgreSQL contract: passed, 43.00 seconds. Callers that intentionally gain access now authenticate again before continuing.
- `cargo test --locked -p opentrack-api --test access_elevation_contract`: passed, 20.93 seconds. Real router/database requests verify remote revocation, preserved administrator cookies, unchanged/demotion and failed-precondition behavior, successful self-elevation with secure clearing cookie and entity/ETag, and invitation replacement lifetimes.
- App all-target and API library Clippy checks passed with warnings denied. The frontend passed 75 tests and its production build before the final same-session navigation correction; the complete shared run below qualifies that correction.

`scripts/check.sh` passed on the final source: 42 Rust tests against the real PostgreSQL test database, 75 frontend tests and 3 Python checks, plus formatting, Clippy with warnings denied, generated OpenAPI/TypeScript drift checks, TypeScript checking and the production Vite build. This includes the same-session navigation correction. Local output is retained in ignored `work/check-access-elevation.log`.

The rebuilt native server passed `/health/ready`. The live browser was used to inspect connected [member settings and its access-change explanation](2026-10-17-access-members.png); no member permission was changed through browser automation in this checkpoint. Backend mutation behavior above is real PostgreSQL/HTTP evidence, while the self-invalidation notice/cache-removal path is component/transport evidence. No complete live-browser self-promotion rehearsal, hosted CI, Docker execution or whole-product restore is claimed.

## Remaining work

Global account disable/enable, enabled-owner preservation across workspace and organization scopes, expired-invitation replacement audit, optional OIDC and remaining identity qualification are still open. Broader product and expansion requirements remain governed by [requirements status](../REQUIREMENTS_STATUS.md). See [account security](../ACCOUNT_SECURITY.md), [architecture](../ARCHITECTURE.md) and [wire rules](../API_DESIGN.md).
