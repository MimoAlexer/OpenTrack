# Risks and assumptions

Risk ratings are qualitative planning judgments, not measured probabilities. L/M/H mean low/medium/high. [Open decisions](OPEN_QUESTIONS.md), [review](ARCHITECTURE_REVIEW.md).

| Risk | Likelihood / impact | Mitigation and trigger | Owner / gate |
| --- | --- | --- | --- |
| Scope resembles years of product work | H/H | AI-free MVP,80 bounded tasks, advanced independent tracks; stop expanding scope without evidence | Product / each milestone |
| Private content leaks through derived surfaces | M/H | One policy, scoped FKs/RLS, negative search/event/export/AI tests; any leak blocks release | Security / M02 onward |
| Event cursor skips late commit | M/H without design | Transactional stream-head order and reverse-commit test; no sequence-as-commit assumption | Backend / M03/M06 |
| Stream-head lock becomes bottleneck | M/M | Short append tail, measured hot-workspace workload; CDC reconsideration trigger | Backend / M06/M08 |
| Worker/agent retry duplicates external action | M/H | Receipts, fences, persistent markers and reconciliation; no exactly-once claim | Integrations / M09–M13 |
| Provider authentication policy changes | H/M | Dated official research, adapter capability gates, API/local defaults | AI owner / M11–M12 |
| Agent escape or secret exfiltration | M/H | Separate hosts/VM boundary, broker outside, current grant, adversarial suite | Security / M12 |
| Runner fabricates test evidence | M/H | Treat logs as untrusted, immutable digests, external CI and human review | Reviewer / M13 |
| Backup misses blobs/keys | M/H | Manifest and suspended GC, separate key custody, isolated restore test | Operator / M08 |
| Rich text or upload becomes XSS | M/H | Restricted schemas, safe renderer, active-content download, hostile fixtures | UI/security / M04 |
| Next preference departure surprises future implementer | L/M | Explicit ADR and rationale; no accidental Next scaffold | Maintainer / M01 |
| Dependency/MSRV/peer drift | H/M | Registry evidence, exact graph resolution and tests; prefer compatible versions over latest | Maintainer / M01/releases |
| Local budgets mistaken for hard provider spending limit | M/H | Distinguish estimates, reported usage and provider-enforced caps; explicit fallback only | AI/product / M11 |
| Accessibility regresses under virtualization/density | M/H | Keyboard/AT modes, nonvirtualized fallback, contrast and zoom gates | UI / all screens |
| Self-host footprint exceeds expectations | M/M | Profile whole topology; no Redis/Node server/search daemon baseline; measured claims only | Operator / M08 |
| License/name/distribution conflict | M/H | Ratify before public release; no copied assets; optional provider binaries separately reviewed | Maintainer / release |
| Long-running exports block vacuum | M/M | Bounded snapshots, batched transfer, maintenance export for larger datasets | Backend / M08 |
| Documentation drifts across many files | M/M | Stable IDs, canonical ownership, link/coverage check and ADR review | Every contributor |

Assumptions: individuals and small-to-medium software teams are first users; operators can provide a PostgreSQL-capable Linux host and backups; application is desktop-first but responsive; English first with locale-aware formatting; one team per project is acceptable for MVP; ordinary collaboration needs entity updates before CRDT; a single-node install can accept planned upgrade downtime; cloud features are optional; no commercial licensing/billing system is needed.

Assumption validation: product task studies and MVP use test workflow needs; M01 resolves toolchain; M04 editor usability; M06/M08 benchmarks and restore; M11 provider modes; M12 hostile execution; M14 desktop/advanced needs. If a major assumption fails, revise the ADR and dependent data/API/UX contracts before implementing a workaround.
