# Identity UI qualification, 22 October 2026

Scope: issue #12 / M02-T05, F01-A3 and F03-A4. Source: this change on base `ae98ab2`. This is browser qualification of the existing local identity services; it does not qualify optional OIDC or a native assistive-technology combination.

## Implemented corrections

- Rejected local credentials retain password-manager values, give a uniform email/password error, associate that error with the fields and return focus to the password. Service failures focus persistent feedback without marking credentials invalid.
- Invitation review focuses its heading. Invalid tokens and rejected sign-in focus their corresponding controls; other failures focus feedback. Error descriptions and busy state are exposed, and token/account input stays in the current view.
- Opening a workspace focuses its page heading. Canceling a switch because of unsaved edits keeps the switcher open, focused and showing the current workspace. A successful switch mounts a fresh account/workspace subtree.
- Team load failure no longer claims the directory is empty. Invitation team discovery and the add-member picker expose loading, empty and retry paths. Invitation success focuses its one-time token, and repeated membership/revocation controls identify their target accessibly.

## Executed checks

Windows Node 24.14.1 / npm 11.11.0 passed all 515 frontend tests across 63 files, application/e2e TypeScript checks and the production build. The final focused account test additionally verifies that an expired account's draft is discarded when another account signs into the same workspace. No dependency or schema changed.

The real application served the built bundle over its dedicated HTTP loopback acceptance origin with PostgreSQL 18, schema 22, under WSL Ubuntu. Playwright 1.64.1 executed the saved workflows in Chromium, Firefox and WebKit. Every browser context blocked all external origins; the local login case also asserted no external requests occurred.

| Workflow | Observed result |
| --- | --- |
| Local login with external origins blocked | Invalid login retains email, announces a uniform error and focuses the password; valid local login opens the real issue list and focuses its heading in all three engines. |
| Workspace rename and switch | A real second workspace and issue were created. Renaming through settings retained the original issue UUID URL and title. Keyboard-driven switching removed that issue pane and the previous workspace's team entry, then focused the new heading in all three engines. |
| Invitation and member permissions | An owner created an invitation, read its one-time token from the UI, and closed the dialog with focus restored. A new synthetic account accepted via keyboard. Member views concealed owner invitation/deactivation actions; replaying the token returned a persistent error with token-field focus in all three engines. |
| Exceptional-state regressions | Tests cover failed sign-in focus/field association, preserved password-manager values, team 403/503 versus empty results, retrying failed team discovery without losing an invitation email, invitation review/rejected-password focus, canceled dirty switching and account-scope draft clearing. Existing mutation conflict, reauthentication, archived/read-only, capability and stale-request regressions also passed. |

Commands: `npm --prefix web test`, `npm --prefix web run typecheck`, `npm --prefix web run build`, and `node node_modules/@playwright/test/cli.js test e2e/identity.browser.ts` from `web/`, using the explicitly disposable local fixture mode and privately supplied synthetic acceptance credentials. No CI environment was fabricated.

Saved browser result: **9/9 passed**, recorded in [the sanitized receipt](2026-10-22-identity-browser.json). The receipt contains test names, engine labels, durations and outcomes; it excludes authentication state, tokens, HTTP bodies, headers, traces, screenshots and video.

## Limits and recovery

Actual NVDA, VoiceOver and Orca operation, physical keyboard/OS input methods and native desktop interactions were not performed. These remain explicit acceptance work for M02-T05; automated browser roles, focus and keyboard actions do not establish WCAG conformance or justify closing the whole issue. This run blocks browser egress while the local API/database remain reachable; it does not simulate a disconnected browser that cannot reach its own server.

There is no migration or persistent storage format change. Reverting the frontend commit and rebuilding restores the prior behavior. Synthetic accounts/workspaces created by these tests belong only to the disposable acceptance database.
