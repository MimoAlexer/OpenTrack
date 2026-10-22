# F28 Import and export

Stage: MVP native/CSV; vendor adapters later. Delivery: [M08_HARDENING](../milestones/M08_HARDENING.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Make ownership of data practical and support migration without corrupting existing work.

## 2. Functional requirements

Approved expansion obligations: X59. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Native versioned JSON manifest with attachment blobs/checksums; CSV issue import/export with explicit field mapping. Preview counts, invalid rows, user/status/label mapping and dry-run before commit. Vendor Linear/GitHub adapters later use documented APIs and explicit credentials.

## 3. Interaction patterns

Wizard: choose format, upload, map fields, preview errors, commit, review report. Export shows authorized scope and retention expiry; whole-workspace backup remains an operator operation.

## 4. Data requirements

transfer_jobs, import_objects and immutable artifact manifest. Source IDs map to new UUIDs unless importing a supported same-instance recovery format; duplicates have explicit skip/update/create policy.

## 5. API requirements

Import create/preview/commit/cancel/report and export request/status/download. Idempotent row processing keyed by job+source ID. Structured imports may batch transactionally by aggregate.

## 6. Authorization rules

Export only current accessible content and granted export capability. Import requires writable destination team; no creation of elevated user roles/provider credentials from a data file.

## 7. Edge cases

Broken references, duplicate keys, huge archive, unsafe filenames, formula-leading CSV cells, missing attachment, unknown schema and user revoked mid-transfer.

## 8. Failure behavior

Preview rejects unsupported schema and traversal/bombs; commit records partial progress and errors rather than falsely claiming atomic entire-file import. Native round-trip validates checksum and relationship maps.

## 9. Performance considerations

Stream parse, file/row/expanded-size caps; process 100 rows/batch. Export snapshots bounded 30 min then require maintenance approach for bigger data.

## 10. Feature dependencies

F04, F06/F05/F11/F13, jobs, F30 audit.

## 11. Acceptance criteria

F28-A1: native export/import preserves supported content/relations with explicit ID map. F28-A2: retry creates no duplicate rows. F28-A3: archive cannot escape extraction root. F28-A4: CSV opens without formula execution by default.

## 12. Testing strategy

Golden round-trip corpus, malformed/bomb/path fixtures, partial retry, permission change, encoding/date mapping and operator backup distinction checks.
