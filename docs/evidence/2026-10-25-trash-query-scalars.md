# Trash query scalar input

25 October 2026. Bounded deleted-issue title/key query correction from
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`.

Removed only Trash's native UTF-16 `maxlength200` cap. The documented and existing
Rust query bound remains 256 Unicode scalars. Delete/restore, expiry, assignee,
team, pagination, router, hooks, types, backend and shared fixtures are unchanged.
No client validator was added.

## Executed acceptance

One focused native-input journey per engine used identical before/after spec
bytes. Fresh fixtures contain a deleted 256-scalar target, an accessible
100-scalar-prefix distractor and another administrator's matching private deleted
issue. The original owner has no membership in that negative-control team.

| Engine | Baseline input/API query | Accessible matches | Corrected input/API query | Matches |
| --- | --- | --- | --- | --- |
| Chromium | 100 scalars | target + distractor | 256 scalars | target only |
| Firefox | 100 scalars | target + distractor | 256 scalars | target only |
| WebKit | 100 scalars | target + distractor | 256 scalars | target only |

Captured real query URLs preserve exact full text and limit 50. A 257-scalar
query reached the existing HTTP422 `validation_failed` diagnostic, retained the
input and removed stale rows/actions without a truncated or unfiltered fallback.
Enter did not open a stale detail or send a mutation. Repair restored the exact
authorized result.

Keyboard opening then performed a current deleted-detail GET200. The correct
title, selected detail and full query survived reload; closing retained the
filtered result. Target/distractor tombstones stayed unchanged at deleted
version 2. The private control remained excluded and direct owner deleted reads
returned 404 before and after. The observed search/detail window sent no writes;
fixture deletion occurred beforehand through existing APIs. Actual restore was
not executed by this query journey.

## Checks, isolation and retained evidence

- 18 focused IssueDeletion tests passed, including the new full-query/error
  repair case and existing delete/restore/expiry, pagination, navigation/draft,
  privacy/authority and uncertain-result regressions.
- Both TypeScript targets passed (`npm run typecheck`).
- One final production build passed (`npm run build --prefix web`).
- Only these three cases were selected per phase. One worker, zero retries,
  60-second test/10-second expectation/action bounds, port18081 guard and disabled
  screenshots/traces/videos remained unchanged. The existing 280ms search debounce
  and page limit 50 were retained.

The same approved scope/backend and immutable baseline frontend served both
phases. Receipts verify empty browser namespaces, reaped exact monitors/API,
bounded private output and no cleanup failures. Node received fixture authority
without PG/database/operator/worker/setup service authority. Private directories
are mode 0700 and artifacts mode 0600. Own DB stop returned exit 0; final audit
found no extra helper-namespace process, absent postmaster PID and retained data.
Earlier branches/qualified assets/maps/indexes/logs remain preserved without
repeating their successful checks.

Spec SHA256:
`459f4d997d1c35b8738b1f9f384c9ebfdb8861d0a3324bad45c4486086106362`.
Backend SHA256:
`4cd979ed41ef326bbaf6ac1d7c74a5eda68cd62677de016287a9086b80af8c39`.
Containment SHA256:
`1088837b37933f07f42dd5f4184181dca1985d1d284bb1f5fcd1a89ea3dd7f23`.

[Machine-readable evidence](2026-10-25-trash-query-scalars.json) binds 12
source/check/caller hashes, 17 runtime artifacts, 18 frozen frontend assets and
exact source-map provenance. Its unique private index names both frontend roots
and all ten result/scope/shutdown/stop/preservation/proof receipt files. No manual
SavedWorkspace construction was added; the pinned shared fixture was not edited.

## Scope and limits

No schema, Rust or wire change is required. Frontend rollback restores native
truncation while stored deleted records and server policy remain unchanged.
Existing restore units and unchanged implementation do not constitute a new
restore browser acceptance claim. Normal contained completion is verified;
forced interruption, physical/native/IME/assistive, screenshots and broader
performance remain separate gates. Scoped local checks do not replace broader
deletion/product or hosted CI gates. Exact-head review and integration remain
lead-owned. No push, PR, merge or issue closure occurred. The ongoing worker Goal
continues for scoped findings and the next explicit assignment.
