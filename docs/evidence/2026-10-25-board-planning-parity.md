# Manual issue pages and planning-count parity

25 October 2026. Base `c1859b811f343a22aba4ca287fa5595d61643424`.

The existing five-issue planning case now compares real created/manual pages and every status-filtered board-equivalent column with project, milestone and current-cycle counts. Page size is one with a six-page budget; opaque cursors, exact row identities, complete issue representations and category totals are checked without client ranks.

A real three-row reorder changes the lane order while preserving row sets/counts. A cross-status position updates all compared category counts, and matching idempotent replay preserves the acknowledgement, current issue/order token and counts. A fresh command restores the original status/order before the original PATCH transition, so the original scenario and every snapshot/exclusion/zero/history assertion retain their meaning. The existing archive, delete, canceled-only and final empty outcomes also pass the new live-view comparisons.

The full stored daily row remains unchanged. Existing consumer registration/checkpoint rows and the receipt count are observed unchanged across the later live mutations. No worker is run: this proves immediate authoritative live reads alongside immutable captures, not a future delayed-reporting pipeline.

Only the strengthened case ran: **1 passed, 0 failed, 0 ignored, 2 filtered out**, in 5.82 seconds. Formatting, compilation and strict focused Clippy pass. The original file is reproduced exactly by removing additions and restoring the expected cleanup order; the other two cases and all original assertions remain unchanged. Two initial compiler iterations are retained separately. PostgreSQL is stopped and original data remain retained; the successful fresh schema uses normal cleanup.

The [structured evidence](2026-10-25-board-planning-parity.json) records precise scope and hashes. The private index `$OPENTRACK_DEV_STATE/board-planning-parity/artifact-index.json` has SHA-256 `e8dfbab4ad4759e9cbd9e5da591acf1d621773e7ff841a14e5509e61708d450f`. Production, harness, migrations, generated contracts and UI are unchanged. Browser/global, broader privacy/concurrency/lag/load/recovery and native/physical/assistive/product gates remain separate; issue #31 is not closed by this case.
