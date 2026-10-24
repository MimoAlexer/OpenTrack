# Bounded discovery across registry wrap

The retained default-profile browser failure recorded in
[the earlier attempt](2026-10-24-agent1-issue12-integrated-default-attempt.json)
remains part of the evidence. Fresh hosted success and the later focused retained
3/3 workflow pass do not replace that full-matrix failure.

## Reproduction and correction

The real PostgreSQL daemon regression creates 94 workspaces, then seven ordinary
team-privacy commands ahead of a normal workflow replacement. It tests both UUID
extremes with the default 2000 ms interval, five connections, 60-second lease and
unchanged 30-second completion criterion.

[The baseline receipt](2026-10-24-worker-discovery-wrap-baseline.json) records the
maximum-UUID job still queued with zero attempts at 30,011 ms; its eventual exact
completion at 31,716 ms does not pass that criterion. The minimum-UUID case took
29,616 ms. Both retained the expected authorized effects, seven maintenance
receipts, matching fences, released leases, history and eligible assignments.
The baseline used the production worker before this correction; its receipt
includes the contemporaneous test-source hashes. The recorded server-source
variant additionally contained a diagnostic unit test inside its existing
cfg(test) module. Its exact bytes were reconstructed and independently reviewed;
production server logic was unchanged and that test was not in the daemon.

An exhausted cursor page previously returned Idle even when it was wrapping an
existing registry traversal. The daemon then added a full idle cooldown before
looking at the head again. Worker::tick now returns Scanning for that wrap. An
empty head lookup still returns Idle. The existing four-call discovery budget,
80-candidate bound, stage rotation, claim ordering, error and lost-lease accounting
remain unchanged. No timeout, configuration, authorization, schema or migration
changes are required.

The corrective dense cases completed in 17,695 ms and 15,599 ms respectively and
passed their exact-effect assertions. Application discovery passed 3/3; daemon
policy checks and the server build passed. An initial broader corrective daemon
run passed 7/9: two older assertions still required the redundant wrap cooldown.
Their expectations were updated to require continuation while preserving
fourth-call cooldown checks. The final complete daemon recheck passed 9/9.

## Matched retained data

[The controlled retained receipt](2026-10-24-worker-discovery-retained-backlog.json)
uses two new restores of the same 116-workspace database and 12-blob snapshot.
Eight ordinary privacy intents were selected before execution, captured through
the restricted production worker function and verified older and eligible before
the normal workflow command. No job, checkpoint, lease or authority rows were
rewritten. Initial setup drains through the real default worker; its separate
600-second collection bound is outside the timed command.

| Runtime | Command return to observed success | 30-second criterion |
| --- | ---: | --- |
| Preserved uncorrected worker | 34,124 ms | Failed; sampled queued with zero attempts after 30 seconds |
| Corrective worker | 18,314 ms | Passed |

Both used the same initial data fingerprint and the default 2000 ms/five
connections/60-second lease. Exact workflow effects, history, assignments,
authority epoch and eight maintenance-progress fences passed. Zero maintenance
jobs had completed at observation end; completion of those jobs is not claimed.
All owned processes were reaped without cleanup failures.

A setup-only restore attempt failed before any API or worker because the driver
removed the default public schema although this dump omits schema creation.
That partial database and receipt were preserved; the corrected setup used new
targets and kept the default schema. This is separate from a product result.

This retained controlled workload reproduces the scheduling class. It does not
reconstruct the historical browser assertion's exact state or establish its
retrospective cause. The historical failed receipt remains intact.

The focused retained observation witnessed eligible queued jobs, attempt starts,
two logged idle waits per case and clean owned-process shutdown. It had no older
eligible jobs in the observed scope and cannot establish the historical full-run
cause by itself. Its registry read reached the 100-row cap, establishing at least
100 visible entries rather than an exact total. Private identifiers, credentials
and raw streams remain outside the repository.

## Qualification boundary

The uncorrected full retained browser matrix passed 126/126 with identical
original title/engine order. WebKit completed 30,128 ms after the response but
29,751 ms into the existing 30-second poll, which began after reload. That
near-boundary pass on the newer snapshot does not replace the historical 124/126
failure. A capped periodic journal omitted WebKit's active job behind 32 recent
terminal rows; its API poll and post-cleanup durable receipt remain separate.

The [matched corrective browser matrix](2026-10-24-worker-discovery-retained-browsers.json)
passed 126/126, 42 per engine, with identical original title/engine order and the
same initial database/blob fingerprint and frontend bytes. Its observer keeps
the same scope and 32-row cap but prioritizes active jobs and records cap metadata;
product assertions, timeouts and the worker profile are unchanged. Both complete
runs reaped their API, worker, observer and browser monitor, verified the anchored
browser namespace empty and restored the original specification byte-exactly.

The first corrective startup stopped before browser launch because the temporary
binary/blob layout correctly triggered the existing storage-overlap guard. The
guard was preserved and the private binary was copied into a sibling bin directory
for the corrected setup. An unchanged-input fingerprint was verified before the
new nonce; the first failure remains recorded.

All four configured hosted checks and independent source/evidence/privacy review
passed at implementation head `9a6a43a`. Fresh checks and final review at the
evidence-updated head remain required before merging. The original
failure, initial test-helper mismatch and superseded daemon expectations remain
preserved. This increment does not close workspace creation, worker operations,
general performance, native or whole-product acceptance gates.
