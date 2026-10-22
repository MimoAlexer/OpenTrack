# Autonomous agent execution architecture

Optional after M08/M09/M11 security and recovery gates. [Lifecycle](AGENT_LIFECYCLE.md), [security](AGENT_SECURITY.md), [Feature34](../features/F34_AGENTS.md), [provider contracts](PROVIDER_INTEGRATIONS.md).

## End-to-end contract

The accountable user selects an issue, allowed repository, base ref, runtime/provider, budget and execution policy. The controller resolves ref to immutable SHA with an authorized Git read, validates current grants and records the run before queueing. A run never receives all repositories merely because the GitHub App can see them. Assignment/delegation is a visible issue action; no ordinary issue text implicitly starts a process.

The core worker schedules orchestration; a separate runner host executes repository code. Runners have no PostgreSQL credentials, application encryption keys, production filesystem mounts or GitHub App private key. The runner authenticates to a dedicated controller API with a registered machine identity. The controller dispatches immutable attempt specifications and short-lived operation capabilities. A compromised runner can falsify its own results; do not treat its test output as independent security review.

## Attempt specification

An immutable attempt contains workspace/run/attempt IDs, lease/fence token, requesting principal, repository ID, canonical allowlisted clone URL, base SHA, generated branch name, runtime identity/version/image digest, issue/context snapshot references, allowed actions, network allowlist, resource ceilings, approved test plan, deadline, artifact rules and policy digest. It excludes permanent credentials. Any change creates a new attempt or explicit grant revision and invalidates pending approvals.

Branch: opentrack/<issue-key>/<run-short-id>-<attempt>. Validate/sanitize components and pass Git args as argv, never shell text. Trusted fetch preparation obtains the allowed snapshot without executing repository code; the sandbox checks out that fresh clone or verified immutable mirror-derived copy into a unique attempt root. No shared mutable worktree or home directory between users/attempts. A Git worktree alone is not isolation. Disable hooks/unsafe config, submodule recursion and LFS/network fetch by default; explicit grants may enable checked dependencies. Do not execute repository setup before the sandbox is active.

## Supervisor and runtime adapter

Rust supervisor owns process tree, stdout/stderr reader tasks, cancellation token, cgroup/VM limits, working directory and artifact collection. It launches the pinned official runtime with a clean environment and trusted configuration under an attempt-specific home. Adapter parses documented structured output and maps it into agent events. Unrecognized protocol version fails preflight; unknown additive event fields are tolerated but do not imply success.

Initial candidate Codex adapter uses documented noninteractive execution; rich app-server interaction is added only when its benefits justify protocol complexity. Claude adapter uses documented programmatic CLI or an isolated official SDK bridge with API-key mode. Settings/flags are pinned and checked from each runtime's current help/schema in M12. Never rely on an undocumented endpoint, parse pretty terminal text as a stable API, or assume a CLI permission flag supplies OS isolation.

Run phases: checkout; optional approved dependency preparation; model work; capture patch; controlled validation; freeze final artifact; await review; optional publication. Each phase records start/end/outcome, revision and resource usage. Build/test scripts are untrusted repository code and execute under the same isolation/network policy; a lockfile does not make dependency scripts safe.

## Validation and artifacts

The grant declares test commands as argv or a trusted recipe identifier, allowed working directory and timeout. Agent may propose additional tests, but cannot expand host/network permissions. Store exact commands, runtime versions, exit status, duration, stdout/stderr digests and head SHA. A successful model message is not a test result. Tests may modify tracked files: either restore generated changes after inspection or regenerate the final commit and rerun required tests. The artifact approved for publication must be the same head SHA tested.

Artifacts: diff/patch, changed-file inventory, a bounded Git bundle containing the approved commit/tree objects and required base linkage, final commit metadata, test manifest, structured summary and redacted log chunks. Each has SHA-256, size, attempt and head SHA. Reject absolute paths, traversal, symlink/hardlink escapes, device files, .git replacements and artifacts exceeding policy. Maximum default artifact 100 MiB, raw logs 50 MiB; when log limit is reached record truncation and retain terminal summary separately. Large binary changes require explicit review and scoped publication policy.

## Logs and progress

Supervisor emits monotonically increasing sequence numbers per attempt; uploads acknowledge highest contiguous persisted sequence. On reconnect resend unacknowledged chunks; controller deduplicates. Chunks max 64 KiB; terminal states use a separate bounded durable event so truncation cannot hide outcome. UI log view renders escaped text and only a bounded window; pause-follow protects reading. Redact known credential patterns/canary secrets but do not claim redaction can detect every secret. Avoid injecting real secrets in the first place.

Progress is stage plus latest factual action, not invented percentage. Expose elapsed time, budget remaining when known and queue reason. No model-generated estimate is labeled measured progress.

## Review and external publication

M12 can deliver an immutable patch with no remote write. M13 adds draft pull requests. Review shows repository/target branch/base/head/diff digest, test manifest and proposed title/body. Approval authorizes push+draft-PR only for this artifact and policy; no merge/deploy. Approval expiry 30 min by default. Project admins may require reviewer distinct from requester; server enforces the configured rule, not a model prompt.

A trusted publisher outside the sandbox retrieves the approved artifact bundle and patch, verifies digests and permissions, imports only the expected objects/refs into a clean isolated verification checkout without hooks/config, and independently applies the patch to the approved base to compare its resulting tree. It then pushes the exact approved commit object, preserving the tested head SHA, only to the generated branch using a narrowly scoped installation credential. Verification holds no publication credential; credentials are obtained only after object/tree/base checks succeed. Recreating a commit from a patch with different metadata is not an acceptable substitute for the approved head. Reject changes that need extra GitHub workflow permission until explicitly granted. It must not execute build scripts while holding publication credentials. Branch creation and PR operation have durable markers; lost responses trigger remote reconciliation by repository/head branch/marker before any retry. Never force-push a user's branch or overwrite another run.

If base moved, show stale-base state and choose review/rebase policy. Default requires a new attempt/rebase, rerun required tests and fresh approval. Concurrent runs may generate conflicting patches but stay on separate branches; no automatic merge between agents. Repeated run on same issue warns and may queue under a per-issue publication mutex, while preserving independent attempt history.

## Capacity and delivery gates

Default at most 2 concurrent attempts/workspace, each 2CPU/4 GiB/10 GiB and 30 min wall duration, separate from four core I/O workers. Scheduler reserves resources before claim and uses weighted round-robin workspace fairness; aged jobs become visible, not silently starved. Max queue depth 100/workspace is a proposed hard admission limit. Canceled/expired grants cannot consume a new slot.

Before enabling execution, demonstrate host isolation, egress denial, clean credential environment, cancellation of grandchildren, resource enforcement, replay-safe logs, stale fence rejection, test-head matching, artifact tamper detection and recovery after controller/runner crashes. These are unperformed M12/M13 tests, not Phase 0 evidence.
