# Invited-account display-name Unicode input boundary

25 October 2026. Bounded F02/F03/M02 increment from
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`. The [JSON record](2026-10-25-invite-name-scalars.json)
binds source, tests, bundles, selected facts and receipts by SHA-256.

This browser/evidence-only correction follows `df71f449`. Its original small-directory
proof is retained separately: reading only the first50 rows could not establish
absence in arbitrary retained directories. Corrected helpers exhaust returned
cursors and fail on missing/repeated cursors or a256-page budget. Actual page-size1
calls, an unrelated synthetic invitation and owner/new-member data exercise
multi-page member, pending and all-invitation directories in each corrected engine
journey. No50-account seed, application rebuild or successful unit rerun was needed.

Removed only `JoinWorkspace`'s invited-new-account name100 native cap. Existing
Rust invitation acceptance validates the Name100 scalar limit. Required fields,
email/password/token controls, expiry/single-use, sign-in and existing-account
behavior, authority and shared code are unchanged. No AuthScreen/bootstrap,
Rust/API/schema, dependency or client validator changed.

One journey per engine ran in Chromium, Firefox and WebKit. The original native
input stopped at50 supplementary scalars; all three invitations stayed pending
with no target workspace membership. The corrected same-spec run passes **3/3**.
The actual101-scalar request receives422, retains the name draft and leaves the
invitation pending/previewable, target membership absent and anonymous session401.
Repair accepts100 scalars once. The exact name persists in the new session and
owner-read member projection after reload. Consumed-token replay returns the
existing404 and cannot add another membership. The separate owner API retains its
original session name/status. Absence of an extra global account after rejection
was not independently queried and is not claimed.

The test creates a fresh anonymous context with empty cookie storage, the same
canonical desktop device profile, locale/timezone, blocked service workers and
same-origin route guard. It copies configured action timeout10000 and default
navigation timeout0 from the pinned Playwright fixture behavior; test60s,
expect10s, one worker and zero retries remain unchanged. It does not modify the
owner API cookie jar. Assertions and private facts select names, statuses and
counts; full credentials or request bodies are not asserted or published.

| Executed check | Result |
| --- | --- |
| Focused `Settings.test.tsx`, `--maxWorkers=1` | Original25/25 retained; source unchanged, no rerun |
| Corrected e2e TypeScript target | Pass; original application target unchanged |
| `npm --prefix web run build` | Original final build/bundles retained; no rebuild |
| Approved scoped `invite-name-scalar-inputs.browser.ts` | Baseline0/3; corrected3/3 |

The preserved unit verifies rejected-name draft preservation and one repaired
authentication callback without comparing credential-bearing request bodies.
Existing Settings units cover simulated invitation errors, sign-in focus,
permissions, request retry and scoped directory behavior. Existing-account and
expiry lifecycle behavior is unchanged, without new runtime qualification here.

Both original bundles are unchanged and remain frozen under
`$OPENTRACK_DEV_STATE/invite-name-scalar/frontends/{baseline,corrected}`; the nine
pinned8b baseline assets retain their exact source-map proof. No unchanged unit
or build was rerun. The changed browser spec ran against both frozen bundles.
New phase files, actual page-count facts, unchanged bundle references and shutdown
receipts are indexed by
`$OPENTRACK_DEV_STATE/invite-name-pagination/artifact-index.json`. Original small-directory
receipts/index remain under the old task root and are explicitly referenced.

Both run receipts verify empty browser namespaces, reaped API processes and no
scope-cleanup failure. Own database shutdown/data retention is recorded under the
correction root in
`shutdown-audit.json`, `db-stop.log` and `db-status.log`; this is handoff-time
evidence. Private directories/files use0700/0600 and output drains retain at most
2MiB each. The approved scope/backend and verified private environment are reused.
All prior branches, data, failures, accepted results and relocated9c4b00c3 assets,
source evidence and artifact index remain preserved.

Rollback restores frontend source/assets without a data rewrite. This does not
qualify global CI, complete account lifecycle, forced cancellation, physical
keyboard/IME, native desktop, assistive technology or whole-product acceptance.
The lead owns hosted checks and integration. No raw fixture IDs, credentials,
request bodies, authentication state, screenshots, videos or traces are committed.
