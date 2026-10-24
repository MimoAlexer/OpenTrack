# Team board authority recovery and bounded job service

This increment preserves the status-board work and the five previously
unpublished commits, including the child denial-state correction. It adds the
connected parent recovery path rather than relying on cached team metadata when
leaving a denied board.

## Authority recovery

A current, unconditional team workflow denial can synchronously invalidate the
parent bootstrap and close scoped editors and overlays. Restoration requires a
fresh canonical session and matching bootstrap/access authority. API session,
principal, component and bootstrap generations fence older responses.

The child receives an explicit synchronous acceptance result. Only an accepted,
still-current authority-loss exit uses confirmed navigation after teardown has
started; an authorized workflow read retains the ordinary dirty-navigation guard.
Missing or rejected callbacks cannot initiate that exit.

The three connected browser journeys use real membership deletion and actual
private404 responses: automatic recovery, Try again after an aborted transport,
and Open all issues. Controlled pending private, canonical-authority and recovered
public/global reads expose the cache boundaries without mocking access denial.
The cases retain private-card/title checks after reload and verify the public
control plus unchanged owner-readable issue.

## Executed client checks

- Unchanged production with the new parent/composition cases:4 passed/21 failed.
- Corrected combined source:97 focused and696 complete frontend cases passed;
  both TypeScript targets and production build passed.
- The first broad test run exposed a contradictory existing fixture: access said
  viewer while its canonical session remained owner. The unchanged-source
  boundary control passed7/7. Aligning that fixture with its canonical owner and
  removing one unused new test helper preserved all assertions; source continues
  to reject real authority mismatches.
- Actual listing is nine connected executions and159 complete executions,
  three/53 per engine. Three stronger journeys replace one earlier revocation
  case, giving153→159; original pending/reload assertions remain covered.
- The first real connected matrix passed9/9 with clean owned-process and browser
  namespace cleanup. Rotation and distinct dirty-editor cases remain controlled
  unit evidence, not physical/native browser claims.

## Stronger retained workload failure

The complete retained159-case run then failed156/159: only the unchanged
30-second workflow success predicate failed, receiving queued in each engine.
All board and authority cases passed. This failure was retained and blocked the
increment despite its targeted success.

A [Chromium53 observation](2026-10-24-team-board-dispatch-before.json) preserved original order, profile and assertions and
bound its private journal to the actual accepted202 response. The exact job was
queued, eligible and attempt0 in all23 sampled points inside the comparator
window. The first sampled older-eligible count was23, all assignments; the last
pre-end sample had11 and the first post-end sample had10. All31 API polls returned
queued/version1. Later rows were not substituted for assertion-time correlation.
Filesystem journaling, an extra read-only connection and sample gaps remain
explicit limits. The observation does not replace full159 acceptance.

## Bounded scheduler correction

The worker previously traversed the idle workspace registry between every
eligible batch. It now permits one discovered batch and at most three additional
ordinary claim/effect probes in that workspace, with the other four stages
between batches. Repeats cannot refill the quantum. A miss, error or lost lease
clears it; a miss consumes discovery credit without also scanning20 workspaces.

Per-workspace FIFO, fresh authority and lease checks, the four-call/80-candidate
no-claim bound and default2000ms/five connections/60-second lease remain unchanged.
There is no job-priority shortcut, cached authority, retained claim, inner drain
loop, new configuration or migration. The state is one transient counter.

[Eight scheduler policy cases](2026-10-24-team-board-worker-quantum.json), all12 real daemon cases, the composite worker
authority/fencing contract and three discovery cases passed. The daemon checks
include two busy94-scope workspaces with exact four-batch fairness/FIFO and stage
rotation, populated --once, and shutdown during a witnessed second cached effect.
Formatting, strict Clippy and the server build passed.

## Qualification boundary

The [corrective backend qualification](2026-10-24-team-board-runtime.json) passed
connected9/9 and complete159/159,53 per engine, using new independent restores of
the same116-scope input and identical frontend/web-source bytes. All processes
were reaped, the browser namespace was empty, private modes passed and source
hashes stayed unchanged. Earlier failed databases, receipts, code snapshots and
diagnostics remain preserved. All per-case/expectation/workflow deadlines, zero
retries and the worker profile are unchanged. Successful workflow durations in
the normal reporter are whole-test durations, not response-to-claim measurements.

Fresh exact-head hosted checks, final independent privacy/source review and
resolved conversations remain required before integration. Manual rank/reorder, complete
planning/reporting, general performance, native/physical/assistive and
whole-product gates stay open. This increment does not close issues19 or30.
