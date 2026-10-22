# M12 Isolated agent execution and patch delivery

Status: implementation authorized; current evidence is tracked in [requirements status](../REQUIREMENTS_STATUS.md). Release tier: Agent preview. Dependencies: M09, M11, M08 recovery. Feature coverage: F34 execution.

## Scope and deliverables

Produce a reviewable patch and exact test evidence with no remote publication privilege.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M12-T01 Runner threat/profile gate | M08, M09, M11 | Choose tested isolation profile; Q03 blocks hostile repo execution until VM boundary established. |
| M12-T02 Machine control plane | M12-T01 | Enrollment, workspace-scoped mTLS identity, immutable attempt spec, fenced claims/renewals and fair capacity. |
| M12-T03 Environment supervisor | M12-T02 | Separate clone/prep, clean home/env, network broker, resource quotas and process-tree cancellation. |
| M12-T04 Runtime adapter | M12-T03; Q02 if using Claude runtime | Pin Codex noninteractive or validated Claude adapter; structured events, terminal semantics and supported configuration. |
| M12-T05 Evidence pipeline | M12-T04 | Bounded replayable logs, immutable diff/artifacts, exact head/test manifest and review UI. |
| M12-T06 Crash recovery | M12-T05 | Lost controller/runner/lease, stale fence, interrupted model and explicit new-attempt retry; confirm old environment stopped. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

F34 nonpublication acceptance, hostile canary suite, resource/egress denial, grandchild cancellation, duplicate dispatch and artifact tamper tests.

## Risks and scope guard

Containers alone do not establish hostile isolation; provider-runtime key exposure can block a mode. No scope expansion to publication before this passes.

## Completion condition

An authorized issue yields a patch and truthful test record; no permanent credential/host access; cancellation and crash recovery prevent uncontrolled effects.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
