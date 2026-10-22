# Complete-product implementation plan

Current execution authorization, 22 October 2026: the user resumed complete-product execution through five assigned lanes, superseding the batch-only stop rule. Follow the assigned queues and shared coordination protocol; Agent 5 owns integration, migration allocation and final qualification. All F/X/M criteria and product gates require real evidence on integrated main before COMPLETE.md or whole-product completion.

Authorization recorded: 17 October 2026. The user activated a goal to finish the complete OpenTrack product. Scope is F01–F47, X01–X66, the original brief's cross-cutting requirements and all release gates below. This supersedes Phase 0-only restrictions. It does not authorize external publication, paid purchases or messages beyond the user's actual instructions. No implementation requirement is marked complete by this planning update.

## What complete means

A capability must have a real persistent backend, server authorization, usable connected UI, successful normal and failure-path checks, import/export/restore coverage where durable, and documented operation. The approved interactive preview is visual/product direction; hardcoded sample data, a route placeholder or a successful build is not completion. Preserve the compact neutral visual style and familiar workspace, with task-appropriate roadmap, retro and release screens. Do not clone Linear or restore the rejected Tracks-centric product model.

The [requirements ledger](REQUIREMENTS_STATUS.md) records each original F acceptance criterion, added F criterion, X expansion capability and complete-product gate. Rows began unverified; narrowly recorded runtime results may now be marked verified. Update only with exact source paths, executed command or browser evidence, environment and observed result. If code exists but lacks acceptance proof, use implemented/unverified rather than complete. Preserve failed checks and the fix/recheck reference.

## Execution sequence

| Stage | Milestones | Deliver a usable increment |
| --- | --- | --- |
| Foundation and isolation | M01–M02 | Reproducible Rust/PostgreSQL/React build, settings, schema contract, local login, workspace/team authorization and real DB harness. |
| Core working product | M03–M07 | Persisted issues/comments/files, projects/cycles, search/views/realtime, notifications/inbox; two users and negative access checked. |
| Core release gate | M08 | Installation, migration, backup/blobs/keys restore, transfer, accessibility and performance. This is a checkpoint, not the goal's end. |
| Integration platform | M09–M10 | Git provider links, scoped public API/webhooks, reliable external intent and bounded automation. |
| Optional-at-runtime AI/agents | M11–M13 | Real permitted adapters, streaming/cancellation, isolated runner, bounded grants, exact artifact evidence and reviewed publication. Run parallel with expansion where prerequisites allow. |
| Advanced platform | M14 | Roadmap baseline, native desktop, collaboration/identity and selected-protocol support with their dedicated ADRs and platform evidence. All authorized tracks require explicit disposition. |
| Shared expansion foundation | M15 | Typed fields, navigation/modules, templates/recurrence, integration health and portable schema sections. |
| Planning and learning | M16–M17 | Initiatives/outcomes, scenario/capacity planning, personal/team routines and complete retrospectives. |
| Decisions and delivery | M18–M19 | Feedback/knowledge/decisions, release/review coordination and incidents with corrective work. |
| Connected product | M20 | Defined reports, unified search/activity and whole workflows across modules; complete native transfer/restore. |
| Qualification | M21 | Audit every requirement, run security/recovery/UI/platform/performance gates, verify distribution and record remaining limitations honestly. |

See [ROADMAP](ROADMAP.md) for exact dependency and task links. Existing F01–F36/M01–M14 identifiers remain stable; additions are F37–F47/M15–M21. Do not renumber historical tasks or substitute a broad done label for their individual deliverables.

## Desktop and browser delivery priority

On 18 October 2026 the user explicitly confirmed both a browser application and an installable desktop application. Bring the F36/M14 Tauri desktop foundation forward alongside core work: shared React assets, real server selection, secure native authentication, an installable Linux build and the same persisted workspace workflows. This changes sequencing, not completion criteria: platform, native-session, capability, deep-link and signed-update acceptance still require their own evidence. A browser shortcut or development server does not satisfy the desktop deliverable. Retrospectives and the rest of the approved product remain in scope.

## Vertical slice contract

For each capability, choose the smallest complete workflow. Update target contract if evidence requires it; add scoped migration and backfill/rollback note; implement domain rules and use case; expose authenticated typed API; build the actual UI with loading/empty/error/read-only/conflict states; run DB/API/negative access and meaningful browser checks; verify event/search/transfer effects; then record evidence. Shared helpers should reduce duplication without erasing feature-specific permission or state rules.

Use real PostgreSQL for transaction, foreign-key, row-level-security, lock and concurrency checks. Browser tests must hit the real API/database, not only fixture routes. Keep core workflows available without provider credentials or internet access. Do not execute repository code in the core server/worker or skip the runner VM boundary to make an agent demo work.

## Expansion delivery contracts

The canonical [database](DATABASE_DESIGN.md), [API](API_DESIGN.md), [security](SECURITY_ARCHITECTURE.md) and [realtime](REALTIME_ARCHITECTURE.md) documents now include expansion rules. F37–F47 supply object behavior. X01–X66 supply an auditable acceptance row for every approved capability, including extensions to old features. Scoped document ownership, source-link authorization, private retro audiences, immutable scenario revisions and typed field validation must be implemented before dependent screens can claim correctness.

Independent work is allowed after prerequisites pass: M11–M14 need not block M15–M20, and a provider outage need not block local retro/report work. Do not treat integration credentials, macOS/Windows test hosts or signing keys as evidence that can be simulated. Implement the safe local path, record the exact unavailable gate and continue independent work. Full completion remains unavailable until required gates are verified or the user explicitly changes scope.

## Evidence and final handoff

Store run records under docs/evidence/ with timestamp, commit or working-tree identity, environment, commands, exit/results and limitations. Screenshots demonstrate rendered UI, not backend persistence; restart/second-client/database checks demonstrate persistence and synchronization. Record provider name/auth mode and tested capability without secrets. Separate fixture adapter checks from live-account evidence and native OS checks from web builds.

The final handoff must identify a working launch/install path, verified product scope, test and recovery results, local repository state and remaining unresolved requirements. Any unverified row prevents a claim that the entire product is finished. License/name, signing and external publication decisions must remain explicit; implementation authorization alone is not a license ratification or permission to publish remotely.
