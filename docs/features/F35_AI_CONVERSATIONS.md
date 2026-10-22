# F35 AI conversations and tool execution

Stage: Optional post-MVP. Delivery: [M11_AI](../milestones/M11_AI.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Offer issue-aware assistance while keeping context selection, cost and tool effects visible to the user.

## 2. Functional requirements

Approved expansion obligations: X53, X54, X65. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Private conversations with selected accessible issue context, streaming messages, cancellation, persistent terminal status and usage. Read-only tools first; write tools propose structured actions requiring explicit user confirmation and ordinary authorization.

## 3. Interaction patterns

Conversation pane shows provider/account/model, context chips and usage status. Partial/interrupted response labeled; Retry creates a new attempt and never silently executes prior tool calls. Tool proposal shows exact fields before Apply.

## 4. Data requirements

conversations, ai_messages, ai_tool_calls, usage and context reference/digest. Store local history; do not assume providers retain it or expose existing chat account history.

## 5. API requirements

Conversation/message create/list, request-scoped streaming/status/cancel and tool proposal approve. Tool call IDs deduplicated; result persistence independent of token-delta batching.

## 6. Authorization rules

Owner-only conversations initially. Selected issue references reauthorized at context assembly/tool execution; copying text to another provider requires explicit selection.

## 7. Edge cases

Stream breaks after partial output, response incomplete, usage absent, context revoked, unsupported tool/schema, provider account changed and prompt injection in issue text.

## 8. Failure behavior

Only provider terminal completed means complete. Partial text survives with error status; no automatic tool replay or silent paid fallback. Unknown usage is unknown, not 0.

## 9. Performance considerations

Batch token persistence every 500 ms or 4 KiB; enforce context byte/token budgets and bounded stream queue; avoid one DB transaction per token.

## 10. Feature dependencies

F33 capabilities, F12 renderer, F04, F30 for applied tool effects.

## 11. Acceptance criteria

F35-A1: interrupted stream is never shown as success. F35-A2: tool text alone cannot mutate an issue. F35-A3: model context excludes revoked/private objects. F35-A4: repeated approval executes a tool effect once.

## 12. Testing strategy

Stream truncation/order fixtures, malicious tool arguments, context ACL changes, cancellation/usage tests and accessible live-output reading flow.
