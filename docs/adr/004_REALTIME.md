# ADR-004 SSE with durable commit-ordered replay

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

Clients need fast invalidation, reconnect recovery and permission-safe multi-instance delivery without a dedicated broker.

## Options and tradeoffs

WebSocket duplex is useful for CRDT but unnecessary for entity invalidations. SSE fits HTTP delivery. NOTIFY/Redis pubsub alone cannot provide durable replay. Plain DB sequences can be allocated out of commit order.

## Decision

REST writes plus SSE; transactional workspace stream-head lock assigns replay order immediately before commit. NOTIFY is a hint; each API node independently tails persisted events with a two-second safety poll.

## Consequences and risks

Hot-workspace append lock is a throughput cost; short critical sections and benchmark trigger protect it. Bounded queues disconnect slow clients. Permission epochs force cache reset; at-least-once delivery needs version-aware dedupe.

## Reconsideration and validation

Revisit CDC/commit-LSN ordering if lock tail dominates latency; WebSockets when actual collaborative editing ships. Tests must reverse commit order and lose wakeups. [NOTIFY](https://www.postgresql.org/docs/current/sql-notify.html).

See [canonical specification](../REALTIME_ARCHITECTURE.md).
