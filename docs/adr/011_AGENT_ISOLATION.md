# ADR-011 Separate runners and exact artifact approval

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

Repository code and AI tools are untrusted and must not inherit core service privileges or publish unreviewed changes.

## Options and tradeoffs

Running inside API is simple but catastrophic for trust. Worktrees isolate files but not processes/secrets. Containers reduce accidental risk; per-attempt VMs offer a stronger hostile-code boundary at greater operational cost.

## Decision

Keep controller/broker/publisher outside sandbox; use fenced attempt grants and isolated runner hosts. Default no agents. Trusted-owner containers are explicitly limited; hostile repositories require approved VM profile. Publication requires exact patch/head/test/policy approval.

## Consequences and risks

VM cost/cold start and runtime credential compatibility are open gates. Cancellation needs process-tree enforcement, not a DB status. Compromised runner results remain untrusted until external CI/human review.

## Reconsideration and validation

No untrusted execution until Q03 and M12 security tests pass. No publication until M13 reconciliation/tamper tests pass. [Docker rootless scope](https://docs.docker.com/engine/security/rootless/).

See [canonical specification](../ai/AGENT_SECURITY.md).
