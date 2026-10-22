# Explicit template title substitutions

Date: 20 October 2026. Scope: the title-default portion of F23 / M04-T05 / issue #24. Project defaults, attachment/reference cloning, mentions, child trees and X09/X41/X66 expansion workflows remain open.

## Behavior

Template authors choose literal or pattern mode. Literal is the default and preserves existing brace text. Pattern mode accepts only `{{team.name}}`, `{{team.key}}` and `{{date}}`, using the currently authorized team and UTC `YYYY-MM-DD`. It does not evaluate expressions or recursively expand inserted text. Malformed/unknown placeholders are rejected; source and resolved titles have a 256-character Unicode bound, and oversized output is reported without truncation.

Preview returns the resolved title, UTC context timestamp and field diagnostics. Applying copies that reviewed title into the editable issue draft. Creation and interrupted retries preserve the explicit draft title across midnight or later team renames. Merge, replace, cancel and keeping a reviewed version remain explicit choices; a retained valid draft title can repair an oversized new default. Created issues remain independent of future template edits.

## Executed checks

Environment: Windows x64, Rust 1.98.1 with the installed MSVC toolchain, Node 24.14.1 and PostgreSQL 18.6. Database tests use separate runtime/migration credentials and generated isolated schemas in the existing local test database. The development database was not changed. Rust tests ran serially; frontend test concurrency was capped at two workers.

| Check | Result and evidence |
| --- | --- |
| Domain/app template unit tests | 7 passed: opt-in literal compatibility, exact legacy request bytes, literal digest compatibility, pattern-sensitive digests, unknown/malformed placeholders, nonrecursive substitution, control rejection, Unicode bounds, leap day and UTC midnight/year boundaries. |
| Application template contracts | 6 passed against real PostgreSQL: current/private authority, independent instances, stale references, atomic rollback, explicit reviewed titles, live team rename, oversized preview/repair and populated 19-to-20 migration. |
| Template race/upgrade contracts | 2 passed: observed ordering/capacity and historical 17-to-18 receipt/grant/RLS assertions. The latter keeps its schema-18 assertions exact and upgrades to the current schema before invoking the current template-create API. |
| HTTP template contract | 1 passed against real PostgreSQL: literal mode default, resolved UTC title/context, invalid modes/placeholders, version conflicts, explicit created title and idempotent replay. The JSON extractor rejects null enum mode with 400; unknown mode and invalid patterns return 422. |
| Frontend template tests | 36 passed in 4 files: date preservation across midnight and interrupted retry, literal braces, diagnostics and repair, mode editing/conflicts, merge/replace/cancel, access refresh and late-response concealment. |
| Rust format and strict lint | `cargo fmt --all --check` passed. Clippy passed for domain, app, API and server with all targets and warnings denied. |
| Generated contract and frontend build | Rust OpenAPI export and pinned TypeScript generation succeeded; `npm run build` passed TypeScript checking and the production bundle. The existing large-chunk warning remains. Combined contract regeneration is part of integration. |

Commands used: `cargo test --locked -p opentrack-domain -p opentrack-app --lib templates::`, `cargo test --locked -p opentrack-app --test templates_contract --test templates_races`, `cargo test --locked -p opentrack-api --test templates_contract`, and `npm run test -- src/templates --maxWorkers=2`.

The first database run was interrupted when the local PostgreSQL process ended. Its isolated fixture was identified and removed after the cluster was restored. Successful reruns are reported above; the interrupted attempt is not counted as passing evidence.

## Migration and limits

Migration 0020 adds a constrained non-null title mode with a literal default. The populated upgrade test preserves existing title text, template version, definition digest, immutable source attribution and old create-retry receipts. Literal requests keep their previous serialized bytes and digest format. Migrations 0001–0019 are unchanged. Server and worker preflight checks now require the title mode column.

Apply 0020 before the matching application. Rollback requires a preserved pre-upgrade database and matching application/frontend; an older executable must not interpret new pattern rows as literal text. This run does not qualify a deployed upgrade, backup restore, live browser interaction or screenshots. No local API process was launched for this increment, and issue #24 remains open for its other acceptance obligations.
