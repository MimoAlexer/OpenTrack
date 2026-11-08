# Combined filters, realtime and in-app inbox qualification

8 November 2026. This record covers the composed development source, not completion of M06, M07 or the product. The core in-app path needs no AI or SMTP service. A fresh laptop checkout can use the [Docker installation](../DOCKER.md); development credentials, local work directories and test databases are not repository handoff artifacts.

## Source composition and boundaries

The branch preserves ancestry for planning `959a8ab`, filters `c570072` (including its denial lifecycle fix), realtime `aee8d9a` plus evidence `71881eb`, and inbox `684b141`. Migrations `0030`/`0031` were preserved; filters `0032`, realtime `0033` and inbox `0034` are applied in order. OpenAPI and client types were generated from the combined Rust source.

Inbox and unread badge use the mounted canonical stream and its degraded fallback. Standalone callers retain bounded reconciliation. Generic notification invalidations refresh the list, badge and source preview. Recipient authorization resolves the notification against workspace, current recipient and ID, with null team and wire actor. No private content or recipient identity is added to invalidation metadata.

Native requests remain inside the credential broker: exact body-only structured issue-query POST with a 1 MiB bound, explicit inbox methods and bounded event-page GET. Other request/response tiers remain unchanged; renderer bearer tokens and native SSE are not introduced.

## Executed evidence

- Fresh PostgreSQL 18 development, test and CI profiles contain all 34 source migration checksums, including `0032`/`0033`/`0034`, unchanged. Development setup was repeated twice, and CI bootstrap was repeated twice after its initial migration. Every strict App/Worker constructor and workflow/notification ACL probe passed. No startup guard or migration ledger was relaxed.
- Actual issue assignment through the real API and restricted worker produced recipient-only `notification.created` SSE. Owner and nonrecipient replay omitted notification and recipient IDs. Remote read produced `notification.updated` and unread count zero; private-team revocation closed/reset the stream and concealed source labels and reads.
- The initial composed frontend passed 1,065 tests across 91 files, both TypeScript targets and a production build. Native core passed vendor/format/strict Clippy checks and 125 tests. Workspace strict Clippy passed.
- The initial broad Rust run preserved 233 passes across 80 completed targets, four ignored qualification checks and one legacy fixture failure. That fixture granted excessive authority after migration; replacing its duplicate setup with the shared strict harness passed the previously failing contract. This partial run is not reported as a complete workspace pass.

## Independent review corrections

Deterministic controls reproduced revoked labels returning after late inbox rollback/success, an older mutation replacing a newer read, a later-page keyboard target being silently replaced, stale bootstrap names, count-dependent private-only replay continuations, and stalled response bodies retaining queue/admission resources.

The corrected source passes 63 focused frontend checks, including fresh denial/read reconciliation and bounded four-page revalidation with stable or neutral selection. Metadata refresh preserves the current stream cursor and dirty editor. The realtime controls pass with different private-only history sizes completing in one public page, mixed visible/private histories converging, and a fixed 10,000-row replay budget. Three expired response bodies can remain alive and unpolled while queued events/bytes and local admission reach zero and three replacement streams are admitted. Old bodies retain a small content-free reset response.

The corrected source is undergoing fresh combined suites. The final 15 actual browser cases and protected exact-head CI/review are pending; their final results must be recorded before merge.

## Remaining gates

No production migration, downgrade, populated recovery, retention qualification, SMTP/email/digest delivery, installed native workflow, physical keyboard/IME, assistive-technology or complete-product acceptance is certified here. The broad storage-full and trusted scanner cases retain their separate explicit qualification requirements. Applied migrations remain immutable; a populated installation's downgrade/restore needs its documented maintenance and backup procedure.
