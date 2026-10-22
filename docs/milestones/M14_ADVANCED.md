# M14 Advanced planning, collaboration and desktop

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: complete-product scope. Dependencies: M08; M10–M13 only for related tracks. Feature coverage: F16, F36 and scoped enhancements.

## Scope and deliverables

All original tracks are now authorized. Each still requires its focused ADR/contract and runtime proof. Advanced roadmap/initiative/scenario work is specified in F37/F38 and M16; M14-T01 retains the baseline F16 projection. Platform or provider limitations must remain explicit in the requirements ledger.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M14-T01 Roadmap track | M08; Q10 scoped contract | Accessible project timeline/list, then separately designed initiatives/dependencies; no hidden private counts. |
| M14-T02 Collaboration track | M08; Q10 scoped contract | Measure conflicts, select CRDT if justified, design persistence/compaction/schema/access revocation and restore. |
| M14-T03 Team/identity track | M08; Q10 scoped contract | Guests, multi-team projects, recurring cycles, MFA/passkeys and OIDC improvements with revised ACL/constraints. |
| M14-T04 Desktop track | M08; Q09 desktop support | Tauri static client, OS keychain/session flow, capability allowlist, deep links and signed update matrix. |
| M14-T05 Assistant ingress track | M10; Q04 host/auth evidence | Resolve Q04 OAuth component and host connectivity, implement scoped MCP tools with real selected-client validation. |
| M14-T06 Scale/forge track | M08 for scale; M09 for other forges; M13 only for agent refinements | Measure before Redis/Tantivy/multi-replica/Kubernetes; other Git providers use same bounded contracts. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F16/F36 applicable acceptance and dedicated ADR test plan per track; actual OS/client/protocol matrix before claiming compatibility.

## Risks and scope guard

Scope expansion can erase simplicity and security; each track must demonstrate user demand, owner and maintenance capacity.

## Completion condition

Each track is complete only with its scoped release evidence. A proven core release may ship earlier, but does not complete the full goal while an authorized advanced track remains unverified.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
