# Contributing to OpenTrack

OpenTrack is under active development in a private repository. Start with the [current handoff](docs/PROJECT_HANDOFF_2026-10-18.md), [requirements ledger](docs/REQUIREMENTS_STATUS.md), and [implementation plan](docs/IMPLEMENTATION_PLAN.md). The roadmap includes unfinished work; a design or successful build does not establish completion.

Use the bug or feature form for a new report, or link an existing issue before making a substantial change. Include the relevant feature, expansion, or milestone IDs when known. Follow [SECURITY.md](SECURITY.md) for vulnerabilities.

## Local setup

The current development path is Linux with Bash, Python 3, PostgreSQL 18, the Rust toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml), and Node supported by [web/package.json](web/package.json). CI pins Node 22.22.1. See [runtime setup](docs/RUNTIME_SETUP.md) for system libraries, alternate ports, and configuration.

From the repository root:

```sh
scripts/dev.sh
```

This prepares or reuses the private development database, applies migrations, installs locked frontend dependencies, builds the app, and starts the API and separately credentialed worker. Open `http://127.0.0.1:18970`. First setup uses the token in `work/postgres/setup-token`; keep it private. The launcher refuses an occupied port and stops its own children on exit.

`work/postgres` contains durable local data and private credentials. Do not delete it to repair a failed run, point tests at development data, or attach it to an issue. Attachment storage lives outside the checkout; its configuration is documented in the runtime guide.

## Make a focused change

- Read [AGENTS.md](AGENTS.md) and the relevant feature, milestone, and ADR. Preserve stable requirement IDs and unrelated changes.
- Keep domain rules and authorization in Rust. Test current workspace/private-team access, not just a successful request.
- Preserve drafts on conflicts and uncertain outcomes. Retry durable creation with the same idempotency key; do not hide failed or unfinished work behind success states.
- Add forward migrations under `crates/opentrack-store/migrations/`; do not rewrite a migration already applied to a shared database. Cover upgrade/backfill, required effects, and recovery implications.
- Commit lockfiles with dependency changes. Generate wire contracts from Rust rather than editing generated TypeScript or OpenAPI by hand.

After changing API handlers or DTOs:

```sh
npm --prefix scripts/contracts ci --no-audit --no-fund
scripts/generate-contracts.sh
```

Include the resulting `docs/api/openapi.json` and `web/src/generated/api.d.ts` changes with their source.

## Verify the affected behavior

The full local/CI entry point is:

```sh
scripts/dev-db.sh setup
scripts/check.sh
```

It requires four distinct runtime, migration, operator, and worker identities in the same isolated `_test` database. It checks formatting, strict Clippy, real PostgreSQL contracts, deployment helpers, generated-contract consistency, frontend tests, TypeScript, and the production frontend build. It installs locked dependencies and rewrites `web/dist`; coordinate with anyone serving those assets. Use [runtime setup](docs/RUNTIME_SETUP.md#verification-and-ci) for an externally provisioned test database.

The first [hosted CI run](https://github.com/MimoAlexer/OpenTrack/actions/runs/39154336914) failed on 18 October 2026 because the runner ran out of disk space. CI now disables development/test debug information and incremental compilation to reduce disk use, while retaining every check and reporting workspace disk usage. The [replacement run](https://github.com/MimoAlexer/OpenTrack/actions/runs/39156790752) passed, and the [20 October implementation checkpoint](https://github.com/MimoAlexer/OpenTrack/actions/runs/39387487712) passed the full Linux suite. Check the current pull-request runs before merging; historical results do not qualify new changes.

Use focused checks while iterating, for example:

```sh
source "$(scripts/dev-db.sh env-path)"
cargo test --locked -p opentrack-app --test postgres_contract
npm --prefix web ci --no-audit --no-fund
npm --prefix web test -- src/api.test.ts
npm --prefix web run typecheck
```

Choose the contract/test file that exercises the change. PostgreSQL authorization, constraints, transactions, and concurrency need real database tests; mocks do not replace them. Add regressions for meaningful failure paths rather than tests that repeat the implementation.

Desktop changes also need `desktop/scripts/check.sh`; Linux prerequisites and packaging are in the [desktop guide](desktop/README.md). That separate suite is not included in the root check. Native core tests and a package build do not establish native-window, file-dialog, vault, or installed deep-link behavior.

For UI changes, verify the real API-backed workflow, keyboard/focus behavior, loading and error states, narrow layout, and both themes as affected. For data changes, verify current access, stale versions, retry behavior, and migration/recovery paths. Keep test fixtures isolated from a running personal workspace.

## Submit a reviewable pull request

`main` requires a pull request, an up-to-date branch, resolved conversations and the `verify`, `runtime (ubuntu-24.04, amd64)` and `runtime (ubuntu-24.04-arm, arm64)` GitHub Actions checks. These rules also apply to administrators; force pushes and branch deletion are disabled. There is no mandatory reviewer count, so the authorized maintainer may merge a verified, reviewable change after all required checks pass.

Both workflows run for every pull request, pushes to `main`, and manual dispatch. Feature branches without a pull request can request a full run through `workflow_dispatch`. Workflow-level path filters are deliberately absent: documentation-only pull requests must still receive every required check instead of remaining pending. A lightweight scope job examines the cumulative pull-request diff from its verified merge base to head, or the push's before/after commits. Known prose-only changes complete the required job successfully without installing toolchains, starting PostgreSQL or building containers. Desktop-only changes retain implementation checks but do not rebuild the container. Source, configuration, deployment, scripts, workflow changes and unclassified paths retain the full relevant suites; source directories take precedence over Markdown file extensions.

Manual dispatch and initial/deleted zero-SHA push ranges always select full checks. Invalid/missing revisions, malformed event data and Git diff failures cannot produce a successful skip: required jobs run even when scope detection fails and explicitly fail their scope guard. Scope decisions use NUL-delimited Git paths and disable rename collapsing, so moving a source file into documentation cannot hide its deletion. Run `python3 scripts/test-ci-scope.py` when changing this classifier or its workflow integration.

Explain the problem, resulting behavior, exact checks and outcomes, and material limitations. Include screenshots for visual changes and migration/rollback notes for data changes. State which browser or native environment was actually exercised; explicitly identify anything unrun.

Update the canonical contracts and acceptance evidence for changed behavior. Evidence belongs in `docs/evidence/` when it supports a requirement or release gate; record the environment, source revision, commands, results, and limitations. Redact credentials and private content. Mark a ledger item complete only when its whole acceptance condition is evidenced, and do not close a broader roadmap issue for a partial implementation.

Project source uses [AGPL-3.0-or-later](LICENSE). Contribution terms and any CLA/DCO requirement remain unratified. Coordinate third-party material and contribution permissions with the maintainer; see [ADR-015](docs/adr/015_LICENSE.md).
