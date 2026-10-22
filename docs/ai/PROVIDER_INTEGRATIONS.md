# Provider integration capability matrix

This matrix separates documented mechanisms from OpenTrack's planned support. [Authentication research](AUTHENTICATION_RESEARCH.md) owns the evidence and restrictions; [architecture](AI_ARCHITECTURE.md) owns normalization. Checked 2026-10-17; no provider account was connected or billed during Phase 0.

| Provider/mode | Documented entry point | OpenTrack support path | Availability gate |
| --- | --- | --- | --- |
| OpenAI API key | Public Responses API, streaming and function tools according to model | Rust HTTP/SSE adapter; separate key-owned billing | M11 schema/capability fixtures and opt-in actual-account smoke |
| ChatGPT plan via SIWC | Eligible OSS/local plan-use flow and Responses restrictions | Dedicated connection mode, companion-assisted sign-in, restricted capability set | Q01 deployment eligibility, refresh custody and preview compatibility; off until resolved |
| Codex automation | codex exec structured output; official SDKs | Rust supervisor of pinned CLI first; SDK wrapper only if needed | M12 CLI output/exit/cancel/version contract verified |
| Codex rich integration | app-server JSON-RPC with auth/history/approvals/events | Later Rust protocol adapter over private stdio, schema pinned to runtime | Need interactive approval/history features; no exposed unauthenticated app-server |
| Anthropic API key | Messages API and streaming/tool use | Rust HTTP adapter | M11 model/tool capability test; credential rules apply |
| Claude Agent SDK | Official TypeScript/Python packages | Optional isolated TypeScript adapter process for supported callbacks | M12 SDK/runtime version and terms review; Rust core remains orchestrator |
| Claude Code CLI | Noninteractive -p and structured output | Optional CLI adapter with API-key mode, pinned published binary | Exact programmatic mode/permission behavior verified; no native credential import |
| User-signed-in Claude binary hosting | Narrow documented unmodified-binary scenario | Deferred deployment-specific research, not an inference provider credential | Distribution/auth conditions and security feasibility Q02 |
| Local inference | Ollama documented local HTTP API; other servers via named adapters | Optional native adapter; generic OpenAI-compatible mode only with declared tested subset | Admin endpoint allowlist and capability probe; no assumption of tool parity |
| External assistants using OpenTrack | MCP/plugin tools acting on OpenTrack data | Separate ingress bridge in M14 | OAuth/resource authorization and host connectivity validation |

## Codex interfaces: evidence and choice

Official documentation distinguishes the SDK for automation from app-server for deeper clients. The current SDK page documents TypeScript and Python and says the old Codex MCP server entry point has been removed. Avoid building a new integration around old codex mcp-server examples. [Codex SDK](https://learn.chatgpt.com/docs/codex-sdk).

The documented noninteractive interface is codex exec, with JSON output available and explicit sandbox controls. Its output, exit and final-state contract must be checked against a pinned release; no deprecated full-auto assumption. CLI authentication files are sensitive and not a generic product credential exchange. [Noninteractive mode](https://learn.chatgpt.com/docs/non-interactive-mode).

For a rich optional adapter, use documented app-server initialization, thread and turn operations over private stdio; generated schemas for the selected runtime determine exact messages. Do not expose its experimental network transport directly through the product proxy. [App-server](https://learn.chatgpt.com/docs/app-server). SIWC-to-app-server requires its documented provider setup and token renewal lifecycle; it is a distinct adapter configuration. [SIWC app-server](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server).

## Claude programmatic interface: evidence and choice

Claude documents noninteractive CLI output modes and a bare mode that skips much automatic configuration discovery. Read the pinned-version behavior carefully: repository hooks/configuration can otherwise affect execution, and bare-mode limitations differ by version. [Programmatic Claude Code](https://code.claude.com/docs/en/headless). Tool permission callbacks in the SDK are useful controls but do not replace OS isolation. [SDK permissions](https://code.claude.com/docs/en/agent-sdk/permissions).

The first candidate is API-key inference plus a separately installed optional coding runtime. Keep provider-branded runtime binaries outside the core image and preserve required terms/notices. If an SDK wrapper is needed, it is a small versioned adapter process with a bounded JSONL control protocol, not a new service owning business data. Do not claim that a Rust-native official Claude SDK has been verified.

## Contract-test matrix before claiming support

For each adapter version record auth mode, endpoint, model ID, advertised and probed capabilities, input limit, output/stream terminal semantics, tool-call schema, cancellation result, rate-limit format, usage availability, error examples and supported OS. Tests include fragmented SSE/JSON lines, split UTF-8, duplicate tool call, unsupported field, streaming error after text,401/403/429/5xx, connection loss and slow downstream consumer.

Model names are discovered or explicitly configured; no hardcoded latest alias in a product guarantee. A catalog entry means candidate availability; a successful request verifies only that request. Cache model metadata for five minutes and clear it on account/credential change. Store provider-reported usage separately from estimates and do not infer remaining subscription allowance from token count.

Compatibility with an OpenAI-shaped endpoint is a tested subset, not universal support for Responses, tools, images or structured output. Local servers can remain usable for plain text when richer features are unavailable. Cloud fallback always needs explicit user selection because it changes cost and data destination.
