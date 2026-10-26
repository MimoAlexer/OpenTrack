# Issue-order worker failure acceptance

Test implementation `84968380960a9e826cc723d2ef3eaf81fe53eee1`, based on exact `330d1bf829398ffe10f0dcf641f46c681e7f2fce`. The [machine receipt](2026-10-25-issue-order-worker-failures.json) records source hashes, commands, raw-log hashes and the observed fault window.

Only the new PostgreSQL test target and this evidence change. Production, migration 0027, the standard Harness, grants and daemon/config source remain unchanged. The existing private namespace/cluster and restricted roles are reused. Each Harness invocation migrates a fresh UUID schema in the isolated test database; retained public schemas and the development environment are not migrated or recreated. Raw fixture issues explicitly supply ranks because ordinary create allocation is not implemented at this base.

The partial-application case stages 501 live issues, commits 100 normalized ranks and retains a 401-row original-rank suffix plus the maintenance guard. A real application membership removal revokes the contributor's private-team access while leaving the account and workspace membership active. The next worker batch blocks with `access_lost`; exact progress, staging, guard, hybrid ranks and team state remain unchanged, with no worker history/event effects. A different, currently eligible private-team manager takes over through controlled fixture requeue. Only the current job requester and requeue bookkeeping change; job identity, staging, progress, closed attempts and original requester provenance remain. Exact 501-row normalization completes without changing logical issue fields, versions, timestamps, documents or history, and without restoring the contributor's access. This is fixture takeover evidence, not acceptance of a manager-resume API.

The expiry case starts with 501 staged and 500 applied. It claims normally using default worker lease 60 seconds and maximum five connections, then shortens only that claimed fixture lease to 600 ms. A fresh-schema trigger gates the final effect only after all ranks/progress are complete and its guard has been deleted. It verifies the actual 3-second statement and 1-second lock settings. The worker must be observed waiting before expiry and again after actual database-clock expiry. Nontransactional witnesses prove the gate returned normally, excluding lock timeout as a false positive.

Observed final window:

- Gate entry: 589.497 ms before expiry; first blocked observation: 585.155 ms before expiry.
- Later blocked observation: 76.709 ms after expiry.
- Normal gate return: 668.276 ms after entry, 78.779 ms after expiry.
- Worker completion: 680.109 ms after entry, returning `LostLease` below the statement-timeout boundary.

Exact pre-effect and post-rollback snapshots match: issue ranks/versions/timestamps, staging, normalization cursor/phase/counts, guard, team generation, claimed expired running job and open attempt. Logical documents, subscriptions, activity/audit/workspace events, stream heads, project links, labels, relations and authorization/membership rows also match. The final effect's rank, progress and guard changes do not survive expiry.

The initial focused run passed both new cases. A stricter normal-return witness was then added only to the expiry function; its focused final rerun passed, with the unchanged access-loss case, fixtures and entry points reused by byte equivalence. No test failed. Targeted Clippy with warnings denied, Rust formatting, diff, privacy and UTF-8/LF checks pass. Existing staging-only/stale-claim tests, browser/UI builds, server binary builds and global checks were not repeated.

Holder release and worker join/reap precede fault-witness assertions. Timeout cleanup aborts and awaits the owned task set. Pools close before fresh-schema cleanup; all three successful Harness invocations awaited schema removal. Failed runs would retain their fresh schema and logs after closing pools. The daemon's 2,000 ms default remains unchanged in source; daemon dispatch was not executed by these direct claim/effect tests.

This qualifies the exact worker foundation and these failure cases. The lead's actual resume API, current API/UI union, manual-read presentation, hosted checks and issue #30 completion remain separate. No F17/M05 or whole-product completion is claimed. The app Goal still reports blocked; this newly authorized assignment executed without creating or forcing a replacement Goal.
