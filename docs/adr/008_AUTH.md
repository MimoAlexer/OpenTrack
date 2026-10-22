# ADR-008 Local identity plus optional OIDC

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

A self-hosted install must work without cloud identity and must not confuse an AI account with product membership.

## Options and tradeoffs

Mandatory SaaS identity breaks offline/core autonomy. OIDC-only burdens personal installs. Local sessions plus optional external identity support both but increase security responsibility. Stateless long-lived JWT sessions simplify reads but make prompt revocation harder.

## Decision

Use local Argon2id passwords, opaque DB-backed sessions and optional maintained OIDC implementation. Roles/membership are OpenTrack data. Sensitive operations reauthenticate; local recovery is explicit and audited.

## Consequences and risks

Hashing/rate limits and recovery need careful operation. Native AI/provider OAuth is separate. MFA deferred means internet-facing administrators may prefer optional OIDC with their own MFA; product does not claim native MFA yet.

## Reconsideration and validation

Revisit session storage only with measured need and preserved revocation. M02 auth/CSRF/session tests are release-critical. [OAuth BCP](https://www.rfc-editor.org/rfc/rfc9700), [password storage](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html).

See [canonical specification](../SECURITY_ARCHITECTURE.md).
