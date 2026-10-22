# ADR-017 Shared browser and desktop application

Date: 2026-10-18. Status: Linux foundation implemented after the user's explicit request for both delivery forms. [Evidence](../evidence/2026-10-18-desktop.md) records packaging and native-core/browser checks; native-window, cross-platform and release qualification remain required.

## Problem

The running OpenTrack client is a browser application. F36 calls for an installable client that connects to the same self-hosted service without duplicating business logic or exposing native privileges to server content. Desktop delivery is being brought forward while the remaining product scope stays active.

## Decision

Package the shared React assets with Tauri 2. Keep its Rust package and lockfile in a separate `desktop/` Cargo workspace so backend verification does not depend on platform GUI headers. Browser requests stay same-origin. A desktop adapter invokes a small Rust command surface that owns the selected server and credentials. PostgreSQL and the application service remain authoritative for both clients.

The bundled webview may call only explicitly listed commands. No frontend filesystem, shell, process or generic HTTP capability, and no privileged remote origin. Block remote navigation and frames. Rust validates relative API routes, methods, headers and sizes; callers cannot supply an origin, cookie, Authorization header, redirect policy or local path. Authenticated requests verify TLS and refuse redirects. Plain HTTP is limited to explicitly selected loopback development servers.

Use system-browser sign-in with random state, PKCE S256 and a short-lived loopback listener. The user approves a named device with a matching confirmation code. A one-use authorization code may cross the callback URL; a session credential may not. Exchange produces an opaque native session with a separate audience. Approval requires recent browser authentication. Exchange rechecks authority under the account guard before consuming the grant and issuing the audited session. Revocation, recovery, account disablement and access elevation cover both audiences. Native reauthentication binds the original user and rotates the old native session.

Browser cookies accept only the web audience; bearer authentication accepts only the desktop audience. Reject mixed or ambiguous credentials. Native mutations skip browser CSRF only after valid native authentication. Setup, password sign-in, recovery, invitation acceptance and consent retain explicit browser audience and Origin requirements. Session management exposes device/audience metadata without credentials.

Store credentials in the OS credential manager; connection metadata contains references only. Never fall back to plaintext storage. Server changes invalidate outstanding responses and scoped UI state after handling unsaved work. A credential must never reach another origin. Unknown-server issue links require ordinary server selection/trust. Deep links contain validated server/workspace/issue references and still require API authorization.

## Consequences and validation

This is an online desktop client. No durable offline replica or mutation queue is implied. Connection failures must preserve drafts and ambiguous writes must retain idempotency keys.

Qualify Linux first; Windows and macOS remain separate build/runtime gates. Require a real credential-store roundtrip, launched native window, shared issue workflow, remote session revocation, server isolation, denied privileged remote navigation and packaged deep links. Compilation or a running process does not establish UI behavior.

Keep updates disabled until a real distribution identity, verification key and controlled feed exist. Invalid-signature/interrupted-update tests remain required by F36. Platform signing, public distribution and unresolved naming/licensing retain their release gates.

Official references checked 18 October 2026: [Tauri capabilities and custom command permissions](https://v2.tauri.app/security/capabilities/), [native application browser/PKCE guidance](https://datatracker.ietf.org/doc/html/rfc8252), and [Tauri deep-link and single-instance handling](https://v2.tauri.app/plugin/deep-linking/). This is OpenTrack's native session handoff, not a claim of general OAuth authorization-server compatibility.
