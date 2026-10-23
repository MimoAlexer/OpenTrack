# Workspace creation handoff recovery

Issue: #12 (M02-T05), with a bounded F01-A1 workspace-creation increment. Baseline: `590725e9e6332f6fe06b20dd870eed55a1e63ab2`. This does not complete the issue or its inherited milestone and native/physical/assistive-technology gates.

## Reproduced defects and behavior

The creation command could acknowledge a workspace, release its busy flag and accept a second create while the parent refreshed the session. The new request used a new idempotency key. Creation now retains the canonical result before opening it, locks committed fields, reports opening failures and offers an explicit retry of that same result. An uncertain creation still retries its original body and key, with current same-account CSRF authority.

The workspace-screen callback refreshed the old bootstrap before asking whether to discard an issue draft. Declining navigation retained the URL but remounted the editor, lost its title and removed the dirty guard. Creation now refreshes session authority without reloading the old bootstrap; navigation retains its existing confirmation.

The zero-workspace branch reused a creator after the account changed. It is now keyed by user identity. Both callers publish refreshed session authority and stop the previous account's navigation if that identity changed. Same-account password confirmation preserves the intended draft.

No backend authorization, wire format, database schema or product worker interval changed. Existing canonical creation and defaults are exercised through their normal API/UI boundaries.

## Executed component evidence

These tests render the actual React components and caller tree with controlled HTTP responses. They are not real-database or native-browser acceptance.

- Original pending-handoff regression: one executed failure; two acknowledged create requests and two distinct keys while the first handoff remained pending.
- Dirty-cancel regression: one executed failure; original URL retained, editor replaced, title reset, dirty guard removed and old bootstrap read twice.
- Initial zero-workspace account regression: one failure and one passing same-account confirmation case. A later strengthened destination assertion failed in both caller branches while same-account confirmation passed.
- Corrected focused tests: 11 passed. Seven component cases cover pending/failed known-result handoff, explicit canonical retry, network/408/500 uncertain outcomes, same-user CSRF rotation and dismissal controls. Four caller cases cover both changed-account destinations, zero-workspace same-account confirmation and dirty-editor cancellation.

Independent source/privacy reviews found no actionable findings. A browser-test cleanup finding was corrected before execution: triggering actions and their event/response waits are consumed together, and dialogs are dismissed before asserting captured properties.

## Runtime qualification

The isolated server was rebuilt successfully against schema 0026 under the shared verification lock. E2E typechecking passed. All 601 frontend tests across 69 files, both TypeScript targets and the production build passed under the shared verification lease. The three API-backed browser journeys passed all nine engine cases at the default 2000ms worker interval; fresh receipt, exit zero and zero remaining owned API/worker processes were verified. The full 123-case functional matrix at the supported 250ms interval also passed (41 cases per engine), with fresh complete receipt, exit zero and zero remaining owned API/worker processes. Verification was acquired before the browser lease. Both runs used retries zero and unchanged test timeouts.

Executed targeted journeys use real persisted workspaces and issues: normal creation/defaults/reload, a real 201 followed by one aborted session refresh and explicit opening retry, and a dirty existing issue whose navigation confirmation is declined before an explicit save/reload. Browser input and a dispatched cancelable beforeunload event do not qualify native dialogs, physical keyboard/IME or assistive technology.

## Migration, recovery and limits

No migration or data rewrite. The component change can be reverted without changing persisted workspaces. A workspace already acknowledged by the server remains canonical; Close dismisses the form and the authorized switcher can reopen it. Existing unknown-outcome retry rules remain intact.

Receipts: [default-profile targeted cases](2026-10-23-agent1-issue12-default-browser.json), [complete functional matrix](2026-10-23-agent1-issue12-browser.json). The prior default-dispatch timing limitation remains open in its original evidence. The current targeted run used 2000ms and the full functional run used 250ms. The latter does not qualify default dispatch performance. Both actual manifests identify Playwright 1.64.1, binary SHA256 `b1a83c548b34aea3d7b71088d71e4c159414bc647d8410502fec4280575910b7` and frontend-tree SHA256 `ea30f1942de8423b2326a72d8212b6ecbec26b2ecce68d9f06ce2b6cb8267620`. Current bytes are not retroactive provenance for older runs. No provider, native GUI, physical-input, screen-reader or whole-product gate is waived.
## 24 October: parallel fixture-grant correction

[Hosted `verify`](https://github.com/MimoAlexer/OpenTrack/actions/runs/40027372460/job/120666989609) at initial head `c80d3ec68d5a5c621ec5dd7dacc91eec1cf95606` failed before the event-consumer selector assertion. Parallel isolated schemas both executed the same database CONNECT grant in `Harness::worker_url`, producing SQLSTATE `XX000`, `tuple concurrently updated`. The other three required checks passed at that head. A separate worker's admission-contract CI encountered the same grant boundary. Those results remain historical failures; no blind rerun qualifies the correction.

Only the shared test worker-provisioning helper changed. It verifies/removes the reviewed script's outer BEGIN/COMMIT frame, uses the existing owned `store::Tx`, takes a transaction advisory lock on the same connection, executes the unchanged grant body and commits. Role validation and granted capabilities stay identical. `operator_url` uses schema-local grants and is unchanged; the production deployment SQL and worker scheduling are unchanged. Independent owner acknowledgement and source reviews cover this narrow hunk.

Real PostgreSQL evidence:

- [Catalog mechanism](2026-10-24-agent1-issue12-grant-mechanism.json): an observed transaction-ID wait reproduced the unprotected grant error. The advisory control observed an advisory wait and both commits succeeded. All four sessions reported read committed; the existing ACL remained identical and owned processes exited. This SQL control did not execute the Rust patch.
- [Actual-helper attempts](2026-10-24-agent1-issue12-grant-helper-attempts.json): two separately migrated Harness schemas exercised both actual helpers while observing their shared-key waits. Both restricted Workers connected and both credentials were refused by `App::connect`. Cancellation controls covered an advisory waiter and a helper holding the advisory lock while blocked by an owned jobs-relation gate. After releasing each owned gate, the backend and locks disappeared, the key could be reacquired and fresh helper/Worker provisioning succeeded. Both 500ms pre-unlock backend-release observations were false; immediate in-flight SQL cancellation remains unqualified.
- The first helper diagnostic exited 101 because its final receipt used a relative path under Cargo's package working directory. Its cleanup completed and its failed receipt is preserved. Correcting the runner's private receipt destination produced a passing run with identical probe source; adding the explicit waiting-cancellation control then passed in 6.283s. Both owned schemas and the exclusively created temporary test target were removed; the reviewed helper was not edited by the probes.
- `cargo fmt --all --check` passed. Normal parallel `event_consumer_contract` passed 5/5 (6.68s), `worker_contract` passed 1/1 (9.34s), and the additionally affected `event_admission_contract` passed 7/7 (5.45s). No retry, skip, timeout increase or global serial-test mode was used.

The helper tested by these probes has SHA256 `44b7986216467793fbb2181a0c8d42091b39c8009341706888fb421ed427b699`. Current main synchronization added documentation/recovery-test changes; production frontend/server sources and the qualified runtime bytes above remain unchanged. The final PR head requires all four configured hosted checks and independent review before integration; old green jobs do not qualify the corrective head. The helper coordinates cooperating test provisioners only; external deployment provisioning is outside this proof.
