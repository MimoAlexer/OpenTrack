# OpenTrack project handoff — 18 October 2026

This is the wrap-up requested by the user before stopping work at the schema 19 checkpoint. The private repository is [MimoAlexer/OpenTrack](https://github.com/MimoAlexer/OpenTrack), branch `main`. It contains project source, tests, migrations, documentation, evidence assets and the full project commit history. Local credentials, databases, dependency/build caches and private raw logs are excluded. The exact published revision is identified by repository history; [current handoff](../HANDOFF.md) provides the continuation point.

**OpenTrack is usable, persistent software, but the complete product is unfinished.** The original authorization covers F01–F47, the approved X01–X66 expansion, and M01–M21. This stopping point does not remove the unfinished scope or mark those milestones complete. The [requirements ledger](REQUIREMENTS_STATUS.md) remains the acceptance authority; the [implementation plan](IMPLEMENTATION_PLAN.md) defines the dependency sequence.

## Product direction

OpenTrack is a self-hosted work application for individuals and teams, with both a browser client and an installable desktop client. Core operation must not require AI, an external cloud account, or a paid service. The user asked for a compact, modern application with substantial capabilities beyond issue tracking, especially retrospectives and planning. Linear screenshots informed breadth, not a request to copy its product or styling. The earlier Tracks-centric experiment was rejected.

The implemented interface uses a quiet, neutral workspace, a document-focused issue detail, compact properties, contextual commands, searchable reference pickers, and explicit conflict handling. Recent work fixed repeated loading flashes, focus loss, nested-dialog Escape behavior, draft loss during refresh, and popup overflow. Preserve that interaction quality as modules are added; empty navigation entries are not implementation.

## Implemented and connected today

| Area | Current behavior |
| --- | --- |
| Accounts and access | Local setup/password sign-in, workspace and team settings, public/private teams, member roles and deactivation, single-use invitations, own-session directory and revocation, password-confirmation rotation, and access-increase session invalidation. A separate local operator can issue recovery codes and disable/re-enable global accounts while preserving enabled owners. |
| Issues | Durable creation and editing, team issue keys, status/priority/labels/assignee fields, filtering and ordering, current reference summaries, version preconditions, idempotent creation and explicit stale-update comparison. Archive/restore can require reference repairs. |
| Hierarchy and relationships | Parent/subissue hierarchy, related/blocking/duplicate links, bounded cycle/depth checks, both-endpoint version changes, historical unlink repair, and reviewed single-issue moves between teams. Old issue keys resolve through current authorization; moving an issue does not expose its private source history. |
| Deletion and Trash | Review before deletion, atomic parent/child/relation detachment, searchable paged Trash, and restoration within 30 days. Children survive deletion. Restoration preserves identity/history and does not recreate removed connections. Expired entries remain visible; no automatic purge is claimed. |
| Workflows and taxonomy | Team status settings, explicit default, ordering, archive/restore/history, and durable status-replacement jobs. Shared/team labels have their own permissions and versions. Assignee candidates reflect current eligibility while historical assignments remain readable. |
| Documents and comments | Independently versioned structured descriptions, initial body saved with issue creation, OSS Tiptap editing, headings/lists/tasks/code/quotes/safe links, 750 ms autosave, explicit conflict comparison and retained drafts. Comments support idempotent posting, author edits, audited moderation, tombstones, permalinks and pagination. Activity is independently paged. Document revision metadata/body reads use current ownership authorization. |
| Files | Browser and native pickers, drag/drop in the shared editor, bounded reservation/checksum/quota validation, streamed uploads, quarantine/scanner state, progress, cancellation, retry and uncertain-result reconciliation. Only ready attachments can be linked. Downloads require current access and verify length and SHA-256. Files are not automatically executed or previewed as active content. |
| Team templates | Versioned definitions with literal title, rich body, status/priority/label defaults, lazy preview, archive/restore, stale-reference guidance and independent instantiated issues. Applying to an edited draft requires merge/replace/cancel; ready files and pending uploads are protected. Template version changes require explicit review before creation. |
| User mentions and subscriptions | Searchable, keyboard-accessible `@` suggestions use the current readable audience, including viewers. Persisted nodes contain IDs only; authorized projections supply names and unavailable references render generically. Personal automatic/subscribed/unsubscribed preferences persist. Creator, assignment and mention sources respect explicit opt-out. Private durable mention intents exist, but inbox/email delivery does not. |
| Background work | Separate restricted worker credentials, durable claims/attempts, lease fencing, bounded status-replacement batches, retries/dead letters/resume, and durable assignment-cleanup consumption with receipts/checkpoints. Access loss denies writes immediately; later cleanup removes ineligible assignments from open issues. Closed, archived and deleted assignments remain historical. |
| Navigation and desktop | Contextual command palette and shortcut help, responsive browser UI and theme preference. Linux Tauri client embeds the shared assets, selects a server, uses system-browser consent for a separate native session, stores credentials in the OS vault, and implements restricted API/file bridges and reviewed issue links. |

For precise boundaries and executed cases, see [identity/security evidence](evidence/2026-10-17-identity.md), [issue graph evidence](evidence/2026-10-18-issue-graph.md), [deletion evidence](evidence/2026-10-18-issue-deletion.md), [assignment consumer evidence](evidence/2026-10-18-assignment-consumer.md), [documents](evidence/2026-10-18-document-editing.md), [attachments](evidence/2026-10-18-attachments.md), [templates](evidence/2026-10-18-templates.md), and [mentions](evidence/2026-10-18-mentions.md).

## Architecture and source map

The server is a Rust modular monolith with PostgreSQL as the durable authority. The current root workspace contains six crates; desktop is a separate Cargo workspace. Some early architecture tables describe planned crates or Phase 0 state and must not be mistaken for the current source tree.

| Path | Responsibility |
| --- | --- |
| `crates/opentrack-domain` | Typed wire/domain models, validation and schemas. |
| `crates/opentrack-store` | SQL projections, transaction/context helpers and numbered migrations. |
| `crates/opentrack-app` | Authorized use cases, identity, tracking, documents/files/templates/mentions, durable worker and consumer orchestration. |
| `crates/opentrack-api` | Axum routes/extractors, credential audience and CSRF boundaries, problem responses and generated OpenAPI. |
| `crates/opentrack-adapters` | Implemented external/storage boundaries, including local blob and scanner handling. |
| `crates/opentrack-server` | Configuration, CLI, API/worker composition and process lifecycle. The worker is a mode of this binary, not a separate current crate. |
| `web` | React/TypeScript/Vite shared UI; Tiptap documents; generated API types. |
| `desktop` | Tauri 2 Rust shell, connection/session/vault state, restricted transport/file commands, package scripts and native checks. |
| `scripts`, `deploy`, `.github` | Development/test database tooling, checks, provisioning, Compose packaging and CI definition. |
| `docs` | Feature contracts, milestone tasks, ADRs, security/database/API rules, design, research and executed evidence. |

The browser uses same-origin JSON APIs and cookie/CSRF authentication. The desktop uses a distinct native audience and a narrow command allowlist; credentials never enter the renderer. Typed OpenAPI and TypeScript are generated from Rust and checked for drift. PostgreSQL jobs provide durable background work; browser SSE and general realtime convergence are still pending.

Preserve these implementation invariants:

- Workspace data and references are scoped. Current access and private-team rules apply to lists, direct reads, histories, candidates, receipts and delayed work; administrator status does not silently grant private-team membership.
- API runtime, migration owner, local operator and worker use separate database capabilities. Serving processes must not receive operator or migration credentials. Worker privileges are narrower than general application privileges.
- Commands use conditional versions and idempotency where required. Required activity, audit, source intents and events commit with the mutation; silently skipped effects fail the transaction. A lost response is reconciled rather than assumed to mean failure.
- Acquire account guards before workspace guards, then the relevant job/idempotency/team/entity locks, with stream heads last. Existing race tests protect revocation, owner preservation, graph updates and worker fencing. Follow the canonical [database](DATABASE_DESIGN.md) and [security](SECURITY_ARCHITECTURE.md) contracts before adding a command.
- Keep transactions out of network and file-transfer waits. Bound payloads, batches, queues, traversal and retry budgets. Treat optional scanner/vault/provider failures honestly.
- Migrations **0001–0019 are applied and immutable**. Add a new migration for future changes. Rollback means restoring the matching backup and binary/assets, not editing applied SQL or pretending a down migration exists.

## Qualification at this stopping point

The latest evidence is [the schema 19 mention checkpoint](evidence/2026-10-18-mentions.md). Its totals are separate suites, not claims that every product requirement is complete:

| Check | Executed result |
| --- | --- |
| Backend | 147 unique Rust tests, including real PostgreSQL, HTTP, concurrency and upgrade contracts; formatting, strict Clippy, doctests, generated-contract drift and server build passed. |
| Shared frontend | 370 tests across 49 files, TypeScript and isolated production build passed. |
| Native | 42 core/integration checks and strict all-target Clippy passed. |
| Helper tooling | 12 helper checks passed. |
| Current browser | Real persisted mention selection/post/reload, literal-text Escape, comment-menu focus restoration, two-tab subscription conflict review and main-workspace continuity passed. Final popup/client measurements showed no unnecessary vertical or horizontal overflow; the browser reported no console warnings/errors in that pass. |
| Upgrade and recovery | A fresh pre-upgrade backup restored 53 table/sequence fingerprints with owner, ACL, RLS and function-path parity. The preserved schema 18 binary passed eight HTTP checks against that clone. Old-schema startup rejection, schema 19 migration, restricted-role behavior and real process restart were also exercised. |
| Current development deployment | Schema 19 API and worker are running with separate restricted credentials; readiness was 200. Both workspace consumers had zero lag and no failed jobs/dead letters at qualification. Existing issue/document references remained consistent. |
| Linux package | A clean package with exactly seven qualified embedded asset files was checked and atomically installed. Built and installed binary hashes matched; connection configuration and the original running native process were preserved. |

The package SHA-256 is `f31a5ff5174f14e1a870efeaccc1a492a88a2828055c6ca43b0b18125e10991e`; the installed binary SHA-256 is `2026ec2bb1283375917713efb0536fbb2e71b6203898ad721905a22d25b2adbd`. The private local receipt is `work/desktop-build/package-mentions-v2-result.json`. An earlier package check rejected obsolete assets before installation; the clean rebuild passed. Preserved logs record failures and their fixes rather than erasing them.

These checks have limits. The existing native window was not restarted, so its loaded executable remains the previous one until next launch. Package integrity, OS-vault/transport checks and a running process do not establish rendered native GUI acceptance. Windows/macOS, signed distribution/update behavior, installed chooser/deep-link workflows and complete native interaction remain separate gates. The restored attachment catalog was empty: this backup does **not** prove recovery of populated blobs. Hosted CI execution and a complete production Compose deployment are not claimed merely because their definitions are present.

Raw run logs, database snapshots, local configuration and package receipts under ignored `work/` are private local evidence, not repository contents. Committed evidence summaries/screenshots are the portable record. Do not publish credentials or local databases to make a handoff appear more complete.

## Run, test and install

Run these commands from the repository root. Use the repository-pinned Rust toolchain, supported Node version (CI uses Node 22.22.1), Python 3 and compatible PostgreSQL/system libraries. The Linux helpers can extract pinned build/runtime dependencies into ignored directories; they do not guarantee every Linux distribution is supported.

```sh
# Prepare and launch the browser application and its separate worker.
scripts/dev.sh

# Complete core check entry point, using a separate test database.
scripts/check.sh

# Focused frontend checks, without replacing live built assets.
npm --prefix web run typecheck
npm --prefix web test

# Native checks, then a fresh Linux package using isolated frontend assets.
desktop/scripts/check.sh
desktop/scripts/build-linux.sh

# Optional current-user installation after a successful package build.
python3 desktop/scripts/install-user-linux.py
```

The development browser URL is `http://127.0.0.1:18970`. The launcher refuses an occupied port without stopping another process. It initializes/reuses the private local database, applies migrations, builds the application and supervises only its API/worker children. Stop that foreground launcher with Ctrl+C; the database remains available. Database status/start/stop commands are documented in [runtime setup](RUNTIME_SETUP.md). Do not delete its state directory to resolve an application problem.

On a fresh instance, complete the browser setup flow using the generated owner-only setup-token file described in the runtime guide; no default account/password is supplied in this handoff. Never commit generated environment files or tokens. The desktop package is produced under `desktop/target/release/bundle/deb/`; install it with a suitable distribution package manager or the explicit user-install helper above. Launch OpenTrack from the applications menu, select the running server, and follow browser approval. Remote origins require HTTPS; loopback HTTP is an explicit development option. A working OS credential service is required, with no plaintext fallback.

The desktop is an **online client**; it does not embed PostgreSQL or provide offline mutation sync. See [desktop setup](../desktop/README.md), [implemented settings](configuration/IMPLEMENTED_SETTINGS.md), [account/operator procedures](ACCOUNT_SECURITY.md) and [runtime/Compose setup](RUNTIME_SETUP.md) for exact prerequisites and maintenance commands. Building/installing does not authorize stopping another running app or rotating its credentials.

## Next concrete work

Resume with the remaining [M04 collaboration](milestones/M04_COLLABORATION.md) contracts, keeping prior acceptance evidence intact:

1. **Structured issue references.** Add ID-only issue-reference nodes, current-authority lookup and display, safe unavailable rendering, source/origin-team restrictions, and keyboard insertion. Preserve local edits, attachments and user mentions. Test private moves, revoked access and guessed references through the real API and editor.
2. **Portable document codec.** Implement the supported native/Markdown transfer contract and corpus round trips for formatting, tasks, code, safe links, attachments and both reference kinds. Explicitly report lossy conversions; never silently strip unsupported nodes. Qualify full keyboard/IME/paste/zoom behavior in real browser/native environments.
3. **Discussion/activity completion.** Audit the remaining F11 acceptance criteria, meaningful before/after attribution, adjacent-change grouping and bounded pagination. Comments and activity currently have separate pages; do not describe them as a fully merged feed. Preserve current/original-team authorization, tombstones, permalink focus and version-aware response merging. Keep notification dispatch in its later delivery milestone.
4. **File and recovery qualification.** Execute the outstanding revoked-session URL matrix, installed antivirus and full-disk cases, interrupted storage recovery, and a populated-blob backup/restore with checksums/permissions. Add native picker/save-dialog and installed-application proof on supported hosts. The existing empty-catalog database restore cannot substitute for this.
5. **Templates and residual core gates.** Retain the verified current template core, then finish project defaults when M05 exists, title expressions and explicitly designed attachment/reference cloning. Do not reuse document-owned attachment IDs across copies. Close remaining M01 configuration/reproducibility, M02 organization/identity and M03 keyboard/event-operations gaps through their acceptance rows. The second-workspace-owner versus global-account recovery authority conflict remains open; local-operator recovery does not silently resolve it.

Then continue through the dependency plan. This is the remaining product scope, not implemented navigation:

| Milestone | Remaining delivery |
| --- | --- |
| M05 | Real projects, milestones, cycle semantics, planning screens/boards and reporting parity. |
| M06 | Canonical filtering, saved views/search, SSE replay, safe cache invalidation and multi-client convergence. Current issue filters and polling are only a foundation. |
| M07 | Recipient-authorized notification fan-out, inbox operations/UI, delivery retries, preferences and recovery. Existing private intents/subscriptions are inputs, not delivered notifications or email. |
| M08 | Portable workspace transfer, administration, supported installation/release packaging, full disaster recovery, accessibility/performance/security campaign and release governance. |
| M09–M10 | Git provider links, public integration authentication/scopes, webhooks and reliable automation/external-effect reconciliation. |
| M11–M13 | Permitted provider connections, optional AI conversations, isolated runners, bounded grants/cancellation and reviewed exact-artifact publication. No paid subscription entitlement or live-provider qualification may be assumed. |
| M14 | Complete roadmap/collaboration/identity/desktop/assistant/scale tracks. Finish other-platform packages, native acceptance and signed updates; the existing Linux foundation is not all of F36. |
| M15–M16 | Workspace customization, custom fields, recurrence/shared expansion machinery, initiatives/outcomes, roadmap scenarios, dependencies and capacity. |
| M17 | Full retrospectives and personal/team routines: participation/privacy, facilitation, voting, action creation and follow-through. These were specifically requested and remain required. |
| M18–M19 | Decisions/knowledge/feedback, releases/reviews, incidents and corrective-work coordination. |
| M20–M21 | Unified reporting/search/activity, connected cross-module workflows, complete transfer, and final audit of every acceptance and release gate. |

Before any claim of complete release, qualify all GQ01–GQ17 gates in the ledger, including whole-product authorization, measured accessibility/performance, complete data/blob/key recovery, supported native platforms and real integration/runner evidence. Ratify license/public-name/distribution decisions and signing policy explicitly; the proposed AGPL license is not an already-granted license. GitHub publication of source does not resolve those release obligations.

## Continuation discipline

Start with [AGENTS.md](../AGENTS.md), [HANDOFF.md](../HANDOFF.md), the [ledger](REQUIREMENTS_STATUS.md), the relevant feature/milestone and its latest evidence. Inspect the actual source and existing running processes before acting. Use fresh isolated schemas/clones for destructive, migration and concurrency checks; preserve the user's running desktop/browser and saved connections. Never overwrite live frontend assets during exploratory builds.

Implement a small complete vertical slice with server rules, persistent effects, connected UI, negative/failure/race checks and actual browser evidence. Generate wire types from Rust. Keep commits reviewable, record migration/restore impact, and update evidence narrowly. Continue useful independent work around unavailable external gates, but never substitute a mock, plan, successful build or sidebar item for completed behavior.

At this handoff the user has requested stopping after publication. Further implementation should wait for a new instruction to resume; the unfinished requirements remain documented rather than silently discarded.
