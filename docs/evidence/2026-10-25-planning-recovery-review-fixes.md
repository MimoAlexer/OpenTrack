# Planning recovery review corrections

Correction implementation `4c659a9cd73dbce6a51a59390ec390f15dbbcf2e`, based on frozen `c3190bd`. The [machine receipt](2026-10-25-planning-recovery-review-fixes.json) records exact sources, commands, log hashes and browser provenance. The [original planning evidence](2026-10-25-planning-write-recovery.md) remains historical; it does not cover the newly reported failure paths.

Three supplied findings are addressed:

- Recovery rows now require a nonempty string ID and an explicit own `milestone_id` field containing null or a nonempty string. Malformed or incomplete decisions cannot complete a scan, confirm a clear/absence, or enable retry. Intent remains unresolved. State comparison no longer supplies null for a missing field.
- A selected-milestone 404 revalidates the matching current project before classifying the target as deleted. Project 403/404 propagates denial cleanup and removes cached labels and intent. A matching authorized 200 retains the unresolved target choice without enabling retry.
- Discard restores focus by the surviving milestone ID or section fallback. It no longer waits for an obsolete resource index after concurrent anchor deletion. The successful-reorder index fence is retained.

The ten new regression cases first ran against exact c319: eight failed and two controls passed, with all 26 existing recovery cases skipped. The corrected recovery file passes 36/36. The initial affected planning/project run passed 52/53; its single failure was the previously released legacy project mock omitting `milestone_id`. Only that object's explicit null was added, preserving every payload, If-Match and no-replay assertion. The corrected project file passes 31/31; the unchanged current-source planning file's 22 passes are retained. That is 89 unique current passing cases across focused runs, not one all-green 89-case invocation.

Both TypeScript targets and one changed production build pass. The two browser journey types retain the original real version conflicts, reviewed retry, late-page membership, committed response-loss without replay or causal acknowledgment, exhausted absence checks, unchanged issue data, reload and focus assertions. The added checks establish:

- Real concurrent milestone deletion produces a stale move. Every background order refresh is held while the old four-row order and disabled invoker remain visible. Discard occurs before release; authorized refresh moves the surviving milestone to another index and focus returns to its edit control without a retry.
- A real API readback is narrowly fault-injected to omit the target's `milestone_id` beyond the first 100 rows. The clear intent remains unresolved, retry stays disabled and no observed-success state appears. Reload confirms the actual saved assignment is unchanged. This is malformed-response fault injection, not a backend defect claim.
- Actual private-team revocation occurs between initial authorized project metadata and the selected-milestone GET. The selected read returns 404; the new project revalidation also returns 404, clearing project/issue labels and review intent. Authorized-project 200 and project 403 controls are covered by focused units.

The first changed browser run passed all six cases across Chromium, Firefox and WebKit. The focus journey then gained stricter refresh/stale-order/disabled-invoker assertions; its three cases passed again on the same immutable frontend/backend. The three byte-identical membership cases are retained from the first run. Six unique cases pass, with exactly two journey types per engine; no single six-case final-spec run is claimed. Canonical worker/retry/time limits remain unchanged. Both runs verified containment, no browser service/database authority, zero remaining processes, reaped monitor/API and clean cleanup.

Only the two production components, focused recovery tests, the existing browser spec and the one legacy mock field changed. Backend `901551...`, schema, contracts and dependencies are unchanged. No old artifact audit, scalar workflow, backend rebuild or global matrix was repeated. Original c319's 79-test/initial-failure/six-browser proof and the independent `0cb1f9b` worker-fault candidate remain preserved.

This candidate awaits one focused independent review of the new delta. No full CLEAR, integration, hosted, native, physical-input, OS IME, assistive-technology or broader feature/product completion is claimed.
