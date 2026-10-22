# Agent 2: live planning scope and captured history

Date: 22 October 2026. Issue: #31, a focused correction to existing planning reporting. Base: `f224100b4f2e519b1037062a4befb298a32351ba`. This increment does not complete #31, its board prerequisite, or the complete-product goal.

## Behavior

`GET /teams/{team}/cycles/{cycle}/scope` calculates current authorized source facts and now returns `kind: "current"`. Previously it used `"daily"`, conflating a fresh read with an immutable persisted daily capture. Reading scope does not create history. The daily capture endpoint still stores at most one record per cycle/local date and returns that original capture on another request that day.

The connected UI labels live scope as **Live scope calculated**, an unsaved close review as **Preview calculated**, and retained reports as **Snapshot captured**, with the corresponding calculation/capture time. No denominator, permission, lifecycle or persisted snapshot kind changes. No migration is required. Rollback uses the previous application binary and static assets; existing durable records remain compatible.

## Executed local checks

Environment: assigned isolated WSL Ubuntu/PostgreSQL 18 runtime and Cargo target; Windows Node for frontend installation, tests, typechecking and build. Rust/backend checks and browser execution use the shared resource locks. Browser fixtures contain synthetic identities only; credentials remain in private runtime state and process environment.

| Check | Observed result |
| --- | --- |
| Baseline `cargo test --locked -p opentrack-api --test projects_contract -- --test-threads=1` | 3 passed against real PostgreSQL. |
| `cargo test --locked -p opentrack-api --test cycles_contract -- --test-threads=1` | 3 passed, including the new same-source project/milestone/cycle regression, existing lifecycle/conflict recovery and private-access/mandatory-effect rollback checks. |
| `npm --prefix web test -- src/cycles/Planning.test.tsx` | 22 passed, including live/captured/preview labeling. |
| `npm --prefix web test` | 543 passed across 65 files. |
| `npm --prefix web run typecheck` | Passed, including browser test types. |
| `npm --prefix web run build` | Passed. Existing bundle-size advisory remains; this change does not qualify performance budgets. |
| `cargo fmt --all --check` | Passed. |
| `cargo clippy --locked -p opentrack-app -p opentrack-api --all-targets -- -D warnings` | Passed. |
| `scripts/generate-contracts.sh --check` | Passed; committed OpenAPI and generated TypeScript match the Rust handlers and DTOs. |
| Real API-backed planning browser suite | 18/18 passed: six saved workflows each in Chromium, Firefox and WebKit, against the production build and private real API/database. [Sanitized receipt](2026-10-22-agent-2-issue-31-browser.json). |
| `git diff --check` | Passed. |

The new PostgreSQL regression assigns the same five issues to one project, milestone and cycle. It compares completed/canceled/unfinished/eligible counts through completion, archive and deletion, then canceled-only and empty scope. It proves scope GETs leave history empty before opening and never create a `current` snapshot. Same-day recapture retains its original response before deletion. A direct comparison of the stored snapshot row before and after all changes proves retention without rewriting.

Deleted or inaccessible source facts are omitted from historical API projections under the existing current-access policy; the stored immutable snapshot remains intact. Existing private-cycle outsider denials remain exercised. Current counts are read from authoritative tables in the API harness without running reporting consumers. This is not a delayed-checkpoint/replay qualification of the future reporting pipeline.

The saved browser regression completes a persisted issue after capturing its daily baseline. Reload shows live 100% completion while the daily history remains at its original counts. Repeated scope reads, another daily capture and a further reload preserve the original history. Existing closure/carry workflows additionally check preview and retained-report labels.

## Review and remaining gates

An independent internal read-only review of the implementation and test diff found no actionable correctness or privacy findings. That review did not execute tests.

#31 stays open for board/list parity and complete reporting qualification. #26's independent project documents and #30's persistent board ordering are separate increments. Native windows, physical input, assistive technology, performance, full transfer/recovery and all other product gates require their own executed evidence. No issue or milestone is closed by this record.
