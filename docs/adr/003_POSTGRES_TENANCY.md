# ADR-003 PostgreSQL and workspace isolation

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

Relational issue/team/project invariants must survive concurrent writes and tenant-scoping mistakes.

## Options and tradeoffs

Database per workspace offers strong operational separation but expensive migrations/pooling. Shared schema with app predicates is simpler but vulnerable to a missed filter. Shared schema plus composite FKs and workspace RLS adds defense in depth; private-team ACL still needs application policy.

## Decision

Use PostgreSQL 18 shared schema, workspace_id on scoped rows, composite FKs, nonowner runtime role and transaction-local RLS context. Authorization guards serialize revocation against writes. MVP projects belong to one team.

## Consequences and risks

RLS context contamination and role bypass are serious risks; real pool/role tests mandatory. DB admins remain trusted. Application-level private-team policy must cover secondary surfaces. Organization role never grants content automatically.

## Reconsideration and validation

Revisit separate databases for regulatory/operator isolation needs, not speculative scale. M02 negative tenant tests and M08 restore evidence required. [RLS documentation](https://www.postgresql.org/docs/current/ddl-rowsecurity.html).

See [canonical specification](../DATABASE_DESIGN.md).
