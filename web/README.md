# OpenTrack browser client

The implemented client provides workspace and team settings, membership management, single-use invitations, and the issue slice: local setup and sign-in, workspace/team navigation, server-filtered issue lists, pagination, issue creation, versioned issue editing, archive/restore, and server-recorded activity. It uses the compact neutral visual direction approved during product design. Navigation shows functional areas only. The broader planning, retrospective, and agent surfaces remain separate implementation work.

## Run and build

Requirements: Node 22.13 or newer on the Node 22 line, or a compatible maintained Node 24 release. The checked environment uses Node 22.22.1 and npm 9.2.0.

```sh
npm ci
npm run typecheck
npm test
npm run build
```

The Rust server serves `web/dist` and the API on the same origin. `npm run dev` provides Vite on `127.0.0.1:5173` and proxies `/api` to `127.0.0.1:8080`; the backend must explicitly allow that development Origin. Production does not need Node. No application content, credentials, or demo records are bundled into the client.

Routes use browser history: `/w/:workspaceId/issues`, `/w/:workspaceId/issues/:issueId`, `/w/:workspaceId/settings/:section`, `/account/security`, `/recover`, and `/join`. Settings sections are workspace, teams, members, and invitations. Query parameters retain team, assignment view, status, priority, and title/key search through detail navigation. Server search is intentionally bounded title/key matching in this slice; it is not the later full-text search feature.

## Write and failure behavior

- Sessions use the server cookie. The CSRF token stays in memory and is included in authenticated mutations. Theme preference is the only value written to local storage.
- Create sends an `Idempotency-Key`. An interrupted or server-error response retains the same body and key; the dialog requires a safe retry before editing or dismissing that unresolved request.
- Edits send only changed fields with the strong `If-Match` issue version. A 412 preserves the draft and shows the latest values. The explicit rebase action keeps the user's changed fields and adopts server changes to untouched fields. Saving remains a separate action.
- Archive and restore send the same version precondition. Archived fields are read-only; permitted contributors can restore the record. The list filter has an explicit archived view. An archive conflict loads the latest issue and requires another explicit action rather than silently retrying.
- Activity is loaded from the issue activity API and refreshed after an acknowledged mutation. The current server records the actor and changed field names; the interface does not invent before/after values or infer whether a historical archive-state change was an archive or restore. The endpoint returns up to 100 events.
- Unsaved edits prompt before application navigation, browser back navigation, or leaving the page. Sign-out explicitly warns that edits will be discarded. Session expiry clears the workspace UI and in-memory drafts; there is no offline or cross-session draft storage in this slice.
- Switching workspaces remounts the complete workspace subtree and aborts in-flight loads. A detail response is also checked against the route workspace before rendering. API authorization remains the server's responsibility.
- Late activity responses from a previous issue/workspace are ignored. Requests carry an internal session generation so a late 401 from an earlier login cannot expire a newer authenticated session.
- Network, validation, unavailable-access, read-only, empty-list, filtered-empty, and conflict states are shown explicitly. Error request IDs remain available for support.

## Identity and workspace settings

- Workspace creation checks organization authority; being a workspace owner is not sufficient by itself. A workspace can be renamed, archived, and explicitly restored without changing its ID.
- Teams can be public or private. The server provides `can_manage`; workspace ownership does not imply private-team access. Team prefixes are permanent. Members can be added, assigned contributor/maintainer roles, and removed with version preconditions.
- Workspace members can have their roles changed or be deactivated/reactivated. Owner changes and deactivation are explicit actions. The server protects the final active owner.
- Invitation tokens are displayed once, copied only on request, and accepted by manual token entry. They are sent in POST bodies, never in URLs or storage. A replay with a missing token explains that the invitation must be revoked and replaced. There is no email delivery in this slice.
- Existing accounts must sign in as the invited email before accepting. A new account can be created from the invitation. Invitation possession cannot reset an existing account password.
- Sensitive changes can require password confirmation after ten minutes. The dialog refreshes the authenticated session and asks the person to retry their original action. It does not silently repeat a mutation.
- Directory loads and settings mutations abort when their view closes. A lightweight access endpoint is checked every two seconds while visible and immediately on focus. An epoch change removes scoped drafts and cached content before refreshing the session and bootstrap. This polling fallback will be replaced by the planned event stream.
- Archived teams are excluded from normal navigation and creation choices. Their issue history remains in the archive, with editing blocked until the parent is restored. Archived workspaces keep settings accessible for restoration.

## Team workflows

Settings → Teams → Workflow edits the selected team's status names, colors, order and default. The route retains `team_id` and `panel=workflow`. Categories stay fixed after creation; changing their meaning uses an explicit replacement. Ordered category groups have keyboard-accessible move buttons, a separate save/discard step, and conflict review that preserves proposed names and order. Workflow versions are separate from team/access versions. Conditional reads refresh visible workflows without remounting issue drafts; permission-sensitive ETags prevent an unchanged content version from retaining obsolete management controls.

New issue creation displays the explicit team default and leaves status selection to the server until the person chooses a status. Renames retain IDs. A retiring choice stays visible but cannot be newly assigned, while an existing issue may retain that unchanged status when editing other fields. Historical lookup preserves status labels, and an unavailable metadata response never hides the issue itself.

Replacement previews the current affected count and default transfer, with explicit acknowledgement for a category change. A confirmed 202 is a queued durable job, not completed work. The progress view shows server-reported moved/remaining counts, automatic retry versus manager resume, and a saved job ID that survives reload. It shows no invented percentage or ETA. Archived statuses have paginated history, replacement details and explicit restoration. Interrupted creation/replacement retains the original body, version and idempotency key for safe retry.

Frontend verification for this increment: 88 tests across 10 files and the production build passed. Thirteen new DOM regressions cover defaults, draft preservation, conflict review, safe replay, progress/resume, protected 304/401 behavior, focus, historical labels and scope cancellation. These results are separate from real PostgreSQL worker and live-browser qualification.

## Account security

`/account/security` is available from the account menu and the no-workspace screen. It lists only server-returned active sessions, with the current-session marker, authentication/activity times, and idle/absolute expiries. Opaque cursors load further pages; the client does not invent device names, locations, or IP addresses.

Individual revocation and “Sign out other sessions” are explicit CSRF-protected requests. Revoking the current session clears the authenticated UI. Reauthentication refreshes the session list after login rotation and leaves the original revocation for a separate, deliberate retry. A protected 401 clears cached session rows; a late response cannot sign out a different account view after the Security page has closed. Optional OIDC controls remain separate work.

DOM regressions cover zero-workspace access, current/other/bulk revocation, refreshed CSRF after reauthentication, expired-session clearing, scope cancellation, and cursor pagination. Browser/API verification still depends on the session-management server endpoints being available.

## Account recovery

`/recover` is public and does not wait for setup or session APIs. The sign-in screen links to it. The operator verifies the person's identity and issues a code outside the browser; the page does not request an email or imply that email was sent.

The form accepts a manual code, a new password, and confirmation, including pasted and password-manager-filled values. It sends only `{token,password}` in a same-origin POST to `/api/v1/auth/recovery/complete`. No CSRF token or current login is required. Codes and passwords stay in the component and request body; they are never written to URLs or browser storage. Invalid, expired, revoked and used codes share one generic error.

A confirmed 204 clears the authenticated client state and secret fields, focuses the success heading, and offers a separate sign-in action. It does not log the person in or change membership. A network interruption, timeout or server failure remains explicitly uncertain: inputs are locked for an exact-body retry, a used-code response is not treated as success, and the person can try signing in with the new password or obtain a replacement code. Rate-limited requests stay editable and are never automatically retried.

Leaving recovery aborts its request and ignores a late completion. Entering recovery also aborts pending startup/session and login responses so those cannot restore stale client authentication. DOM regressions cover these boundaries, public-route independence, password-manager input, confirmation, secret clearing, generic failures and exact-body retries. Live server/browser evidence is recorded separately by the backend integration check.

## Keyboard and accessibility

`C` opens issue creation, `/` focuses list search, `?` shows shortcuts, `Escape` closes an issue or native dialog, and `Control/Command + Enter` submits create or edit forms. Single-key commands pause while typing. Native dialogs provide focus containment and return focus to their trigger. Issue links retain native open-in-new-tab behavior. Controls have visible focus, labels, and a skip link; touch targets grow on coarse pointers and reduced-motion preferences disable animation.

At narrow widths, the issue detail replaces the list and the navigation uses a native dialog. Desktop uses a list/detail split where both panes have sufficient room. The appearance follows the system by default, with light/dark overrides available in the account menu.

## Verification

Package versions are pinned and committed in `package-lock.json`. Initial `npm install` reported zero advisories. That is registry audit output, not a security certification.

`npm run build` passes TypeScript checking and builds the production artifact. Targeted tests cover invitation authentication and secret handling, safe idempotency replay, server-provided private-team capabilities, access-epoch invalidation, directory cursor isolation, archived-parent restrictions, membership conflict handling, and changed-field patches, conflict rebase behavior, session credentials/CSRF headers, error details, ambiguous-network handling, strong ETags, workspace-state clearing, session-expiry clearing, mismatched-workspace responses, archive/restore permissions and conflicts, activity refresh/abort behavior, and late responses from an earlier session. DOM tests use jsdom 29.1.1, the compatible line for the installed Node 22.22.1; current jsdom 30 requires Node 22.22.2 or newer.

Real-browser verification against the Rust/PostgreSQL application is tracked in the repository's implementation evidence. Unit/DOM tests do not substitute for that runtime check or a screen-reader audit.

## Labels and eligible assignees

Workspace settings → Labels manages shared labels; Settings → Teams → Labels manages that team's labels. Each row uses the server's current management capability. New labels use idempotency keys; uncertain creation keeps the original body and key. Edit/archive/restore use permission-sensitive label ETags, preserve conflict drafts, and allow an explicit name repair during restoration.

Issue forms use paginated, debounced label and assignee comboboxes rather than the first bootstrap member page. Arrow keys and Enter select results; Escape closes only the picker and returns focus to its trigger. Queries request 20 results per page and offer explicit Load more. Current selections remain visible outside search pages. Historical assignees and attached archived labels use the authorized issue summaries, without exposing email addresses. Archived label chips support remove/undo; new selections stop at 50. Team changes visibly reset the create form's team-bound selections.

Issue list filters support Any/All selected labels, including archived labels. Priority order preserves the server's global ordering with unset last instead of regrouping it by status. The default view remains newest first within status groups. Dense rows limit visible label pills while preserving the complete accessible name.

All issue patches send the exact server-issued `Issue.etag`, including the reference projection digest. Label renames and eligibility changes can therefore produce an ordinary conflict without discarding the draft. Restoring an archived issue opens an explicit review for status, assignee, and labels; only those repair fields accompany `archived:false`. Historical label links can remain, and an unavailable assignee must be explicitly changed before restoration.

Frontend validation on 18 October: 109 tests across 11 files and TypeScript passed; production assets were built into ignored `work/web-taxonomy-dist` to preserve the running workflow checkpoint. Tests cover candidate pagination/self eligibility, historical display, native keyboard/focus behavior, access-epoch remount and abort, expired cursor restart, private-scope denial, label conflicts/replay/restore, set-based draft changes, global priority order, and atomic restore repair. Live server/browser qualification is separate; these results do not claim screen-reader or browser-layout verification.
