# Template Unicode input boundaries

25 October 2026. Bounded issue #24 / F23 / M04 template-editor increment from
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`.

Removed the two native UTF-16 `maxlength` caps from template name and issue title.
The existing Rust limits remain authoritative: 100 Unicode scalars for the name
and 256 for the title. Required-name, trimming, literal/pattern mode, server
diagnostics, conflict review, uncertain retry and permission behavior are retained.

## Executed browser evidence

The same focused spec bytes ran against independently retained baseline and
corrected frontend bundles, using one unchanged pinned backend binary and fresh
synthetic private teams in the helper's own database. Input used native sequential
typing. The original runs remain retained separately; the final evidence uses the
additional runs under the approved browser PID containment.

| Engine | Baseline name/title scalars | Corrected name/title scalars | Corrected journey |
| --- | --- | --- | --- |
| Chromium | 50 / 128 | 100 / 256 | Passed |
| Firefox | 50 / 128 | 100 / 256 | Passed |
| WebKit | 50 / 128 | 100 / 256 | Passed |

Each final journey verified create, edit and full reload through the real API.
Name101 and title257 each reached Rust and returned `validation_failed` HTTP422
on both create and edit. Both draft fields survived every rejection; rejected
creates left the directory empty, and rejected edits left the full saved
definition and version unchanged. Repair produced exactly one successful create
and one successful edit, one stored template, and version `1` to `2`.

Both contained phases verified the browser namespace empty, exact monitor and API
processes reaped, private output drained and no cleanup errors. Browser processes
received only dedicated fixture authentication; database and service authority
were omitted. Final inspection of this helper's network namespace found only its
PostgreSQL processes and the inspecting process. Artifacts remain private with
mode0600 under mode0700 phase directories. Original fixtures and failures remain.

## Focused checks and immutable bytes

- 39 template unit tests across four files passed, including existing preview,
  pattern-mode, conflict, uncertain same-key retry and access regressions, plus
  scalar draft/repair and required-name/trim coverage.
- Both TypeScript targets passed (`npm run typecheck`).
- One final candidate production build passed (`npm run build --prefix web`).
  A separate baseline bundle was retained before changing the editor.
- The existing three-engine profile kept one worker, zero retries, the 60-second
  test timeout, 10-second expectation/action timeouts and port18081 origin guard.
  Traces, screenshots and videos stayed disabled.

Spec SHA256:
`b4f22dcd46f22bed0b32c8c6899de93a00f854dd43c35481db2147a8ecc793c1`.
Backend SHA256:
`4cd979ed41ef326bbaf6ac1d7c74a5eda68cd62677de016287a9086b80af8c39`.
Approved containment SHA256:
`1088837b37933f07f42dd5f4184181dca1985d1d284bb1f5fcd1a89ea3dd7f23`.

[Machine-readable evidence](2026-10-25-template-scalar-inputs.json) records exact
source/test/check hashes, all four retained run receipts, observed scalar counts,
write/version assertions, cleanup receipts and private artifact hashes.

## Scope and limits

No schema, API, storage or Rust changes are required. A frontend rollback restores
the native cutoff; existing scalar-valid saved definitions remain supported by
the unchanged server.

These feature journeys verify normal completion and namespace emptiness, without
exercising forced cancellation. Earlier session-cleanup runs retain that explicit
limitation. Physical IME, native desktop, assistive technology, screenshot review,
global browser matrices and broader template expansion remain separate gates.
Issue #24, F23, M04 and whole-product completion are not claimed. Independent
review, all four hosted checks and main integration remain lead-owned gates.
