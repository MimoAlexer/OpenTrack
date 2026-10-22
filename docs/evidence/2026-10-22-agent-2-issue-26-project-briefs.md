# Agent 2: independent project briefs

Date: 22 October 2026. Issue: #26. Initial browser base: `d01fb0c010bef0c12739bcc332074f1a4dbc999f`; initial publication base: `0781476c655acd398a0cb0cac3e3a2f143e5e575`. The correction incorporates integrated main `de1caafd3cb611586d33d1598420396b4e7d19b0` through neutral merge `a4245c1a4a6b4bfd6d7f561f4bdc7976beb9bd70`. Recovery snapshots preserved pending work through synchronization; all peer changes were retained.

## Connected behavior

The project overview edits its brief through the existing rich document editor. Metadata and brief saves have independent validators: changing a project's name, dates, state or health does not overwrite its brief, and saving the brief does not invalidate a metadata draft. Brief conflicts preserve the rejected text and require review before an explicit save.

Project briefs support the existing formatting, user mentions, issue references and file attachments. References resolve under current access; a project mention never creates an issue subscription. Archiving makes the brief read-only. Losing source access conceals the project and its document projections.

**Brief history** opens saved revision summaries and loads a body only when selected. Revisions are read-only; selecting one preserves the current draft. History uses the existing 90-day document retention policy. A revision expiring after it was listed displays an unavailable message after checking current document access, without discarding the live draft. Actual owner access loss still conceals the enclosing project.

Existing briefs become one explicitly marked migrated snapshot. Its author and earlier editing time are unknown; the displayed capture time is migration time. New saves record the real actor. Unsupported historical formats remain readable as inert plain text and cannot be rewritten through an unsupported editor.

## API and storage contract

- `GET/PATCH /teams/{team}/projects/{project}/metadata` returns `ProjectSummary`, with its own strong metadata validator. Supplying `narrative` to this PATCH returns validation failure.
- `GET /teams/{team}/projects/{project}/document` discovers the standard document. Existing `/documents/{document}` and revision routes own brief saves and history.
- The existing full project representation remains available. Its narrative comes from the authoritative document; additive `narrative_version` and `metadata_etag` identify the independent resources. Its strong validator includes the changing body representation and version.
- Existing full conditional project PATCH commands remain compatible. A supplied brief passes through the same transaction-local document mutation and effects; a stale full validator fails. Combined writes commit or roll back together. Child planning commands accept the current full or metadata parent validator.
- Creation retries preserve their recorded metadata, brief bytes and known brief version. Current read access, permission/lead presentation and the returned validators are rechecked. A pre0026 receipt has `narrative_version: null`, preserving its original brief without inventing a document revision or substituting current content. Current project reads always expose the actual document version.

Migration `0026_project_documents.sql` adds exactly one scoped live document per project, immutable ownership, restricted migrated provenance, and typed project mention sources. It preserves existing issue/comment authorship, effects and current-access rules. Legacy project narrative columns remain immutable snapshots; documents is the sole mutable brief authority. Runtime users cannot forge migrated provenance or authorless later revisions.

The upgrade must run with the table-owning migration role and the matching application/static assets. Quiesce incompatible writers and retain a database, blob and key backup before rollout. Old binaries cannot safely write the new project schema. There is no destructive down-migration: recovery requires a matching pre-upgrade backup and binary, or a reviewed forward repair. Retained columns do not qualify an old-binary rollback. Their eventual removal requires a supported release overlap and a restore-tested backup.

## Executed checks

Isolated WSL Ubuntu/PostgreSQL runtime and Cargo target; Windows Node for ordinary frontend checks; shared resource locks for heavy verification and the loopback browser harness. Fixtures use synthetic identities. Credentials and backup archives remain in private ignored runtime state.

| Check | Observed result |
| --- | --- |
| `cargo check --locked -p opentrack-api --lib` | Passed. |
| API `projects_contract` | 6 passed, including independent versions, concurrent/stale saves, atomic legacy commands, mandatory-effect rollback, reference privacy and mismatched-route lock ordering. |
| Corrective API `projects_contract` | 8 passed after the creation-replay correction, including original outcome/version preservation, legacy unknown versions, current viewer/archive permissions and revoked-access denial without duplicate durable effects. |
| API document/attachment/revocation/mention/reference targets | 12 passed. |
| App `project_documents_upgrade` | 3 passed: fresh schema, populated 0025-to-0026 preservation/invariants, failed-upgrade atomic rollback and retry. |
| App `milestones_upgrade_contract`, `cycles_races` | 1 and 7 passed. |
| Full frontend unit suite after the final expired-revision correction | 571 passed across 66 files. |
| Frontend unit suite after integrating main PR142 and replay correction/contracts | 579 passed across 66 files. |
| Final `DocumentHistory.test.tsx` | 18 passed, including expired revision versus current owner denial and late reauthorization after close. |
| Frontend typecheck and production build after that correction | Passed. Existing bundle-size advisory remains; no performance gate qualified. |
| Strict Clippy for app, API and server, all targets | Passed with `-D warnings`. |
| Native protocol tests | 13 passed. This verifies route policy, not native GUI behavior. |
| Additional App document, mention, attachment, template and mention-race targets | 6, 4, 5, 2 and 2 passed respectively. |
| Generated contracts | Fresh Rust OpenAPI export byte-identical to the working artifact; pinned Windows `openapi-typescript` output byte-identical to the generated client. |
| App issue-reference contract | 4 passed; final format check and strict all-target Clippy rerun passed after historical fixture repairs. |
| Real API-backed browser matrix | 39/39 passed: 13 saved workflows in each of Chromium, Firefox and WebKit, against the production build and upgraded private database. [Sanitized receipt](2026-10-22-agent-2-issue-26-browser.json). |

Initial populated-upgrade tests exposed deferred owner checks running after the migration changed workspace context. The migration now flushes its new deferred constraint within each workspace; all three upgrade tests then passed. Two existing document author assertions were updated to require a real optional actor and `migrated=false`. The historical milestone fixture now seeds its actual old schema directly before exercising current APIs after migration.

An additional App attachment regression initially passed four of five tests: its schema16 upgrade fixture called the current document reader before upgrading. Attachment, mention and template upgrade fixtures now capture their actual historical SQL rows first and compare both exact stored data and current API representations after migration. Their focused suites passed; no production compatibility fallback was added.

A later review found that creation replay substituted the current brief/version into its stored result. A new real API test failed on original head `888057cf0114500a1f2af4bf730e41ef344b2007` after confirming no duplicate stored effects. The correction preserves original body/version, while honestly representing unknown pre0026 receipt versions as null. All eight project API and three upgrade tests then passed; strict all-target Clippy passed. The first corrective compilation rejected an unsupported schema-description attribute; a normal schema doc comment corrected it. The reserved migration did not change.

Hosted AMD64 run `39826093657`, job `119989737974`, failed its complete browser matrix at 93/96: the project reference/file workflow failed in all three engines. Its [sanitized receipt](2026-10-22-agent-2-issue-26-browser-hosted-initial.json) is preserved. Native and ARM64 checks passed at that earlier head; those results do not qualify the correction. The case's failure location is the owner-mention persistence check. A combined local keyboard/planning/project Chromium run reproduced it: [13/14 passed](2026-10-22-agent-2-issue-26-browser-keyboard-before.json), with the same sole failure. Earlier workflows create an eligible member that sorts before the owner; the test selected the first candidate after merely checking that the owner was visible. A private two-candidate fixture and explicit selected-owner keyboard assertion remove that assumption, preserving reference, download-byte, history and narrow-layout assertions. The same combined run then [passed 14/14](2026-10-22-agent-2-issue-26-browser-keyboard-after.json). An approved filename/line-only reporter extension preserves all raw diagnostic exclusions. Final typecheck/build passed; the complete 111-case matrix is running before the corrective publication.

The first expanded local matrix was interrupted after a workflow replacement remained queued: its local launcher had started the API without the restricted worker required by that existing case. This did not establish a product regression or a complete matrix result; no new final receipt was written, and the preceding 14-case receipt was not counted as this run. The launcher now applies the existing scoped worker grants, starts the existing worker with its own role credentials, and stops/waits for both owned processes before releasing the browser lock. A restricted `worker --once` poll passed; the complete rerun remains required.

The complete worker-backed rerun finished [110/111](2026-10-22-agent-2-issue-26-browser-full-before.json). Every project case passed in all three engines; Chromium and WebKit each passed all 37 checks. The single Firefox failure is an existing workflow test's immediate reload after a successful issue POST: the created issue appears in the list, while the detail textbox expected after reload is absent. Its owner has been asked to review awaiting the created UI/navigation before reload while retaining the original post-reload assertion. No peer test source was changed, and this full matrix is not marked accepted.

The first complete browser run passed 36/39 checks. All three failures were the same creation-test assertion: replacing text in a bold brief correctly retained its bold mark, while the expected body omitted it. The assertion now checks the exact retained formatting. The initial [sanitized receipt](2026-10-22-agent-2-issue-26-browser-initial.json) is preserved; the complete corrected rerun passed 39/39. An intervening harness start exited at its port probe before executing tests; no listener was present on inspection, and the probe now allows reuse after a closed socket without bypassing a live listener.

A private pre-upgrade schema24 dump was created and its archive listing verified: 504215 bytes, SHA-256 `6f6d876ca932a75720ccb1748c232d333ca8f45489f3b95b1910d1bd1cce1abd`. This is not a successful backup-restore rehearsal. Failed-upgrade rollback tests likewise do not prove restoration after a committed upgrade.

## Remaining qualification

The browser matrix covers independent saved brief/metadata changes, explicit two-editor conflict review, historical selection without writes, keyboard mentions/references, uploaded/downloaded byte equality, narrow-layout history, planning regression and existing issue conflicts. It does not qualify physical keyboard input or assistive technology. An independent read-only review found the expired-revision UI issue described above and verified its correction without further findings. Staged privacy and whitespace checks passed; hosted checks and coordinator review remain required at the published exact head.

#26 stays open until its full acceptance and inherited product gates have evidence. Native windows, physical input, assistive technology, full transfer/restore, provider and release qualification remain separate. This increment does not complete the assigned queue or whole-product goal.
