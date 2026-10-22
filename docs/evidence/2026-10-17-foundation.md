# Foundation implementation evidence — 17 October 2026

This is an in-progress implementation record, not a release certification. The full goal remains F01–F47, X01–X66 and M01–M21. No milestone is claimed complete by this record.

## Environment

Linux x86_64; Rust/Cargo 1.98.1; Node 22.22.1/npm 9.2.0. Isolated PostgreSQL 18.6 on loopback port 55432. Runtime and migration roles are different; runtime cannot create tables, is not superuser and has no BYPASSRLS. Dependency resolution is pinned by Cargo.lock and web/package-lock.json. Application source uses a five-crate Rust workspace and React/Vite. The runtime serves built web assets on the same origin.

## Executed checks

- `cargo check --workspace`: passed after adapting fixed SQL fragments to SQLx 0.9 QueryBuilder. Dynamic data remains in bind parameters.
- `scripts/dev-db.sh migrate`: applied migration 0001 to the isolated development database using the migration owner. No system PostgreSQL installation/service was changed.
- Live HTTP smoke against the actual API/database: setup, secure cookie attributes appropriate to explicit loopback HTTP development, authenticated session/bootstrap, issue creation, exact idempotent replay, changed-body key rejection (409), version increment, stale write (412 with authorized current issue), missing precondition (428), invalid CSRF (403), inaccessible workspace (404), filtered listing, atomic activity and logout revocation (401 on old session). Scratch script/result are under ignored work/; durable router tests are being added separately.
- The runtime agent executed `postgres_contract` against a unique schema in opentrack_test: setup/login, cross-workspace access, private-team exclusion even for workspace owner, viewer write rejection, FORCE RLS with missing context, sequential/concurrent idempotency, stale update, unknown patch rollback, event/activity counts, keyset pagination, archive and revoked-team retry visibility passed. See the committed test source; broader identity lifecycle and revocation-race requirements remain unimplemented.
- Final `scripts/check.sh` passed against a freshly bootstrapped isolated CI-native test database with separate runtime and owner identities. It ran formatting, warning-free Clippy, migrations, ten Rust tests (eight unit tests and two real PostgreSQL contract suites), twenty frontend tests across five files, TypeScript and Vite production build. The contract suites include multiple behavioral and negative-access assertions; test counts are not feature-completion counts.
- Migrations 0002 and 0003 were applied to development: audit history rejects UPDATE/DELETE and runtime grants also exclude UPDATE/DELETE/TRUNCATE. The restricted-role negative tests pass. A TTL index supports bounded deletion of expired login-attempt buckets; auth admission is capped by peer IP and globally, with two limiter unit tests.
- `cargo-audit` 0.22.2 reported zero vulnerabilities and zero warnings against RustSec advisory commit `999ec89d5d3f10f20be14750ec8c2d8b44e0ac9b`. `npm audit --omit=dev` reported zero findings. These dated advisory results are not a complete security audit.
- The GitHub Actions workflow passed actionlint 1.7.12 locally; its native verification command passed. No remote CI run is claimed. Docker Compose configuration validated; actual container execution is still unverified because no daemon is available.

## Browser verification

Tested the built application through the Codex in-app browser on `http://127.0.0.1:18970`. Port 8080 had cached content from an unrelated local app/service worker; OpenTrack was moved to a dedicated port without clearing the user's browser data.

The development fixture account belongs to a new local-only workspace. No mock server or seeded production rows were used. Actual browser login succeeded. Pressing C opened the create dialog; creating ENG-2 persisted the issue in PostgreSQL and showed it in the list. The initial detail request used an unscoped URL and failed; the frontend was corrected to workspace-scoped GET/PATCH/conflict reload routes, rebuilt and retested successfully. Initial create-dialog focus was also corrected to the title field.

Changing title and status to In progress saved successfully and moved the issue to the corresponding list group. Two tabs loaded the same issue version: one edited the title locally, the other saved a different title. Saving the stale tab showed both values and preserved the draft. Choosing Keep my edits for review did not save automatically; explicitly saving then reloading showed the persisted resolved title.

Restarted the native server and reloaded the existing browser session: authentication and saved issue content persisted. Archiving removed the issue from the active list and made its editor read-only; restoring returned it to active work with a new attributable activity entry. The backend accepts only explicit restore on an archived issue; the real PostgreSQL test rejects an archived title edit with 409. At an emulated viewport width of 390 pixels, document width equaled viewport width, navigation and the create dialog remained usable, and no horizontal overflow appeared. This is browser reflow evidence, not native mobile testing.

Rendered evidence: [desktop](2026-10-17-desktop.png), [390-pixel mobile reflow](2026-10-17-mobile.png). Screenshots demonstrate rendering; the contract tests, real API writes, two-client check and server restart establish the corresponding persistence/conflict behavior.

## Explicit limits and next work

This slice has local setup/login/session/logout, basic scoped issue creation/list/detail/update and matching UI. It does not yet complete invitations, member/role lifecycle, recovery, OIDC, workflow management, hierarchy, rich documents, realtime consumers, projects, retrospectives, scenarios, integrations, agents or desktop. Description is currently plain text on the issue row; the specified independently versioned document aggregate remains required in M04. Full configuration catalog/provenance and generated API/client contracts remain required. Security review, accessibility, sustained performance, backup/restore and packaged-container execution are not yet release-qualified. The current schema and features must not be described as the finished product.
