# Agent 4 runner and downstream gate inventory

Date: 22 October 2026. Inspected source: `4072705406ec0e154a2556869b1d3359a1804096`.

This is read-only prerequisite and test-contract preparation for #65. It does not establish execution, isolation, provider eligibility or publication acceptance. No runner was launched, no hostile fixture executed, no external publication attempted and no issue closed.

## Observed checks

- Read the current assignment, coordination protocol, canonical runner/security/lifecycle contracts and live GitHub issue bodies. Issues #65–75, #80, #81 and #86 were open with unverified acceptance checklists at inspection.
- Inspected application, API, adapter and migration sources. The current source has no product runner, machine-control listener, provider broker, agent-artifact publisher or MCP transport. The planned `runner_hosts`, `agent_runs`, `agent_grants`, `agent_attempts`, `agent_events`, `agent_artifacts`, `approvals` and `external_operations` families in [database design](../DATABASE_DESIGN.md) are contracts, not implemented schema.
- The current core worker claims only `workflow.status_replace` and `assignment.reconcile` in [worker.rs](../../crates/opentrack-app/src/worker.rs). Its database fencing is not evidence that a repository process or its children can be terminated.
- The [implemented settings](../configuration/IMPLEMENTED_SETTINGS.md) do not accept future runner/provider configuration. The larger [configuration catalog](../CONFIGURATION_POLICY.md) is a target.
- Ran one read-only capability inventory through `work/wsl.ps1` in the assigned development context. `/dev/kvm` existed; `os.access(..., os.R_OK | os.W_OK)` returned false. `shutil.which` returned no command for `firecracker`, `cloud-hypervisor`, `qemu-system-x86_64`, `virsh`, `docker` or `podman` in that context's `PATH`. This describes the inspected account and environment only; it does not qualify a VM engine or establish what another host supports.
- Inspected `work/agent-env.sh`: distinct PostgreSQL/API ports, state/build directories, one Cargo build job and a distinct Compose project separate development resources. These controls do not supply the product's hostile-repository security boundary.

## Dependency and acceptance gates

| Work | Required gate | Evidence still needed |
| --- | --- | --- |
| #65 / M12-T01 | M08, M09, M11; Q03 profile decision | An actual supported runner host/image, selected profile and independently observed containment, cancellation and resource/egress tests. The inherited milestones remain incomplete in the current ledger. |
| #66 / M12-T02 | #65 | Workspace-bound one-use enrollment and mTLS; immutable specifications; fenced claim/renewal and bounded fair admission. |
| #67 / M12-T03 | #66 | Trusted snapshot fetch separate from execution; clean home/environment; brokered network; enforced resource limits; verified process-tree stop. |
| #68 / M12-T04 | #67; Q02 additionally for Claude runtime | A pinned supported runtime/protocol, truthful terminal/interrupted states and a safe credential boundary. No runtime or provider-account qualification was attempted here. |
| #69 / M12-T05 | #68 | Persisted bounded replayable logs, immutable bounded artifacts, exact tested head and manifest, and connected review UI. |
| #70 / M12-T06 | #69 | Controller/runner/lease-loss and stale-fence tests. A replacement attempt starts only after the old environment is confirmed stopped or quarantined. |
| #71–75 / M13 | M12 and M10, followed by each preceding publication task | Exact artifact approval; separate clean object/tree verification; exact-head push; concrete draft-PR preview and durable marker; lost-response/revoke/cancel reconciliation; G04 evidence. |
| #80 / M14-T05 | M10; Q04 | Selected maintained OAuth component and actual client/version; resource/audience/consent/refresh/revocation behavior; host connectivity and real selected-client tool validation. Remote MCP remains disabled until its gate passes. |
| #81 / M14-T06 | M08 for scale; M09 for other forges; M13 for agent refinements | Comparable measurements before added infrastructure, and provider-specific authorization/reconciliation evidence before claiming another forge. Search implementation is owned by Agent 3. |
| #86 / M15-T05 | #84 and Agent 5's #85 | Redacted connection/retry UI, safe eligible retries and an actual authorized expansion export/import round-trip including typed fields, preferences and partially completed template runs. |

The [runner milestone](../milestones/M12_RUNNERS.md), [publication milestone](../milestones/M13_PUBLICATION.md), [advanced tracks](../milestones/M14_ADVANCED.md) and [workspace expansion milestone](../milestones/M15_WORKSPACE_EXPANSION.md) own these prerequisites. This record does not waive them.

## #65 test contract to execute after prerequisites

The selected profile must satisfy [ADR-011](../adr/011_AGENT_ISOLATION.md) and [agent security](../ai/AGENT_SECURITY.md). The documented trusted-owner profile is explicitly limited and requires its own validation. Hostile repositories require the disposable VM boundary selected by Q03. Agents remain disabled until the applicable profile passes.

| Boundary | Required fixture and independent observation |
| --- | --- |
| Host/filesystem | Attempt host-canary, socket, process-environment and unauthorized mount reads; observe host state as well as runner errors. Exercise artifact traversal, symlink/hardlink escapes and cache poisoning. |
| Network/credentials | Attempt private/link-local/metadata access, unsafe DNS/redirect destinations and requests outside the broker grant. Verify denied effects and grant expiry without supplying permanent provider or Git credentials to execution. |
| Resource/process lifetime | Exercise bounded fork, memory and disk pressure in the selected isolation environment; cancel grandchildren and verify the environment stopped. Unknown cleanup remains `reconciliation_required`. |
| Fencing/recovery | Lose controller contact, renew an old fence, repeat dispatch and revoke grants during work. Verify no old attempt can finalize artifacts, consume broker capability or cause publication. |
| Evidence | Interrupt streams/tests, replay log chunks and forge terminal events. Preserve failed/not-run tests, truncation and exact head. A model success message is not test success. |
| Review/publication | Tamper with patch/bundle/head/test/policy bindings; expire or revoke approval. Verify clean publisher rejection before credentials are acquired. Publication tests belong to M13 and cannot be inferred from repository maintenance PRs. |

The [lifecycle contract](../ai/AGENT_LIFECYCLE.md) requires 20-second renewals with 60-second leases and local termination enforcement. The [execution contract](../ai/AGENT_EXECUTION.md) requires separate fetch, controller, broker and publisher boundaries; exact tested commit identity; bounded logs/artifacts; and no automatic merge or deployment.

## Existing reusable verification foundations

[Application test support](../../crates/opentrack-app/tests/support/mod.rs) provides fresh real-PostgreSQL schemas, restricted runtime roles and version-selective migrations. [API test support](../../crates/opentrack-api/tests/support/mod.rs) exercises actual routers. Existing worker/recovery race tests are useful patterns, but cover core database effects rather than runner lifetime.

[Browser configuration](../../web/playwright.config.ts) uses one worker and the dedicated loopback port 18081. [The packaged browser launcher](../../scripts/browser-acceptance.py) explicitly requires the configured Linux GitHub CI identity and disposable project. Do not spoof that identity for local runs; use an authorized local fixture/API workflow under the shared browser lock, or the configured CI path. Full/heavy checks use the verification lock. This audit ran neither workflow.

[Performance strategy](../PERFORMANCE_STRATEGY.md) defines the reference dataset/load, warm/cold conditions, percentiles, fairness and scaling triggers. Its 100k-issue/skewed-private-team fixture, steady/burst runs and soak remain required measurements, not observed capacity. The current audited scripts contain no implementation of that reference-scale benchmark. Preparing the deterministic fixture and measurement contract can proceed without adding Redis, a search engine or replicas; integrated acceptance still needs its source features and M08 gate.

## Safe next preparation

Continue dependency-first M08/M09/M11 coordination and current provider/authentication evidence. Preserve the runner-disabled boundary. Within #65, prepare the host/profile decision and bounded adversarial test plan; a current account permission/tool inventory is not a reason to substitute ordinary processes or worktrees for the required boundary. No downstream runner implementation is made acceptance-ready by this document.

After their prerequisites pass, useful vertical slices are persisted immutable attempt/grant and fencing state; clean fetch/supervisor controls; then exact-head evidence and review. Coordinate migration numbering with Agent 5 before SQL is published. Coordinate export registration with Agent 5 and scale/search contracts with Agent 3. Provider and real-client gates remain explicit alongside locally testable contracts.
