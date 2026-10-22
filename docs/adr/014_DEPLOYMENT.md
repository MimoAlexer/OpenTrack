# ADR-014 Compose first and restore before scale

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

An open-source product must be installable, upgradeable and recoverable by a small team.

## Options and tradeoffs

Kubernetes offers orchestration but raises baseline expertise. Single binary plus external DB is lean but Compose documents consistent topology. Managed cloud simplifies operations but cannot be mandatory.

## Decision

Ship Compose with API/static, worker, PostgreSQL and TLS proxy; agents optional separate hosts. Immutable versions, explicit migrator, encrypted DB+blob backups and rehearsed restore precede scaling.

## Consequences and risks

Single-node deployment has downtime; no HA claim. Operators own keys/TLS/backups. Multi-replica needs shared blobs and DB capacity. RPO/RTO are targets until measured.

## Reconsideration and validation

Advanced packaging after core operator experience and measured demand. M08 offline install/upgrade/restore gate. [PostgreSQL backup](https://www.postgresql.org/docs/current/backup.html).

See [canonical specification](../DEPLOYMENT_ARCHITECTURE.md).
