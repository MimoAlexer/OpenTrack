# F24 Search

Stage: MVP PostgreSQL; advanced engine conditional. Delivery: [M06_DISCOVERY](../milestones/M06_DISCOVERY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Find known work quickly and explore relevant issues without requiring an external indexing service.

## 2. Functional requirements

Approved expansion obligations: X64, X65. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Exact issue key, title/body full-text, project search, filters, bounded fuzzy title fallback and safe snippets. Rank exact key first, then weighted title/body; show stable tie-break order. Comments/semantic search are later scope.

## 3. Interaction patterns

Global search page with query/filter bar, result groups and recent queries; / focuses it outside inputs. Keep previous results dimmed while loading and show query-specific no-results suggestions.

## 4. Data requirements

Transactionally maintained tsvector/plain_text and pg_trgm title index; query metadata need not persist beyond private saved searches. Indexes are rebuildable projections.

## 5. API requirements

GET/search with query/filter/cursor and type limits; suggestions endpoint bounded 20. Explain unsupported filters; timeout returns recoverable error rather than truncated unlabeled results.

## 6. Authorization rules

ACL applied before returning titles/snippets/counts; index alone never grants visibility. Search admins cannot bypass private teams.

## 7. Edge cases

Unicode/diacritics, mixed code identifiers, empty query, excessive tokens, deleted issue, private-team revoke and very common term.

## 8. Failure behavior

If optional fuzzy index unavailable, exact/FTS remains with degraded notice. Reindexing preserves old usable index until switch; no empty success masking failure.

## 9. Performance considerations

Target server p95<=250 ms at 100k issues; search timeout 1s; benchmark tenant skew and permission joins. Tantivy only after measured failure and separate projection design.

## 10. Feature dependencies

F06/F05, F12 plain text, F17 filters, F04 ACL.

## 11. Acceptance criteria

F24-A1: known issue key is first result. F24-A2: edited description is searchable after commit. F24-A3: private/deleted content never appears in result/snippet/count. F24-A4: long query cannot exhaust DB.

## 12. Testing strategy

Relevance corpus with expected top results, ACL matrix, EXPLAIN/load tests, Unicode fixtures, timeout/abort behavior and keyboard search-to-detail E2E.
