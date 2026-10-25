# Planning text Unicode input boundaries

25 October 2026. Bounded M05 / F05, F14 and F15 correction based on integrated
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`. The [machine-readable record](2026-10-25-planning-scalar-inputs.json)
binds source, tests, built assets, receipts and retained private artifacts by SHA-256.

Removed only four native `maxLength` attributes: project name 160, project summary
500, milestone title 160 and cycle name 160. HTML counts UTF-16 units; Rust
`bounded_text` counts Unicode scalars. Rust remains authoritative, including
`projects.rs::project_summary` and its empty-summary behavior. No schema, API,
dependency, date, timezone, ordering or client-validation policy changed.

Two real API-backed journeys ran in Chromium, Firefox and WebKit against the
helper's private PostgreSQL and network namespace, using separate synthetic teams
for each journey and phase. Native `pressSequentially` reproduced all four original
cutoffs in every engine: 80 supplementary scalars in each 160-limit field and 250
in the 500-limit summary. All six original cases failed at the preservation
assertions; their exact failure contexts, spec and original frontend remain retained.

The corrected run passed **6/6** with identical spec bytes, SHA-256
`b476a72bfeb7a490464d25a4ccd9725580865a727d6aec095d69015d1306c9ba`.
For each field, boundary text survives create, edit and reload. Actual 161/501-scalar
create and edit requests reach Rust and receive 422; rejected drafts remain intact,
rejected creates add no entity, and rejected edits preserve saved text and version.
Repair produces one entity or exactly one version increment. Milestone rejection
also preserves the parent project, and the cycle journey retains its explicit
Europe/Berlin local dates across DST.

| Executed check | Result |
| --- | --- |
| `npm --prefix web test -- --maxWorkers=1 src/projects/Projects.test.tsx src/cycles/Planning.test.tsx` | 60/60; two files |
| `npm --prefix web run typecheck` | Application and e2e targets pass |
| `npm --prefix web run build` | Pass; existing chunk-size warning retained |
| `node node_modules/@playwright/test/cli.js test planning-scalar-inputs.browser.ts` through the approved containment scope, once per phase | Original 0/6, corrected 6/6 |

Focused unit coverage includes create/edit rejection and repair, required/trim and
empty-summary behavior, existing state/date/access/dirty/conflict and uncertain
command paths. The browser spec and canonical configuration are unchanged between
phases: 60-second tests, 10-second action/expect timeouts, one worker, zero retries
and loopback port 18081.

The frozen helper-built backend is reused before and after. Browser children run
inside the approved PID namespace with service/database authority removed. Both
phases verified an empty browser namespace, reaped the exact API process and
reported no scope-cleanup failures. Private logs are capped at 2 MiB per stream;
retained artifacts use private directories/files. No authentication state, raw
fixture IDs, HTTP bodies, screenshots, videos or traces are committed.

Baseline receipt SHA-256:
`c48b50d58156a4992564ed789e5ca98c32cac27ea60668be088240bb91b3cfae`.
Corrected receipt SHA-256:
`dba50a55c940a115fff870487aff82fb4d965301923dc822d064e904c7912c4e`.
Original and corrected receipts, private diagnostics and source/spec hashes remain
under the helper's `$OPENTRACK_DEV_STATE/planning-scalar/{baseline,corrected}`;
the JSON record contains their relative artifact hashes.

Rollback requires a frontend source/build rollback; no data rewrite is needed.
This evidence covers only the six focused browser cases and affected unit tests.
Global CI, physical keyboard/IME, assistive technology, native desktop, broader
planning/board acceptance and whole-product completion remain separate gates.
The lead owns hosted checks, review and main integration.
