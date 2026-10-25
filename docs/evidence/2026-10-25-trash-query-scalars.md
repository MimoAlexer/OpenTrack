# Trash query observation correction

25 October 2026. Reviewer-driven test/evidence correction from immutable
`9b474788ed25c3c187a342ae065a39f3c8653fbb`. Production, unit source, backend and
frozen frontend bytes remain unchanged.

The original proof remains retained with two instrumentation limits: its list
recorder ignored requests without `q`, and its detail counter saw completed
responses rather than all issued requests. Those records still support their
positive query/result observations; they did not fully establish absence of an
unfiltered fallback or pending/late stale detail request.

The corrected recorder captures **every exact-list-path GET** with `q` as
`string | null`, including absent and empty values. The complete rejection window
must equal the exact 257-scalar query, so null/empty/truncated fallback fails.
Exact-detail GETs are counted when issued, including requests that have not
completed. The journey requires zero through rejected Enter, one deliberate
opening, two after reload, and still exactly two at its end.

The identical corrected spec ran against the existing frozen baseline/corrected
bundles. Chromium, Firefox and WebKit each reproduced the native 100-scalar
cutoff, then passed full 256 search, real 257 HTTP422, retained query/no stale
rows/actions/fallback, repair, current detail/reload/close, private exclusion,
unchanged deleted version 2 and zero writes in the observed window. All three
corrected receipts record rejected-window `[257]` and issued-detail total 2.

Only the changed browser TypeScript target was rerun and passed. The original
18-unit, app type-check and one-build results are retained by reference; no
production build, unit suite or old artifact audit was repeated. Assertion
strength, canonical 60s/10s bounds, one worker, zero retries, origin/context guards,
Node noPG/service authority and approved scope 108 lifecycle remain intact.

New phases verify empty browser namespaces, reaped exact monitor/API processes,
bounded private output and no cleanup failures. Own database stop returned exit 0;
final audit records no extra helper-namespace process, absent postmaster PID and
retained data. Original fixtures, branches, logs and proof were not reset.

[Machine-readable evidence](2026-10-25-trash-query-scalars.json) references the
original immutable evidence SHA256
`d6464df674f70d95a72093d819ff7027b242ff381d977ee2d9f0f2ad07dcc667`,
records only the changed-input checks and new 17 runtime artifacts, and names a
unique private index with both existing frontend roots and all eight new exact
result/scope/shutdown/stop references. Original asset/provenance records remain
linked without duplication. Corrected spec SHA256:
`e2a84364a83dcf1bc790ec8d4d1a93314942ab52e5d38616ea4ed73494a400db`.

Actual restore, forced interruption, physical/native/IME/assistive, screenshots,
broader performance and whole-product acceptance remain separate gates. This
correction is source/evidence for local focused journeys, not a hosted/integration
waiver. No push, PR, merge or issue closure occurred. The ongoing worker resumes
the already-dispatched IssueList slice, then requests a larger independent
user-visible feature under the approved throughput instruction.
