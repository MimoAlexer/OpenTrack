# Browser CI cleanup lifecycle correction

Focused delta over `884e36f5ddec9dc57bbf789534c2ac73bccdd509`. Its original 43-test proof and all earlier candidates/evidence remain unchanged.

A fully successful cleanup now unlinks the creation claim, including when every owned target is already absent. Validation, identity, Docker and cleanup-deadline failures retain it. A later cleanup without a claim performs no resource inventory or deletion, so stale successful authority cannot sweep later matching-name resources.

Cleanup verifies the inspected full IDs against its inventory and reads each container's AutoRemove policy. It reconciles those exact IDs before stop and after stop/remove, waits for automatic removal inside the existing 120-second cleanup deadline, and continues verified network/volume cleanup when a helper has naturally gone. A healthy exact-ID absence observation is required to reconcile a failed stop/remove; real daemon errors, still-present targets and replacement IDs fail and retain the claim. No replacement object receives authority. No forced deletion or broad sweep was added.

**50 pure tests passed**, with Docker and HTTP mocked. New cases cover success/empty/failure claim lifecycle, immediate/delayed AutoRemove followed by network/volume removal, natural disappearance before and during stop/remove, real Docker failures, replacement IDs and cleanup expiry. The companion JSON includes the actual untruncated original test output and current executed-source hashes. The 43 existing tests remain included. `git diff --check` passed.

The shared 900-second suite deadline, 300-second preparation cap, read/cleanup bounds, per-test/action limits, default worker profile, Node environment and SQL/resource guards are unchanged. No actual Docker, fixtures, browsers, CI or full matrix was run. **Exact-head reviewer clearance remains pending before CI.** No Goal/accounting, shared-ledger, workflow, Compose, UI, backend or migration changes occurred.
