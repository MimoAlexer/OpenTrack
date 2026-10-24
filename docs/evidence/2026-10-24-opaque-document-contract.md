# Opaque future document-format HTTP contract

24 October 2026. This adds regression coverage for existing behavior, without
changing production source, migrations or document formats.

The preserved test increment extends the existing document/discussion HTTP
contract. A synthetic future writer stores schema99 JSON containing unknown
fields, nested content and a private issue reference. The real Axum router and
isolated PostgreSQL preserve its opaque body, plaintext, ETag and version-addressed
revision response. The owner can resolve the private reference; a viewer of the
public document receives unavailable reference metadata and cannot read the
private issue directly.

The unsupported document stays read-only. Schema1 downgrade attempts with both
stale and current ETags, schema99 writes and a combined legacy title/description
write return `unsupported_document_schema`. Document/issue responses, revision
listing, version-addressed response and canonical stored JSONB text remain
unchanged after those rejected writes.

The one extended HTTP test passed on the recorded current source. Formatting and
targeted strict Clippy also passed, with owned test processes reaped. The
[receipt](2026-10-24-opaque-document-contract.json) records exact source/packet
hashes and command results. No earlier run is substituted for this execution.

This is in-process HTTP/PostgreSQL evidence, without a TCP listener or browser
rendering/script-execution claim. It does not qualify future-format project or
comment documents, persisted older future-format revision rows, native/physical
input or assistive technology. Stored document-format versions are separate
from database migration history. No data rollback is needed for this test-only
increment. Fresh exact-head CI and final integration review remain required;
M04/F12 and whole-product obligations remain open.
