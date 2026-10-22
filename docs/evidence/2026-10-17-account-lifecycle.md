# Account lifecycle checkpoint — 17 October 2026

This increment follows access-elevation checkpoint `3b531bf`. It adds separate installation-operator account disable/enable commands. The full-product goal, remaining M02 gates and future cleanup surfaces remain open.

## Implemented behavior

`operator-account-disable --user UUID --reason TEXT` and `operator-account-enable --user UUID --reason TEXT` require the restricted operator credential. They do not expose workspace-owner HTTP authority. Their typed JSON receipts report the resulting state, timestamp, change flag, operation correlation and revocation counts; no reason or bearer is printed. Missing/invalid parameters and output-file flags are rejected. Unknown accounts and last-enabled-owner failures produce safe errors. Uncertain completion tells the operator to repeat the same desired state.

Disabling preserves another enabled active workspace owner, including archived workspaces, and another enabled organization owner. It atomically changes disabled state, invalidates all unrevoked sessions, pending recovery codes and outgoing pending invitations, advances affected epochs and appends mandatory account/workspace audit and access events. Inactive workspace memberships remain in the scope so old outgoing invitations cannot survive reenable. Repeating a state records an explicit no-op audit with no epoch changes.

Reenable preserves membership, authorship and the password hash, but never restores old credentials or signs the account in. Review found that pre-0007 disabled records could still have retained credentials; enable now revokes those too, with actual cleanup counts in its receipt. Incoming invitations remain pending but cannot be accepted while disabled.

## Database and authority

New forward migration 0007 leaves applied 0001–0006 unchanged. It adds scope-correct enabled-owner constraints, guarded disabled-state changes, real operator actor shapes and two explicit capability functions. FORCE RLS remains enabled. Fixed-path owner-check functions use migration-owner-only metadata policies on organization membership/workspaces; operator and runtime roles receive no helper execution or private-content policy. The runtime preflight rejects direct or reachable execution of any of the four operator functions. Operator provisioning grants exactly those four functions, no table or sequence access.

The target account guard precedes sorted organization and workspace guards. Other owner account rows are not locked afterward. Required audits and events must each insert the expected count; both raised errors and silent BEFORE INSERT suppression roll back state, credentials and epochs. Operator audit identifies actual database session_user, with one shared correlation across distinct workspace audit IDs and account events. Generic workspace access events carry neither operator identity nor reason/private-team details.

## Verification

Linux, Rust 1.98.1, PostgreSQL 18.6 on isolated loopback 55432, Node 22.22.1. Integration fixtures use fresh schemas in the separate `_test` database and distinct runtime/migration/operator logins.

Executed focused results:

- App account lifecycle PostgreSQL contract: passed, 23.95 seconds on the final two-loop migration. Covers enabled ownership across two organizations and archived workspaces, inactive-member invitations, complete invalidation/nonresurrection, no-op audit, runtime/helper denials, direct operator EXECUTE startup rejection, audit/event failure rollback, four observed login/write lock orderings, competing workspace and organization owner disables, and runtime attempts to forge operator audit/event rows.
- Additional PostgreSQL race contract: 1 test passed, 17.95 seconds. Observed both lock orders for owner disable versus self-demotion and inviter disable versus invitation acceptance; a disabled co-owner cannot satisfy ownership, accepted membership survives, and revoked invitations remain unusable after enable. All entity/audit work completes before the sorted stream-head/event loop.
- Existing recovery and access-elevation App contracts passed, 37.45 and 47.75 seconds respectively.
- HTTP recovery compatibility: 1 test passed, 12.09 seconds.
- CLI recovery/lifecycle: 2 tests passed, 12.30 seconds. Covers the exact receipt, ownership denial, runtime credential denial, invalid flags/unknown UUID, rollback/uncertain guidance, repeated states and legacy disabled credentials that must not revive.
- Targeted Clippy checks passed with warnings denied. CLI argument/configuration tests passed.
- The actual deployment grant helper ran against an isolated schema containing all seven migrations. All four capabilities were available only to the operator; the private helper and application tables remained unavailable, and runtime EXECUTE was denied. The probe schema was removed afterward.

`scripts/check.sh` passed on the final source: 45 Rust tests, 75 frontend tests and 3 Python checks, formatting, Clippy with warnings denied, generated contract drift checking, TypeScript and the production Vite build. The full run includes both account lifecycle suites. Local output is retained in ignored `work/check-account-lifecycle.log`. Shell syntax and 1,312 local documentation links were also checked with no failures.

A protected pre-0007 development database archive was created using the local cluster administrator and its archive directory was read successfully. This does not establish a successful restore. Migration 0007 and all four operator grants applied successfully to the native development database. The rebuilt server restarted on loopback 18970 and passed `/health/ready`; browser member-settings continuity was checked separately from lifecycle mutations. No browser account disable/enable, Docker execution, hosted CI, external-provider flow or complete-product recovery is claimed.

## Remaining work

Expired-invitation replacement audit, consume-time expiry after guard waits, mandatory insert-count checks in existing identity event helpers, remaining identity qualification and optional OIDC are still open. Organization membership management, assignment cleanup and key/job invalidation must be implemented with their owning capabilities. The F02-A3 workspace-owner/global-account recovery authority conflict remains explicitly unresolved. See [account security](../ACCOUNT_SECURITY.md), [runtime setup](../RUNTIME_SETUP.md) and the [requirements ledger](../REQUIREMENTS_STATUS.md).
