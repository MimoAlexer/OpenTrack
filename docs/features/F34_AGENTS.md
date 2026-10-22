# F34 Autonomous coding agents

Stage: Optional after security gates. Delivery: [M12_RUNNERS](../milestones/M12_RUNNERS.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Delegate a bounded engineering task and receive inspectable evidence and a reviewable change.

## 2. Functional requirements

Approved expansion obligations: X45, X53, X54. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Explicit issue/repository/provider grant, immutable base SHA, isolated attempt environment, tests, artifacts, human review and optional draft PR publication in M13. Persistent states, fencing, cancellation and reconciliation; no automatic merge/deploy.

## 3. Interaction patterns

Run wizard previews repository/ref, capabilities, network, budget and expected tests. Dashboard shows queued/running/approval/failed states; detail provides logs, diff, exact test revision and cancel. Human assignee remains accountable.

## 4. Data requirements

agent_runs, grants, attempts, events, artifacts, approvals and runner_hosts; immutable artifact digest and grant hash; no repository credentials in issue text.

## 5. API requirements

Run request/status/cancel/retry, artifact/log read, approval decision; separate internal machine protocol. User retry starts a new attempt after previous environment is confirmed stopped/quarantined.

## 6. Authorization rules

Intersection of user/team/repository/provider/runner grants. External write requires reviewer approval bound to digest and current permission; loss of grant fences further effects.

## 7. Edge cases

Runner lost, expired lease, process ignores signal, test modifies tracked files, base branch moves, two runs touch same issue, model says success without tests and remote PR response lost.

## 8. Failure behavior

Persist cancel intent, revoke broker access, supervised TERM/KILL and cleanup; uncertain side effects become reconciliation_required. No blind retry of arbitrary commands or publication.

## 9. Performance considerations

Default 2 concurrent runs/workspace,2CPU/4 GiB/10 GiB per attempt,30 min wall limit; fair scheduling and separate core job capacity.

## 10. Feature dependencies

F26 Git, F33 provider, F04/F30, M08 recovery and M12 isolation; M13 publication.

## 11. Acceptance criteria

F34-A1: hostile repo cannot access host secrets/socket. F34-A2: stale lease cannot publish. F34-A3: approved digest differs from diff => blocked. F34-A4: cancellation leaves no running child or usable broker grant.

## 12. Testing strategy

Hostile repo canary suite, process-tree and resource-limit tests, kill/restart/lease chaos, artifact tamper, lost PR response and explicit exact-SHA review integration test.
