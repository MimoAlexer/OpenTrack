# Issue deletion, Trash and command navigation — 18 October 2026

This increment extends M03 with recoverable issue deletion and a shared keyboard command registry. It does not close the complete-product goal, the full keyboard/search feature, native GUI qualification or later planning milestones. The earlier [interface repair](2026-10-18-ui-refinement.md) remains separate evidence for the styling and refresh-flicker fixes.

## Implemented behavior

Deletion is separate from archive. Its review shows the current issue, direct-child count, incident relationship count and parent effect. A dirty editor must first save or discard its draft. Only server-authorized actions appear. A conditional deletion atomically detaches connections and moves the issue to Trash for a 30-day restore window. Children are not deleted. Current readers can browse/search/page through Trash, while mutation authority remains author, visible team maintainer or authorized workspace manager with a non-viewer role. Private teams remain concealed from administrators outside them.

Restore explicitly submits current status, nullable assignee and label references. Existing archived labels can remain; invalid references need repair. The original identity and key survive. Removed graph edges never return implicitly. Expired entries remain readable without a restore action; physical purge is not implemented. Stale deletion requires a fresh review; stale restoration retains proposed repairs. Interrupted responses require a read before another mutation. Desktop reauthentication preserves the repair draft and uncertainty state, and denied access hides restricted details.

Commands, navigation and shortcut help share one registry. The palette supports keyboard selection, search, cancellation and focus restoration. Dispatch ignores composition, held keys, editable fields for single-key shortcuts, handled events and other open dialogs. Context controls command availability; workspace/access changes clear the palette. The palette currently contains local actions and navigation, not remote entity search or recent-item storage. User shortcut preferences and sequence shortcuts remain unfinished.

## Executed focused checks

- Real PostgreSQL application contract: actual 0013→0014 upgrade preserves prior data and create receipts; historical graph endpoints detach without revival; mandatory entity/graph/activity/audit/stream suppression rolls back all effects; every distinct endpoint advances once; 106 events append contiguously across bounded chunks; 10,001 connections fail safely; the restricted worker can still change status but cannot change deletion state.
- Observed PostgreSQL lock races: both parent-attachment/deletion orderings, both delete/restore orderings, concurrent restore with one committed result, and restore waiting until the database clock crosses the deadline. These use observed database blocking, not timing-only assumptions.
- Two HTTP integration tests: author/maintainer/manager/viewer boundaries, private-team concealment, missing and stale preconditions, strict restore payloads, normal-read/key/list/lookup exclusion, Trash filters and cursor binding, repair, current authority and expiry. Both passed in 34.95 seconds on the isolated test database.
- Frontend suite: 231 tests across 24 files passed, with explicit draft/conflict/ambiguous-outcome, access invalidation, pagination/refresh and native reauthentication cases. After responsive-layout and help-dialog refinements, 24 focused command/deletion tests passed again. Type checking and a production build passed before those final layout-only changes; the comprehensive check below owns final qualification.
- Native boundary: five protocol tests passed, including the exact deletion/Trash/restore route allowlist and rejection of unsupported methods/identifiers. Desktop all-target Clippy passed.

Testing found and corrected an account-audit shape violation, a private-object authorization ordering leak, native reauthentication draft loss and denied-content retention. Review also corrected sidebar wrapping, mobile Trash caption hiding, help behind mobile navigation, long-key/metadata overflow and the palette input's accessible name.

## Recovery checkpoint

The actual running 0013 API/worker binary and matching web assets were preserved before rebuilding. After stopping the verified owned development supervisor, a private 222,124-byte database archive was restored into isolated `opentrack_restore_0013_20261018`. All 34 data/sequence fingerprints, table owner/RLS/ACL and function owner/ACL/search-path checks matched; development data stayed stable and the restored ledger was exactly 0001–0013.

The preserved old binary served its matching assets against only the restored database on temporary loopback 18971. Eight checks passed: restricted child credentials, readiness, matching assets, login, bootstrap, persisted issue identity/version, logout and subsequent session denial. The temporary server stopped cleanly. Private archives, credentials, binaries and raw receipts remain in ignored local `work/` storage. Rollback requires this database and matching build; there is no down-migration.

## Browser verification

The complete workflow ran through the actual browser against an isolated copy on loopback 18972. The command palette created ENG-6, then the issue controls created ENG-7 as its child and added a related connection. Deletion review showed one child and one relationship. Deleting ENG-6 kept ENG-7 readable with both connections removed and an activity entry explaining the detachment. Trash showed ENG-6 with its restore deadline. Restoring with the In progress status preserved its original UUID, key, description and history, while leaving the former connections detached. Reload preserved the result; Trash became empty. These disposable issues exist only in the isolated copy.

Palette search/dispatch, shortcut help, Escape and focus return were checked. Final review also found a pending search debounce that could close a newly opened restore dialog; the timer now pauses while detail is open, retaining proposed repairs. A routed fake-timer regression verifies the dialog and status draft survive, then filtering resumes after closing.

Screenshots: [commands](2026-10-18-command-palette.png), [delete review](2026-10-18-issue-deletion-preview.png), [restore review](2026-10-18-issue-restore-review.png), [restored issue](2026-10-18-issue-restored.png), [empty Trash](2026-10-18-trash-empty.png). Responsive behavior has source and automated coverage; an exhaustive rendered mobile or native GUI review is not claimed.

## Deployment and regression checkpoint

The full `scripts/check.sh` completed successfully: 84 Rust tests including real PostgreSQL API/application/race contracts, Python deployment checks, formatting, all-target workspace Clippy, generated-contract comparison, 231 frontend tests across 24 files, type checking and a production build. Log: `work/check-deletion.log`. The final frontend debounce refinement is qualified separately below.

Before applying 0014, a fresh private 222,137-byte `before-0014-deploy.dump` was restored into `opentrack_restore_0013_deploy_20261018`. All 34 fingerprints and authority checks matched again, followed by all eight HTTP checks using the preserved 0013 build. This later backup is the deployment rollback point. The earlier backup remains preserved.

`scripts/dev.sh` applied migration 0014, refreshed restricted worker grants and started the rebuilt API and worker on 18970. Readiness returned 200; the migration ledger is exactly 0001–0014. Process inspection confirmed separate runtime/worker credentials and no migration/operator/test credentials in either child. Receipts: `work/deployment-0014-result.json`, `work/postgres/restore-before-0014-deploy-result.json`, `work/restore-before-0014-deploy-http-result.json`.

The main browser loaded the existing ENG-5 with its owner assignment and Browser review label intact. The command palette opened and navigated to the empty Trash. No verification issues were created in the main workspace during this slice. The current [main palette screenshot](2026-10-18-main-command-palette.png) is distinct from isolated workflow evidence.

After the final debounce fix, all 232 frontend tests across 24 files passed again, followed by type checking and a production build. Logs: `work/test-deletion-final.log`, `work/build-deletion-release.log`. Verified assets were copied before atomically replacing entry files, preserving previously built hashes. A final browser reload showed the unchanged existing issue; [workspace screenshot](2026-10-18-main-workspace-final.png). The isolated test tab and its verified owned server were closed; the main API, worker and browser remain available.

## Final Linux package

All 30 native tests passed after the route extension. The opt-in production Runtime probe used the actual saved connection for four concurrent authenticated reads: session, bootstrap, access and ENG-5 all returned 200 in 285 ms. It performed four OS-vault reads, attempted no credential writes, and left the original configuration byte-identical. Logs: `work/desktop-build/deletion-native-tests.log`, `work/desktop-build/deletion-saved-connection.log`.

The final shared frontend was packaged into `desktop/target/release/bundle/deb/OpenTrack_0.1.0_amd64.deb`, 8,003,668 bytes, SHA-256 `b634700187aa548a9d44ebe6c762c5165fbf39a213f15042a28baf062b8cc871`. Package extraction, desktop-entry validation, URI forwarding and the expected bundle-marker-only binary difference passed. Atomic per-user installation produced the exact release executable with SHA-256 `e7e6d833cecc2c9c98c33696d4b47a66ccd5fd132a7ff1387ac7791072ebd679`. Receipt: `work/desktop-build/package-deletion-result.json`; build log: `work/desktop-build/package-deletion-release.log`.

The existing native process was preserved, so this installed version takes effect on its next launch. Native rendered interaction and visual acceptance remain unverified because native GUI inspection is unavailable; package/core/browser checks do not substitute for them. The complete-product goal remains active.
