# Workflow and label scalar input

25 October 2026. Bounded F08/F09 input correction from
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`.

Removed exactly three native UTF-16 `maxlength` caps: status name, label name and
Search labels. Existing Rust validation still owns the 100-scalar bounds, required
and trim semantics, and diagnostics. Categories, defaults, ordering, label colors
and scope, restore-name repair, version preconditions and uncertain retries retain
their existing behavior.

## Executed acceptance

Two journeys per engine used native sequential input, the real API and the same
before/after spec bytes. Each journey created a fresh synthetic private team in
the existing isolated worker database. The original pinned frontend was already
retained; its source maps match both baseline production files, avoiding another
baseline build. One final candidate build supplied the corrected frontend.

| Engine | Baseline status / label / query scalars | Corrected boundaries | Journeys |
| --- | --- | --- | --- |
| Chromium | 50 / 50 / 50 | 100 / 100 / 100 | 2 passed |
| Firefox | 50 / 50 / 50 | 100 / 100 / 100 | 2 passed |
| WebKit | 50 / 50 / 50 | 100 / 100 / 100 | 2 passed |

Status and label names preserved 100 supplementary scalars through create, edit
and full reload. Their 101-scalar create and edit requests reached Rust and
returned HTTP422 `validation_failed`. Rejected creates added no entity; rejected
edits left saved data and versions unchanged. Drafts remained editable. Repair
produced one accepted create and one accepted edit, with exactly one version
increment per accepted write. Workflow requests carried current aggregate ETags;
all pre-existing statuses, default flags, categories and positions were preserved.
Label identity, scope and color remained unchanged, with version `1` to `2`.

Label search sent the complete 100-scalar query and found the saved label. Adding
the 101st scalar reached the existing HTTP422 diagnostic, retained the typed query
and cleared the result list. Captured directory requests showed no truncated or
unfiltered fallback. Repair returned the same filtered saved label without writes.

Both phases used the approved copied PID containment and the same frozen backend.
Receipts verify empty browser namespaces, reaped monitor/API processes, drained
bounded private output and no cleanup errors. Browser environments omitted all
PG/database/operator/worker/setup service authority and retained only dedicated
fixture authentication. Each phase directory is mode0700 and its artifacts are
mode0600. Final shutdown inspection found no extra process in the helper network
namespace; PostgreSQL was stopped with its data retained.

## Checks and retained evidence

- 38 focused Workflow/Taxonomy unit tests across two files passed, including
  existing conflict, restore-name repair, permission and uncertain-command cases.
- Both TypeScript targets passed (`npm run typecheck`).
- One final production build passed (`npm run build --prefix web`).
- The existing profile retained one worker, zero retries, 60-second test and
  10-second expectation/action timeouts, port18081 origin guards, and disabled
  traces, screenshots and videos. Only the six new browser cases were selected.

Spec SHA256:
`5a2bf5fd7013e4df9e16f7da50577cac0d26ca6f50a14cee3be14574ab3a95cd`.
Backend SHA256:
`4cd979ed41ef326bbaf6ac1d7c74a5eda68cd62677de016287a9086b80af8c39`.
Approved containment SHA256:
`1088837b37933f07f42dd5f4184181dca1985d1d284bb1f5fcd1a89ea3dd7f23`.

[Machine-readable evidence](2026-10-25-workflow-label-scalars.json) records exact
source/check/caller hashes, both raw-result summaries and cleanup receipts, all20
private runtime artifact hashes, all18 retained frontend asset hashes and exact
source-map provenance. The earlier template candidate and its nine qualified
assets/maps were preserved and hash-verified before this build; its accepted
checks were not repeated and its fixtures were not reset.

## Scope and limits

No schema, Rust or wire change is required. A frontend rollback restores the
native cutoffs; the unchanged server still reads scalar-valid saved names.

Normal contained completion is verified; these feature journeys did not exercise
forced interruption. Physical IME/keyboard, native desktop, assistive technology,
screenshots, global matrices and broader F08/F09 acceptance remain separate gates.
Independent exact-head review, hosted checks and main integration remain with the
lead. No push, PR, merge or issue closure was performed. The worker's ongoing Goal
continues for scoped review findings and the next explicit assignment.
