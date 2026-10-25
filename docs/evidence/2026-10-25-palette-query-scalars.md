# Command-palette query scalar input

25 October 2026. Bounded F22 input correction from
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`.

Removed only the palette's native UTF-16 `maxlength120` cap. Its existing hook
uses ordinary issue queries, whose Rust bound is 256 Unicode scalars; F22 has no
separate 120-character policy. Local command matching, 150ms debounce, 20-result
limit, cancellation/scope keys, current resolution and keyboard/focus behavior
are unchanged. No client validator or hook/registry/backend/IssueList/router
change was added.

## Executed acceptance

One focused journey per engine used native sequential input and identical
before/after spec bytes. Fresh fixtures contained a unique supplementary
256-scalar target title, an accessible 60-scalar-prefix distractor, and a matching
issue in another synthetic administrator's private team. The original owner had
no membership in that negative-control team.

| Engine | Baseline input/query | Baseline accessible matches | Corrected input/query | Corrected matches |
| --- | --- | --- | --- | --- |
| Chromium | 60 scalars | target + distractor | 256 scalars | target only |
| Firefox | 60 scalars | target + distractor | 256 scalars | target only |
| WebKit | 60 scalars | target + distractor | 256 scalars | target only |

The initial before/after pair used CRLF spec bytes and remains retained. The final
staged whitespace check required LF normalization. An exact equivalence receipt
proves only 124 CRLF sequences changed; assertions and profile stayed identical.
The final LF spec repeated these same focused before/after cases on the existing
frozen frontend/backend bytes. Both pairs and all fresh fixtures remain separate;
the table above is confirmed by the final exact-spec pair.

Captured real API URLs preserve the exact full query and limit 20. A 257-scalar
query reached Rust and returned HTTP422 `validation_failed`; the query remained
in the field, previous results disappeared and no truncated or unfiltered query
fallback was sent. Enter on this empty rejected result kept the palette open
without a resolve request or navigation. Repair restored the intended result.

Keyboard activation then performed a current authorized GET200 before closing
and opening the same issue. Its full title survived reload, and both accessible
fixture records remained unchanged. The private control identity/key never
appeared in results; direct owner reads returned 404 before and after the journey.

## Checks, isolation and retained artifacts

- 22 focused palette/command unit tests across two files passed, including the
  new full-query/rejection/repair/resolve case and existing stale/revoked/scope,
  debounce, current invocation, focus and IME guards.
- Both TypeScript targets passed (`npm run typecheck`).
- One final production build passed (`npm run build --prefix web`).
- The profile retained one worker, zero retries, 60-second test and 10-second
  expectation/action bounds, port18081 origin guards and disabled screenshots,
  traces and videos. Only these three cases ran per phase.

The same frozen backend and approved copied PID containment served both phases.
Both browser namespaces were empty after exact monitor/API reaping, private
bounded output drained and cleanup failures remained empty. Browser environments
omitted PG/database/operator/worker/setup service authority. Private phase
directories are mode 0700 and artifacts mode 0600. The own database stop returned exit 0;
final inspection found no extra helper-namespace process, absent postmaster PID
and retained data. Earlier qualified branches/assets/maps/logs remain preserved;
their successful checks were not repeated.

Spec SHA256:
`9aaa41ff6a068427130e571ad0c9db75490149cafd2cadb404c889c7b8219f9d`.
Backend SHA256:
`4cd979ed41ef326bbaf6ac1d7c74a5eda68cd62677de016287a9086b80af8c39`.
Containment SHA256:
`1088837b37933f07f42dd5f4184181dca1985d1d284bb1f5fcd1a89ea3dd7f23`.

[Machine-readable evidence](2026-10-25-palette-query-scalars.json) binds 12
source/check/caller hashes, all 34 phase runtime artifacts, all 18 retained frontend
assets and exact palette source-map provenance. Its unique private artifact index
names both frontend roots and all 22 result/scope/shutdown/stop/preservation/format
receipt and retained-source filenames with hashes. The pinned baseline was reused without rebuilding;
the previous moderation candidate's nine qualified assets/maps remain intact.

## Scope and limits

No schema, Rust or wire change is required. Frontend rollback restores native
truncation while existing issue-query policy remains authoritative. This covers
one bounded palette input correction, without claiming broader search/palette,
milestone or product acceptance. Normal contained completion is verified; forced
interruption, physical IME/keyboard, native desktop, assistive technology,
screenshots, global matrices and broader performance remain separate gates.
Independent exact-head review, hosted checks and integration remain lead-owned.
No push, PR, merge or issue closure was performed. The ongoing worker Goal
continues for scoped findings and the next explicit assignment.
