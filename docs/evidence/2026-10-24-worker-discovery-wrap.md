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
includes the contemporaneous test-source hashes.

An exhausted cursor page previously returned Idle even when it was wrapping an
existing registry traversal. The daemon then added a full idle cooldown before
looking at the head again. Worker::tick now returns Scanning for that wrap. An
empty head lookup still returns Idle. The existing four-call discovery budget,
80-candidate bound, stage rotation, claim ordering, error and lost-lease accounting
remain unchanged. No timeout, configuration, authorization, schema or migration
changes are required.

The corrective dense cases completed in 17,695 ms and 15,599 ms respectively and
passed their exact-effect assertions. The application discovery cases and daemon
policy checks passed. A broader corrective daemon run passed 7/9: two older
assertions still required the redundant wrap cooldown. Their expectations were
updated to require continuation while preserving fourth-call cooldown checks;
the resulting complete daemon recheck is pending.

The focused retained observation witnessed eligible queued jobs, attempt starts,
two logged idle waits per case and clean owned-process shutdown. It had no older
eligible jobs in the observed scope and cannot establish the historical full-run
cause by itself. Its registry read reached the 100-row cap, establishing at least
100 visible entries rather than an exact total. Private identifiers, credentials
and raw streams remain outside the repository.

## Qualification boundary

Complete retained baseline/candidate browser matrices, final daemon recheck and
fresh checks at the changed pull-request head remain pending. The original
retained failure, the initial test-helper mismatch and the superseded daemon
expectation failures are preserved. This increment does not close the workspace
creation, worker operations, performance, native or whole-product acceptance gates.
