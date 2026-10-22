# Developer experience and project standards

The repository contains the schema 21 Rust/PostgreSQL service, shared React client and a separate Tauri desktop workspace. These standards combine implemented conventions with explicitly pending release policies. Start with [CONTRIBUTING](../CONTRIBUTING.md), [runtime setup](RUNTIME_SETUP.md) and the [active handoff](../HANDOFF.md); consult [architecture](ARCHITECTURE.md), [configuration](CONFIGURATION_POLICY.md) and [roadmap](ROADMAP.md) for canonical contracts.

## Source organization

The root Cargo workspace contains six crates under crates/: domain, store, app, api, adapters and server (each prefixed opentrack-). The worker is a server mode. desktop/ is a separate Tauri workspace; web/ contains React feature modules, shared UI, generated API types and tests. Numbered SQL lives under crates/opentrack-store/migrations/. scripts/ owns development and verification commands; deploy/ owns Compose and image definitions; docs/ retains specifications and evidence. Add new components when their behavior exists.

Use Rust 2024 edition and a pinned maintained stable compiler after M01 checks the maximum dependency MSRV. Cargo.lock is committed for deployables. The web client and contract generator are independent npm packages with committed package-lock.json files. The hosted workflow pins Node 22.22.1; use the supported Node/npm versions documented in runtime setup. No nightly or global tool install is assumed. scripts/dev.sh manages the native local PostgreSQL/API/worker environment with separate test/dev credentials and ports. Compose deployment definitions exist, but full packaged execution remains a qualification gate. Editor tooling is optional; one documented terminal workflow must work without a proprietary IDE.

## Coding conventions

Rust: rustfmt, Clippy with warnings treated as errors for owned code, descriptive domain types, explicit Result errors, no unwrap/expect in request/worker paths except proven construction invariants with explanation. Domain validation is shared, serialization DTOs do not become database models accidentally. Use tracing spans with safe fields; never Debug a secret-bearing object. async functions document blocking work and cancellation behavior where nontrivial. Use bounded channels and supervised tasks. Public interfaces expose only necessary types; unsafe code needs a specific justification and independent review.

TypeScript: strict mode, no unchecked any escape in application paths, runtime validation at network boundaries, explicit null handling and stable typed query keys. Components use semantic HTML and central tokens; no duplicate permission truth, direct provider credentials or ad-hoc global state for server data. Separate route state, transient UI state and feature-specific server state. Editor JSON is a validated format, not arbitrary HTML. Generated clients are never hand-edited.

Naming: snake_case Rust modules/DB columns; PascalCase types/components; camelCase TypeScript locals; kebab-case routes; durable event names domain.action. Database tables plural; IDs UUID with clear type wrappers. Time units named explicitly. No helper called manager/util/service without a cohesive responsibility. Prefer a small concrete function over a framework that hides transaction ownership.

## Local workflow

Read relevant milestone/feature/ADR and capture expected behavior. Validate tool versions and configuration. Run scoped format/lint/type/unit checks, real database tests for changed SQL/permissions, and the actual UI/API flow for behavioral changes. For migrations, rehearse on a sanitized previous-schema fixture. Keep private production data out of local fixtures. Record exact commands and results in PR; failed/unrun checks are visible.

M01 chooses a small task runner only if it removes repeated commands while respecting native/TOML formats; a README command list is sufficient initially. Current checks use real PostgreSQL contracts and generated OpenAPI/TypeScript drift checks. SQLx offline metadata refresh is not part of the current pipeline. No schema drift patched manually in production. Local app runs with least-privilege DB role to catch RLS mistakes early.

## Git, review and contribution

main currently contains an incomplete development checkpoint, not a qualified release; short branches feat/<topic>, fix/<topic>, docs/<topic>. Prefer focused commits with conventional prefixes feat/fix/docs/refactor/test/chore; do not squash unrelated schema/security changes into vague cleanup. Small PRs target a vertical slice with feature/acceptance IDs, concrete before/after behavior and verified checks. Protected main requires CI and human review; security/tenant/crypto/runner changes need a reviewer familiar with that boundary. These are future repository policies, not remote settings created now.

Contributors receive CONTRIBUTING and CODE_OF_CONDUCT before public opening. Ratify license, contribution terms and security contact; do not invent an email or require a CLA without a maintainer decision. Recommend DCO sign-off over copyright assignment if governance selects it. Architectural changes use ADRs with alternatives and reconsideration triggers; update canonical docs instead of accumulating contradictory notes.

## Dependencies, releases and documentation

Review dependency updates weekly in bounded batches; security fixes expedited according to exposure. Do not merge automatically solely because tests pass. Inspect licensing, MSRV, peer dependencies, runtime behavior and release notes. SBOM and reproducible image digests accompany releases; test artifacts are promoted, not rebuilt. Version product with SemVer, pre 1.0 with explicit compatibility notes; API/config/import/document schemas version independently. Support current and previous minor application release during migration window, subject to security advisories.

Documentation states proposed/implemented/measured status and date. Research links official sources near claims; provider capability changes have a fresh evidence date. Each feature's data/API/UI/tests/milestone references stay aligned. Future Codex sessions begin with AGENTS.md and the milestone status, preserve unrelated work, avoid broad speculative refactors and report actual runtime validation separately from builds.
