# ADR-005 PostgreSQL jobs, leases and receipts

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

Background effects must survive crashes and retries while a small install remains operable.

## Options and tradeoffs

In-memory tasks lose work on restart. Redis/AMQP queues add operational dependencies and cross-store atomicity. PostgreSQL can atomically record intent and domain state but needs careful lease/vacuum management.

## Decision

Use jobs with SKIP LOCKED claims, bounded retries, attempt history, renewable leases and fences; per-consumer receipts make DB effects idempotent. External writes have operation records and reconciliation. Agents have a separate state machine.

## Consequences and risks

No exactly-once external effect guarantee. A lease does not kill zombie processes. Fair queues and separate CPU budgets prevent imports/agents starving interactive features. Retention must not erase unprocessed evidence.

## Reconsideration and validation

Revisit an external durable queue after measured DB contention/volume justifies it. Kill-after-commit and lost-external-response tests are required before release. [PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html).

See [canonical specification](../ARCHITECTURE.md).
