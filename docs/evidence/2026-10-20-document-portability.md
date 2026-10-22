# Document JSON and Markdown portability

Date: 20 October 2026. Scope: the document portability slice of
[M04-T02 / issue 21](https://github.com/MimoAlexer/OpenTrack/issues/21), based on
`3910a869bfc4ad2be702585caf274adea40cc783`. This record describes the focused
commit containing this file. It does not complete M04-T02 or F12-A3.

## Behavior

The shared rich editor offers text import/export for its current draft. A
read-only issue description offers export only. OpenTrack document JSON uses an
explicit format/schema envelope and retains validated document structure and
reference IDs. It never includes attachment bytes, access grants, discussion
history or workspace state. Scope and attachment restrictions are visible before
copying or replacing content.

Markdown export/import preserves the supported formatting, nested lists, task
states, quotes, safe links, code and literal text. Conversion warnings identify
unrepresentable details: reference identities, attachments, empty paragraphs,
trailing hard breaks, multiline inline code, code carriage returns, combined
decorative/code marks, oversized list starts and escaped output exceeding the
Markdown import limit. GitHub-style task/strike syntax is identified explicitly.
JSON remains the fidelity format for these cases.

Import requires a successful review followed by **Replace current draft**.
Editing the input or changing format invalidates the review; closing aborts
outstanding work. The normal editor save/conflict path owns persistence. A failed
editor application leaves the review open with an error. Import remains undoable
through the real editor history.

Markdown parsing runs in a disposable module worker with a five-second deadline,
256 KiB UTF-8 input bound, depth/node limits and strict output validation. There
is no main-thread parsing fallback. HTML, images and unsupported blocks remain
inert source with warnings; unsafe URLs are rejected. Plain names/keys never
become application references. Native input has a 257 KiB envelope limit and
independently enforces the existing 256 KiB body limit.

## Executed checks

Environment: Windows x64, Node 24.14.1, npm 11.11.0, locked frontend dependencies.

| Check | Result |
| --- | --- |
| `npm ci --no-audit --no-fund` in `web/` | Passed. |
| `npm test` in `web/` | 424 tests across 56 files passed. |
| `npm test -- --maxWorkers=2 src/documents/portability.test.ts src/documents/portability-markdown.test.ts src/documents/portability-worker.test.ts src/documents/DocumentPortability.test.tsx src/documents/RichTextEditorPortability.test.tsx` | 43 focused tests across five files passed. |
| `npm run build` in `web/` | Typecheck and production build passed; emitted a separate module worker. Existing large-main-chunk advisory remains. |
| Production worker in Chromium 155 under the API CSP | Supported heading/marks/task content returned structured JSON; hostile HTML stayed literal; unsafe links were rejected; terminating a large parse produced no late response. Zero CSP violations and zero final browser-console errors/warnings. |
| `git diff --check` | Passed before commit. |

The browser check served the actual emitted
`portability.worker-DEvlQVr4.js` from an isolated, loopback-only static fixture.
SHA-256: `458380312c22af14633f3c83fd1d7886573bafb43c2fe6634577eb385ff5bf5a`.
The response header matched `crates/opentrack-api/src/lib.rs`, including
`default-src 'self'` and `script-src 'self'`. It contacted only local fixture
resources; no API or database was involved. The fixture was stopped afterward.
An initial scratch harness string-escaping error was corrected before these
measurements; no application change was needed for that setup error.

## Failures found and fixed

- The initial passing corpus missed marked emoji/punctuation next to letters,
  marked tabs, literal repeated newlines and adjacent same-kind lists. Regression
  cases reproduced lost marks or changed block structure. Entity boundaries,
  literal newline encoding and alternating list markers now preserve those cases.
- Trailing hard breaks previously became visible backslashes; code carriage
  returns and oversized escaped output had no limitation notice. Export now
  omits trailing hard breaks with an explicit warning and reports the other
  conversions rather than claiming a faithful Markdown round trip.
- The real Tiptap test reproduced a JSON import failure for code combined with
  bold, although the existing document schema accepts that combination. The
  editor now permits schema-approved combined code marks. The replacement,
  formatting and undo regression passes, and apply errors remain recoverable.
- Integration review reproduced a valid 262,134-byte JSON body whose link
  defaults exceed the unchanged editor size limit after normalization. Tiptap
  returned success even though its transaction filter preserved the old draft.
  Import now checks the normalized editor representation before dispatch and
  verifies the resulting document instead of trusting the command result. The
  real-editor regression retains the draft and review on failure, emits no
  unapplied change, and successfully retries a smaller replacement. A separate
  rejecting-filter test covers the actual-state check when the command returns
  true. The focused five-file command above passed **45 tests** after this fix;
  `npm --prefix web run typecheck` also passed. No size limits were increased.

## Remaining acceptance and rollback

The local API startup was rejected by automatic execution approval review.
No API startup retry or workaround was used. Therefore this increment does not
claim saved-document import/export/reload acceptance, API-backed reference
validation after import, two-client imported-draft conflict behavior, full OS IME
interaction, zoom qualification or native webview worker support. Existing
component/worker tests and the limited production-worker browser result are
distinct from those gates. Issue 21 and F12-A3 remain open.

No migrations, backend routes or document schema versions change. Deployment
must include the emitted worker asset alongside the frontend bundles. Rollback
removes the transfer controls; existing saved schema-1 documents remain durable.
Older editor builds retain their pre-existing limitation around combined code
marks, so do not describe an older editor as supporting every schema-valid body.
