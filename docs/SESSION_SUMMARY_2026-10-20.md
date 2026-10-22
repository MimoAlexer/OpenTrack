# OpenTrack review and recovery summary

Date: 20 October 2026. This checkpoint summarizes the repository review, implementation and recovery work. It is a technical handoff with synthetic identities only; private logs, credentials, local account paths and support correspondence are excluded.

## Latest continuation: Projects

Pulled the reviewed remote history, preserved the older local history separately, and continued from the recovered collaboration branch. PR #121 passed all protected checks and was merged before publishing the Projects increment.

Nine delegated workstreams covered backend, shared UI, native routes, two independent reviews, browser regression coverage, documentation, prior-PR qualification and database recovery. The integrated result, merged in PR #122 after all required checks passed, adds a team project directory, rich briefs, leads and dates, lifecycle and manual health, issue membership and progress, immutable authored updates, and reviewed archive/restore and conflict flows.

The running browser application was checked against saved data upgraded from schema 21 to 22. Project creation, formatted text, dates, issue links, 50% progress, authored snapshots, concurrent-edit recovery, completion acknowledgement, archive/restore and reload passed. Native core qualification passed 47 tests. A separate restored database retained the original issues/documents and enforced RLS before and after migration. The full local check passed 183 Rust tests, 495 frontend tests and 12 helper checks. Hosted Docker qualification passed on both architectures with all 27 browser cases passing. See [PR #122](https://github.com/MimoAlexer/OpenTrack/pull/122), [Projects evidence](evidence/2026-10-20-projects.md) and the [usage guide](PROJECTS.md).

Next planning work is independent project-document behavior, milestones, cycles and timeline scheduling. These remain unfinished, alongside the previously documented M04 interaction and recovery gates. The new screens are a working Projects increment, not completion of M05 or the whole product.

## Work already merged

- Reconciled the implementation plan, requirements ledger and GitHub backlog, then resumed work on the collaboration milestone. The complete product remains unfinished.
- Fixed vendored-source verification on Windows, qualified native Windows storage/runtime behavior, and standardized migration line endings.
- Added authorized cross-team issue references: documents store IDs and resolve visible details under the reader's current access.
- Added persistent Docker Compose installation with separate database, API, worker and initialization responsibilities. Native AMD64 and ARM64 qualification covered credentials, saved data, attachments, worker behavior, shutdown and local-CA HTTPS. [PR #115](https://github.com/MimoAlexer/OpenTrack/pull/115) and [documentation follow-up #116](https://github.com/MimoAlexer/OpenTrack/pull/116) are merged; [Docker evidence](evidence/2026-10-20-docker.md) records the limits.
- Added protected-main checks and neutral repository fixtures/contribution metadata in [PR #118](https://github.com/MimoAlexer/OpenTrack/pull/118). Historical cleanup was performed separately; removal of retained hosted copies is not confirmed by a clean current tree.

## Collaboration increment

[PR #121](https://github.com/MimoAlexer/OpenTrack/pull/121) collects the following work:

| Area | Implemented behavior |
| --- | --- |
| Document transfer | Reviewed JSON and Markdown import/export, bounded worker parsing, conversion warnings, explicit replacement, undo, and preservation of rejected imports for correction. |
| Activity history | Actor attribution and captured before/after facts, atomic activity/event writes, safe historical projections, bounded grouping, pagination and stale-response guards. |
| Templates | Explicit opt-in team/date title substitutions with bounded preview, preserved literal templates and compatible historical receipts. |
| Keyboard controls | Persisted caller/workspace shortcut preferences, version-conflict review, visible alternatives, guarded navigation sequences and isolation of nested-dialog save shortcuts. |
| Native client | Authorized issue-reference lookup and hosted native broker/API contract verification. |
| Attachment access | PostgreSQL-backed saved-URL replay tests across session/account/workspace/team revocation and historical-content access. |
| Browser qualification | An isolated Docker CI harness and 18 real-browser workflows across Chromium, Firefox and WebKit, covering saved document transfer, templates, keyboard behavior and activity history. |

Migrations 0020 and 0021 add template title modes and user preferences. Earlier migrations remain immutable. API and worker startup require the combined schema. Rollback requires a matching database backup and compatible application binary; there is no automatic down-migration.

## Recovery and verification

The previous review stopped after a connection failure during context compaction. Its published collaboration branch was intact. Two completed local test commits were recovered into that branch: attachment revocation coverage and the Docker browser acceptance harness. The earlier main checkout also retained partial copies of already-integrated feature work; those are not a separate product increment.

The previous collaboration checkpoint recorded 472 frontend tests in 59 files, TypeScript and production builds, generated-contract checks, workflow guards and focused PostgreSQL/native tests. The attachment qualification separately passed seven tests with 800 denied URL replays and authorized byte/hash controls. These historical results are linked in [batch evidence](evidence/2026-10-20-collaboration-batch.md) and [attachment evidence](evidence/2026-10-20-attachment-revocation.md).

The recovered PR had passing AMD64, ARM64 and native-contract checks, but its full verification failed because a schema-upgrade test still expected 19 migrations after migrations 0020/0021 were added. The recovery corrects that expectation and reruns verification. The new browser harness is subject to actual hosted execution; enumerating its 18 tests is not a passing browser result. Final executed results belong in [recovery evidence](evidence/2026-10-20-review-wrapup.md).

The recovered implementation verification subsequently passed on `6ce92e3`: 178 Rust tests, 474 frontend tests, strict lint, PostgreSQL/generated contracts and the production build. The first hosted browser run passed its keyboard workflow in all three engines but exposed test-locator mismatches in the remaining workflows. Those harness selectors were corrected. The [hosted rerun](https://github.com/MimoAlexer/OpenTrack/actions/runs/39466834785/job/118783299829) then passed all 18 saved-browser checks on `ad18634252559de7a2ee55e3cb4a19a7ad6554dd`. This confirms the browser workflows listed above; it does not complete the remaining qualification gates.

## Remaining work

- Complete physical IME, zoom, assistive-technology and native GUI/webview qualification. Synthetic composition events and component tests do not establish these outcomes.
- Finish installed-scanner, actual full-disk and populated-blob backup/restore qualification. Requests after committed revocation do not prove cancellation of an already-streaming download.
- Continue unfinished M04 acceptance, independent project documents, milestones, cycles, timeline scheduling, search, realtime and notification delivery through the full dependency plan.
- Preserve the remaining integration, AI/isolated-runner, expansion, performance, recovery and release gates. No complete milestone or full-product completion is inferred from this checkpoint.
- Keep retained-history cleanup separate from current-tree privacy verification; confirmation remains outstanding.

The [requirements ledger](REQUIREMENTS_STATUS.md) remains the source of acceptance status. This summary wraps up the recovered work without representing the broader product as finished.
