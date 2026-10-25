# Workspace and team name Unicode input boundaries

25 October 2026. Bounded F01/F02/M02 correction from integrated
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`. The [JSON record](2026-10-25-workspace-team-scalars.json)
binds source, tests, assets, receipts and retained diagnostics by SHA-256.

The production change removes only five native `maxLength={100}` attributes:
workspace create/rename, first-team name during workspace creation, and ordinary
team create/rename. HTML counts UTF-16 units; existing Rust `bounded_text` counts
Unicode scalars and remains authoritative. ASCII prefix controls, organization
permissions, acknowledged workspace handoff, uncertain command recovery,
dirty/conflict handling, cache behavior and private-team policy are unchanged.
There are no Rust, schema, API, dependency or shared-contract changes.

Two focused journeys ran in Chromium, Firefox and WebKit. The final baseline
independently verifies all **15 cutoffs**: each of the five controls stops at
50 supplementary scalars when attempting the valid 100-scalar boundary. The
corrected run passes **6/6** with identical spec bytes, SHA-256
`32e6bc634e6c54e3a92122b12bf9b56f8ee2276dbbf94b919c535420277cd39b`.

Actual 100-scalar workspace/first-team/team creation and workspace/team renaming
survive reload. Independent 101-scalar workspace-name and first-team-name creates,
ordinary-team create and both rename paths reach the existing Rust 422 response.
Drafts remain intact and repairable; rejected creates add no entity and rejected
renames preserve saved values/versions. Repair commits once. Organization/prefix,
workspace/team IDs, seeded defaults, private-team state and existing issue links
and keys are verified unchanged through rename and reload.

| Executed check | Result |
| --- | --- |
| `npm --prefix web test -- --maxWorkers=1 src/settings/Settings.test.tsx src/settings/WorkspaceCreation.test.tsx` | 36/36 in two files |
| `npm --prefix web run typecheck` | Application and e2e targets pass, including the final selector edit |
| `npm --prefix web run build` | One final application build passes; existing chunk warning retained |
| `node node_modules/@playwright/test/cli.js test workspace-team-scalar-inputs.browser.ts` through the approved scope | Final identical-spec pair: original 0/6, corrected 6/6 |

New unit cases verify rejected drafts, callback counts, independent workspace and
first-team repair, unchanged prefix/organization fields and rename preconditions.
Existing focused cases retain permission, conflict, dirty/membership, uncertain
same-command creation and locked acknowledged-handoff coverage.

Earlier runs remain separate. The first baseline reproduced cutoffs but its
workspace-rename exact-label selector omitted existing helper text. After that
selector correction, a complete baseline verified all cutoffs. An earlier corrected
run passed all three workspace journeys while the three ordinary-team journeys
stopped at an issue-prefix selector with the same label issue. Only those selectors
changed; no assertion, timeout or retry was weakened. The final comparison reused
the retained original baseline assets and unchanged corrected build, with exact
spec/caller snapshots and every earlier spec, receipt and failure context retained.
The initial unit-mock TypeScript diagnostic is also retained privately.

All nine prior qualified first-wave assets/maps were copied and hash/size verified
before another build, under
`$OPENTRACK_DEV_STATE/qualified-first-wave-6bf5f630/web-dist`. Its relocation receipt,
source evidence and original `6bf5f630` branch/data/logs are retained; accepted
first-wave checks were not rerun. New baseline assets are relative to
`work/workspace-team/baseline-frontend`; new corrected assets are under `web/dist`.

Private execution reused the verified network namespace, existing PostgreSQL state,
frozen backend and approved PID scope. Fresh synthetic workspaces separate journeys
and phases without resetting data. The canonical profile remains 60-second tests,
10-second action/expect timeouts, one worker, zero retries and loopback port 18081.
Browser environment excludes service/database/PG authority; private output drains
retain at most 2 MiB each. All five retained run receipts verify an empty browser
namespace, reaped API and no scope-cleanup failures. Final caller snapshots match.
No authentication state, raw fixture IDs, HTTP bodies, screenshots, videos or traces
are committed. Private artifact hashes and exact roots are listed in the JSON.

Rollback restores frontend source/assets without rewriting data. Physical keyboard
or IME, native desktop, assistive technology, broader F01/F02/M02 lifecycle/security
and recovery, hosted checks and whole-product acceptance remain separate. The lead
owns shared contracts, issue tracking and main integration.
