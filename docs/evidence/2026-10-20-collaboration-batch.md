# Collaboration batch integration — 20 October 2026

This batch combines independently reviewed document transfer, activity history, explicit template titles, shortcut preferences and native request-policy work. The native CI job is added to close the hosted broker-contract gap.

## Integrated source

- [Document transfer](2026-10-20-document-portability.md), including the normalized-editor rejection regression.
- [Activity facts](2026-10-20-activity.md), preserving private historical audiences and atomic mutations.
- [Template titles](2026-10-20-template-titles.md), additive migration 0020 and legacy literal/hash compatibility.
- [Shortcut preferences](2026-10-20-keyboard-preferences.md), additive migration 0021 and caller/workspace isolation.
- [Native reference lookup](2026-10-20-native-reference-route.md) and [hosted native checks](2026-10-20-native-ci.md).

The server and worker startup preflight combines both new schema requirements: template title_mode and preference/event tables. Both activity and preference OpenAPI contract tests are retained. The Rust exporter regenerated the combined OpenAPI and TypeScript types; all previous published paths remain and the only added path is `/api/v1/workspaces/{w}/preferences`.

## Executed integration checks

- 472 frontend tests passed across 59 files with two workers, after all feature source and generated types were combined.
- TypeScript checking and the production Vite build passed; the emitted parsing worker is included in the build. The existing large-chunk warning remains.
- 14 scope/workflow guard tests, six desktop entrypoint tests and verification of 121 upstream vendor files passed.
- Workspace Rust formatting passed. Shared workspace package artifacts were refreshed before the combined Rust API export to avoid cross-worktree stale-artifact reuse.
- Individual feature evidence records the actual PostgreSQL, HTTP, native protocol, Clippy and migration tests. Hosted full-workspace, Docker and native-core verification must pass on the final PR head before merge.

No full API/browser import-save-reload, complete IME/assistive-technology, native GUI or notification-delivery qualification is claimed. The previously rejected persistent API launch was not retried. The dedicated local PostgreSQL test cluster was restored under the coordinating session after its prior startup session ended; development data was preserved.

## Retrospective

Parallel worktrees reduced source contention. Shared build targets still required explicit ownership and artifact refresh; migration numbers and final generated contracts were coordinated centrally. Review found a real near-limit import rejection case that passing parser tests had not covered. Preserve those failure tests and keep acceptance limits visible when updating issues.
