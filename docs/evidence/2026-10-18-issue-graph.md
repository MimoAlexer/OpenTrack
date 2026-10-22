# Issue hierarchy, relationships and team moves

Implementation based on taxonomy checkpoint `10f9591`, executed on 18 October 2026. This record distinguishes focused checks from integrated and browser qualification. M03 and the complete product remain unfinished.

## Behavior

Issues support same-team parent/subissues, directed blocking links, normalized related work and canonical duplicates. Parent depth is limited to eight levels; blocking traversal is bounded at 10,000 distinct nodes. Links never change status. An active source may explicitly remove an unavailable historical target without seeing hidden identity or title.

The team-move preview reports incident-link blockers and reference repairs before moving one active, childless, fully unlinked issue. The command rechecks the signed review state, current authority and references. It preserves the issue UUID, allocates a new destination key and keeps the old key as an authorized alias. Historical receipt and activity reads retain both current and recorded-team authorization boundaries.

The connected UI provides keyboard-searchable relationship targets, parent/children and relative link directions, reviewed move repairs and dirty-editor handling. A late graph refresh rebases reference data without overwriting a draft. The graph resources have their own complete-projection validators; writes use source/target issue validators.

## Focused checks

Environment: Linux x86_64, PostgreSQL 18.6, Rust 1.98.1, Node 22.22.1. Integration suites create isolated test schemas with restricted runtime identities.

| Check | Observed result |
| --- | --- |
| App graph contract | Passed, 9.99 seconds: hierarchy depth/cycles, relationships, historical repair, capacity rejection, correlated child creation and mandatory-effect rollback. |
| App graph/move races | Passed, 7.14 seconds: observed opposite block/parent writers, direct-SQL block cycle, related normalization, both duplicate-root winner orders, opposite team moves and both private-grant removal/move orders. |
| App move contract | Passed, 10.82 seconds: preview repairs, blockers, aliases, stale review rejection, current/recorded-team receipt privacy, original-team history and atomicity. Status/label changes here are sequential stale-reference checks, not claimed concurrent races. |
| Actual 0011 to 0012 upgrade | Passed, 2.52 seconds: nonempty-label legacy create receipt/hash compatibility, inherited runtime grants, FORCE RLS, no added worker authority and real status replacement through the existing worker connection. |
| API graph contract and units | Passed: real database HTTP preconditions, CSRF, required/null fields, projection validators and authorized 304, dereferenceable relation Location, replay, atomic child creation, private move and alias denial. |
| Frontend | 129 tests across 12 files passed, including 20 graph interaction cases and three deferred-refresh draft regressions. TypeScript and isolated production build passed. These fixture checks do not replace the real browser workflow. |

Logs remain in ignored local `work/`: `test-graph-app-final.log`, `test-graph-races-final.log`, `test-graph-move-first.log`, `test-graph-upgrade.log`, `test-graph-api-first.log`, `clippy-graph-app-final.log`, `test-relations.log`, and `build-relations.log`. Committed tests and this record are the portable evidence; local log availability is not assumed elsewhere.

## Review fixes

Initial capacity qualification returned a timeout rather than the intended capacity conflict. The actual cold, unanalyzed schema chose an index that scanned every team edge on each recursive step. A redundant team predicate obscured the selective source key. Keeping the workspace, relation kind and globally unique source predicates preserves same-team scope through the endpoint foreign keys. The corrected trigger rejected the actual restricted-role, FORCE-RLS 10,001-node fixture in 47.726 ms, without raising timeouts or changing planner/RLS settings. The full graph contract then passed. Local before/after plans are in `work/graph-capacity-plan.txt`.

Review also added serialized key/alias namespace checks and shared child/parent event correlation. The frontend late-refresh bug now retains typed edits and presents a comparison when remote editable fields changed; obsolete or unmounted responses are ignored.

## Recovery preparation

The previous `10f9591` server and matching frontend were preserved with hashes before rebuilding. The verified local API/worker supervisor was stopped for backup. A private 187,574-byte PostgreSQL archive restored into isolated `opentrack_restore_0011_20261018`; all 30 table/sequence fingerprint entries, table owners/RLS/ACL and function owners/ACL/search paths matched, with source data stable. Restored migrations were exactly 0001–0011.

The preserved server then served its matching frontend on loopback 18971 against only that clone. Eight HTTP checks passed: runtime-only child credentials, free local port, readiness, exact assets, login, bootstrap, persisted issue identity/version and logout followed by 401. The temporary server stopped. Receipts are `work/postgres/restore-before-0012-result.json` and `work/restore-before-0012-http-result.json`. This proves this checkpoint's database/authority and HTTP recovery, not native desktop behavior or complete-product recovery.

## Integrated and browser qualification

The final shared `scripts/check.sh` passed: 71 Rust tests, 129 frontend tests across 12 files and 11 Python checks, plus formatting, all-target Clippy, generated-contract drift and the production build. Local log: `work/check-graph-final.log`. The qualified local launcher then applied migration 0012 and refreshed the restricted worker grants. Readiness returned 200; all twelve migrations report success. Runtime inspection confirmed the API has only its runtime database/setup credentials and the worker only its worker database credential. The main browser reopened ENG-5 with its original title, active shared label and assignee, plus the new connection controls. [Main workspace screenshot](2026-10-18-graph-main.png). Migrations 0001–0012 are now immutable.

Real browser verification ran against an isolated database cloned from the verified 0011 restore, migrated to 0012 and served with the graph binary and matching production assets on loopback 18972. The main development database was unchanged by this workflow:

- Created ENG-6 as a subissue of ENG-5 with the keyboard shortcut; both child and parent links appeared.
- Typed a parent description while connections were visible. Connection changes stayed disabled until the draft was saved, and the description persisted.
- Added ENG-5 blocks ENG-2 through the searchable picker. ENG-2 displayed the reverse Blocked by link, including after a full reload.
- Created a public Desktop QA destination through team settings; the password-confirmation flow retained the entered team name/prefix.
- The first move review correctly blocked ENG-6 because it still had a parent. Review connections returned to the issue. Explicit detach required confirmation.
- The next review exposed destination status, assignee, labels and visibility. Confirming moved ENG-6 to DQA-1 with the same UUID and recorded move activity.
- Opening the old `/issues/by-key/ENG-6` URL resolved to that same UUID and displayed DQA-1. Returning to ENG-5 showed the child removed and its independent blocking relationship retained.

Screenshots: [moved issue](2026-10-18-issue-moved.png), [connections](2026-10-18-issue-connections.png). These are the isolated browser verification workspace, not claims that its test team or test issue was added to the main workspace.

Public issue deletion/restoration, assignment cleanup/notifications, bulk/subtree moves, projects and full search remain separate unfinished work. F07-A3 cannot be completed from fixture-only deleted targets. Desktop work is being brought forward under ADR-017 and does not change these remaining obligations.
