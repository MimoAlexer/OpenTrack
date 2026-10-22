# M01 Foundation and executable contracts

Status: in progress; the [foundation evidence](../evidence/2026-10-17-foundation.md) proves only the current slice. Full task completion remains governed by [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP foundation. Dependencies: None. Feature coverage: All subsequent features.

## Scope and deliverables

Establish the smallest reproducible Rust/API/static-client path and test architecture. No business feature is claimed complete.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M01-T01 Compatibility spike | None | Resolve/pin compiler and packages from research; record licenses/advisories, peer constraints and successful build; prefer the compatible tower-http line. |
| M01-T02 Workspace boundaries | M01-T01 | Create only the needed domain/store/app/API/composition modules; enforce dependency direction and Rust 2024/format/lint rules. |
| M01-T03 Typed settings | M01-T02 | Implement TOML merge/provenance/version/validation and secret redaction; invalid configuration prevents listening. |
| M01-T04 Contract slice | M01-T03 | Serve a health/capabilities API and static React route with generated TypeScript schema; deep-link fallback excludes API/asset 404s. |
| M01-T05 Design primitives | M01-T04 | Build token/theme/button/input/dialog/combobox fixtures and verify contrast/focus in real browser; no feature UI scaffolding beyond exercised slice. |
| M01-T06 Reproducible checks | M01-T04, M01-T05 | Add native-format lockfiles and mandatory CI/Compose exceptions, real PostgreSQL test harness and bounded graceful shutdown checks. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

Build with pinned toolchain; typecheck/generated schema drift; config precedence/redaction; smoke browser route; shutdown without orphan tasks. No installation of production DB outside the dev/test environment.

## Risks and scope guard

Compatibility major changes, component accessibility and generator limitations. Time-box each spike and update ADR if baseline fails; do not quietly replace stack.

## Completion condition

A new contributor can reproduce the same build/test slice from documented commands, all settings are validated, and no claimed feature exceeds the tested path.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
