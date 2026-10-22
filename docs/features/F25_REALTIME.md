# F25 Realtime collaboration and synchronization

Stage: MVP entity sync; text collaboration later. Delivery: [M06_DISCOVERY](../milestones/M06_DISCOVERY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Keep teammates current while preserving edits and making disconnected or conflicting state visible.

## 2. Functional requirements

Entity invalidation via SSE, optimistic metadata changes, durable replay, version-checked writes and permission-epoch reset. MVP collaboration means shared issue/comment state, not simultaneous character editing or presence. CRDT editing is M14.

## 3. Interaction patterns

Updated rows retain focus/selection; edited field warns about remote change. Offline banner indicates read-only operation, preserved draft and explicit retry. Connection status is unobtrusive until action is needed.

## 4. Data requirements

workspace_events and stream-head sequence, entity versions, mutation IDs; independent per-node replay cursor; no client-wide database replica.

## 5. API requirements

/bootstrap returns snapshot+cursor; /events streams authorized invalidations/checkpoints; REST remains mutation channel. Reset_required triggers refetch after expired/oversized replay.

## 6. Authorization rules

Authorize each emission; no private entity identifiers in hidden events. Connection/session revocation closes stream and clears scoped cache.

## 7. Edge cases

Write response arrives after event, duplicate/late event, reversed transaction commit, lost NOTIFY, slow client, multi-tab limit and actor removed during replay.

## 8. Failure behavior

Disconnect on bounded queue overflow, then replay or bootstrap. Never drop an event silently while claiming current state. Outage does not acknowledge unsaved client writes.

## 9. Performance considerations

p95 commit-to-delivery<=500 ms connected baseline; two-second safety poll; max 256 events/1 MiB queued per client. Track DB read amplification.

## 10. Feature dependencies

F04, F06, transaction outbox, F17 queries; all mutable features reuse it.

## 11. Acceptance criteria

F25-A1: reversed commits cannot skip updates. F25-A2: reconnect converges without duplicate visible changes. F25-A3: stale local edit is preserved on 412. F25-A4: revocation clears scope.

## 12. Testing strategy

Two-client/two-node tests, chaos listener reconnect, transaction ordering, hidden checkpoint, cache-race model tests and slow-consumer load.
