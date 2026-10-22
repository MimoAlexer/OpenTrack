# Collaboration browser acceptance infrastructure

Date: 20 October 2026. This record describes checked-in coverage and its executed qualification. **All 18 saved-browser checks passed** on `ad18634252559de7a2ee55e3cb4a19a7ad6554dd` in [Docker run 39466834785](https://github.com/MimoAlexer/OpenTrack/actions/runs/39466834785/job/118783299829), completing at 17:45:21 UTC. The nested issue transfer-dialog shortcut fix and corrected control selectors were included.

## Real runtime and boundaries

The existing `Verify Docker runtime` workflow keeps its current scope decision, required job names and native amd64/arm64 smoke checks. Its amd64 job additionally installs exact `@playwright/test` 1.64.1 and the matching official Chromium, Firefox and WebKit builds. The already-built Docker application serves its production frontend and a real PostgreSQL database. No frontend API responses are mocked. Test setup uses the installed credential helper and ordinary owner setup/login, issue, document and template APIs with role-based synthetic fixtures.

`scripts/browser-acceptance.py` requires Linux GitHub Actions and the concrete run/attempt. It accepts only `opentrack-browser-<run>-<attempt>-amd64` on HTTP loopback port 18081. Before startup or cleanup it validates the fully resolved Compose configuration: project-scoped non-external volumes, no host-directory mounts, loopback-only application publication and no database port. It rejects existing project containers/volumes before setup. The normal installation and the existing smoke/HTTPS project use different project names, volumes and ports. Cleanup runs both in `finally` and in an unconditional guarded CI step. Images are reused from the same native Docker build; the browser project does not rebuild or fetch unrelated runtime images.

Setup tokens and random passwords remain in captured pipes and child process environment. Session state is shared in memory only. The custom reporter prints only test titles, browser names, outcomes, durations and relative source locations. It excludes raw request/error logs, headers and response bodies. The only public receipt is `work/browser-acceptance/receipt.json`; no artifacts are automatically uploaded. Traces, videos and screenshots are disabled. Any future screenshot evidence requires role-based fixture pixels and a separate visual/privacy review before publication.

The runtime restriction is intentional: local application startup was blocked by automatic approval review for this qualification. Local type checking, test enumeration, build and pure/component tests do not launch the API. The browser runner must not be used to bypass that restriction.

## Coverage passed in the hosted runtime

Each of these six workflows runs in all three browsers, giving 18 enumerated checks:

| Area | Real saved workflow |
| --- | --- |
| F12 native document transfer | Reviewed JSON import, durable save, exact export, toolbar undo/redo, browser reload, issue and user reference IDs and authorized rendering; Markdown reference-loss warnings. |
| F12 Markdown and CSP | Actual bundled module worker under the application's CSP, structured import/save/export/re-import/reload, inert script/image source, rejected JavaScript link, rejected blob worker verified by a browser policy event. |
| F21 keyboard | Actual key presses in comment editor, durable comment submission, saved character-shortcut disable and reload, visible create control and Mod+K palette alternative, G then M sequence and 800 ms expiration. |
| Nested modal guards | Ctrl/Meta+Enter in existing-issue export/import fields cannot mutate issue metadata; nested import cannot create an issue; Escape closes only the upper dialog and retains the draft. |
| F23 templates | Real pattern template and preview, resolved team/date title, explicit cancel/merge/replace with edited title/body/priority, durable creation and independent title/body after the template changes. |
| F11 activity | 27 durable property updates plus UI status save, attributable before/after facts, real 25-event cursor pages, all 29 events retained across grouping/pagination, expandable individual timestamps and reload. |

The composition check dispatches synthetic composition events in the browser. It verifies the application's guard only and **does not prove a real OS IME**. Physical international keyboard layouts, screen-reader browse/navigation behavior, native desktop/webview flows, screenshot review, two-client conflicts and broader release acceptance remain open. No feature, issue or milestone should be marked complete from this infrastructure commit alone.

## Checks executed locally

Environment: Windows, Node 24.14.1, npm 11.11.0, Python 3.12.10.

| Check | Result |
| --- | --- |
| Pinned dependency installation with lockfile integrity | Installed `@playwright/test` 1.64.1; npm audit reported zero vulnerabilities in the resolved 142-package graph. |
| `npm --prefix web run typecheck` | Application and browser-suite TypeScript passed. |
| `npm --prefix web run test:browser:list` | 18 tests discovered; no browser or application runtime launched. |
| `npm --prefix web run build` | Production frontend built, including separate Markdown worker. Existing large-chunk warning remains. |
| `npm --prefix web test` | 59 files, 472 tests passed; these are existing unit/component tests, not saved browser proof. |
| `python scripts/test-browser-acceptance.py` | Three pure boundary tests passed, including non-CI/non-loopback/project/volume/port rejection cases. |
| `python scripts/test-ci-scope.py` | 14 scope tests passed; existing required-status semantics retained. |
| Workflow YAML parse and `git diff --check` | Passed. |

No Docker application, database or browser was started locally. Record the exact hosted commit/run and its receipt separately after observing the CI result.

Tooling references inspected 20 October 2026: [official Playwright configuration](https://playwright.dev/docs/test-configuration), [official browser projects](https://playwright.dev/docs/test-projects), [official CLI](https://playwright.dev/docs/test-cli), and the [official package](https://www.npmjs.com/package/@playwright/test). The npm registry resolved exact version 1.64.1 and its integrity into the committed lockfile.
