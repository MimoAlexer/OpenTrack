# Manual ordering review corrections

25 October 2026. Corrected private source: `ab00bf4ec3bfd9ebb68e44d6a39b35202ec5a9bc`.
The [receipt](2026-10-25-issue-order-review-corrections.json) binds source and private logs.

Review found six corrupted Unicode literal occurrences in two files, including the
existing character-limit test. A byte-aware comparison against the earlier source
identified the exact damaged runs; those runs were restored. The complete domain
suite now passes 25/25, including the Unicode boundary. The earlier four ordering
unit tests did not qualify the full domain suite. Damaged source snapshots and the
audit remain retained.

The generated contract now declares the ETag already returned by positioning,
maintenance reads (200 and 304), and resume. Those four header declarations are
the complete semantic OpenAPI delta; TypeScript was regenerated normally. All
nine HTTP-router/PostgreSQL ordering cases pass against the corrected source,
with strict affected Clippy. The existing large maintenance case now executes
unchanged 304 responses with empty bodies, old-validator requests denied as 401
and 404, and a newly authenticated caller whose changed role produces 200 with
a new validator and current resume capability.

A separate retained test at `7d4245d` starts the real default worker daemon and
normalizes 501 issues with its unchanged 2000ms poll, 60-second lease and
five-connection limit. Explicit create retry then succeeds as `ORD-502`; all 502
rows retain version 1 and their expected ranks. Its first compile attempt used
an absent internal number field; that attempt and source remain preserved.

The corrected server build is frozen by source manifest and binary hash for the
connected UI worker. Migration 27 is byte-identical to the earlier candidate;
retained public schema 26 was not upgraded. Prior API, ingress, native-core and
schema-startup records retain their original source boundaries. Neither these
checks nor the new build establish full workspace runtime, connected UI, native
GUI, physical-input, assistive-technology, integrated-main or whole-issue acceptance.
