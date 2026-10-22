# Started issue batch — 22 October 2026

Execution scope: complete the already-started work, publish and merge its verified pull requests, then stop assigning backlog issues. Ten distinct agents contributed to nine existing issues; issue #18 received separate implementation and keyboard-qualification increments. The complete product remains unfinished.

## Implementation and evidence

| Existing issue | Pull request | Delivered behavior and evidence |
| --- | --- | --- |
| #3 Workspace boundaries | #124 | Executable locked Cargo graph/edition guards and actual internal-audience rejection at web/native user ingress. [Evidence](2026-10-22-workspace-boundaries.md). |
| #4 Typed settings | #121 | Source-aware redacted diagnostics, strict versions, safe secret-file handling and fail-before-listen process checks. [Evidence](2026-10-22-typed-settings.md). |
| #5 Contract slice | #127 | Strict HTML negotiation, reserved-path failures, health caching and actual emitted frontend delivery through the real router. [Evidence](2026-10-22-contract-slice.md). |
| #6 Design primitives | #123 | Theme/control contrast, bounded pickers, stable active choices, dialog boundaries and focus restoration. [Evidence](2026-10-22-design-primitives.md). |
| #12 Identity UI | #129 | Login/error focus, workspace rename/switch, account-scope clearing, invitation and member feedback. [Evidence](2026-10-22-identity-ui.md). |
| #18 Keyboard and palette | #122, #130 | Authorized issue search, reference-only session recents, stale/deleted result guards, composition/modifier/repeat guards and real revoked-team concealment. [Palette](2026-10-22-palette-issues.md), [keyboard qualification](2026-10-22-keyboard-qualification.md). |
| #27 Milestone model | #125 | Scoped ordered outcomes, assignment, live progress, empty-scope semantics and safe detach on deletion. [Verification](../MILESTONES.md#verification). |
| #28 Cycle semantics | #128 | Exclusive local dates, concurrent overlap exclusion, immutable snapshots/reports, explicit close and selective resumable carry-forward. [Evidence](2026-10-22-cycles.md). |
| #29 Planning screens | #131 | Connected milestone and cycle screens, exact accessible history table and reviewed carry-forward recovery. [Guide](../PLANNING.md), [qualification](2026-10-22-planning-ui.md). |

All ten implementation PRs in this table are merged. Each exact reviewed head passed all four configured checks. PR #126 separately reconciled the settings acceptance ledger; this final documentation update records the combined batch without starting another issue.

## Verification and corrections

The pull requests retain the exact tested heads and configured CI results. Required merge checks are `verify`, `native-contracts`, `runtime (ubuntu-24.04, amd64)` and `runtime (ubuntu-24.04-arm, arm64)`. Review and merging use the same immutable head. Merge commits use role-based synthetic author and committer metadata.

Local verification used isolated PostgreSQL 18 schemas and a separate disposable browser database, production frontend assets and actual Chromium, Firefox and WebKit. Identity and keyboard browser evidence initially used schema 0022; planning uses the combined 0024 API. Those records must not be misrepresented as native GUI, physical keyboard/IME or screen-reader checks. Credential-bearing environment files and raw logs remain private and untracked.

The final combined frontend at `9a374421df561fc26756d329a0ccb5508d7a0804` passed 542 unit/component tests across 65 files, application/browser type checks and a production build. The saved planning, Projects, palette and keyboard run passed all 39 cases: 13 in each browser engine. Independent review's three carry-forward findings were fixed and rechecked. Representative light, dark and mobile screenshots were inspected; all eight reviewed planning captures contain no text/EXIF metadata chunks.

Failures were retained with their corrections: isolated per-worktree Cargo targets resolved shared-artifact interference; the Docker worker rejected extension helpers in the application schema, so migration 0024 isolates the trusted extension without relaxing the worker gate; realistic full-scope reads were corrected under the unchanged statement timeout; picker key propagation and held Enter were reproduced and fixed in real browser engines. Planning review additionally checks in-flight receipt retention, stale retry review and read-only operation permalinks.

The source privacy pass found no known local identifiers or provider-token patterns in changed text. Commit metadata uses role identities and synthetic email domains. Screenshot pixels and metadata were reviewed separately. These checks do not claim an exhaustive security audit.

## Remaining acceptance and execution boundary

Issues #12 and #18 retain explicit native, physical-input and assistive-technology gates. Their tested implementation increments can merge while those issues remain open. No NVDA, VoiceOver, Orca or physical OS IME result is fabricated.

No new backlog assignment follows this batch. Automatic recurrence/daily scheduling, boards, timelines, retrospectives and wider product/release qualification remain the existing roadmap, rather than newly started work.

The repository was recreated with a clean root history. Historical documents preserve their earlier measurements, but their old PR numbers can now collide with current PRs. This table identifies the current batch; it does not reinterpret historical checks as current acceptance. Current account-plan limitations prevent the requested branch protection, so all configured checks are enforced manually before each merge.
