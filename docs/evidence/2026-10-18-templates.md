# Team issue templates and interaction follow-up

Date: 18 October 2026. This is a bounded F23/M04 increment and a follow-up to the reported interface flicker and rough interactions. It does not close the complete-product goal, remaining M04 work or native rendered-UI qualification.

## Implemented behavior

Team settings now has a templates panel with metadata-only listing, lazy body loading, preview, version-aware editing and archive/restore. New issue creation can apply a team template after reviewing its title, rich document and status/priority/label defaults. Applying to an edited draft requires merge, replace or cancel. Ready attachments and independent assignee/estimate/due-date edits are preserved; pending uploads prevent changing insertion positions. Invalid defaults are shown individually and require repair. A changed template version requires review without discarding the local draft. Ambiguous creation retains an identical request and idempotency key.

Migration0018 adds current definitions and immutable per-issue source version/digest attribution. Issue documents are independent copies; template edits cannot rewrite them. Private team membership remains required even for workspace owners/admins. Committed issue-create receipts resolve before mutable template checks, under current issue authority. Definition/source effects, required audit/events and receipts are atomic. Existing non-template create request serialization and hashes remain compatible across the upgrade.

The shared Modal now prevents Escape from propagating into an underlying dialog. The final template interaction fixes retain a same-scope preview while refreshing, disable application until revalidation succeeds, recover from transient refresh failures, clear inaccessible content, and keep keyboard focus on the stable section heading after archiving/restoring a row. These extend the [earlier layout repair](2026-10-18-ui-refinement.md) and [theme/editor fixes](2026-10-18-attachments.md).

## Executed qualification

- Four isolated real-PostgreSQL application contracts passed, including eleven suppressed-write rollback cases. They cover lifecycle/pagination, independent issue bodies and attribution, maintainer/contributor/viewer/private-team authority, stale references, archive/replay and unsupported documents. Log: `work/test-templates-contract-r1.log`.
- The observed-lock race suite passed: both orderings of edit/archive vs create, status/default changes vs create and access revocation vs use, plus normalized-name and final-capacity-slot races. Log: `work/test-templates-races-r1.log`.
- The workspace Rust gate passed138 tests; the final API contract rerun passed13 tests, including one additional contract assertion, for139 unique tests. Formatting, strict all-target Clippy and the server build passed. Logs: `work/check-templates-rust.log`, `work/test-templates-api-contract-final.log`, `work/clippy-templates-final.log`. The final generated contract drift check and subsequent TypeScript check passed (`work/generate-templates-contracts-final.log`, `work/typecheck-templates-generated-final.log`).
- The HTTP template contract passed after correcting a test expectation: the existing API maps well-formed JSON with unknown/null fields to422, rather than400. The first failure is retained in `work/test-templates-http-r1.log`; the passing rerun is `work/test-templates-http-r2.log`.
- Native core/integration checks passed41 tests; strict native Clippy passed. They include exact scoped-template route allowlisting, bounded rich-document requests, and rejection of unregistered actions. Logs: `work/test-native-templates-all.log`, `work/clippy-native-templates.log`. Twelve development-credential/supervisor checks also passed in `work/test-templates-scripts.log`.
- Isolated real-process qualification on port18973 passed six stages: current API/worker refusal of schema17 without writes; migration18 and restricted grants; create/preview/independent instantiation; stale-version412 and committed retries through edit/archive; persistence through API restart; and a restricted worker cycle. Receipt: `work/templates-process-result.json`. Main workspace data was not changed by this harness.
- A fresh schema17 database snapshot restored51 table/sequence fingerprints plus matching table/function ownership, ACLs and RLS. The preserved schema17 server then passed eight real HTTP restore checks, including exact startup theme delivery. The attachment catalog was empty; this is not a populated blob-backup proof. Receipts: `work/postgres/restore-before-0018-snapshot-result.json`, `work/restore-before-0018-snapshot-http-result.json`.

The final shared frontend suite passed337 tests across44 files (`work/test-templates-release.log`). Final typecheck and isolated production build passed (`work/build-templates-geometry.log`); the existing chunk-size warning remains. The26 targeted template tests also passed after the final layout/focus repairs (`work/test-templates-geometry.log`).

## Browser interaction and visual checks

The isolated workspace on port18973 was exercised through the real browser UI, without changing the main development workspace:

- Created Release check through team settings, archived it, switched to archived definitions and restored it. Each mutation persisted, the row moved to the appropriate list and focus landed on the stable Issue templates heading. Opening a new definition in the final build focused Template name; cancel restored the initiating button.
- Entered an original title and description, opened Bug report, and verified Ctrl+Enter could not submit the underlying issue. Escape closed only the picker, preserved both fields and restored focus to Use a template.
- Reviewed the rich preview and refreshed it, then explicitly merged. The original title and paragraph remained alongside headings, ordered steps and checklists; status and priority came from the preview.
- In a second tab, changed the selected definition from version5 to6. Submitting the original draft showed the conflict and disabled creation without removing edits. Reviewing the new version and explicitly choosing Keep draft with reviewed version enabled creation. TMP-2 was created once with the preserved title/body; reload retained them and showed no false discard prompt.
- That check exposed focus falling to the page after the disabled submit. The final fix was reproduced with a version6→7 conflict: focus now moved to the applied-template review button. The temporary second draft was explicitly discarded, with focus returned to New issue.
- At1280×720, the original660px chooser hid its action buttons below the viewport. The final860px layout keeps its header and actions visible while list and preview scroll independently. The [final screenshot](2026-10-18-templates-picker.png) records the rendered result. Narrow layouts have CSS support but were not exhaustively rendered in this increment.
- Typing C in the actual comment editor left the text in that editor and did not open issue creation; the temporary local draft was cleared without posting.

The earlier full frontend run and first native package preceded the final geometry/focus corrections. The final337-test rerun and second package use the corrected source. First-package output was not installed.

## Local deployment and desktop installation

The final API binary was first restarted against the isolated schema18 database. Readiness, three template reads and the browser-created document survived that restart (`work/templates-final-process-result.json`). Its SHA256 is `8242fc349381bccc336f8a82c75909ae4ff8f40975e8b2d0b6fff9973abfe9a0`.

The exact owned development supervisor and schema17 API/worker were stopped before the final backup. The fresh368,502-byte database archive restored51 data/sequence fingerprints with matching ownership, ACLs and RLS; the preserved schema17 server passed eight HTTP restore checks. Receipts: `work/postgres/restore-before-0018-deploy-final-result.json`, `work/restore-before-0018-deploy-final-http-result.json`. The catalog was still empty of attachments, so this backup does not establish populated blob recovery.

Migration0018 and restricted worker grants were then applied. The qualified static assets were published with the HTML entry last and older hashed assets preserved for open clients. The new API and worker match the qualified binary, use separate restricted credential sets, and pass readiness. Both workspaces have required consumers registered, no failed jobs or dead letters, and zero lag. Issue/document projections remain consistent. The startup theme script and entry JS/CSS return exact qualified bytes with correct content types. Receipt: `work/deployment-0018-result.json`.

The real main browser tab was refreshed onto the deployed build. ENG-5 retained its title, assignee, label and empty description. New issue opened with title focus, the template picker correctly showed no active definitions in the main team, Escape returned to the outer dialog and focused Use a template, and Cancel returned to New issue without a discard warning. No issue or template was created in the main workspace. The [deployed interface screenshot](2026-10-18-templates-deployed-main.png) records that result. Isolated browser tabs and the verified owned test API were then closed; main services and the existing native process remained running (`work/templates-process-cleanup-result.json`).

The final Linux package was built from the same qualified frontend and atomically installed for the current user. The8,994,834-byte Debian package SHA256 is `912205cefb5fc0d36f0506ff38a6347d06ffa317a8dd4c7c8bc4194b73e9c800`; the installed executable SHA256 is `dd9914d53bfc9468d937f46e4e5b3a10b7d5456696189fddb6be53c2dd0c664e`. Decoded embedded JS/CSS/theme bytes match the qualified frontend; HTML parser normalization and equivalent relative source-map paths are the documented comparisons. The desktop entry, URI handler, icons and atomic installation passed. Receipt: `work/desktop-build/package-templates-v2-result.json`.

Native PID692136, its start time and executable hash were preserved, as were connection configuration and credential references. The installed update applies on the next launch. No fresh native window was launched or visually inspected, so package and core checks do not establish the rendered native workflow.

## Remaining scope

Project defaults, title substitutions, template attachment cloning, mention/subscription delivery, child trees, playbooks and cross-feature templates remain unfinished. Native route/core/package checks do not establish the rendered native workflow. No external publication, signed release or updater qualification is claimed.
