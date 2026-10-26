# Integrated planning freshness and private scope

PR #168 merged as `dd9d2a22c25fabe441225becf14f27150107ae5b` from reviewed
`7a63dc60579d21a9fcfd22bba1521fc696af5ee7`. The actual merge tree equals the
reviewed tree. All four required checks passed, review conversations were
resolved, and the lead merged under the integration lock after independent
source, privacy and runtime review.

Planning refresh reloads both counts and contributing rows. Failed refreshes
retain the previous calculation time and expose the failure. The added browser
cases cover exclusions, empty scope, retained reads and populated private counts
after access revocation. They do not implement a reporting projection consumer.

The browser text, final summary and JSON agree on **249 unique passing cases**,
83 per engine. All previous 240 cases remain, with nine new planning cases.
Verify passed **854 frontend tests in 78 files**, with 95 successful Rust test
summaries: 284 passed, zero failed, four ignored and one filtered. The official
Node 22 tooling checks passed 22 tests.

- [Implementation checks](https://github.com/MimoAlexer/OpenTrack/actions/runs/40303915255), attempt 2.
- [Runtime checks](https://github.com/MimoAlexer/OpenTrack/actions/runs/40303915428), AMD64 attempt 2; ARM64 retains its successful first execution.
- [Machine-readable receipt](2026-10-26-planning-freshness-integrated.json).
- [Source and focused checks](2026-10-26-planning-integration-candidate.json).

The first Verify attempt exhausted its 30-minute job window before frontend
verification completed. The first AMD64 attempt exhausted its 45-minute window
during system dependency downloads, before browser execution. Both original
logs remain preserved; neither partial result is counted as a successful run.
The successful retries use the same source and limits. The approved browser
suite watchdog remains 900 seconds, with 60-second test/hook limits, 10-second
actions, one worker and zero test retries.

This batch also declares the Linux CA package dependency. It does not establish
a new package build or installation. Actual reporting-consumer replay, broader
native authentication, physical input, assistive technology, provider, recovery
and governance acceptance remain open. Issue #31 and the full product remain
incomplete.
