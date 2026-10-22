# Structured issue references — 20 October 2026

Scope: the M04/F12 issue-reference slice, including issue descriptions, comments and retained revisions. This is not completion of M04, F12, portability, realtime or notification delivery.

## Implemented behavior

- Inline `issueReference` nodes contain only a canonical UUID, with at most 50 distinct targets. Templates reject dynamic references. Plain text stores `[issue]`; labels are resolved separately for each reader.
- The authenticated, source-bound candidate endpoint searches readable live issues across teams within one workspace. Unknown, private and foreign-workspace targets cannot be inserted by guessing IDs.
- New references acquire source/target team locks in UUID order before source entity locks, then recheck target eligibility and source location. Existing unavailable IDs can remain during unrelated edits; removing and reintroducing an ID requires current authorization.
- Document, comment and revision projections redact unavailable targets without retaining their key/title. Target rename, move, deletion or membership changes affect representation ETags without incrementing the source document version. Revision responses now expose a strong ETag as well.
- Comment retry receipts omit resolved issue-reference labels. Replays load the current authorized document by identity, including after target moves or renames. No migration or notification side effect was added.

Implementation: `crates/opentrack-domain/src/documents.rs`, `crates/opentrack-app/src/issue_references.rs`, the document/comment/issue write paths, and `crates/opentrack-api/src/mentions.rs`. Canonical contracts: [API](../API_DESIGN.md#structured-issue-references), [storage](../DATABASE_DESIGN.md#structured-issue-reference-document-nodes).

## Executed backend checks

Environment: Windows host, pinned Rust 1.98.1 (`48a229cea`, 1 September 2026), VS 2022 Build Tools via the local `work/platform/cargo-msvc.cmd` wrapper, and isolated PostgreSQL 18.6 at loopback port 55433. Tests use separate runtime/migration/operator/worker roles and fresh disposable schemas in an explicitly named `_test` database. Test environment was loaded with `. ./work/platform/test-env.ps1`; credentials and raw private logs are not committed.

Commands below use `cargo` to abbreviate `& ./work/platform/cargo-msvc.cmd` in that PowerShell environment.

| Executed command | Observed result |
| --- | --- |
| `cargo test -p opentrack-domain -p opentrack-app --lib --locked` | 18 passed: 12 domain and 6 app unit tests, including bounded ID-only references and redacted derived text. |
| `cargo test -p opentrack-app --locked --test issue_references_contract` | The initial three completed reference scenarios passed in 17.02 seconds. |
| `cargo test -p opentrack-app --locked --test issue_references_contract real_postgres_reference_source_move_and_reciprocal_team_lock_order` | The fourth reference scenario passed in 10.01 seconds after being added. |
| `cargo test -p opentrack-app --locked --test issue_references_contract real_postgres_issue_reference_visibility_rename_move_delete_and_revisions` | Passed again in 15.39 seconds after adding the receipt-label stripping regression. |
| `cargo test -p opentrack-app --locked --test documents_contract --test mentions_contract --test mentions_races` | 12 passed: 6 document contracts, 4 mention contracts and 2 mention race/upgrade tests. |
| `cargo test -p opentrack-api --locked --test issue_references_contract` | 1 passed in 16.47 seconds through actual Axum routing, authentication, CSRF/preconditions, query extraction, response headers and JSON, backed by PostgreSQL. |
| `cargo test -p opentrack-api --lib --locked` | 15 passed, including the required nullable reference projection and candidate-route OpenAPI contract. |
| `cargo clippy -p opentrack-domain -p opentrack-app -p opentrack-api --all-targets --locked -- -D warnings` | Passed. |

The four new app scenarios cover private/public and cross-workspace access; forged/missing target rejection with full transaction rollback; literal search and scoped pagination; renamed/moved/deleted targets; current and historical document projections; comment-only references; idempotent current-authority replay; membership grant/revocation; retained versus reintroduced IDs; template rejection; and real PostgreSQL contention in both insertion-before and move/delete-before orders. A further contention case checks source moves and simultaneous reciprocal references across two teams. These checks observe database blocking rather than using timing-only races.

The HTTP test runs the real router in-process with `oneshot`; it is not evidence of an external socket, deployed server or browser interaction. It verifies anonymous and invalid query rejection, private source invisibility, exact compact candidates, forged/private insert rejection, current-reader redaction, document/comment/revision validators, stale-write 412 and comment replay after a target rename.

OpenAPI was exported by the real API example (`cargo run --quiet --locked -p opentrack-api --example export_openapi`); TypeScript was generated with the pinned `openapi-typescript` CLI. Both were independently regenerated and compared against the saved artifacts. Changed Rust files passed `rustfmt --edition 2024 --config skip_children=true --check`; no unrelated workspace formatting was applied.

## Frontend evidence and limits

The coordinating agent executed:

```text
npm test -- --maxWorkers=4 src/references src/documents/IssueDocument.test.tsx src/documents/IssueComments.test.tsx src/documents/model.test.ts src/mentions
```

Result: 57 tests passed across 9 files. Coverage includes reference selection with keyboard and IME event handling, ID-only serialization, inaccessible/current-context labels, late lookup responses, source/access switches and unsaved navigation.

The coordinator subsequently ran the complete existing/reference frontend suite with `npm test -- --maxWorkers=4 --exclude "src/documents/portability*.test.ts" --exclude src/documents/DocumentPortability.test.tsx`: 381 tests passed across 51 files. The excluded files belong to the separate, still-in-progress portability slice. `npm run build` also passed TypeScript checks and the production build. These are automated frontend checks, not a real keyboard/IME or browser qualification. Real browser verification remains pending at this evidence checkpoint; the coordinator records later results separately.

## Migration, failures and retrospective

All applied migrations remain unchanged; schema 19 is still the storage version. Existing JSON bodies/revisions require no backfill, and older serialized receipts default the newly added projection arrays when decoded. Deploy matching backend and frontend support together. An older binary does not understand the new node: a rollback requires a compatible application or the preserved pre-change data snapshot, rather than a speculative SQL down migration. No backup/restore rehearsal is claimed here.

An initial new test incorrectly expected `cursor_expired` for another principal's signed cursor. Existing cursor semantics return validation failure for a mismatched binding; the assertion was corrected and the real DB scenario passed. During review, the comment receipt would have persisted its hydrated issue labels even though replay already reloaded current state. The receipt now omits that projection, and both raw stored-receipt assertions and actual HTTP replay passed afterward.

The existing sorted team-lock boundary made cross-team references possible without a new table or a broad workspace write lock. Keeping labels out of document nodes and retry receipts makes target privacy and rename behavior a current read concern; tests must still cover both target and source moves because locking only the target would miss a changed source team.
