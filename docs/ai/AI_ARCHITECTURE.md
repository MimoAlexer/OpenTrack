# Provider-neutral AI architecture

AI is an optional application module and optional adapters. Tracking, search, authentication, exports and notifications remain usable when it is disabled. [Feature33](../features/F33_PROVIDER_CONNECTIONS.md), [Feature35](../features/F35_AI_CONVERSATIONS.md), [provider evidence](PROVIDER_INTEGRATIONS.md).

## Separation of responsibility

AI core owns connection/grant records, context assembly, conversation persistence, capability negotiation, budgets, tool authorization and normalized errors. Providers own protocol translation and transport. Agent runtime owns repository execution and cannot be smuggled through an ordinary text completion interface. External assistants calling OpenTrack use an ingress API/MCP boundary, not an inference credential.

| Logical port | Inputs / outputs | Required semantics |
| --- | --- | --- |
| ConnectionVerifier | Credential reference + mode → identity/scope/status | Does not leak credential or treat model-list as entitlement |
| CapabilityProvider | Connection/version → supported modalities/tools/streaming/limits | Separate advertised, probed and disabled capabilities with reason |
| ModelCatalog | Connection → model IDs, display labels, limits if known | Account-scoped, cache TTL, unknown metadata allowed |
| InferenceProvider | Immutable request/context snapshot → normalized async event stream | Bounded stream; explicit terminal status, request correlation and cancellation handle |
| UsageReporter | Provider report + request record → observed/estimated usage | Unknown stays null; no invented cost or quota |
| ToolDispatcher | Versioned tool name/arguments + principal/grant → proposal/result | Validate schema, dedupe call ID, authorize at execution, approval for writes |
| CredentialBroker | Connection ref + scoped operation → transport authorization | Keeps long-lived secret outside browser and untrusted runtime |
| CodingRuntimeAdapter | Attempt spec → execution events/artifacts | Separate lifecycle and permission contract in AGENT_EXECUTION.md |

These are conceptual interfaces, not instructions to create a trait or crate for each row. Implement only useful substitution boundaries. Rust app modules use cancellation-aware streams; use a TypeScript/Python SDK bridge only where official protocol behavior materially justifies it.

## Request lifecycle

Authenticate OpenTrack user, check active workspace/team and provider grant, validate selected capability/model, reserve configured budget, assemble context from authorized objects, persist request intent and first message, then call provider outside a database transaction. Stream deltas through a bounded channel; batch durable text every 500 ms or 4 KiB. Terminal event persists final content/status/usage and releases budget reservation. A dropped browser connection does not silently mean cancel; request policy states whether server continues or cancels, and status/cancel endpoints are always available. Initial default cancels when the sole subscriber disconnects for 30s unless the user chose a durable background task.

Normalized events: request.started, content.delta, tool.proposed, tool.started, tool.result, usage.reported, request.completed, request.failed, request.incomplete, request.canceled. Persist order per request. Provider terminal completion is required for completed status; EOF is interrupted. Network/proxy adapters parse incremental bytes correctly, cap event sizes and never accumulate unbounded tool JSON.

Context is explicit: issue body/comments selected by user, current project summary, authorized attachment excerpts when supported, and current grant/tool manifest. Record IDs, entity versions, content digests and redacted inclusion summary. Do not read all workspace data or private conversations by default. Recheck access before tool retrieval and before restarting a request. A summary has provenance to its source versions; it is not a new trusted instruction.

## Conversation and tool policy

Conversation visibility is private to owner in the first release. It is not automatically shared because it references a public issue. User may explicitly publish a proposed summary as a normal issue comment after reviewing it. Server stores its own conversation history; provider-side thread IDs are opaque resume hints, not a substitute for local durable messages or a promise of portable state.

Read tools initially: search_issues, get_issue, get_project, list_my_work. Write proposals: create_issue, update_issue, add_comment. Proposal includes exact validated arguments, affected object/version and scope; explicit user Apply executes a normal command. A model-generated tool name never selects arbitrary function/URL/shell. The dispatcher has a versioned allowlist, argument length limits, idempotent call receipt and maximum call chain. Agent shell tools belong only to an authorized isolated coding attempt.

A prompt-injected issue can influence model output but cannot change the grant, access other workspaces, reveal credentials, create a network destination or publish code. Treat model output as untrusted rich text and escaped code, never HTML or executable instructions to the host.

## Budgets, fallback and errors

Budget dimensions: maximum concurrent requests, elapsed time, context size, known token limits, API cost ceiling when enforceable and per-run resource limits for coding. Reserve an upper estimate for API requests where reliable limits exist; reconcile to reported usage. Streaming/cancellation and provider accounting can overshoot a local estimate. SIWC preview may not accept the same output-limit fields as API-key mode; show limits as local stop targets, not guaranteed provider spend caps. No automatic purchase, credit top-up, paid fallback or account rotation.

Errors normalize to authentication_required, permission_denied, rate_limited, quota_exhausted, unsupported_capability, context_too_large, provider_unavailable, interrupted, canceled and unknown. Keep a safe user message, retry hint and redacted provider code. Before any output, safe transient read/inference retry may occur once under explicit request policy; after partial output preserve it and offer new attempt. Side-effectful tools are never blindly replayed.

## Privacy and operation

Show provider and data scope before sending. Local inference can remain entirely inside the operator's network; allowlist the exact endpoint and avoid an SSRF exception for arbitrary private URLs. AI content is excluded from normal logs/metrics. Retention follows user deletion/workspace policy; deleting a local conversation cannot promise deletion of all provider-side logs. Document actual provider retention in connection help at implementation time without inventing universal zero retention.

Tests use deterministic fake providers for error/stream/tool behavior, plus opt-in live smoke tests with an authorized account when implementing each adapter. AI quality evaluation uses a versioned synthetic issue corpus, human-reviewed expected task outcomes and injection cases; no private production text is uploaded merely to run CI.
