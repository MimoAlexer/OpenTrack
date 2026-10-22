# Quality engineering and release gates

Phase 0 had documentation checks only. Implementation is now authorized; the strategies below are required executable acceptance work. Record current results in [REQUIREMENTS_STATUS](REQUIREMENTS_STATUS.md). [Feature matrix](FEATURE_MATRIX.md) maps every feature to measurable acceptance criteria; [threat IDs](SECURITY_ARCHITECTURE.md) define adversarial cases.

## Test layers

| Layer | What belongs here | Required examples |
| --- | --- | --- |
| Rust unit | Pure invariants, normalization, state transitions, error mapping | Priority unset ordering, filter AST limits, issue graph, grant intersection, valid lifecycle transitions |
| Property based | Combinatorial rules with meaningful invariants | Related-pair normalization, no graph cycles, parser limits, permission monotonicity, document round-trip |
| PostgreSQL integration | Real server/migrations/RLS/constraints/transactions | Composite tenant FK, last-owner removal, optimistic race, stream commit order, SKIP LOCKED lease/fence, duplicate consumer receipt |
| API contracts | Wire schema, auth, version/idempotency/error/pagination semantics | OpenAPI/client agreement,428/412, scoped tokens, malformed filters, cursor binding and negative tenant cases |
| Adapter fixtures | Provider/Git/mail transports and versioned outputs | Stream fragmentation/terminal error, HMAC, rate-limit, refresh rotation, ambiguous external write |
| Frontend components | Interaction states and semantics | Modal focus, combobox keyboard, error association, optimistic rollback layering, editor conflict, log pause |
| Browser E2E | Golden product journeys on real API/DB | Two users create/comment/assign/revoke; reconnect; search; inbox; export/import; offline draft; keyboard-only flow |
| Accessibility | Automated checks plus manual AT/zoom/keyboard | Contrast, focus traps, live output,400% reflow, board alternative, actual screen reader report |
| Security | Threat-boundary abuse with independent observation | IDOR/CSRF/XSS/SSRF, upload bombs, wrong token audience, secret canary and runner isolation |
| Concurrency/chaos | Timing-dependent safety | Kill after commit/before response, revoke during write, out-of-order commits, deadlock retry, stale lease, canceled grandchild |
| Migration | Fresh and previous-version schema, content upgrade | Expand/backfill/contract, checksum mismatch, failed nontransactional index, old client overlap, restore |
| Performance | Budgets under realistic skew/load | Query plan, browser memory, SSE 500 connections, queue fairness, hot workspace append contention |
| Self-host release | Packaging and operator workflow | Nonroot install, no-internet core operation, upgrade, config error, backup plus blobs/keys restore |

Do not replace real PostgreSQL tests with SQLite or mocks. Test isolation uses one database/schema per suite with migration fixtures and explicit teardown; parallel tests must not accidentally share role/context/session state. Deterministic fake clocks control TTL, snooze and lease cases. Critical temporal invariants need real concurrent transactions, not only sequential unit tests. Loom may be used for custom in-process concurrency primitives if introduced; do not invent primitives just to use it.

## Golden scenarios

G01: fresh offline setup → local login → workspace/private team → create/assign/comment → search/filter → second user notification → revoke membership → export authorized scope → restart → restore from backup.

G02: two clients edit same issue/document → one success/one conflict → retained draft → explicit merge → reconnect with lost wakeup → converge, including hidden events and expired cursor.

G03: import malformed relationships/attachment → preview reports exact failure → valid subset commit → crash/retry → no duplicates → native export round-trip with explicit ID mapping.

G04: allowed repository/agent grant → isolated attempt → tests at final SHA → review artifact → publish draft PR → lost provider response → reconcile one PR. Revocation/cancel/stale fence variants must prevent further effects.

G05: AI disabled, missing SMTP, local-only network → every MVP core task still completes; optional features expose clear disabled/degraded state.

## CI architecture, to be implemented

Pull requests run docs/link/phase rules, Rust fmt/clippy/check/unit, TypeScript strict/lint/component tests, generated API drift and real-DB integration suite; affected E2E smoke and accessibility checks run on the same built artifact. Cache dependencies by lockfile/toolchain, never secret-bearing output. Fork PRs get no publish/provider credentials. Pin third-party actions by digest/SHA, minimize token permissions and isolate release signing.

Nightly/scheduled project CI can run broader migration, concurrency, load, browser matrix and optional provider contract tests only when maintainers configure it; this planning task creates no automation. Release candidates run full golden flows, dependency/SBOM/license review, image scan, install/upgrade/restore, accessibility signoff and measured performance. Build once and promote the exact tested artifact; do not rebuild different bits after validation.

Coverage metrics identify untested critical decisions; no arbitrary 100% requirement. Require tests for every authorization boundary, unsafe external effect and recovery transition. Avoid snapshots of entire responses that hide semantic regressions, tests mirroring implementation or flaky arbitrary sleeps. Use observable conditions and bounded deadlines.

## Definition of done for a vertical slice

The feature acceptance IDs pass in real environment; schema/migration and rollback implications reviewed; negative authorization and conflict/failure paths tested; screen keyboard/empty/error/loading states checked; API/schema/docs updated; no credentials or unbounded tasks; core/optional feature boundaries preserved; actual tests/measurements recorded with commands and versions. Human review inspects behavior and risk, not only generated test counts.

A milestone is complete only with linked evidence for its completion conditions. A static typecheck is not browser proof; a container running is not restore proof; provider stream completion is not test success; a patch is not a PR until remote state is verified. Phase 0 final review reports only planning validation.

## Complete-product expansion scenarios

G06: private retro → individual collection → reveal/group → race two vote requests → lock round → discuss → preview action into another authorized team → create once → next cycle → record observed improvement → next retro follows the same action. Verify no draft/author/summary audience leakage.

G07: initiative → linked private/public projects → capacity assumptions → immutable scenario → dependency change → comparison → concurrent live edit → reject stale apply without partial writes → refresh → atomic apply → restart → export/import scenario and rationale. Revoke source access and inspect old snapshots, search and events.

G08: feedback report/contact → duplicate triage preserving sources → issue/project → exact-version review → release readiness → reviewed publication → lost external response → reconcile once. General readers cannot retrieve the contact data.

G09: incident → correction preserving original event → explicit capacity proposal → approved scenario change → post-incident review → corrective issue → observed result. Incident response closure must not close the review or corrective tasks.

G10: create typed custom fields, disabled-module content and a multi-object template → failure after one step → resume without duplicates → native export → isolated restore/import → verify IDs/links/audiences/history. Run defined reports with missing estimates and source revocation; unknown values stay unknown.

Every G06–G10 scenario runs with AI disabled and a real API/database. Add real browser keyboard/narrow-screen checks and a second authenticated user. Export/restore checks must include all new table families and attachments, not only the first issue table. M21 audits the ledger rather than inferring complete scope from a smoke route.
