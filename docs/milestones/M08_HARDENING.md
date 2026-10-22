# M08 Core MVP release and recovery

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP release. Dependencies: M07. Feature coverage: F28–F29; all core acceptance.

## Scope and deliverables

Make the complete AI-free product installable, portable, recoverable and supportable.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M08-T01 Transfer formats | M07 | Native manifest+blobs and CSV mapping/preview/resumable import/export with safe formula handling. |
| M08-T02 Administration | M07 | Quota/retention/job diagnostics, redacted support export, key rotation and local recovery commands. |
| M08-T03 Release packaging | M08-T01, M08-T02 | Nonroot immutable Compose images, explicit migrator, TLS/proxy streaming and offline assets. |
| M08-T04 Recovery rehearsals | M08-T03 | Nightly backup profile, DB+blob/key restore, corrupted/missing blob, failed migration and prior-release upgrade. |
| M08-T05 Quality campaign | M08-T04 | Golden flows, threat matrix, accessibility manual review, baseline/soak load and dependency/SBOM review. |
| M08-T06 Release governance | M08-T05 | Ratify license/name/contribution/security contact before public release; document known limits and operator guide. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

All MVP feature acceptance IDs, G01/G02/G03/G05, install/upgrade/restore evidence, private-team negative matrix and measured performance report.

## Risks and scope guard

Backup incompleteness, operator UX and unmeasured capacity are release blockers. License/name decisions gate public distribution, not local work.

## Completion condition

MVP works with AI/GitHub/SMTP disabled, data survives tested recovery, no critical unaddressed boundary failures and all release claims have evidence.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
