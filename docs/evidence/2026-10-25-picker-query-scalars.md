# Shared picker query scalar input

25 October 2026. Bounded label, assignee and project-lead query correction from
`8b730204089f0ce63c61e0615c9bb7a49ebeaa98`.

Removed only the shared picker's native UTF-16 `maxlength100` cap. Existing Rust
label and eligible-assignee endpoints retain their 100-scalar bounds. Consumers,
hooks, types, backend, registry and CSS are unchanged; no client validator was
added. Selection/deduplication, pagination, layout bounds, scope/authority,
IME/modifiers and keyboard/focus behavior retain their current implementation.

## Executed acceptance

Two journeys per engine group LabelPicker/AssigneePicker issue creation and
ProjectLead project creation. Long synthetic names were seeded through existing
invitation/acceptance and label APIs. Fresh private teams contain eligible
targets and distinguishable partial-prefix distractors; matching nonmembers are
ineligible. A label in another administrator's private team returns 404 to the owner.

| Engine | Baseline label / assignee / lead query scalars | Corrected query scalars | Final journeys |
| --- | --- | --- | --- |
| Chromium | 50 / 50 / 50 | 100 / 100 / 100 | 2 passed |
| Firefox | 50 / 50 / 50 | 100 / 100 / 100 | 2 passed |
| WebKit | 50 / 50 / 50 | 100 / 100 / 100 | 2 passed |

Captured real API queries preserve exact full text and limit 20. Baseline partial
queries matched target and distractor; corrected full queries found only the
eligible target. Every 101-scalar query reached the existing HTTP422 diagnostic,
retained its input and removed remote candidates without a stale selection or
unfiltered fallback. Repair restored current authorized choices. Existing
Unassigned, No lead and self choices remain ordinary controls.

One issue create persisted the selected label/assignee IDs and full 100-scalar
names; one project create persisted the selected lead. Both remained at version 1
and exact after reload/current reads. Ineligible people and the private label
remained excluded. No write was triggered by Enter on rejected picker input.

The initial combined-case baseline stopped at a dialog-anchored inner locator;
its ProjectLead cases did record the 50-scalar cutoff. That entire first attempt
and exact spec remain retained. Only two relative selector anchors were corrected;
assertions, settings and timeouts were unchanged. The complete final baseline and
corrected pair used identical final spec bytes and fresh retained fixtures.

## Checks, isolation and evidence

- 57 focused tests across PickerBounds, PickerKeyboard and unchanged
  Taxonomy/Projects consumer tests passed. The new controlled-query test checks
  full text, stale active-selection reset, error repair and restored focus.
- Both TypeScript targets passed (`npm run typecheck`).
- One final production build passed (`npm run build --prefix web`).
- Only these six cases were selected per phase. Canonical one-worker,
  zero-retry, 60-second test/10-second expectation/action settings and port18081
  guard remained unchanged; screenshots, traces and videos stayed disabled.

All three retained phases verified empty browser namespaces, reaped exact
monitor/API processes, bounded private output and no cleanup failures. Browser
environments omit PG/database/operator/worker/setup service authority. Private
directories are mode 0700 and artifacts mode 0600. The own database stop returned
exit 0; final audit records no extra helper-namespace process, absent postmaster
PID and retained data. Earlier branches/qualified assets/maps/logs remain intact
without repeating their successful checks.

Final spec SHA256:
`081f13a4a12bcdf48144f2812365a88e81394fd2aa2d8dbb18c69da97dd66dfd`.
Backend SHA256:
`4cd979ed41ef326bbaf6ac1d7c74a5eda68cd62677de016287a9086b80af8c39`.
Containment SHA256:
`1088837b37933f07f42dd5f4184181dca1985d1d284bb1f5fcd1a89ea3dd7f23`.

[Machine-readable evidence](2026-10-25-picker-query-scalars.json) binds 17
source/check/caller hashes, 33 runtime artifacts and 18 frozen frontend assets
with exact source-map provenance. The unique private index names both frontend
roots and all 18 exact result/scope/shutdown/stop/preservation/selector/source
receipts with hashes. Original and final callers/specs are retained explicitly.

## Scope and limits

No schema, Rust or wire change is required. Frontend rollback restores native
query truncation while existing server validation/authority remain unchanged.
This is a bounded input slice, without broader feature, milestone or product
completion. Normal contained completion is verified; forced interruption,
physical/native/IME/assistive, screenshots and broader performance remain separate
gates. Local scoped checks do not replace hosted CI or resolve unrelated retained
failures. Independent exact-head review and integration remain lead-owned.
No push, PR, merge or issue closure occurred. The ongoing worker Goal continues
for scoped findings and the next explicit assignment.
