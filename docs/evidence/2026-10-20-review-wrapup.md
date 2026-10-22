# Recovered review checkpoint

Date: 20 October 2026. Scope: the existing collaboration increment and two completed qualification commits recovered from an interrupted review. [Session summary](../SESSION_SUMMARY_2026-10-20.md) describes the complete recovered scope; [PR #121](https://github.com/MimoAlexer/OpenTrack/pull/121) is the publication path.

## Recovered work and correction

- Integrated saved-attachment URL revocation tests and their prior execution record.
- Integrated the guarded Docker browser acceptance harness, which runs six saved workflows in each of Chromium, Firefox and WebKit.
- Corrected the schema-18 upgrade test's stale hardcoded migration count. It now checks against the latest bundled migration while retaining its legacy-fixture, backfill and idempotency assertions.
- Preserved an outstanding native `AppHandle` import cleanup from the earlier checkout.

The previous `verify` job failed at the migration-count assertion on `c39474e`; its Docker AMD64/ARM64 and native-contract jobs passed. Those successes are historical results for that head and are not substituted for the recovered branch's checks.

## Checks executed during recovery

| Check | Observed result |
| --- | --- |
| Frontend unit/component suite | 474 tests passed across 60 files with two workers. |
| Locked frontend dependency installation | Passed. |
| Application and browser-suite TypeScript | Passed. |
| Production frontend build | Passed, including the parsing worker; existing large-bundle advisory remains. |
| Browser test enumeration | 18 checks discovered; no local browser runtime result is implied. |
| Browser runner boundary tests | Three tests passed. |
| Real PostgreSQL mention/migration regressions | Both tests in `mentions_races` passed, including schema-18-to-current upgrade. |
| Selected integrated backend suites | 43 tests passed: 19 application/PostgreSQL, 17 API unit and seven attachment revocation HTTP/PostgreSQL tests; none ignored. |
| Strict workspace Clippy | Passed with warnings denied. |
| Workflow scope and desktop entrypoint guards | 14 scope tests and six desktop entrypoint tests passed. |
| Rust workspace and desktop formatting | Passed. |
| Summary/evidence links and Git whitespace | Passed. |
| Staged privacy scan | 769 UTF-8 files and outgoing commit metadata checked; no known personal identifiers or credential patterns found. No new image/binary evidence was added. |

Full hosted workspace/PostgreSQL, Docker and native-contract results must be checked on the published head. The new browser suite's actual execution is part of the AMD64 Docker job. [PR checks](https://github.com/MimoAlexer/OpenTrack/pull/121/checks) report the authoritative result; this checkpoint does not predeclare a passing run.

## First hosted browser execution

[Docker run 39463681948](https://github.com/MimoAlexer/OpenTrack/actions/runs/39463681948), on `6ce92e30e61ac5730d4ed72888c07d67d7c75a37`, passed ARM64 runtime qualification and the AMD64 runtime/HTTPS checks. Browser acceptance passed three of 18 checks: the saved keyboard workflow passed in Chromium, Firefox and WebKit. The other workflows failed while locating wrapped form controls. No passing browser qualification is claimed from this run.

An isolated static-markup Chrome reproduction confirmed that exact label-text selectors matched zero controls when the label also wrapped select options or textarea content, while exact accessible-role/name selectors matched the intended controls. The harness now uses exact named comboboxes/textboxes and observes parsing-worker creation without leaving an orphan timeout promise. Saved-state and security assertions remain unchanged. Application/browser TypeScript, discovery of all 18 tests, the static control reproduction and whitespace checks passed after this harness-only correction; a new hosted run is required.

## Completed hosted implementation verification

The [implementation verification job](https://github.com/MimoAlexer/OpenTrack/actions/runs/39463681987/job/118769421082) completed **successfully** at 17:29:55 UTC on `6ce92e30e61ac5730d4ed72888c07d67d7c75a37`. It passed **178 Rust tests with none ignored**, **474 frontend tests across 60 files**, formatting, strict lint, real PostgreSQL contracts, generated-contract drift checks and the production frontend build. No backend failure or stalled test was found.

The overall workflow was marked cancelled when the next push superseded it; the verification job's recorded conclusion is success. Keep that distinction and exact source revision when citing the result. The subsequent browser-selector changes and this documentation still require the latest protected-branch checks. The PR remains open while that rerun is pending; the failed initial browser run is not counted as passing acceptance.

## Privacy and limits

The subsequent [Docker rerun](https://github.com/MimoAlexer/OpenTrack/actions/runs/39466834785/job/118783299829) on `ad18634252559de7a2ee55e3cb4a19a7ad6554dd` passed all **18/18** saved-browser workflows across Chromium, Firefox and WebKit at 17:45:21 UTC. Both native Docker architectures and the native contract check passed for that head. The first failed browser run remains part of the record; its selector failures were corrected rather than counted as successful acceptance.

Only repository source, tests and sanitized documentation are included. New commits use role-based contribution identities. Local database files, tokens, raw run logs, support screenshots and other private working files remain excluded. No new screenshot or binary evidence is published in this wrap-up.

This checkpoint does not complete the product, remaining IME/accessibility/native GUI gates, installed-scanner/full-disk/populated-blob recovery, notification delivery or removal of retained hosted history. Existing [attachment](2026-10-20-attachment-revocation.md) and [browser](2026-10-20-browser-acceptance.md) evidence retain their narrower boundaries.
