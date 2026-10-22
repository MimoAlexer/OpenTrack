# Project milestones

Milestones represent explicit intermediate outcomes in one project. Their title, optional target date, manual order and planned/completed state are persisted. Completing a milestone records its completion time; reopening clears it. Neither operation changes issue status or assumes that remaining scope is complete.

The authenticated API is implemented. Connected planning screens and keyboard/date-label usability are separate M05-T04 acceptance work; this increment does not claim a milestone screen or native GUI qualification.

## Scope and permissions

A project lead or current team manager can create, rename, reorder, complete, reopen and delete milestones. A project contributor can assign or clear a milestone on an issue already linked to that project. Viewers can read. Archived projects, teams and workspaces are read-only. Inaccessible projects and private teams return concealed 404 responses, including on retries.

The `project_issues` relation remains the only project membership authority. Its nullable `milestone_id` has a composite foreign key including workspace, team and project. An assignment to any other project's milestone fails in both PostgreSQL and the API. Project issue lists return the assigned milestone ID. Removing project membership clears the assignment by removing that membership row; adding membership again never restores it implicitly.

Deletion is atomic under the same team lock used by issue and project writes. The foreign key clears only `milestone_id`; all issue rows and project membership rows survive. Concurrent assignment waits and revalidates after deletion. There is no asynchronous retirement window or partially detached state. A timeout rolls back the entire operation.

## Progress denominator

Progress includes only issues in this milestone's project and currently accessible owning team, excluding archived and deleted issues. `total` includes canceled issues; `canceled` reports them separately. `eligible = total - canceled`, `completed` counts the completed workflow category, and `open = eligible - completed`. Thus canceled work is neither open nor completed. All-canceled and empty scopes return `eligible: 0` and `unscoped: true`; clients show an empty-scope label and must not invent a percentage. For nonempty scope, a client may calculate `100 * completed / eligible` and label the basis. Manual milestone completion is independent.

The list aggregates progress for a whole page in one SQL query. Detail progress uses the same exclusions and categories. `as_of` is the database statement timestamp; these are live reads, not historical or cached projections.

## Concurrency and API

All paths are relative to `/api/v1/workspaces/{w}/teams/{t}/projects/{p}`.

| Endpoint | Contract |
| --- | --- |
| `GET /milestones` | Rank/ID page; default 50, maximum 100. Each item contains `milestone` metadata and `progress`. The page includes `project_etag`. Cursors bind actor, permission epoch, project version and page size. |
| `POST /milestones` | Title and optional target date; requires Idempotency-Key and current project If-Match. Appends to the order; returns 201, Location and milestone ETag. A replay rechecks access and current existence. |
| `GET/PATCH/DELETE /milestones/{id}` | Detail metadata; PATCH/DELETE require the milestone If-Match. PATCH accepts title, nullable target date and planned/completed state. DELETE returns 204 after detachment. |
| `GET /milestones/{id}/progress` | Authorized live counts with the denominator above. |
| `PUT /milestones/order` | Complete ordered `milestone_ids`, each exactly once; current project If-Match. Returns the new project and ETag. |
| `PUT /issues/{issue}/milestone` | Explicit nullable `milestone_id`; current project If-Match. Returns the new project and ETag. |

Fetch current project metadata before a project-versioned write. Milestone metadata ETags exclude the separately returned live progress. Changing milestone metadata, order or assignment advances project version, invalidating stale order/edit intent. Milestone order changes also advance changed milestone versions. No-op order/assignment requests preserve versions. The server accepts at most 500 milestones per project and at most 500 IDs per complete order; ranks are server-owned positive integers. Invalid incomplete/duplicate orders are 422; stale validators are 412 with authorized current state. Mandatory audit, durable events and create receipts commit atomically with each change.

The native transport explicitly permits these scoped routes and their published methods. Its route/identifier and generated-contract parity tests cover transport reachability; they do not substitute for an interactive native workflow.

## Migration and recovery

Migration 0023 adds `milestones`, its workspace RLS policy and immutable-identity/version guard, plus the nullable project-membership milestone foreign key and indexes. Existing project memberships remain unchanged with null assignments. Positive rank uniqueness is deferred until commit so a complete reorder can swap ranks atomically. Runtime roles inherit existing provisioning grants; no extra database capability or external dependency is introduced. Server and worker readiness require the new relation and membership column.

Applied migrations are immutable. Rollback is a restore to a verified pre-0023 database and matching binary; simply downgrading a serving binary does not remove milestone data. Standard PostgreSQL backup/restore includes milestones and assignments. No production database migration or restore is claimed by the contract tests.

## Verification

Executed on 22 October 2026 with Rust 1.98.1 and PostgreSQL 18 under Linux; frontend checks used Node 24.14.1 on Windows. Every database contract uses a unique schema in the isolated test database and restricted runtime credentials.

| Check | Observed result |
| --- | --- |
| `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed. |
| `cargo test -p opentrack-domain --locked` | 20 tests passed, including omission/null/explicit-clear validation. |
| `cargo test -p opentrack-api --test milestones_contract --test projects_contract --locked` | Six real PostgreSQL HTTP contracts passed: lifecycle, order, stale validators/cursors, completed/open/canceled/archived/deleted count parity, empty and all-canceled scope, project/DB FK rejection, safe detach, lead/contributor/viewer permissions, private-team concealment, RLS, mandatory audit/event rollback and delete/assignment serialization. Existing Projects contracts remained green. |
| `cargo test -p opentrack-app --test milestones_upgrade_contract --locked` | Populated schema 22 upgraded to current schema; existing membership and issue survived; new table runtime grants came from deployment defaults; assigning then deleting a milestone preserved the issue and membership. |
| `scripts/generate-contracts.sh`; `scripts/generate-contracts.sh --check` | Generated OpenAPI and TypeScript match the final Rust DTOs and handlers; existing endpoint contracts retain their prior semantic definitions. |
| Desktop format, Clippy and `cargo test --manifest-path desktop/Cargo.toml --no-default-features --locked -j 2` | Passed, including explicit milestone method/identifier restrictions and the published API native-route parity check. |
| `npm --prefix web test`; `npm --prefix web run typecheck`; `npm --prefix web run build` | 495 tests across 61 files, application/browser type checks and production build passed. Existing Vite chunk-size advisory remains. |

An initial local native compile lacked GLib development headers; installing the required core development package allowed the same checks to pass. Early HTTP test expectations were corrected to the existing 412 stale-validator, 400 malformed-body and 422 invalid-cursor contracts; application behavior was preserved. These results establish the milestone model and API, not browser/native milestone UI acceptance or a production recovery exercise.
