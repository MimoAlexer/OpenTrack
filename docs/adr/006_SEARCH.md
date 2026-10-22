# ADR-006 PostgreSQL search before Tantivy

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

Search must be useful, private and immediately consistent without adding another default daemon/index lifecycle.

## Options and tradeoffs

PostgreSQL FTS+trigram shares transactions and ACL joins. Tantivy offers Rust-native advanced indexing but is a library requiring lifecycle/distribution design. Meilisearch/OpenSearch add operational footprint; semantic-only search is nondeterministic and may need paid inference.

## Decision

MVP uses weighted FTS, exact issue keys and bounded title trigram fallback with current ACL. Search documents update in the same transaction as source text. No mandatory embeddings.

## Consequences and risks

Ranking languages/code identifiers and GIN growth need real corpus tests. Multi-column ACL joins can dominate; indexes and query limits precede a new engine. Comments are later corpus scope.

## Reconsideration and validation

Revisit after tuned PostgreSQL fails measured latency/relevance at reference size. Any new engine needs ACL-at-query, rebuild, lag and delete/revocation behavior. [FTS](https://www.postgresql.org/docs/current/textsearch.html), [Tantivy](https://github.com/quickwit-oss/tantivy).

See [canonical specification](../PERFORMANCE_STRATEGY.md).
