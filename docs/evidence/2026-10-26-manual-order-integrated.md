# Manual ordering integrated qualification

[PR #167](https://github.com/MimoAlexer/OpenTrack/pull/167) was merged on 26 October 2026 as `16bde4ea34541ad177a7d65862888b40651e69d6`. Its tree exactly matches reviewed head `592ffbbc08f93e514c987d1839a8105a4c0125fd`.

All four required checks passed at that head. The real browser run completed 240 unique cases, 80 per engine, with matching all-pass text, end summary and JSON receipt. Verification recorded 95 successful Rust suite summaries and 848 passing frontend tests across 78 files. The official Node 22 tooling passed all 22 wrapper/privacy tests. The [structured receipt](2026-10-26-manual-order-integrated.json) records exact runs, jobs and hashes.

The merged scope includes persisted manual ordering, pointer/keyboard placement, concurrency recovery, bounded normalization, scoped discovery, document transfer and planning-write recovery. Earlier component receipts retain their own source and execution boundaries; the final matrix does not relabel those historical runs.

The fixture coordinator keeps the production authentication limits and original browser assertions. Its first integrated run passed all 240 browser cases but verification found the Node-only test filename in Vitest discovery. The byte-identical rename restored the intended test runners; fresh checks at `592ffbbc` passed. Earlier fixture, migration, browser and parser failures remain retained separately.

The merge used independent source/privacy and runtime review, zero unresolved conversations, the integration lock, fresh main/tree checks and the neutral merge helper. Migration 0027 is now integrated; the retained lead application database was not upgraded. Prior-version rollback still requires the matching database snapshot.

This closes the integration gate for this batch. Issues #30 and #31 remain open for their wider acceptance criteria. Native authentication, physical input, assistive technology, reporting projections, broader recovery and complete-product qualification remain separate work. No milestone or whole-product completion is claimed.
