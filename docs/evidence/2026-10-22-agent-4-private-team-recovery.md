# Private-team cold recovery fixture

Implemented 22 October 2026 as bounded Agent 4 assistance for #7. Agent 5 retains recovery and M08 ownership. Baseline PR #140 merged as `e3716adf19ab151a5fa19e26bd011ffb22978fb3`; this increment extends its synthetic fixture after synchronizing integrated main.

The [Docker qualification](../../scripts/docker-smoke.py) seeds a synthetic private team, viewer, issue, structured document and populated attachment through the existing API. The invitation grants team access before minting the reader session. Saved private issue, both document routes and attachment metadata/content must be readable before the owner conditionally removes only the team membership.

Before snapshot and after cold restore, the same reader cookie must still authenticate the original viewer and read the existing public issue, document and attachment. The owner must still read the private objects and matching attachment bytes. Every saved private URL must instead return the exact generic 404 problem, `Cache-Control: no-store`, and no validator, download, range or redirect metadata. Conditional and range requests cannot disclose existence. Private fixture markers must be absent from response headers, and the reader cookie must remain unchanged without another sign-in.

The fixture runs inside the existing guarded recovery lifecycle. Its rows, revoked grant, sessions and blob join the stopped source snapshot and existing authority/byte comparisons. Target cleanup and source restart retain their nested failure handling; reader cookies are cleared afterward, including on snapshot or cleanup failure. The private synthetic backup remains subject to the baseline's owner-labelled cleanup rules and is never published as an artifact.

## Executed checks

The complete [boundary suite](../../scripts/test-docker-recovery.py) passed 15/15 cases under isolated Python with a cleared environment and separate user/network namespaces. No Docker, API, database or credentials were used by that run. All 13 existing safeguards remain, including source restart when snapshot and cleanup both fail. New cases reject altered, missing or leaking problem fields, unauthenticated/success/conditional representations, cacheable responses, and all six prohibited metadata headers even when empty. The cleanup failure case verifies reader-cookie disposal and suppresses mocked runtime PASS receipts.

Independent read-only review found no actionable source, API-contract or privacy issue in the prepared controller. Its invitation, team-removal, saved-resource and error-envelope contracts were checked against integrated main. This is controller-boundary evidence; the synthetic actors in unit tests do not prove actual private-team recovery.

## Qualification still required

Require all four configured checks on the follow-up's unchanged head and actual AMD64/ARM64 cold restore execution. Both architectures must execute the authorized controls and current private-team denials before and after restore. Preserve failure receipts and correct observed defects before recording acceptance.

The [baseline recovery evidence](2026-10-22-agent5-offline-recovery.md), [M08 criteria](../milestones/M08_HARDENING.md) and [administration requirements](../features/F29_ADMINISTRATION.md) retain their broader obligations. Provider encryption keys, optional-volume contents, encrypted scheduled backups, prior-release/failed-migration and different-host restore remain outside this fixture. F29-A1, GQ08, M08 and whole-product completion remain unverified.

This fixture has no migration or production permission change. Reverting its test and evidence increment restores the preceding qualification flow; it does not change stored application authority or qualify recovery.
