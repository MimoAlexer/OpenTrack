# API contracts

Status: target API contract for authorized implementation; executable coverage is recorded in [REQUIREMENTS_STATUS](REQUIREMENTS_STATUS.md). [Feature contracts](PRODUCT_SPECIFICATION.md), [data model](DATABASE_DESIGN.md), [security](SECURITY_ARCHITECTURE.md), [realtime](REALTIME_ARCHITECTURE.md).

The implemented endpoint contract is generated from Rust handlers and DTOs into `docs/api/openapi.json`, served at `/api/v1/openapi.json`, and consumed by generated TypeScript types. `scripts/generate-contracts.sh --check` rejects drift. This target catalog remains broader than the current application; generated routes do not imply the unimplemented resource families below exist.

## Shared wire rules

HTTPS, UTF-8 JSON, /api/v1, opaque UUID identifiers. Dates use YYYY-MM-DD; timestamps use RFC 3339 UTC. JSON integer counters that could exceed JavaScript safe integer range, including event sequence and entity version, are decimal strings. An unknown field in a command is rejected; readers tolerate additive fields in a response. Null explicitly clears a nullable field; omitted fields remain unchanged. Empty strings are not a second null representation.

Workspace prefix is /api/v1/workspaces/{workspace_id}; below it use plural resource names. The browser and public clients use the same app services and ACL. /internal/v1 is a separate runner/control audience, never exposed as a privileged browser shortcut. Anonymous public issue sharing is not MVP; public API means documented authenticated developer API.

Session cookies are same-origin, HttpOnly, Secure, SameSite=Lax. Mutations require CSRF token and allowed Origin. API bearer tokens are opaque, hashed at rest, workspace-scoped, expiring and limited by both token scopes and the user's current role. They cannot authorize HTML session actions. Stream/auth tokens never appear in URLs. CORS is denied except explicitly configured origins; credentials and wildcard origin must never be combined.

Successful GET returns 200; creation returns 201 with Location and ETag; durable asynchronous work returns 202 with job ID and status URL; deletion returns 204 after tombstoning. Versions use strong ETags. Issues use `"issue:UUID:version:reference-digest"`; clients send the returned `etag` unchanged rather than constructing a validator from the numeric version. PATCH/DELETE and actions modifying an existing aggregate require If-Match; missing is 428, stale is 412. Domain conflicts such as cycles or archived statuses use 409. A write response contains the canonical entity and version; callers do not invent authoritative timestamps. Mutation identifiers apply where the generated contract exposes them.

A member/team-grant update that expands the caller’s own access returns its normal successful entity, status and ETag, plus `X-OpenTrack-Session-Invalidated: true` and a clearing session cookie. This means the mutation committed. Clients clear authenticated views and ask for sign-in without replaying the mutation or treating it as a failed `401`. Updating another account omits this signal and preserves the caller cookie; the affected account’s old sessions fail subsequent authentication. A failed authorization/precondition does not revoke sessions. Existing-account invitation acceptance instead returns the replacement Session and cookie and invalidates all earlier sessions, preserving the prior authentication time and absolute expiry.

POST commands that create an object or external effect require an Idempotency-Key, scoped to workspace, authenticated principal and route. Store a request digest and exact success response atomically for 24 hours. Repeating the same key/body returns that outcome, even after connection loss; changed body is 409. Concurrent first submissions serialize on the scoped unique record. Expiry is advertised; clients must reconcile by returned object/job identifier before retrying an unknown older operation. Public clients may supply a unique client_reference for import/agent launch where longer deduplication matters.

Invitation issuance has a narrow secret-display exception: the invitation metadata is idempotent, but the bearer token is returned only on first issuance. Replay returns `token:null`; an issuer who lost the first response revokes that invitation and creates another. Only the token hash is retained, never the raw bearer in an idempotency response. Offline invitations use manual token entry and POST preview/accept; no SMTP account is needed. Existing accounts must authenticate as the invited email; accepting a token cannot reset their password. New-account acceptance consumes the token and creates membership/session atomically. Preview and final acceptance use current database clock time; the consuming UPDATE rechecks the token, version, terminal state and expiry, so waiting on a lock cannot extend validity. An expired or already-used token returns the existing concealed 404. Replacing an expired pending invitation records restricted retirement audit without returning or publishing its old ID/private grants. Replay still returns only the original replacement metadata and `token:null`.

## Representative contracts

| Operation | Input | Result and failure contract |
| --- | --- | --- |
| POST /workspaces/{w}/issues | team_id; title 1–256 Unicode characters; optional document, status_id, priority 0..4, assignee_id, project_id, milestone_id, cycle_id, parent_id, label_ids; mutation_id | 201 canonical issue with readable key; default workflow applied server-side; invalid/cross-scope IDs rejected before commit |
| PATCH /workspaces/{w}/issues/{id} | If-Match; allowlisted changed fields; no workspace/team transfer through generic patch | 200 version+1; 412 gives current version and authorized current fields; user draft retained client-side |
| POST /workspaces/{w}/issues/{id}/move | If-Match, destination_team_id, destination_status_id, confirmation of fields to clear | 200 new key, preserved UUID, old-key alias; no cross-workspace move |
| POST /workspaces/{w}/issue-batches | action, at most 100 IDs with expected versions | 202 operation report; authorization and result per item; no claim of all-or-nothing unless explicitly requested and supported |
| PATCH /workspaces/{w}/documents/{id} | If-Match, schema_version, validated ProseMirror JSON <=256 KiB | 200 canonical document version; no blind whole-document retry on 412 |
| POST /workspaces/{w}/attachments | name,size,mime hint; existing document context or draft team_id | 201 pending upload ID bound to uploader/team; bytes streamed separately; MIME hint is untrusted |
| PUT /workspaces/{w}/attachments/{id}/content | session+CSRF, Content-Length <=25 MiB, checksum | 202 quarantine/scan or validation state; no ready link before final validation |
| GET /workspaces/{w}/attachments/{id}/content | linked-document read access or unlinked uploader plus current team access | authorized stream with nosniff/content-disposition; unavailable/revoked returns 404 |
| POST /workspaces/{w}/agent-runs | issue_id,repo_id,immutable base SHA or resolvable permitted ref,grant_id,provider_id,budget | 202 run ID; no process starts before persisted authorization and budget reservation |
| POST /workspaces/{w}/approvals/{id}/decision | decision,expected artifact/head/base/policy digest,If-Match | one-use approval; stale digest is 409; a later changed diff needs another approval |

Examples are field contracts, not generated application code. Create issue defaults: priority=0 (unset), assignee=null, project/cycle=null, status=team default, subscribers=creator. Duplicate title is allowed. Description content is validated before opening the transaction. A referenced object outside scope is indistinguishable from a missing object to unauthorized callers.

## Resource inventory

All workspace paths below inherit the prefix above. Named action subresources use POST; GET is side-effect-free except explicitly documented session activity bookkeeping.

| Surface | Routes / operations | Scope and special semantics |
| --- | --- | --- |
| Bootstrap/auth | /api/v1/setup/status; /auth/login,logout,session,csrf,recovery; /auth/oidc/start,callback | Setup consume requires local one-time secret; login throttled; recovery never reveals account existence |
| Account sessions | GET /auth/sessions; POST /auth/sessions/{id}/revoke; POST /auth/sessions/revoke-others | Own account only, independent of workspace membership; signed bounded pagination; revoke other sessions requires recent authentication; current revoke clears cookie |
| Organizations/workspaces | /api/v1/organizations; /organizations/{o}/members; /api/v1/workspaces; /workspaces/{w} | admin authority separate from content access; deletion is delayed job |
| Membership | /members; /invitations; /invitations/{id}/revoke; /teams/{id}/members | last-owner invariant; link-invite token exchanged by POST; single use |
| Teams/workflows | /teams; /teams/{id}; /teams/{id}/statuses; /statuses/{id}/replace | replacement maps referenced issues atomically or as locked maintenance task |
| Issues | /issues; /issues/{id}; /issues/by-key/{key}; /issues/{id}/children,relations,subscribers,activity,move | ETags on mutable resources; relationships enforce graph invariants |
| Taxonomy/ownership | GET/POST /labels; GET/PATCH /labels/{label}; GET /teams/{team}/assignees; issue PATCH label_ids | Shared/team label management, complete label-set replacement and paginated eligible assignees; existing links preserve historical metadata |
| Discussion/editor | /issues/{id}/comments; /comments/{id}; /documents/{id}; /documents/{id}/revisions | author edits own comments; maintainer may moderate with audit; no secret history in public errors |
| Projects | /teams/{t}/projects; /teams/{t}/projects/{id}; /teams/{t}/projects/{id}/issues,progress,updates | Implemented single-team scope; see [Projects](#projects-0022) for exact methods and version rules |
| Milestones | Project milestone CRUD/order and project issue milestone assignment | Implemented, versioned and authorized; [wire and denominator contract](MILESTONES.md#concurrency-and-api). |
| Further planning | /teams/{id}/cycles; /cycles/{id}/close | Target only: cycle close idempotent; roadmap is an authorized project projection |
| Views/search | /views; /saved-searches; /search; /search/suggestions | constrained filter AST, no raw SQL; private saved searches; never disclose inaccessible suggestions |
| Discussion references | /mention-candidates; /issues/{id}/subscription | current authorized readers only; own subscription GET/PATCH with If-Match; no recipient selection on writes |
| Inbox | /notifications; /notifications/{id}; /notifications/mark-read; /notification-preferences | recipient-only mutations; mark-all uses an upper bound cursor, not future arrivals |
| Issue templates | GET/POST /teams/{t}/templates; GET/PATCH /teams/{t}/templates/{id}; GET /teams/{t}/templates/{id}/preview | Team-scoped metadata list and lazy body; preview current references; regular issue create carries the reviewed template ID/version |
| Realtime | /events; /bootstrap | same-origin session or fetch-stream bearer; cursor is opaque and bound to tenant/principal epoch |
| Git | /integrations/github/installations; /repositories; /repositories/{id}/pull-requests; /issues/{id}/pull-requests | permission-granted repositories only; read link does not grant write grant |
| Automation | /automation-rules; /automation-rules/{id}/simulate; /automation-executions | versioned rules; bounded actions; simulation has no external side effects |
| Transfer | /imports; /imports/{id}/preview,commit,cancel; /exports; /transfer-jobs/{id} | preview mapping, resumable job, downloadable report; recheck access at download |
| Audit/admin | /audit; /admin/jobs; /admin/jobs/{id}/retry; /admin/storage; /admin/retention | scoped administrative views; no secrets or private issue bodies in job summaries |
| Developer access | /tokens; /webhook-subscriptions; /webhook-deliveries; /webhook-deliveries/{id}/retry | single-display token/secret; secret rotation retains a short dual-validation window |
| AI | /provider-connections; /provider-connections/{id}/verify,revoke,models; /conversations; /conversations/{id}/messages; /ai-requests/{id}/cancel | per-user grant, capabilities/availability shown; no automatic paid fallback |
| Agents | /agent-grants; /agent-runs; /agent-runs/{id}/cancel,retry; /agent-attempts/{id}/events,artifacts; /approvals | explicit grant and exact artifact approval; cancel is durable intent |
| Runner control | /internal/v1/runners/enroll,heartbeat; /leases/claim,renew; /attempts/{id}/events,artifacts,finish | mTLS/machine audience, workspace+attempt+fence scope, no arbitrary shell API |
| Incoming GitHub hook | /api/v1/hooks/github/{connection_public_id} | raw-body HMAC before parse; bounded durable inbox; ACK only after commit |

## Body and ordering concurrency details

Issue/project metadata ETag is separate from document ETag; a body-only save increments the document and emits document.updated without pretending to change the issue metadata version. Comment edit representation includes both comment and document version in its ETag; PATCH /comments/{id} validates both and changes body/edited_at atomically. Direct document edits resolve owner-specific author/moderator policy, so an issue contributor cannot bypass another author's comment protection through /documents.

The implemented issue DTO still contains a derived `description` plaintext projection and a read-only `body_version`. Its strong ETag binds both, so even a formatting-only document edit invalidates an issue move preview. This does not increment issue metadata `version`. `GET /issues/{id}/document` discovers the independent document. Issue creation accepts an optional `{schema_version,body}` document atomically; a nonempty legacy description and a document cannot be supplied together. Legacy description PATCH is accepted only for a losslessly plain-text-shaped existing body, preventing an older client from flattening formatting.

Document schema 1 accepts the implemented common text formats under a 256 KiB serialized body, 32-level depth and 10,000-node bound. Unknown schemas are rejected on write with `unsupported_document_schema`; reads expose `schema_supported` and plaintext for a read-only fallback. Migration-grandfathered oversized legacy bodies remain readable. ID-only [user mentions](#user-mentions-and-issue-subscriptions-0019), [issue references](#structured-issue-references) and [attachment references](#implemented-attachment-transport-0017) use their implemented authorization and resolution workflows. Reusable team-template bodies separately reject these bound references.

Comments default to 25 items (maximum 100), ordered newest first by creation time and ID; the UI renders them chronologically. `GET /comments/{id}` supports reconciliation after an interrupted mutation. POST requires an idempotency key, PATCH and DELETE require If-Match, and another author's moderation additionally requires a bounded `moderation_reason` (optional JSON body on DELETE). Deleted comments return tombstones without document bodies. `GET /documents/{id}/revisions` returns paginated metadata; `GET /documents/{id}/revisions/{revision}` loads one body. `GET /issues/{id}/activity-page` adds cursor pagination without changing the legacy activity response. Cursors bind caller, issue/document, query and workspace authorization epoch. Historical comments and revisions require both current issue access and access to their recorded team audience.

POST /issues/{id}/position accepts If-Match, optional target status and before_id/after_id. Server checks current neighbors and assigns rank under the team ordering lock; stale neighbor/group state is 409. POST /issues/{id}/restore is versioned and must revalidate current workflow/project references. The API inventory is a resource family contract; implementation OpenAPI enumerates every action and error before release.

## Pagination, filters and search

Return data, next_cursor and has_more; counts are omitted unless requested and cheap. Cursors bind sort, filters, workspace and permission epoch and expire after 24 hours. Keyset ordering always includes immutable ID as tie-breaker. Concurrent changes may move an item between pages; clients deduplicate by ID and invalidate when events affect the current sort. No promise of a permanently frozen listing.

Filter AST version 1: all/any nodes, maximum depth 4 and 50 predicates; fields status,priority,assignee,project,cycle,label,team,created_at,updated_at,due_date; operators eq,in,not_in,before,after,is_null. In/not_in lists max 100. Server maps known names to prepared SQL; no regex, arbitrary JSON paths, SQL fragments or user-provided sort expression. Datetime relative filters resolve with a recorded workspace/team timezone and request time. Saved query values are data, not executable queries.

Search accepts q <=256 characters and optional validated filter; result types issue/project initially. Results include type,id,authorized title,escaped snippet,rank and next cursor. Exact issue key lookup outranks lexical matches. Empty q uses recent accessible items; 150 ms client debounce, abort older requests, server timeout. Fuzzy matching is fallback for short misspelled titles, not an unrestricted edit-distance scan.

## Error envelope and streaming

Use RFC 9457 application/problem+json: type URI under product documentation, title,status,detail safe for user,instance request ID,code stable machine string,field_errors path/code/message where appropriate,retryable boolean. Codes: validation_failed,not_found,forbidden,version_conflict,idempotency_conflict,rate_limited,dependency_unavailable,reconciliation_required,cursor_expired. Never include SQL, token values, raw provider bodies or inaccessible object metadata. 401 requires sign-in, 403 indicates known action disallowed, 404 conceals object existence, 413 is oversized, 422 invalid semantics, 429 includes Retry-After, 503 means retryable service unavailability.

Browser entity events use SSE as specified separately. AI message deltas are separate request-scoped streams and are not durable workspace events per token. Agent logs use attempt sequence/cursor with bounded chunks. End-of-stream alone never means success. Status endpoints are authoritative after disconnect; a completed mutation may survive a failed HTTP response.

## Versioning and contract gates

Publish OpenAPI 3.1 JSON in M01/M03 from maintained Rust annotations or checked schemas; generated TypeScript clients must pass strict type checks. The chosen generator must preserve nullable types, decimal-string counters, ETags, error envelopes, stream endpoint documentation and security schemes. Add examples of every error status and scope rule. Run consumer contract and authorization tests against real PostgreSQL. Incompatible changes need a new major API version, migration notes and at least one supported release overlap. Experimental APIs are explicitly named and advertised in capability discovery.

## Approved expansion API contracts

These resource families are authorized F37–F47 scope and are included in generated OpenAPI before their feature release. All paths inherit /api/v1/workspaces/{workspace_id}, current content authorization, pagination, ETags and idempotency. A route being documented does not mean it has an implementation. [Requirements status](REQUIREMENTS_STATUS.md) tracks executed coverage.

| Surface | Operations | Additional command contract |
| --- | --- | --- |
| Initiatives/outcomes | /initiatives; /initiatives/{id}/projects,reviews; /outcome-measures; /outcome-measures/{id}/observations | Observation requires kind/unit/source/date; corrections reference previous observation. Linked project reads are separately authorized. |
| Roadmap/capacity | /roadmap; /project-dependencies; /capacity-periods; /capacity-allocations | Edge input source_id,target_id,finish_to_start,lag_working_days; capacity requires team/unit/calendar version and known/unknown estimate state. |
| Scenarios | /planning-scenarios; /planning-scenarios/{id}/revisions,compare,apply | Snapshot at most 100 projects. Apply requires scenario revision, selected edit IDs, expected source versions and rationale; all selected edits succeed atomically or none do. |
| Personal work | /me/focus; /me/work-order; /me/drafts; /reminders | Principal is taken from authentication, never a trusted user_id body field. Source revocation removes subsequent readable content. |
| Team routines | /checkins; /checkins/{id}/responses; /team-agendas; /working-agreements; /calendar | Submitted responses are team-visible; drafts author-only. Calendar is a permission-filtered projection with date/timezone metadata. |
| Retros | /retrospectives; /retrospectives/{id}/participants,cards,themes,rounds,transition,summary,publish,follow-up | Phase transitions use expected phase/version; role and retention policy are readable before participation. Draft summaries and raw cards have distinct representations. |
| Retro votes/actions | /retro-rounds/{id}/votes,lock; /retro-themes/{id}/actions | Set vote allocations with expected round/member budget version. Action input includes destination team, exact copied text, owner, review date and success condition. Preview token binds those values and audience; execution revalidates current access. |
| Decisions/knowledge | /decisions; /decisions/{id}/supersede,links; /knowledge-notes; /playbooks; /playbooks/{id}/preview,instantiate | Supersede requires predecessor/successor versions and rationale; cycle rejected. Playbook execution records template revision and per-step outcomes. |
| Feedback | /feedback; /feedback/{id}/disposition,links,merge,contact | Contact routes require a separate team grant and never piggyback on general report response. Merge preserves original reports and source references. |
| Releases/reviews | /releases; /releases/{id}/scope,conditions,notes,transition,publication; /review-requests; /review-requests/{id}/decision | Review approval requires exact target version/digest. Publication is durable external intent; no automatic merge/deploy. |
| Incidents | /incidents; /incidents/{id}/events,issues,transition,review; /incident-events/{id}/correction | Event append and correction are idempotent; original content remains. Response and review closure are separate state transitions. |
| Reports | /reports/definitions; /reports/runs; /reports/runs/{id}/rows,export | Date window at most 366 days initially. Response includes definition version/as_of/window/unit/unknown counts limited to authorized data. Every download reauthorizes. |
| Modules/templates | /module-preferences; /me/navigation; /workflow-templates; /workflow-templates/{id}/preview,instantiate; /template-runs; /integration-health | Maximum 100 steps. Preview lists exact types, titles and destination audiences. Result distinguishes created/skipped/rejected/failed; retry resumes only safe missing steps. |
| Typed fields | /custom-field-definitions; /custom-field-definitions/{id}/migration-preview,archive; /issues/{id}/field-values; /projects/{id}/field-values | Definition version required. Validate type and constraints before atomically mutating owner+values. Filter AST permits known field IDs and type-compatible operators only. |

Preview endpoints are side-effect-free GET or POST simulation commands; POST previews still require session CSRF but do not reserve writes or imply approval. Preview tokens expire, bind requester/input digest/permission epoch and are not authorization. Mutation rechecks current versions and permissions. Forbidden source IDs must not appear in conflict lists, comparison counts or hidden-node placeholders. A scenario with any selected inaccessible target fails generically without leaking which private object caused the denial.

Expansion search types include initiative, decision, note, feedback, release and incident; raw retro cards, private drafts and feedback contacts are excluded from global search. Retro search is explicit session-scoped and applies role/privacy rules. Filter schema versions, module capability discovery and native transfer schema versions must evolve together. Unknown disabled-module commands return a documented unavailable capability, not a fake success.

## Implemented team workflow surface

All paths below are relative to `/api/v1/workspaces/{w}/teams/{t}`. Reads require current team access, including for workspace administrators. Changes require a team maintainer or an authorized workspace manager. Session mutations retain the shared CSRF and origin checks.

| Route | Result and precondition |
| --- | --- |
| `GET /workflow` | Complete live workflow, decimal-string version, `can_manage`, and permission-sensitive ETag; authorized conditional 304 supported. |
| `GET /statuses?state=active\|archived\|all` | UUID-ordered directory, maximum 100 rows; signed cursor binds actor, team, permission epoch, workflow version and 24-hour expiry. |
| `GET /statuses/{id}` | Authorized historical status lookup, including archived rows. |
| `POST /statuses` | 201 with current workflow, ETag and Location; workflow If-Match and Idempotency-Key required. |
| `PATCH /statuses/{id}` | Rename/recolor only; category and identity immutable; workflow If-Match required. |
| `PUT /workflow/default` and `/workflow/order` | Explicit default or complete live ordering within fixed category groups; workflow If-Match required. |
| `POST /statuses/{id}/archive` and `/restore` | Workflow If-Match; archive refuses a default or any referenced status; restore does not move issues back. |
| `GET /statuses/{id}/replacement-preview?replacement_status_id=…` | Current reference count, default impact and workflow version; no ETag because issue counts change independently. |
| `POST /statuses/{id}/replace` | 202 durable replacement with Location/job ETag; workflow If-Match and Idempotency-Key required. |
| `GET /workflow-replacements/{id}` | Current durable state, processed/remaining counts and safe error code; authorized conditional 304 supported. |
| `POST /workflow-replacements/{id}/resume` | Current manager and job If-Match required; only blocked/dead-letter jobs resume after revalidation. |

Workflow ETags include team UUID, workflow version and current `can_manage`; a role change cannot preserve stale management controls through 304. Job ETags include job UUID/version. A stale mutation returns 412 with the current authorized representation, missing preconditions return 428, and duplicate creation/replacement keys replay only the original matching intent. Reads and 304 responses use `Cache-Control: no-store`.

A retiring status remains readable on existing issues but cannot be newly assigned. Unrelated edits may retain it. The replacement job includes archived and soft-deleted issue references; the source is archived only after every reference has moved. Job states are `queued`, `running`, `retry_wait`, `blocked`, `dead_letter`, and `succeeded`. Counts are decimal strings. The source retains its replacement job pointer so progress survives reload and worker restarts. Generated OpenAPI and TypeScript are the exact executable contract.

## Label and assignee surface

Relative to `/api/v1/workspaces/{w}`, labels use `GET/POST /labels` and `GET/PATCH /labels/{label}`. Creation requires an idempotency key and returns 201, Location and ETag. PATCH accepts name, color and archived state under If-Match. Label scope is immutable. Shared labels require workspace owner/admin management; team labels allow current team maintainers or workspace managers with team access. Private-team content stays concealed from administrators outside that team. Label ETags include UUID, version and current `can_manage`.

Label directories accept scope `visible` (default), `workspace`, `team`, or `applicable`. The last two require `team_id`; the first two reject it. `applicable` includes shared labels and the chosen team's labels. State is `active` (default), `archived` or `all`; literal name search is at most 100 characters. Pages default to 20 and cap at 100. Names are trimmed, 1–100 characters, and colors use approved tokens. Archive preserves links; restoration may atomically rename/recolor to resolve a live name conflict.

`GET /teams/{t}/assignees` searches display names and returns at most 20 eligible people per page. `self_assignable` is computed independently of the current search/page. Candidates must be enabled, active nonviewer workspace members and, for private teams, explicit team members. Archived parents return an empty directory. Responses expose ID and display name without email or an ineligibility reason. Each issue returns its own historical `assignee` summary with current `assignable`, independently of bootstrap or picker limits; nonnull `assignee_id` always has a summary.

Issue create/PATCH accepts a complete `label_ids` set of at most 50 IDs, deduplicated before application. Attached archived labels may remain or be removed; they cannot be newly added. Issue responses include sorted `label_ids`, matching historical `labels`, `assignee` and a required `etag` equal to the HTTP validator. Its digest covers the reference metadata, so a label rename/archive or eligibility change can produce 412 without incrementing the issue's own version. Conflict `details.current` contains the same complete representation. An archived issue can restore with `archived:false` plus status, assignee and label repairs in one command; unrelated fields require a subsequent edit.

Issue lists accept CSV `label_ids`, `label_mode=any|all` and `sort=created|priority`. Every requested label is validated for current visibility; invalid or hidden filters never broaden the result. Archived labels remain valid filters. Priority sorts 1,2,3,4,0, then newest timestamp and UUID descending. Issue, label and assignee cursors authenticate the principal, workspace, relevant team, normalized query, limit and permission epoch and expire after 24 hours. Older issue cursor formats fail safely. Changing the setup-token-derived signing key invalidates existing cursors.

Pre-upgrade create-issue retries preserve their original outcome/version, with missing additive summaries hydrated from the recorded authorized references. Empty label sets do not change the historical create-request hash. Assignment notifications and access-loss cleanup remain separate unfinished work; directory eligibility and access denial do not wait for cleanup.

## Issue hierarchy, relationships and team moves

The graph surface is relative to `/api/v1/workspaces/{w}/issues`. `GET /lookup` accepts a required team, literal key/title query and optional issue exclusion, with at most 20 candidates per page. `GET /by-key/{key}` resolves a current or historical key only after authorizing the issue's current team. It returns the canonical issue without an HTTP redirect. A hidden, deleted, foreign or unknown key returns the same not-found response.

| Route suffix | Contract |
| --- | --- |
| `/{id}/parent` | GET authorized parent projection; PATCH `{parent_id, parent_etag?}`. `parent_id` is required and nullable. Additions require the selected parent's ETag; detachment omits it. |
| `/{id}/children` | GET a bounded page of visible children. |
| `/{id}/relations` | GET a bounded relative-direction page; POST `{kind,target_id,target_etag}` with an idempotency key. Creation returns 201, the updated source issue, relationship and dereferenceable Location. |
| `/{id}/relations/{relation}` | GET one authorized relationship; DELETE explicitly removes it. |
| `/{id}/move-destinations` | GET currently eligible destination teams. |
| `/{id}/move-preview?destination_team_id=…` | GET current blockers, destination workflow/default, audience change and label/assignee repair information with a ten-minute signed preview token. |
| `/{id}/move` | POST explicit destination, status, nullable assignee, full label set and preview token. Requires an idempotency key; returns the same issue UUID with its new key. |

Graph mutations require the source **issue** ETag in `If-Match`. Addition also checks the selected target's issue ETag. Missing source preconditions return 428; stale source returns 412 with `details.current`, while a stale target returns 412 with `details.current.target`. Parent, children and relationship GET validators hash their complete authorized projections. These graph validators are for conditional reads and must not be sent as issue mutation preconditions. Authorization runs before a possible 304. Graph pages default to 20 and cap at 50; their cursors bind the query, caller, access and graph generation and expire after 24 hours.

Hierarchy is same-team, at most eight levels including the root, and cannot contain a cycle. Relations support `blocks`, `blocked_by`, normalized symmetric `related`, and canonical `duplicate_of`; reads may additionally return `has_duplicate`. Links do not change issue status. An active writable source can detach an unavailable historical endpoint without receiving its hidden identity or title; its representation uses `target:null` and an explicit removal capability.

Moves initially accept one active, childless issue without any incoming or outgoing graph link. Archived and deleted historical links still block the move and require explicit removal. The preview is informational, not an authorization grant: the command rechecks current access, issue/reference versions, destination workflow and both graph generations. It validates all repairs, preserves allowable shared labels, removes incompatible source-team labels, allocates a new destination number and records the old key atomically. Historical create/relation/move receipts require both recorded-team access and current-issue access before replay. Activity additionally filters each record through its original team, so moving an issue does not disclose previous private history.

Creating a child can include the optional paired `parent_id` and `parent_etag` fields in issue creation. Omitted parent fields preserve old create request hashes. Child creation, parent/child version changes and correlated events commit together. Public soft-delete/restore and bulk/subtree transfer remain separate unfinished capabilities.

## Native client session handoff

ADR-017 adds a separate desktop session audience to the shared API. A request supplies exactly one OpenTrack credential: its browser session cookie or an `Authorization: Bearer` desktop token. Duplicate session cookies, duplicate Authorization headers and mixed credentials are rejected. Authentication verifies the stored audience before dispatch. Browser writes retain allowed-Origin and CSRF checks; authenticated desktop writes use the native audience instead. Setup, password sign-in, recovery, invitation preview/acceptance and native consent remain browser-only operations.

`GET /api/v1/client-capabilities` advertises the supported native protocol. `POST /api/v1/auth/native/requests` creates a ten-minute request from an S256 challenge, canonical loopback callback, random state and bounded device name. It is anonymous for initial sign-in or authenticated with the existing desktop session for bound reauthentication. It rejects browser cookies. The 201 receipt returns a public request UUID, relative browser approval path, matching confirmation code and expiry, without a native-readable consent Location or session credential.

The browser reads `GET /api/v1/auth/native/requests/{id}` and approves through `POST /api/v1/auth/native/requests/{id}/approve`, with its session, recent authentication, allowed Origin, CSRF token and matching confirmation code. The response contains the exact loopback callback with a one-use code, state and request UUID. `POST /api/v1/auth/native/exchange` accepts only that code and its PKCE proof, with no cookie or Authorization header. It returns the opaque desktop bearer once; replay, expiry, account mismatch and invalidated source sessions return the same `native_auth_invalid` 422 response. The backend stores code and session hashes, never their cleartext values. There is no idempotent replay of secret-bearing approval/exchange responses.

All native-auth responses, including rejection paths, and the browser approval document carry `Cache-Control: no-store` and `Referrer-Policy: no-referrer`. The native client accepts only a literal loopback callback and exchanges against the explicitly selected server, with verified TLS and no redirects. Credentials remain in the OS credential store and are never returned through the JavaScript bridge. A lost exchange reply may have created a session; the client reports uncertainty and the account session directory permits review and revocation.

Session responses carry `audience`; session-directory entries add required-nullable `device_name`. Desktop session CSRF values are empty. Revoking the current session returns `X-OpenTrack-Session-Invalidated: true`; a native client clears its stored credential, while a web response also expires the browser cookie. Other-session revocation does not invalidate the caller. Bound native reauthentication preserves the approving browser's authentication time and limits the new absolute lifetime to its remaining expiry or seven days, whichever comes first.

## Issue deletion and Trash

All routes remain workspace-scoped under `/api/v1/workspaces/{w}`. `GET /issues/{id}/deletion` returns the current issue, `can_delete`, `has_parent`, direct `child_count` and incident `relationship_count`, with the issue's current ETag. `DELETE /issues/{id}` requires that ETag in `If-Match` and returns 204. It tombstones the issue, detaches its parent and every direct child, and removes incident relationships atomically. Children remain independent issues. Archive is a separate operation that preserves connections.

`GET /issues/deleted` returns a page of `DeletedIssue` records, each containing the existing issue representation, `deleted_at`, `restore_until` and `can_restore`. Optional filters are `team_id` and a literal title/current-key query `q` of at most 256 characters. Pages default to 50 and cap at 100, ordered by `(deleted_at,id)` descending. Signed cursors bind actor, workspace, access epoch and normalized query. `GET /issues/{id}/deleted` returns one currently authorized tombstone with its issue ETag. Ordinary issue reads, key resolution, lists and lookup do not return deleted issues.

`POST /issues/{id}/restore` requires `If-Match` plus all three body fields: `status_id`, nullable `assignee_id`, and `label_ids`. It restores the original identity/key with current valid references and returns the canonical issue and ETag. Previously attached archived labels may be retained; unavailable assignees/statuses require explicit repair. Restore never recreates old graph links. The final database mutation checks the 30-day deadline after lock waits. Expired rows stay readable in Trash with `can_restore=false`; these routes neither purge nor promise a purge schedule.

Current authorized readers, including viewers, may read Trash. Delete/restore additionally require a non-viewer workspace role and authorship, current team-maintainer authority or workspace owner/admin authority. Private-team access remains necessary even for administrators. Archived issues, teams and workspaces are read-only. Missing preconditions return 428, stale representations return 412, unavailable private objects return 404, and expiry or the 10,000-connection deletion bound returns 409. Larger graph detachments must be reduced with ordinary versioned graph commands before deletion. A lost mutation response requires reading the tombstone/live issue before another write; there is no secret retry or automatic overwrite.

## Implemented attachment transport (0017)

Reservation requires `Idempotency-Key`, original filename, exact byte length, lowercase SHA-256, and exactly one `document_id` or draft `team_id`. The 201 response includes ETag and a Location pointing to the metadata resource. The returned public ID and ETag are unrelated to the opaque filesystem key. `document_id` remains null until a ready document reference commits; `expires_at` remains set until then. Draft metadata is visible only to its uploader with current parent/team access.

PUT `attachments/{id}/content` requires `If-Match` and `Content-Length`; the immutable reservation supplies the checksum. Raw bytes stream with backpressure separately from the 1 MiB JSON extractor limit. A matching ready receipt can reconcile an ambiguous PUT under current read authority without another mutation or a fresh write precondition; ready finalize replay has the same read-only behavior. The server returns 200 for ready/rejected state and 202 for quarantine awaiting a working scanner. POST `attachments/{id}/finalize` retries scanning without reupload. Configured file bounds appear in `/capabilities`; no full file is buffered by the server. Eight admitted transfers bound streaming work. Download producers have a 180-second deadline independent of response polling, retain at most three 64 KiB chunks, and release file locks/admission on disconnect or timeout.

GET metadata and GET content independently reauthorize the current document, including a comment's historical audience. Content is served with `application/octet-stream`, attachment disposition, `no-store`, `nosniff`, and a sandbox CSP. Missing/corrupt ready bytes return a retryable service error and an integrity diagnostic, not a cached missing-resource response. DELETE requires the current ETag; comment moderation follows its reason requirement. Public responses never include blob keys, lease secrets or filesystem paths.


## Implemented team issue templates (0018)

`GET /workspaces/{w}/teams/{t}/templates` returns metadata only, sorted by case-folded name and ID. `archived` selects active (default) or archived definitions; `q` is bounded to 100 characters, `limit` is 1–100 (default 25), and the signed cursor binds the actor, team, query and authorization epoch. Detail is fetched separately. Template detail and mutations carry a permission-aware ETag; PATCH requires `If-Match`, while POST requires `Idempotency-Key`.

A definition contains a name, title default, priority, label IDs, optional status ID and supported rich document. Migration 0020 adds `title_mode: "literal" | "pattern"`; omitted mode on create means literal and existing brace text stays literal. Pattern mode accepts only `{{team.name}}`, `{{team.key}}` and `{{date}}` (UTC `YYYY-MM-DD`), with exact spelling and paired double braces. There are no expressions, recursive expansion or arbitrary context lookups. Source and resolved titles are bounded to 256 Unicode characters; output is never silently truncated. Unknown/malformed placeholders fail create/update with 422. PATCH mode-only changes validate the existing title in the new mode.

A null status explicitly follows the team's current default. Preview resolves current references and returns `materialized_title` (string, or null on failure), `title_context_at` (UTC timestamp), and per-field diagnostics. A title failure uses `field: "title"`, a null reference ID and `malformed_pattern`, `unknown_placeholder`, `title_too_long` or `invalid_title`; `can_create` is false for that preview; the user may repair the explicit issue draft. Other diagnostics report unavailable, archived or retiring defaults individually without exposing inaccessible names. Preview always returns a current 200 response: team names, calendar dates, statuses and labels can change independently of the template version, so the template ETag is not a validator for preview. List/detail are readable by current team readers. Management requires team-maintainer or workspace-administrator authority plus ordinary private-team membership; use requires current write access. Archived teams and workspaces are read-only.

PATCH replaces each supplied defaults/document object in full; omission preserves it and explicit null is invalid. Archive/restore uses the same PATCH route. An archived definition must be restored before editing. Names are unique within the team after case and whitespace normalization, including archived definitions. The 100-definition limit also includes archived entries. Template documents currently support the existing text, formatting and checklist schema; attachment references, unsupported nodes and moderation reasons are rejected.

Issue creation uses the existing POST `/workspaces/{w}/issues` with `template: {id, version}`, an explicit resolved `status_id`, and an independent document body. The version is a canonical positive decimal string. Clients may edit or clear any proposed default before submission; the normal issue validation remains authoritative. Under the team's write lock, a first create checks the current definition version, active state and document support before atomically recording the issue and its source version/digest. A changed definition returns 412 with `details.current`; an archived current definition returns 409 and an inaccessible one returns 404. A committed issue-create retry is resolved before these mutable template checks and returns the original issue under current access rules, even after the template changes or is archived. Omitting `template` preserves the original create request hash.

The shared create dialog previews before applying. It copies the server-resolved title into an editable draft; create persists that explicit title without re-evaluating dates or team names, including after UTC midnight. A title diagnostic blocks creation until the user supplies a valid replacement. Switching after edits requires an explicit merge, replace or cancel choice; applying never discards ready attachments. Stale defaults remain visible and block creation until repaired. A changed version requires review; accepting a newer version does not silently overwrite the edited draft. Uncertain create outcomes retain the identical request and idempotency key. Omitted or explicit literal mode preserves the pre-0020 create request hash, and stored legacy receipt bodies deserialize as literal.

This increment does not implement project defaults, executable title expressions, template attachment/reference cloning, mentions, child trees, playbooks or cross-feature template runs. Those requirements remain in their feature and milestone ledgers.

## User mentions and issue subscriptions (0019)

| Route under `/workspaces/{w}` | Contract |
| --- | --- |
| GET `/mention-candidates` | Exactly one of `team_id`, `issue_id`, `document_id`; literal name search `q` at most100 characters; limit1–50, default20; opaque cursor bound to principal, scope, query and authority epoch. Returns `{data:[{id,display_name}],has_more,next_cursor}`. |
| GET `/issues/{id}/subscription` | Current caller's preference, effective subscribed state, automatic sources, version, ETag and can_change. Does not list other subscribers. |
| PATCH `/issues/{id}/subscription` | If-Match and `{preference:"automatic"|"subscribed"|"unsubscribed"}`. Only the caller's preference is mutable; no recipient/body/source arguments. Stale412 requires review. |

An inline user mention is `{type:"userMention",attrs:{id:"UUID"}}`, with no supplied label, text, children or marks. A document accepts at most50 distinct user IDs; repeated occurrences count as one recipient. Document and revision reads add `mentions:[{id,display_name,available}]`, resolved under current source and recipient authority. A hidden or disabled recipient has `display_name:null` and `available:false`; persisted plain text uses `@someone`. Projection changes participate in the strong document validator. Existing unavailable IDs may be retained or removed while editing; newly added IDs must identify enabled current readers. Comment candidates also respect the comment's immutable origin team. Revision display additionally respects the revision's origin. Viewer role permits being mentioned and changing one's own subscription, but never authoring content.

New issues follow their creator by default. Automatic creator, assignment and mention sources persist after later source changes; `unsubscribed` continues to override them until explicitly changed. Explicit direct mentions remain separate from following preferences. An absent subscription row reads as automatic, unsubscribed, version`"0"`, with an actor-bound ETag; GET does not create a row. Personal preferences remain changeable for readable archived issues. Source and preference commands serialize under the same team/issue locks, so a concurrent automatic source update can cause a412 rather than silently replacing a preference.

No caller can create an arbitrary recipient notification. New mention references and new comments append private reference-only source records atomically with their ordinary mutation; repeated request receipts and retained mention IDs create no duplicate source. Subscription preference changes append private events and required restricted audit, without exposing follow/unfollow through general issue activity. These records are future delivery input, not proof that an inbox item or email was sent. M07 delivery, preferences and inbox processing remain separately required.

## Structured issue references

GET `/api/v1/workspaces/{w}/issue-reference-candidates` requires exactly one `team_id`, `issue_id` or `document_id` identifying the readable source. It searches currently readable, live, nonarchived issues across the workspace by literal title/current key or an exact historical key. `q` is at most 100 characters; `limit` is 1–20, default 20. The response is `{data:[{id,key,title}],has_more,next_cursor}`. Signed cursors bind the actor, workspace, source's current and historical team, normalized query and authorization epoch. A private source remains inaccessible even if candidate targets would be public.

The inline leaf is `{type:"issueReference",attrs:{id:"canonical-uuid"}}`. No key, title, text, marks or children may be stored on the node; a body supports at most 50 distinct issue IDs. Issue creation, document saves and comment creation/editing validate newly inserted targets against the author's current access and the target's live, nonarchived state. Existing IDs may be retained or removed after access loss, archive, move or deletion. A removed ID requires current eligibility when inserted again. References may cross teams within one workspace; they do not create parent/relation graph edges or notifications. Team templates continue to reject dynamic references.

Document and revision responses require `issue_references:[{id,available,key,title}]`, sorted by ID. Keys and titles are resolved for the current reader on every read; missing, deleted, foreign-workspace and inaccessible issues all return `available:false,key:null,title:null`. Archived readable references retain their label. Persisted plain text uses `[issue]`, so issue descriptions, revisions and receipts do not retain private target labels. Target renames, moves and permission changes affect document/comment strong ETags without incrementing source versions. Revision GET responses also carry an ETag over the complete authorized representation. Comment-create retries resolve the current document rather than returning previously stored reference labels.

## Activity facts and attribution

Both issue activity routes return `actor_id` for account events and null for system events, alongside the display actor and timestamp. Readers must authorize the current issue and each entry's recorded team. The paged route retains signed cursors bound to workspace, issue, reader, authorization epoch and page size; the default is 25 individual events.

New issue-update metadata includes actual changed `fields`, `source`, and optional `{before,after}` facts for `status_id`, captured `status_names`, `priority`, `estimate`, `due_date`, `archived`, `assignee_id` and `label_ids`. Nullable values are explicit on both sides. Old rows may omit facts and actor identity; clients must not invent missing values. `source=workflow_replacement` and `source=access_cleanup` remain distinct from manual actions.

The activity projection omits title/body values and unknown metadata, including on older durable rows. A title or description change exposes only its field name. Grouping is a client rendering choice and never changes event identity, pagination or durable records. See [F11](features/F11_COMMENTS_ACTIVITY.md) for the bounded grouping contract and [activity evidence](evidence/2026-10-20-activity.md) for verification.

## Personal shortcut preferences (0021)

`GET /api/v1/workspaces/{w}/preferences` returns the current caller's `{workspace_id,shortcut_mode,version,etag}`. `shortcut_mode` is `enabled` or `disabled`; an absent row is an enabled default with version `"0"`. GET never creates a row. The strong ETag binds the workspace, caller and version, and the response is `no-store`.

`PATCH` on the same route requires `If-Match` and exactly `{shortcut_mode:"enabled"|"disabled"}`. User IDs, alternate workspace IDs, null values and unknown fields are rejected. Current members, including viewers and members of archived workspaces, may change this personal accessibility setting. It does not authorize content writes. Missing membership is concealed as 404; missing preconditions return 428; a stale version returns 412 with the caller's current setting in `details.current`. The frontend preserves the proposed choice and requires explicit review before another update. After an uncertain response it reads the saved setting before another write.

Disabling character shortcuts pauses C, /, ?, and navigation sequences. Mod+K, Escape, form submission shortcuts, palette actions and visible controls remain available. The only currently available G sequence is G then M for My work; it expires after 800 ms. Inbox is not advertised. The same route is admitted by the restricted desktop transport.

## Projects (0022)

All implemented routes below use `/api/v1/workspaces/{w}/teams/{t}/projects`. Current workspace membership and owning-team access are rechecked on every request, including idempotent replay. Inaccessible private teams/projects return 404. Viewers can read; active contributors can edit. Archive and restore additionally require current team management authority. Archived workspaces or teams are read-only.

| Method and suffix | Contract |
| --- | --- |
| GET base path | Search by `q`, choose `archived` (false by default), and page with `limit`/`cursor`. Returns metadata summaries without narrative bodies. Default page is 25, maximum 100. |
| POST base path | `Idempotency-Key`; name, optional summary, narrative, lead and dates. Returns 201 with canonical project, Location and ETag. Initial state is `planned`, health `on_track`. |
| GET `/{id}` | Canonical project with narrative, current lead projection, permissions and ETag. |
| PATCH `/{id}` | `If-Match`; supplied metadata fields, `archived`, or both where allowed. Missing fields are preserved; explicit null clears lead/start/target dates. Returns the saved project and ETag. |
| GET `/{id}/progress` | Authoritative `completed`, `eligible`, `canceled`, `open`, `total`, `unscoped` and `as_of`. Counts are separate from the metadata validator. |
| GET `/{id}/issues` | Current non-deleted, non-archived linked issues; bounded `limit`/`cursor` pages, maximum 100. |
| PUT `/{id}/issues/{issue}` | Project `If-Match` and `{included: boolean}`. Changes membership without changing issue metadata or state. Returns the canonical project. |
| GET `/{id}/updates` | Newest-first authored snapshots; `limit`/`cursor`, maximum 25. |
| POST `/{id}/updates` | `Idempotency-Key` and `{body: string}` of 1–8000 trimmed characters. Snapshots current state, health and completed/eligible counts; returns 201 with Location and ETag. |
| GET `/{id}/updates/{update}` | Canonical immutable update with current author-name projection and matching ETag, for reads and uncertain-response reconciliation. |

Names are 1–160 characters and summaries at most 500. Start and target are optional local dates; target cannot precede start. State is `planned`, `active`, `paused`, `completed` or `canceled`; manual health is `on_track`, `at_risk` or `off_track`. A newly assigned lead must be an active contributor who can access the owning team. An omitted lead preserves a historical assignment, including one that is no longer eligible.

Use the returned project ETag unchanged. It includes the project version and current representation, including lead presentation and permissions. Missing `If-Match` returns 428; a stale value returns 412 with the authorized current representation. The UI retains the draft for explicit review rather than overwriting a newer save. Completing a project while eligible open issues remain requires `acknowledge_open_issues: true`; it never changes their states. Restore is a separate `{archived: false}` edit before changing other fields on an archived project.

One issue belongs to at most one project and must share its workspace and team. A linked issue must be detached before the current team-move command can move it. Archived and deleted issues are excluded from progress; canceled issues are counted separately and excluded from `eligible`. `open = eligible - completed`; zero eligible scope sets `unscoped: true`, not 100% completion. Update counts remain the recorded snapshot when later issue states change.

Project narratives use the validated template editor subset of document schema 1, with its existing bounds and derived plain text. They currently share project metadata versioning and do not expose the independent document/revision API. Attachments, user mentions, issue references and moderation reasons are rejected. Update bodies are plain text. Milestones, cycles, boards, timelines and independent project-document history remain separate work. Execution results and outstanding qualification are recorded in the [Projects evidence](evidence/2026-10-20-projects.md).


## Implemented team-cycle commands (schema 0024)

Base path: `/api/v1/workspaces/{w}/teams/{t}/cycles`. GET lists a cursor-paginated directory; POST creates an explicitly zoned local-date interval with an idempotency key. `/{id}` supports GET, versioned PATCH and DELETE of unused planned cycles only. `/{id}/issues/{issue}` PUT requires the cycle If-Match and an issue ETag in `{included,issue_etag}`. One cycle membership per issue is enforced independently of project membership.

`/{id}/open` and `/{id}/reopen` POST require If-Match; reopen also requires `{reason}`. `/{id}/scope` GET returns current metric definition/counts and the issue-version scope table. `/{id}/snapshots` GET returns immutable paginated history; POST explicitly captures/deduplicates the daily local-date snapshot and requires If-Match.

`/{id}/close-preview` GET returns the current cycle, split scope and review token. `/{id}/close` POST accepts `{review_token}` with If-Match and Idempotency-Key and returns an immutable close report. Scope-version changes require fresh review (409); stale cycle versions return 412. Closure never changes issue workflow or membership. `/{id}/reports/{report}` retrieves saved reports; cycle detail supplies `latest_report_id`.

`/{id}/carry-forward` POST requires source If-Match, Idempotency-Key, report/destination IDs, destination ETag and 1–100 issue ID/version pairs. It returns a durable operation with per-item outcomes. `/{id}/carry-forward/{operation}` GET reconciles the result. POST to its `/resume` child requires the operation If-Match, a fresh Idempotency-Key, current destination ETag and explicitly reviewed unresolved issue versions. Same-key replay returns the stored result. [Cycle semantics and recovery](CYCLES.md) define scope exclusions, bounds, lifecycle, DST and resumable conflicts. Rust-generated OpenAPI contains the executable wire schema.

Historical snapshot, close-report and carry-operation reads (including idempotent receipt replay) reauthorize every issue against current issue/team access as well as the recorded cycle team. Inaccessible issue IDs and facts are omitted, displayed counts/estimates are recomputed from visible source facts, and no hidden-item count is returned. The projection indicates `redacted` when incomplete; immutable database facts remain unchanged for authorized recovery.
