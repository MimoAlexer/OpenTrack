# Security architecture and threat model

Scope: core web/API/database/blob deployment and later providers/runners. [Agent isolation](ai/AGENT_SECURITY.md), [auth research](ai/AUTHENTICATION_RESEARCH.md), [database RLS](DATABASE_DESIGN.md). These are requirements and test plans, not security certification.

## Assets, actors and trust boundaries

Protect issue/project content, private-team membership, credentials, attachment bytes, repository access, encryption keys, execution evidence, backups and administrator capabilities. Threat actors include anonymous internet clients, malicious invited members, compromised sessions/integrations, hostile repository contributors, prompt-injected model output, malicious packages, and an attacker on a runner host. A server root/DB owner is trusted for operation and can access plaintext during use; self-hosting does not make that actor cryptographically unable to read data.

Boundaries: browser↔TLS proxy; proxy↔API; API/worker↔PostgreSQL and blob store; trusted worker↔provider network; controller↔runner; runner supervisor↔untrusted execution; operator↔backup/key storage. Never conflate the logged-in OpenTrack user with a provider identity, Git installation, repository principal, or machine runner identity.

## Permission model

Active workspace membership is necessary for all content. Roles owner/admin/member/viewer are workspace-scoped. Workspace-visible teams are readable by all active members; member/admin/owner may contribute. Private teams require explicit team membership for any content operation, regardless of workspace role. Viewer never writes team content even if added to a team; recipient-only preferences such as following an issue remain personal state. Team maintainers manage team workflow and membership only within authority; they cannot create workspace administrators. Issue-level custom ACL is deliberately excluded from MVP.

| Action | Viewer | Member | Admin / owner | Extra condition |
| --- | --- | --- | --- | --- |
| Read issues/projects/search | Yes | Yes | Yes | Current team access, including private membership |
| Choose mention candidates / manage own issue subscription | Yes | Yes | Yes | Current source read access; candidates are enabled current readers; no other person's subscription can be changed |
| Create/edit issue, comment, attachment | No | Yes | Yes | Accessible writable team; authorship rules for comment edits |
| Archive/unarchive an issue | No | Yes | Yes | Current writable team; unarchive is a versioned action |
| Delete/restore an issue | No | Its creator or team maintainer | Yes | Current team access; restoration within retention window |
| Moderate another person's comment | No | No | Yes, or team maintainer | Visible team; audit moderation reason |
| Change team workflow/template | No | Team maintainer only | Yes | Private-team access still required |
| Invite workspace user / change role | No | No | Yes | Admin cannot grant/alter owner; last owner cannot leave |
| Make team private / add private member | No | Team maintainer | Yes | Existing team access; membership/epoch update |
| Export own accessible work | No bulk export by default | Yes | Yes | Whole-workspace export additionally requires access to every included team; otherwise filtered export |
| Manage installation retention/backups | No | No | Owner via operator capability | No implicit private content browsing; backups protected as full-content artifacts |
| Connect a personal provider | No, unless later explicit inference grant | Yes | Yes | Secrets owned by user; subscription connections never pooled |
| Request agent run | No | Yes | Yes | Explicit repository/runner/provider grant and budget |
| Approve PR publication | No | Granted reviewer | Granted reviewer | May require separation from requester; exact artifact scope |
| Merge / deploy from agent | No | No | No | Outside initial agent product; use external review process |
| Read audit | No | No | Yes | Security metadata only; no hidden issue bodies |

Instance operators can perform emergency recovery via a local authenticated CLI process. A break-glass action is explicit, time-limited (15 minutes), reasoned, recorded and visible in security audit. It creates a scoped temporary read grant rather than quietly bypassing request policy. Root-level filesystem/database access cannot be prevented by the app; documentation must state this residual risk.

For mutation/revocation races acquire user authorization guards before the workspace authorization guard, following ARCHITECTURE.md. Targeted access changes lock the actor shared and the target exclusive in sorted UUID order; a self-change takes one exclusive guard directly. Never upgrade a shared guard or acquire another account guard after a workspace guard. Account revocation uses the user guard exclusively; workspace permission changes use the workspace guard exclusively. Ordinary commands hold both shared guards while rechecking their session and workspace authority. Reads authorize within their database snapshot. Once revocation commits, new commands and new stream emissions must not use old authority; already delivered/in-flight authorized bytes cannot be recalled. Pending webhooks, email, exports and AI context retrieval recheck authority at dispatch. Active agent grants are revoked when their issuing user loses required access; broker denies new operations and runner cancellation is requested.

## Authentication and credential lifecycle

Local accounts are mandatory; OIDC is optional. Bootstrap owner creation requires a single-use local setup secret, disabled permanently after first owner creation except audited recovery. Closed registration by default. Email delivery is optional: admins can hand an expiring invite link to a user; no mail service is necessary for core setup. Without configured mail, password recovery requires local operator recovery, not a secret fallback URL. Login failures never reveal whether an email exists. Invitation validity is checked using current database time again at the consuming write, after any guard waits and grant validation. Failure rolls back provisional accounts, memberships and sessions. Expired invitation replacement records restricted retirement audit; it never publishes inaccessible old invitation IDs or private grants.

Passwords use Argon2id with a reviewed library; initial target parameters 64 MiB memory, 3 iterations, parallelism 1, then calibrate within the hashing budget. Unique salt per password. Password length 12–128 characters; permit paste and password managers, no arbitrary composition rules. Hash operations are bounded to two concurrent tasks initially and independently rate limited. Rehash on successful login when parameters change. Optional TOTP/passkey work is M14; do not imply MVP MFA.

Session tokens contain at least 256 bits from OS randomness, stored as hashes server-side. Replace or invalidate prior tokens on login, privilege elevation and recovery; privilege elevation and recovery must not leave older sessions able to use the new authority. Idle lifetime 12 hours, absolute lifetime 7 days; sensitive operations require reauthentication within 10 minutes. Sign-out invalidates the server row and clears local caches/drafts. Recovery/invite tokens are single-use hashes with expiry and replay protection. OIDC checks state, nonce, PKCE, issuer, signature/JWKS, audience, expiry and exact callback. Account linking requires reauthentication of the existing account; matching email is insufficient.

Targeted access expansion invalidates the affected account’s existing sessions atomically with the change, epoch and audit. This covers active workspace role increases (`viewer < member < admin < owner`), inactive-to-active membership, new private-team membership, and new maintainer rights. Unchanged access, reductions, and adding a public-team contributor row do not require global sign-out. Creating one’s own workspace/team and workspace/team archival or privacy publication use their existing permission/epoch rules; they are not targeted principal grants. A disabled account cannot receive these member updates. The administrator’s own session remains valid when updating another account. A self-elevation commits successfully and signs the caller out. Existing-account invitation acceptance preserves only its newly rotated cookie, carrying forward the original authentication time and absolute expiry; all prior sessions are invalidated. No new password check is inferred from accepting an invitation.

Global account disable/enable is a separate installation operator capability, never workspace-owner HTTP authority. The operator role has only the four reviewed recovery/lifecycle functions; runtime startup rejects direct or role-reachable execution rights on any of them. Disabling preserves another enabled owner in every affected workspace (including archived) and organization, revokes sessions/recovery codes/outgoing pending invitations, and commits required audit and epochs atomically. Enabling also invalidates leftover credentials from legacy disabled accounts before restoring authentication; old credentials are never revived. The target account guard precedes sorted organization and workspace guards, and no later co-owner account lock is taken. Runtime DML cannot change disabled state or impersonate operator audit. Authorship and memberships are preserved; subsequent assignment/job cleanup remains tracked with its owning feature.

Secrets use AEAD envelopes with associated data binding workspace, owner, provider and credential ID. Key IDs support rotation: decrypt old and new during bounded re-encryption, encrypt only with the new key, verify restored records before retiring the old key. Master keys live in read-only secret mounts outside DB and image, with separate backups. API returns redacted status and last-four display hints only when meaningful; secrets are never returned after initial entry. Prefer file secret references; environment overrides are allowed but operators are warned that their process environment is sensitive.

## Threat/mitigation/test register

| ID | Attack or failure | Required control | Verification / residual risk |
| --- | --- | --- | --- |
| S01 | Credential stuffing / password DoS | Per-IP and per-account throttles, bounded hash workers, generic failure | Load login paths without exhausting issue traffic; reverse-proxy IP trust list |
| S02 | Session fixation/theft | Rotate opaque session, Secure/HttpOnly, expiry, server revocation | Old tokens fail after recovery; XSS can still act as user until contained |
| S03 | CSRF | SameSite plus Origin and synchronizer-token checks on every unsafe method | Cross-site form/fetch and null Origin tests; callbacks exempt only with validated OAuth state |
| S04 | IDOR / tenant confusion | Scoped routes, composite FKs, workspace RLS, centralized team predicates | Cross-workspace IDs in all endpoints, joins, exports, counts and jobs fail closed |
| S05 | Stale permission race | Authorization guard, current membership at dispatch, epoch reset | Concurrent revoke/write test and two-node SSE revocation |
| S06 | Stored/reflected XSS | Allowlisted editor schema, safe text rendering, no arbitrary HTML, strict CSP | Malicious pasted HTML, links, SVG, Markdown, filenames and provider text fixtures |
| S07 | SQL/command injection | Prepared SQL and allowlisted AST; argument vectors for Git; no shell concatenation | Filter fuzzing, branch names beginning with options, paths with metacharacters |
| S08 | SSRF through webhooks/providers/import | URL policy, DNS+IP checks at connect time, no redirects by default, egress proxy, block metadata/link-local/private ranges | DNS rebinding, IPv6/mapped IPv4, redirect-to-private and scheme smuggling tests |
| S09 | Legitimate local inference conflicts with SSRF policy | Admin-defined exact host/port allowlist for local inference, separate from user-supplied webhooks | A private IP exception grants only the configured inference route, never arbitrary LAN scanning |
| S10 | OAuth state mix-up/token substitution | PKCE, nonce, issuer/audience validation, registration identity tuple | Tampered callback, swapped client IDs/accounts, reused code rejected |
| S11 | Token leakage/refresh race | Envelope encryption, single refresh owner/fencing, redact auth URLs/headers | Concurrent rotation and lost-response tests; invalid_grant forces reconnect |
| S12 | Uploaded malware/decompression bomb | Quarantine, size/MIME detection, safe image re-encode, optional AV; no archive extraction in core | Oversized/polymorphic files, zip bombs, SVG scripts; AV absence displayed honestly |
| S13 | Blob path traversal | Generated opaque keys, no filename-derived path, no symlink traversal, authorized download | ../, Unicode separators, hardlinks and symlink replacement race fixtures |
| S14 | Webhook forgery/replay | Raw-body HMAC, constant-time comparison, delivery ID/body-hash dedupe | Altered body and reused ID with different hash rejected; GitHub has no generic signed timestamp guarantee |
| S15 | Realtime leak / hijack | Same-origin SSE, session checks, no tokens in URL; later WS requires Origin and handshake auth | Unauthorized reconnect, cursor swapping, proxy logs and expiry tests |
| S16 | Unsafe dependencies/build | Lockfiles, advisories, licenses, SBOM, pinned CI actions/images, least CI permissions | Malicious lifecycle script and untrusted fork PR receives no publish secrets |
| S17 | Prompt injection / malicious repository config | Explicit capabilities, no repository-derived grants, trusted adapters, disabled auto-discovered hooks/plugins where supported | Repo requests credentials/network/PR write and cannot expand grant |
| S18 | Sandbox escape / host access | Separate runner VM boundary, no Docker socket/host mounts, nonroot, seccomp/AppArmor, cgroup limits | Escape suite, host-canary probes; containers alone not a hostile-tenant security guarantee |
| S19 | Secret exfiltration through model/tools/logs | Provider broker outside sandbox, short run-scoped credential, network allowlist, redaction and budget | Canary secret tests; models still see selected source code by explicit consent |
| S20 | Approval artifact substitution | Approval binds digest/base/head/tests/policy and expires | Change one byte or policy; publish blocked pending review |
| S21 | Retry creates duplicate external effect | Persistent operation marker, reconciliation before retry | Lost PR-create response resolves existing PR by branch/marker |
| S22 | Log/audit tampering | Append-only runtime grants, access audit, optional external signed export | DB administrator can still tamper; do not call it tamper-proof |
| S23 | Backup theft / incomplete restore | Encryption, separate key custody, blob manifest, restore rehearsal | Restore to isolated network, re-key/revoke external secrets before activating integrations |
| S24 | Malicious self-host config/proxy | Unknown-key rejection, secure defaults, trusted proxy list, no public database | Forged X-Forwarded-* must not bypass Origin/rate checks |
| S25 | Exhaustion / abusive automation | Body/query/connection quotas, tenant fairness, chain-depth limits, circuit breakers | One workspace flood does not starve others beyond configured budgets |

OWASP [SSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html) and [upload guidance](https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html) inform the boundary controls. [RFC 9700](https://www.rfc-editor.org/rfc/rfc9700) is the OAuth review baseline. OpenTrack-specific defaults above are design choices.

## Browser and response controls

CSP starts with default-src self, object-src none, base-uri none, frame-ancestors none; hashes/nonces only where required by tested build behavior. connect-src is self; browsers do not call AI providers directly. img-src allows self and bounded local blob previews, not remote tracking images. Serve attachments from a non-executable download route; force download for active formats. Set nosniff, Referrer-Policy no-referrer for sensitive flows, HSTS only on validated HTTPS deployments, and restrictive Permissions-Policy. Never cache authenticated API data at a shared proxy. OIDC/provider callback responses have no-store and immediately remove sensitive query parameters by redirect.

## Release gates and incident response

No open critical vulnerabilities; high findings require fix or documented exposure-based disposition by a human maintainer before release. Security review covers every new external URL path, privilege, secret type, renderer and runner capability. Rehearse user disablement, session revocation, provider-key rotation, runner quarantine, restoration and compromised webhook-secret rotation. Incident runbook: contain affected identity/runner, preserve redacted evidence, stop external dispatch, determine scope, rotate credentials, restore known-good state, then notify through a maintainer-approved channel. No contact address or vulnerability-reporting service is invented in this plan; establish it before public release.

## Approved expansion authorization and privacy

F37–F47 use existing active workspace/team access as a prerequisite. Initiative, scenario, release and report links additionally recheck each underlying source. Source access cannot be inferred from the container. Automatically derived titles, snapshots, counts, dependency edges, summaries and exports must never widen source visibility. User-authored text can be explicitly shared to a broader audience only through a preview showing destination and exact copied content; no system-generated private text crosses silently.

| Surface | Read rule | Write/privileged rule |
| --- | --- | --- |
| Initiative / scenario | Container team access plus current source access for each linked item and snapshot revision | Team contributor creates/edits; scenario apply additionally requires edit access to every selected target, checked atomically. |
| Capacity | Team access to aggregate availability and reservations; no private calendar event details | Team maintainer changes assumptions; proposed scenario overrides remain proposals until reviewed apply. |
| Personal focus/draft/reminder | Authenticated owning user only, plus current source access if linked | Owner only; admin has no ordinary browse bypass. |
| Team routine | Team access for submitted responses/agenda/agreement; drafts author-only | Author edits own response; facilitator manages session state; maintainer manages agreements. |
| Retro raw content | Current team access and explicit facilitator/participant membership; collection phase restricts cards until reveal | Participant manages own cards/votes in allowed phases; facilitator manages phases/themes, never unsubmitted drafts. |
| Retro author-hidden cards | Same raw access, but author identity omitted from participant output, search, events and exports | Operator can access database metadata; feature is not cryptographic anonymity. Identity is not disclosed to facilitator by ordinary UI. |
| Retro published summary | Current team access and listed summary audience | Facilitator publishes exact reviewed content; private raw material is a separate endpoint/representation. |
| Feedback contact | Team access plus explicit feedback_contact_grants permission | Team maintainer grants/revokes contact read/export; ordinary report permission is insufficient. |
| Release / review / incident | Team access plus independent evidence/source checks | Release lead or team maintainer transitions release; designated reviewer decides current target; incident lead/maintainer transitions incident. |
| Reports / snapshots | Current permission on every source and matching scope epoch; otherwise rebuild/filter safely | Requester may create only permitted reports; export checks again at dispatch and download. |
| Field/module/template definitions | Relevant team access; hidden module still requires normal ACL | Team maintainer edits shared definitions; personal navigation remains owner-only. |

All expansion role/member changes acquire the workspace authorization guard exclusively and increment permission epoch. Removing a retro participant, feedback contact grant or source-project membership invalidates affected cached query/report/snapshot results and subsequent stream emissions. Background report, reminder, template, export and publication jobs reauthorize the initiating principal. An admin cannot repair a UI permission problem by bypassing private team content checks.

Additional threat cases: S26 retro drafts/hidden authors leaking through generic documents or events; S27 vote-budget races and facilitator phase races; S28 old scenario/report snapshots leaking revoked source access; S29 feedback contact data leaking through import/export/search; S30 template retry duplicating tasks or copying private text to broader audiences; S31 stale review approving changed evidence. Each requires adversarial API and two-client tests plus export/event checks. Keep security audit to operation metadata, not raw retro, contact or private draft bodies.
