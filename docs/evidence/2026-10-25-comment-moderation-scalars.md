# Comment moderation reason scalar input

25 October 2026. Bounded F11 input correction from
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`.

Removed only the two native UTF-16 `maxlength500` caps from edit and delete
moderation reasons. Rust retains the 500-scalar bound, author/manager permission,
required/trim/control rules and current comment representation ETag. Comment body
schema, ordinary-author commands and uncertain-outcome behavior are unchanged.

## Executed acceptance

One focused journey per engine used native sequential reason input, a fresh
synthetic member-authored comment and an authorized owner in a private team.
The same spec bytes ran against the retained pinned frontend and the corrected
frontend, with the same frozen backend and approved copied PID containment.

| Engine | Baseline edit/delete scalars | Corrected edit/delete scalars | Journey |
| --- | --- | --- | --- |
| Chromium | 250 / 250 | 500 / 500 | Passed |
| Firefox | 250 / 250 | 500 / 500 | Passed |
| WebKit | 250 / 250 | 500 / 500 | Passed |

Each 501-scalar edit and delete reached Rust and returned HTTP422
`validation_failed`. The full reason survived rejection; the edited body draft
also survived. The current comment and its exposed document, including their
versions, remained unchanged. Rejected deletion left the live comment visible.

Repair sent the exact 500-scalar reason with the same current `If-Match` and
committed once. Edit advanced comment and document versions once and retained
the reviewed body after reload. Delete advanced the comment once, hid its
document and persisted a tombstone after reload. Captured requests were exactly
rejected edit422, accepted edit200, rejected delete422 and accepted delete204.
Exact audit-row storage and the hidden deleted document's internal version were
not separately read and are not inferred from these responses.

## Checks, isolation and retained artifacts

- 20 focused comment unit tests passed, including scalar rejection/repair,
  required/whitespace reasons, ordinary-author commands without moderation
  reasons, current server permission flags and existing draft/auth/retry behavior.
- Both TypeScript targets passed (`npm run typecheck`).
- One final production build passed (`npm run build --prefix web`).
- The profile retained one worker, zero retries, 60-second test and 10-second
  expectation/action bounds, port18081 origin guards and disabled screenshots,
  traces and videos. Only these three cases were selected per phase.

Both browser namespaces were verified empty, exact monitors/API processes were
reaped, bounded private output drained and cleanup failures remained empty.
Browser environments omitted PG/database/operator/worker/setup service authority,
using dedicated synthetic fixture authentication. Private phase directories are
mode0700 and artifacts mode0600. The own database stop returned exit0; final
inspection found no extra process in the helper namespace, absent postmaster PID
and retained database data.

Spec SHA256:
`198ab88637b360581cc9374c7cf5b832c9c44f82673d4b961ded496f55050105`.
Backend SHA256:
`4cd979ed41ef326bbaf6ac1d7c74a5eda68cd62677de016287a9086b80af8c39`.
Containment SHA256:
`1088837b37933f07f42dd5f4184181dca1985d1d284bb1f5fcd1a89ea3dd7f23`.

[Machine-readable evidence](2026-10-25-comment-moderation-scalars.json) binds ten
source/check/caller hashes, all17 private runtime artifacts, all18 retained
frontend assets, exact source-map provenance and the unique private artifact
index. That index names both frontend roots and every result/scope/shutdown/stop
receipt with hashes. The exact pinned frontend was reused without rebuilding;
the earlier `af249b3` qualified assets/maps and both prior branches remain
preserved, without repeating their successful checks or resetting fixtures.

## Scope and limits

No schema, Rust, body schema or wire change is required. A frontend rollback
restores the native cutoffs while existing server validation remains authoritative.
Normal contained completion is verified; forced interruption was not exercised.
Physical IME/keyboard, native desktop, assistive technology, screenshots, global
matrices and broader F11/M04/product acceptance remain separate gates.
Independent exact-head review, hosted checks and integration remain lead-owned.
No push, PR, merge or issue closure was performed. The ongoing worker Goal
continues for scoped review findings and the next explicit assignment.
