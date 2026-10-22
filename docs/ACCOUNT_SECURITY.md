# Account security

Open **Account security** from the account menu at the bottom of the workspace sidebar. Accounts without workspace membership can open it from the account home screen. The direct route is `/account/security`.

The page lists active sign-ins across your account's workspaces. Each row shows its public session ID, client type, creation time, most recent authentication, last activity, and expiration. The current session is marked. Times use your device's timezone. Desktop rows show the device name you approved during sign-in; the application does not infer a location or invent browser device names.

Sessions expire after 12 hours without activity or 7 days after creation, whichever comes first. Refresh retrieves current activity. Large lists have a **Load more** action; revocation rechecks authority on the server regardless of what the page previously displayed.

Use **Revoke** to end another sign-in, or **Sign out other sessions** to keep only the current one. These actions require password authentication within the last 10 minutes. If asked, confirm your password and then retry the intended action. A successful confirmation rotates this browser's session and refreshes the list. A wrong password leaves the existing session intact.

The current row's **Sign out** action remains available without recent password confirmation. In the browser it revokes the server session, expires the cookie and clears this tab's authenticated views. Other tabs using that same cookie lose access on their next authorized request. In the desktop app it revokes the native session and removes the OS-vault credential.

## Desktop sign-in and revocation

Connect the desktop app to the same server you use in the browser. The app opens the server's sign-in approval page in your system browser. Verify the server, named device and matching confirmation code before approving. If your browser authentication is more than ten minutes old, confirm your password there first. Return to the desktop app after approval.

The desktop session has its own identity and credential. It never imports your browser cookie, and signing out of one client does not automatically end a different client's session. Use **Revoke** on the named desktop row, **Sign out other sessions**, or account recovery when you intend to end those sessions. Revocation is observed on the desktop client's next authorized request, when it also removes the local credential.

Sensitive desktop actions request confirmation through the browser. Complete that confirmation with the same account. A successful handoff rotates the native session and preserves the mounted workspace's unsaved draft. If the exchange result is uncertain, the app signs out locally and asks you to sign in again; review active sessions to revoke an unwanted device. Do not assume a canceled or interrupted browser window undoes an already approved sign-in.

The OS credential service must be available and unlocked. A vault failure does not cause plaintext storage. Forgetting a server removes its local connection and credential; if server revocation could not be confirmed, use another signed-in client to review and revoke the remaining session.

## Implementation and operations

Public session IDs are independent UUIDs; bearer-token hashes and CSRF tokens do not appear in the session directory or security audit. Session creation, login/invitation rotation and revocation append account-scoped audit metadata in the same database transaction. This history does not depend on workspace membership. Runtime roles cannot update, delete or truncate it.

All authenticated application commands acquire the user guard before any workspace guard and recheck the session while holding it. Revocation takes the account guard exclusively. A competing write either commits before revocation, or waits and is rejected after revocation; an already-authorized response cannot be recalled.

Pagination is limited to 100 records per page, ordered by immutable creation time and ID. HMAC cursors bind the account, current session, page size and a 24-hour lifetime. Changing the server setup secret invalidates these pagination cursors, requiring a fresh list; it does not invalidate active sign-ins. Login rotation also invalidates the previous session's cursors.

Migration 0005 adds public session IDs and append-only account security history. Apply migrations with the migration role before starting this version. Existing sessions receive IDs; historical creation events are not fabricated. Audit session identifiers retain their meaning after future credential-row retention, with ownership checked when an event is inserted. Automated session/audit retention and optional OIDC remain separate unfinished work.

## Access increases and sign-in

Promoting a workspace role, reactivating membership, granting new private-team access or granting maintainer rights ends the affected member’s existing sign-ins. They sign in again to use the added access. Updating another person preserves your own sign-in. If you grant yourself maintainer rights, the change is saved and the app shows **Access updated. Sign in again.** It does not ask you to repeat the change.

Unchanged roles, demotions, removals and a contributor row in an already-public team do not cause an account-wide sign-out. Existing authorization checks still apply immediately. Accepting an invitation as an existing account keeps the newly rotated browser session and ends the account’s other sessions; it does not extend the previous authentication window or absolute expiry.

Session revocations and their access change commit together. Account event `user_id` identifies the affected account. A shared correlation ID links those events to the workspace audit that identifies who changed the access. No migration is needed beyond 0006 for this addition. Validation evidence is recorded separately from the recovery checkpoint.

Invitation expiry is checked again when it is consumed. If the token expires while the request waits, no account, membership or session change is saved; request a new invitation. Issuing a replacement for an expired invitation retires and audits the old one without revealing its private grants. Repeating the same issuance request returns the replacement metadata but never reveals the bearer again.

## Recover a local password

An installation operator can issue a one-use recovery code without SMTP. The account holder opens **Reset your password** on the sign-in page, or `/recover`, and enters the code and new password. Codes belong in the form, never in a URL. The password follows the existing local-password rules. The recovery form does not require an existing sign-in.

A successful recovery changes the password, consumes the code, invalidates other pending recovery codes and revokes every session for that account in one transaction. It preserves authorship and memberships, creates no membership, and does not sign the person in. Sign in with the new password afterward. Recovery links open `/signin`, which requires credential entry even if this browser still has a valid session; an existing cookie cannot substitute for checking the new password. Malformed, unknown, expired, used, revoked and disabled-account codes receive the same `422 recovery_invalid` response. Password validation can report its specific rule before checking the code. No email or account details appear in these errors.

Codes expire after 30 minutes by default; `auth.recovery_ttl_minutes` accepts 5–60 minutes. The lifetime applies when issuing a code, so changing configuration does not extend existing codes. The code contains a version, public account UUID and 256 random bits. Only its SHA-256 digest is stored. Issuing another code invalidates the previous pending one; submitting a code twice cannot reset the password twice.

## Operator procedure and authority

Provision the separate restricted operator login using the [runtime setup instructions](RUNTIME_SETUP.md), after applying migrations. Supply its PostgreSQL URL using `operator.database_url_file` or `OPENTRACK_OPERATOR_DATABASE_URL`; keep this credential outside the serving process. The operator command accepts the existing public account UUID, not an email lookup or a workspace-owner session.

```sh
opentrack-server check-config --for operator --config /etc/opentrack/operator.toml
opentrack_recovery_dir="$(mktemp -d /tmp/opentrack-recovery.XXXXXXXX)"
opentrack-server operator-recovery-issue --config /etc/opentrack/operator.toml \
  --user 00000000-0000-0000-0000-000000000001 \
  --reason "Account holder requested password recovery" \
  --output-file "$opentrack_recovery_dir/recovery-code.txt"
opentrack-server operator-recovery-revoke --config /etc/opentrack/operator.toml \
  --user 00000000-0000-0000-0000-000000000001 \
  --reason "Recovery code was delivered to the wrong recipient"
```

Replace the example UUID with the intended account. `--help` lists these commands and `--recovery-ttl-minutes`. Reasons require 1–500 characters and cannot contain control characters. They are audit metadata, so do not put passwords or codes in them. Configuration validation does not connect to the database; the actual command checks the database role before creating any bearer material.

Issuance requires a new absolute output file in an existing directory owned by the operator's OS user with mode `0700`. Every ancestor must be root-owned or owned by that user, with no group/other write permission; root-owned sticky temporary directories such as `/tmp` are the exception. The example creates a private temporary directory rather than relying on a shared repository directory. The CLI traverses pinned directory handles without following symlinks, creates a new `0600` file, and persists it before activating the code. Standard output contains only the receipt ID, account UUID, expiration and output path. Revocation reports its affected count. Deliver the file contents through the operator's chosen private channel; delete the local copy and its private directory when no longer needed. The application does not send a recovery message automatically.

If the file cannot be created or persisted, no database issuance is attempted. If database issuance cannot be confirmed, the command preserves the private file and reports an uncertain outcome. Do not assume the code is usable or unused. Reissue to a new file with a fresh receipt/code to supersede any pending code, or revoke recovery explicitly. Reusing a receipt does not resurrect a code. Revoke is safe to repeat; each operator invocation still records its identity and reason.

The original account operator capabilities use database `CONNECT`, schema `USAGE`, and `EXECUTE` on four reviewed functions: `operator_issue_recovery(uuid,uuid,text,integer,text)`, `operator_revoke_recovery(uuid,text)`, `operator_disable_account(uuid,text)` and `operator_enable_account(uuid,text)`. It receives no application table/sequence access, schema creation, ownership, role membership, superuser, `BYPASSRLS`, `CREATEROLE`, `CREATEDB` or replication privilege. The operator service rejects incorrectly provisioned credentials. The serving service rejects credentials with direct or role-reachable execution rights on any of these operator functions. The functions run as the migration owner with a fixed schema-qualified search path and no caller-supplied SQL; table row-level security remains forced. The runtime role cannot call these functions or directly manufacture a recovery token.

Recovery issuance/revocation audit records capture the database `session_user` inside the function, the target account, reason and correlation ID. Runtime writes cannot impersonate an operator audit entry. Completion has its own recovery audit event, with no fabricated session. Session-reference ownership checks and append-only audit protections still apply. Password replacement, token consumption, session invalidation and mandatory audit all roll back together on failure.

Recovery and operator commands acquire the exclusive account guard before token/session locks. Ordinary authenticated writes use the shared guard and recheck the session. Password verification runs outside database transactions; login rechecks the verified password hash under the exclusive guard before minting a session. A password reset therefore fences login work that verified an old hash as well as requests carrying old sessions.

Migration 0006 adds recovery tokens and capability functions, and extends account audit with nullable session references and recovery/operator metadata. Existing session events remain valid. Apply it using the migration credential, then apply explicit operator grants; the migration never discovers or grants an operator login automatically. Audit keeps public receipt IDs without foreign keys to token rows so future retention can erase hashes while retaining history. The retention worker is not implemented yet. This is a forward migration; no downgrade or rollback compatibility has been qualified.

The local operator is an installation authority. Workspace ownership alone does not authorize resetting a global account that may belong to unrelated workspaces. **F02-A3 remains unverified and has an unresolved authority conflict:** its second-owner recovery requirement must define consent and global identity authority before implementation. This operator path implements the separately audited recovery capability in F03; it does not silently reinterpret F02-A3. SMTP recovery delivery, optional OIDC and cross-surface cleanup qualification remain separate work. See the [requirements ledger](REQUIREMENTS_STATUS.md) for evidence and unfinished acceptance gates.


## Disable or re-enable a global account

Use the separate installation operator credential. A workspace owner cannot disable a global account through the workspace UI or HTTP API.

```sh
opentrack-server operator-account-disable --config /etc/opentrack/operator.toml \
  --user 00000000-0000-0000-0000-000000000001 \
  --reason "Account access suspended by the installation operator"
opentrack-server operator-account-enable --config /etc/opentrack/operator.toml \
  --user 00000000-0000-0000-0000-000000000001 \
  --reason "Account access restored by the installation operator"
```

Replace the example UUID with the intended account and run only the desired command. Both require a bounded reason, reject `--output-file`, and return a JSON receipt containing the account UUID, resulting disabled state and timestamp, whether the state changed, correlation ID, and counts of revoked sessions, recovery codes and outgoing invitations. They do not print the reason, password, code, or database credential. Repeating the same desired state returns `changed: false` and records a distinct operator no-op audit event. If a connection fails before a result is confirmed, retry the same command to confirm the desired state; do not infer success from a missing response.

Disabling preserves at least one enabled active owner in every affected workspace, including archived workspaces, and at least one enabled owner in every organization the account owns. It never chooses a replacement owner automatically. An account that is the sole organization owner cannot be disabled until another owner is explicitly appointed. Organization membership administration is still unfinished; do not use an unreviewed database edit as a substitute.

A successful disable atomically sets `disabled_at`, revokes all unrevoked sessions, pending recovery codes and pending outgoing invitations, advances affected workspace epochs, and records mandatory account/workspace audit and access-change events. It includes outgoing invitations in workspaces where the inviter's membership is now inactive. Existing account sessions and cached actors fail subsequent authorization checks. Incoming invitations remain pending, but a disabled account cannot accept them. Authorship, membership records and the password hash remain intact.

Re-enabling first invalidates any leftover unrevoked credentials or outgoing invitations from older disabled-account records, then clears `disabled_at` and records the state change. Receipt counts include this cleanup when present. It does not revive any old session, recovery code or outgoing invitation, and does not sign the person in. The account must authenticate again; a new recovery code can be issued if needed. Assignment cleanup, API-key cleanup and background jobs will be added with those features and are not claimed here.

Migration 0007 adds these capabilities, operator actor shapes and enabled-owner constraints. Apply it with the migration role and then rerun the explicit operator grant step. Older schemas without all four grants fail the operator preflight. Migration preflight rejects pre-existing ownerless scopes instead of silently assigning ownership. The runtime database role cannot change `disabled_at` directly or fabricate operator audit/events. Owner checks retain FORCE RLS and use migration-owner-only metadata read policies; there is no caller-controlled RLS bypass.

The lock order is the target account exclusively, sorted organization guards, then sorted workspace guards. Counts are checked after guards are held, without acquiring other account locks afterward. Each operator audit row records the actual database `session_user`; workspace events identify a local operator without exposing the operator identity, reason, or private-team details. All effects share a correlation ID. A required audit failure rolls back the entire operation. This is a forward migration; rollback requires the matching protected database backup and binary, and no downgrade or complete restore qualification is claimed.

Migration 0008 strengthens required-write checks in the existing operator functions. It changes no command or permission. A silently skipped account/credential write or required audit causes the entire operation to fail; bulk session and recovery operations use the same all-or-nothing rule. Apply the migration before running the updated version. See [verification](evidence/2026-10-17-atomic-audit.md).

Migration 0015 adds four separate consumer-operation capabilities to the same isolated installation-operator role: status, explicit dead-letter disposition, enable/disable and failed assignment-cleanup resume. They grant no account recovery authority to a worker or workspace member. See [consumer recovery](DEPLOYMENT_ARCHITECTURE.md#assignment-consumer-recovery) for exact commands, bounded status and audited reasons. The current grant script provisions all eight published operator functions; internal helpers remain unavailable to the login.
