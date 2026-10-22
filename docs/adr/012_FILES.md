# ADR-012 Local immutable blobs, optional S3

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

Attachments need durable recoverability and authorization without a mandatory object-storage service.

## Options and tradeoffs

Local files are simplest for single node but require explicit shared-storage plan for replicas. S3-compatible storage supports multi-node but adds infrastructure/auth. Database bytea centralizes backup but inflates DB and hot queries.

## Decision

Store metadata in PostgreSQL and immutable opaque-key blobs locally; quarantine/validate before ready. Authorize every download through parent; no public paths. Design S3 adapter later using same metadata contract.

## Consequences and risks

DB/blob transaction is not atomic: staged lifecycle and reconciliation are required. Backups suspend GC and preserve referenced blob manifest. Scanner is optional but its absence is visible.

## Reconsideration and validation

Adopt S3/shared storage before multi-node blob writes; benchmark large files. M04 upload failures and M08 restore prove correctness. [Upload controls](https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html).

See [canonical specification](../DEPLOYMENT_ARCHITECTURE.md).
