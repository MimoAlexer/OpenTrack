# Planning count projection candidate

Scope: M05-T06 / #31. Local review candidate; hosted CI, integrated-head review and the focused browser run remain separate gates. [Machine-readable record](2026-10-26-planning-counts-projection.json).

## Behavior

One Rust metric fold owns project, milestone and cycle definitions, including historical visible-source recounts. SQL selects or groups raw category, exclusion and estimate inputs. Completed, canceled, unfinished, excluded, eligible and null-estimate semantics remain unchanged.

Migration 0028 registers the fixed required `planning_counts/v1` consumer for existing and new workspaces. `planning_issue_facts` stores versioned current facts with stable scoped issue references. Current source relations continue to own membership and authorization; no team or planning scope membership is cached.

Normal processing applies at most 100 events. Every issue-version event refreshes current facts, including nonmetric edits. Fact changes, the unique receipt and checkpoint advance commit together. Suppressed writes and unsupported relevant schemas preserve the gap and durable retry/dead-letter state. Equal-version encounters preserve `calculated_at`.

Initial and operator-requested reconciliation use existing fenced jobs and bounded ID-cursor batches. The prior checkpoint remains until the final batch commits facts and target checkpoint with job completion. Tail events reconcile insertions behind the cursor and later updates, deletion and movement. Audited rebuild never skips directly to the stream head.

Current authorized source membership and issue versions drive reads. Any missing or stale contributing fact causes whole-scope live fallback. Empty scope remains live. Milestone pages retain one aggregate query per page. Extra obsolete facts, unrelated private activity, global checkpoints and backfill state do not determine public source flags.

`calculation_source` distinguishes live, projection and captured snapshot results. Project/milestone `as_of`, and `captured_at` for current cycle scope, identify count validation in the source-reading statement, including empty scope. Nullable `facts_as_of` is the oldest contributing fact time only for projection. The UI labels both dates separately, retains earlier provenance after failed reads, and gives empty scope no fabricated fact date. Captured snapshots, close reports, carry operations and writes retain authoritative metric facts and original capture semantics.

## Executed checks

- Three canonical-fold cases; 70 planning UI cases and 36 recovery cases.
- Application and browser TypeScript targets; Rust-derived OpenAPI/client contract; a fresh production frontend bundle with the previous distribution unchanged.
- Four new real PostgreSQL worker contracts, one all-family private API scenario, four existing cycle and three milestone API cases, one historical moved-source case, five existing assignment-consumer cases and one populated 27-to-28 upgrade case.
- One operator CLI case, eight scheduler cases, formatting and strict Clippy across all five affected crates and their targets. The final current-cycle statement-clock change passes five focused API cases and strict store/app Clippy; the upgrade test also passes strict Clippy.

The upgrade enrolls two existing workspaces at zero baseline and preserves issues, planning links and captured history. Injected enrollment failure rolls back DDL, registration and the migration ledger. Migration connections use the existing close-on-drop discipline. Other PostgreSQL cases cover atomic fact/receipt/checkpoint rollback, baseline intent rollback, duplicate replay, unsupported schema, multi-batch insertion/update/delete/move races, stale leases, retention, audited repair, live fallback, projected parity, empty/page behavior, revoked ACL and private-activity/rebuild timestamp noninterference. Existing assignment fixtures scope their mutations to that consumer and explicitly coordinate the additional required retention checkpoint.

The 12 planned browser cases include actual default-daemon projection and revoked-assignment cleanup; they have not run at this checkpoint. The file declares authentication cost 3 for three synthetic invitation acceptances. The accepted full browser matrix is not repeated.

Earlier failures remain retained in the private proof root: fixture compile/lifecycle/cursor corrections, newly pinned legacy retention fixtures, generator availability/runtime attempts, formatting/Clippy corrections and the upgrade fixture's pooled migration-lock failure. Passing attempts do not replace them.

## Migration and recovery boundary

Validation uses fresh isolated test schemas in the existing private cluster and namespace, with the same four restricted roles and canonical grants. The retained application database was neither upgraded nor reset. Serving credentials cannot write facts or invoke planning capabilities; the worker receives fixed definer capabilities, and repair requires the separate operator credential.

Logical issue and planning source data are preserved. Matching binaries and grants are required; older binaries fail schema compatibility. Repair proceeds forward through bounded audited backfill while preserving the required checkpoint. Removing required registration or dropping retained data is not a rollback strategy.

No push, hosted check, issue closure, native qualification or complete-product acceptance is claimed.
