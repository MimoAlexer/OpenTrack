# Historical project lead acceptance

Executed 24 October 2026 against the real isolated API/PostgreSQL runtime, backend from qualified main `39c363d8` and frontend at local `f60dbbf`. One temporary owned Projects test passed frontend/E2E typechecking and listed exactly three cases. Chromium, Firefox and WebKit each passed: [3/3 receipt](2026-10-24-agent-2-project-lead-acceptance.json). Source, executable and frontend hashes stayed unchanged. Worker startup reported the existing 2000 ms, five connections and 60-second lease; this journey does not measure dispatch performance.

The owner selected and saved a distinct active synthetic member as project lead. Deactivating that member through the real owner API, then explicitly reloading, retained the historical name and lead identity with `assignable=false`. The stored project version and update time stayed unchanged while its projected metadata validator changed. A fresh eligible-member search excluded the inactive member; its retained current option was selected and disabled.

A summary-only UI save used the current validator, omitted `lead_id` and narrative, and preserved the historical lead through reload. Explicitly submitting the inactive lead with the latest validator returned HTTP422 without mutation. Choosing **No lead** sent an explicit null, persisted it through reload and removed the historical current option. Each successful metadata edit advanced the project version once. The canonical brief identity, body, version and stored timestamps remained unchanged throughout.

The existing test file was restored byte-for-byte after the harness stopped and waited for its own API/worker and released the browser lease. The executed patch is preserved for later focused regression publication; this evidence does not establish permanent checked-in coverage or qualify a later changed head.

Issue #26 remains open. This tests an unchanged owner's project workflow after a different member is made inactive; it does not qualify account deletion, member-management UI, real-time authority propagation, the separate private-board repair, native/physical/assistive interaction or whole-product completion.
