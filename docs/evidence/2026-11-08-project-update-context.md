# Project update context qualification

Local candidate, 8 November 2026, based on `043cc454fd58774716daa0dd6878996f7c1dbf60`. X13/X17 implementation and these checks do not establish final integrated-main or complete-product acceptance.

Updates now capture dates with immutable, explicitly authored risk/decision notes. Legacy publications retain unknown context; body-only clients capture dates while leaving notes unspecified. The overview combines current planning information with the latest publication and its recorded attention notes. Comparison uses the immediately preceding snapshot even outside the feed page. No independent M18 decision objects or update editing API are introduced.

## Executed checks

| Check | Result |
| --- | --- |
| Domain project tests | 4 passed, including legacy request serialization/receipt defaults, explicit empty versus unknown notes and scalar/identity bounds. |
| `project_update_context_contract` | 5 PostgreSQL tests passed: immutable context, conditional/idempotent/lost-response replay, genuine schema-29 upgrade and unchanged legacy receipts, malformed stored JSON rejection, private-team/role/session loss, observed publication/revocation lock order, tied timestamps and maximum Unicode byte-bounded pagination. |
| API `projects_contract` | All 9 passed, including actual Axum/PostgreSQL optional-validator responses, replay, canonical detail/feed/ETag agreement and absent edit route. These are in-process HTTP contracts, not network listener checks. |
| Existing project lead/document/milestone upgrade tests | 6 passed. |
| Frontend | 910 tests across 83 files passed; both TypeScript targets and production build passed. |
| Native | 14 protocol tests and all 22 HTTP transport boundary tests passed; strict native Clippy passed. Exact project-update POST requests allow 128 KiB; only the exact GET feed receives 4 MiB. Update detail and unrelated responses retain 1 MiB. |
| Rust/contract gates | Strict domain/app/API all-target Clippy, formatting, generated-contract consistency, dependency boundary checks and 10 boundary-validator tests passed. |
| Real browser workflow | [All 3 cases passed](2026-11-08-project-update-context-browser.json), one each in Chromium, Firefox and WebKit against the built frontend and local API/PostgreSQL. |

The browser workflow used keyboard note creation, a concurrent target-date edit producing 412, explicit current-context review, publication, Overview navigation, reload persistence and reads proving old snapshots did not change. No timeouts, retries or test limits were loosened.

The first [two](2026-11-08-project-update-context-browser-launch-1.json) [runs](2026-11-08-project-update-context-browser-launch-2.json) passed Chromium/Firefox but could not launch WebKit. Missing official Ubuntu libraries were extracted into disposable local browser storage; the bundled WebKit wrapper required its private library directory to resolve them. The unchanged three-engine workflow then passed. Those launch failures remain retained separately from product behavior.

Independent review identified an inherited ambiguity bug: after a lost publication response, a 429 retry could unlock the composer and allocate a new key. A focused regression first failed, then passed after preserving the original unresolved body, note IDs, validator and idempotency key through that response. The 429 is a simulated transport response, not a claim that the project-update handler emits it today.

[A PostgreSQL plan](2026-11-08-project-update-predecessor-plan.json) with 5,000 synthetic rows used `project_updates_feed` for the outer 26-row query and 26 predecessor index lookups returning at most one row each. Fixture writes were rolled back. Maximum valid Unicode requests stayed below 128 KiB; feed pages counted actual encoded JSON, including predecessor notes, stayed below 3 MiB and traversed every row without loss or duplication. This is bounded-query evidence, not a production throughput benchmark.

## Migration and remaining qualification

Migration 0030 appends date/capture/note columns and validation without guessing historical values. Existing append-only and workspace-RLS protections remain in force; application guards reauthorize private team access before receipt replay. Deploy matching API, frontend and native changes together. Rollback requires a matching pre-upgrade database backup and binaries; no destructive down-migration or full recovery rehearsal is claimed.

Hosted composition, physical input, assistive technology, installed native GUI and full recovery qualification remain separate gates. X13/X17 ledger rows remain In progress until integrated acceptance is recorded.
