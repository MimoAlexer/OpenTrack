# Invitation integrity checkpoint — 17 October 2026

This increment follows global account lifecycle checkpoint `151ffbd`. It closes the discovered invitation expiry-during-wait defect and adds the missing expired-replacement audit. No API shape, UI screen or migration changes are introduced; applied migrations 0001–0007 remain unchanged.

## Behavior

Invitation preview, directory filtering, issuance expiry and final acceptance use current database clock time. The consuming UPDATE checks workspace, ID, token digest, expected version, unused/unrevoked state and current expiry, and must affect exactly one row. An expired request receives the existing concealed 404 and rolls back provisional account/membership/team changes. A lock wait or later grant validation cannot extend token validity.

Replacing an expired pending invitation increments its version and appends mandatory `invitation.expired_replaced` restricted audit, then creates and audits the replacement in the same transaction. A hidden old invitation can be retired without revealing its ID or private grants in the response or new workspace events. Retirement changes no usable access, so it creates no separate authorization epoch/event. Same-key replay returns the original replacement metadata with `token:null` and no repeated effects; different-key contention still preserves one pending invitation.

The common identity audit/event helper now verifies expected inserted rows and guard/stream-head presence. The single-session security audit helper verifies its inserted row too, covering acceptance session creation/rotation. Raised errors and silent BEFORE INSERT suppression both abort these paths. Broader bootstrap, recovery and bulk-session write-count qualification remains separate unfinished work.

## Verification

Environment: Linux, Rust 1.98.1, PostgreSQL 18.6 on isolated loopback 55432, Node 22.22.1. Fresh schemas in the separate test database use distinct restricted runtime, migration and operator identities.

Focused App PostgreSQL invitation contract passed: 1 test, 14.20 seconds on the final source, including session.created and session.invitation_rotated audit suppression. HTTP invitation contract passed: 1 test, 8.95 seconds. App/API and store library Clippy checks passed with warnings denied, and diff whitespace checks passed. Local focused logs are retained under ignored work/.

Final shared `scripts/check.sh` passed on 17 October 2026 at 21:35 UTC: formatting, workspace/all-targets Clippy with warnings denied, migrations, 47 Rust tests against the isolated PostgreSQL test database, 3 Python provisioning tests, generated OpenAPI/TypeScript consistency, 75 frontend tests, TypeScript checking and the production frontend build. Full local log: ignored `work/check-invitation-integrity.log`.

The real-database acceptance cases establish contention using `pg_blocking_pids`, then cross the database expiry boundary before releasing the blocked operation. One blocks at the account guard; another blocks after membership work but before final consume. Assertions check original cookie validity, no new memberships/grants/sessions, unchanged audit/epoch and unused expired invitation state. Replacement tests cover exact version/audit effects, idempotent and competing requests, private metadata, and raised/silently suppressed audit/event failure rollback.

No live-browser invitation mutation, external message delivery, Docker execution, hosted CI or whole-product restore is claimed. The rebuilt native server restarted on loopback port 18970, `/health/ready` returned ready, and the existing authenticated browser session reloaded the Members settings page with the expected owner and team. This is read-only continuity evidence, not browser mutation coverage. This source-only change needs no new migration; downgrade would restore old acceptance behavior for future requests and has not been qualified.

## Remaining work

M02 retains organization administration, remaining endpoint/role and UI qualification, broader privileged audit failure qualification, and optional OIDC. The second-workspace-owner/global-account recovery authority decision remains unresolved. M03 workflow/label/assignee work and the full product expansion remain required. See [requirements](../REQUIREMENTS_STATUS.md) and [API contract](../API_DESIGN.md).
