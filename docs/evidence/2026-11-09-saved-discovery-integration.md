# Saved discovery and realtime integration, 9 November 2026

This composes the structured-filter/realtime/notification union with saved discovery, preserving schema `0035` and current-source authorization. Browser runtime qualification below pins application/test source `1ceee2bbfd8d6e66f6667beca7c31b381f88ff3c`. The later strict notification-runtime status-contract merge and this evidence/ledger update change qualification scripts and documentation only; they are not a claim that a later application binary was exercised.

## Implemented behavior

Personal invalidations identify only their recipient principal (`discovery_personal`); shared view creation/update/deletion follows the retained current-authorized source and tombstone policy. Earlier schemas hide unsupported families. Browser SSE and native bounded pages recognize the explicit event/source pairs. Mounted discovery directories, selected metadata and current searches revalidate after invalidation without replacing a name/filter draft. Confirmed per-object deletion fences older writes/listings, while selection and mutation generations prevent delayed metadata from superseding a newer Open or local save.

## Executed checks

- Full web unit suite: **1,122 passed**; TypeScript application/browser checks and production build passed.
- Focused discovery review suite: **33 passed**, including three original-failing reviewer reproductions for deletion and same-object Open callback order. Re-review independently passed the same 33 checks.
- Realtime consumer suite: **19 passed**, including explicit saved-source contracts and browser/native invalidation delivery.
- Saved-discovery database app contract: **11 passed** with development roles and again **11 passed** with separately provisioned CI roles. Actual source-registry checks cover recipient privacy, private-team creation/deleted tombstone visibility and revocation, plus fail-closed replay before schema `0035`.
- Discovery HTTP contract: **1 passed**. Composed realtime HTTP/listener suite after the initial SSE-frame merge: **11 passed**.
- Native bounded-protocol tests: **23 passed**. One stale earlier assertion forbidding the now-supported search-query route was updated to check its exact body budget and reject malformed variants. This is protocol evidence, not a native GUI/session qualification.
- Strict notification-runtime contract regression checks after the later script merge: **14 passed**.
- Workspace Clippy with warnings denied, formatting and generated OpenAPI/TypeScript drift checks passed.
- Fresh combined development and CI databases applied `0032`, `0033`, `0034`, `0035` in order. Development provisioning and CI provisioning were each repeated **twice**; recorded SQLx SHA384 checksums match all four source files. Runtime/operator/worker roles remain nonsuperuser, without CREATEDB, CREATEROLE or BYPASSRLS. Operator and worker have no direct saved-definition, favorites, personal-event/head or shared-source table privilege. The [sanitized provisioning result](2026-11-09-saved-integration-provisioning.json) records these checks without credentials or local paths.

The frozen `0035` SHA256 remains `3ddff0a596cd3ad04f6276cad3f58c5f17cc97efe4847658fd3802c9ed837b26`. Integration adds no migration. Downgrade/rollback still requires a matched pre-upgrade database/blob/key backup; do not edit or reverse an applied migration.

## Browser qualification

The [final receipt](2026-11-09-saved-integration-browser.json) records **24/24 passed** across Chromium, Firefox and WebKit against the built frontend, real API/worker and isolated PostgreSQL. It covers saved-definition round trips and favorites, current body search, private searches, narrow keyboard draft/reconciliation, current-access Open/denial cancellation, explicit redacted-definition repair, automatic directory rename/deletion and issue offline/replay recovery. Source/tree and server/frontend SHA256 digests are pinned in the receipt. Owned API/worker processes were reaped afterward and the loopback application listener is closed.

The first [22/24 attempt](2026-11-09-saved-integration-before.json) exposed a test-order assumption: normal realtime deletion correctly removed the button before a manual stale-Open test clicked it. An [intermediate transport-isolation attempt](2026-11-09-saved-integration-intermediate.json) also failed shared readiness because it deliberately disconnected live delivery before the normal editable bootstrap completed; that exploratory fixture iteration is not exact-head qualification. The corrected manual path first obtains real authorized data, then uses a real browser offline/online cycle and an events-only abort to exercise manual Open/404 while live delivery is unavailable. Data/search requests remain real. Separate checks require automatic rename/deletion through the connected stream; no transport success is inferred from the deliberately disconnected case. The corrected manual case also passed a focused three-engine probe before the complete rerun.

## Limits

Exact protected GitHub checks and integration review govern merging; local results do not replace hosted qualification. M06/M07 and complete-product gates remain in progress. Project title/current-document conjunction and private-team ACL are exercised, but the combined expression has no combined GIN index in frozen `0035`, and project-scale latency is unmeasured. Fuzzy/trigram search, global navigation/palette integration, virtualization, native GUI/session behavior, physical input, assistive technology and broader product acceptance remain outside this slice's evidence.
