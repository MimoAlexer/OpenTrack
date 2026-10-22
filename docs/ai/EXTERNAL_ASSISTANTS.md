# OpenTrack tools inside external assistants

This is a separate optional M14 ingress integration, complementing model inference and coding execution. It lets a user of ChatGPT/Codex/Claude-compatible clients read or propose changes to OpenTrack through documented tools. It does not import those products' subscriptions or conversation history into OpenTrack.

OpenAI documents plugin MCP authorization with OAuth resource-server metadata and client authorization flows. [Plugin authentication](https://developers.openai.com/plugins/build/auth). The [MCP authorization specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization) is the protocol baseline; pin a supported version and test the actual selected hosts. Do not assume every assistant product or account exposes the same connector features.

## Proposed tools and authorization

| Tool | Scope | Behavior |
| --- | --- | --- |
| search_issues | issues:read | Same ACL/filter/limit as REST; safe snippets |
| get_issue / get_project | issues:read / projects:read | Returns authorized content and current version |
| list_my_work | issues:read | Bound to authenticated OpenTrack subject, not arbitrary username |
| propose_issue / propose_update | issues:write | Returns exact structured proposal and approval link; explicit apply uses version/idempotency |
| add_comment | comments:write | Explicit authorized write with actor/client attribution |
| request_agent_run | agents:request | Creates a grant review request, never bypasses run wizard/approval policy |
| get_agent_run | agents:read | Only runs/artifacts visible under normal policy; redacted logs by default |

For local clients, a Rust stdio bridge can use a short-lived scoped OpenTrack token from its own protected credential store. For remote assistants, Streamable HTTP requires reachable HTTPS plus a compliant OAuth authorization server/resource server design. A private LAN installation need not expose itself or buy a tunnel; local mode or export remains useful. Remote connector reachability is an optional operator choice.

Use a maintained OAuth server/identity component and explicit client registration rather than inventing a permissive token endpoint. Q04 selects the component and verifies public-client PKCE, metadata, resource/audience binding, refresh/revocation and consent. Scopes map to current OpenTrack subject/workspace and can only narrow access. Never accept a token issued for an AI provider as an OpenTrack access token. Do not pass provider tokens through MCP.

Every tool includes stable versioned schemas, bounded outputs, idempotency for writes and explicit error mapping. Tool descriptions disclose consequential effects; client-side confirmation is useful but server-side grants remain mandatory. Reauthorize on each tool call. Treat repository/issue text as untrusted even when returned by a trusted transport.

M14 acceptance: read/search in one selected real client, rejected wrong audience, revoked user cannot read, stale version cannot overwrite, repeated write key creates one effect, requested agent cannot skip approval, local-only installation remains fully operational. No plugin has been created or published during Phase 0.
