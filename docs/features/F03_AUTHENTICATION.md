# F03 Authentication and sessions

Stage: MVP local; OIDC optional. Delivery: [M02_IDENTITY](../milestones/M02_IDENTITY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Provide reliable local access and recovery while allowing an organization to use its own identity provider.

## 2. Functional requirements

Approved expansion obligations: X60. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Local password login/logout/session management; one-time bootstrap; hashed reset/invite tokens; optional OIDC authorization-code PKCE flow. No AI connection is required for sign-in. MFA/passkeys are later work.

## 3. Interaction patterns

Login supports password-manager paste, visible errors without account enumeration, and return-to restricted to same origin. Settings lists sessions and allows revocation. OIDC login does not auto-link an existing email.

## 4. Data requirements

users, user_identities, sessions and recovery_tokens; opaque session hash, expiry/revocation and auth_time. Secrets/passwords never enter activity records.

## 5. API requirements

/auth/login,logout,session,csrf,recovery and OIDC start/callback; all auth responses no-store. Recovery requests use a uniform response.

## 6. Authorization rules

Authenticated users manage their own sessions. Local operator recovery is a separately audited capability. OIDC identity is issuer+subject; sign-in cannot grant workspace membership.

## 7. Edge cases

Clock skew, key rotation, changed OIDC email, blocked mail, compromised session, forgotten password without SMTP and callback mix-up.

## 8. Failure behavior

Rate limits return Retry-After; unavailable OIDC leaves local recovery path documented. Invalid token or secret configuration fails closed, not a guest session.

## 9. Performance considerations

Bound Argon2 work to configured memory/concurrency; avoid coupling hash pool to Tokio I/O workers. Cache JWKS with expiry and controlled refresh on unknown key.

## 10. Feature dependencies

F01 membership, F04 authorization, F29 administration, F30 audit; security baseline.

## 11. Acceptance criteria

F03-A1: old cookie fails after logout/recovery. F03-A2: session cannot access another workspace after a crafted URL change. F03-A3: invalid issuer/audience/state/nonce is rejected. F03-A4: offline local login works.

## 12. Testing strategy

Cookie/CSRF integration tests, password hashing calibration, OIDC fixture issuer tests, rate-limit abuse, keyboard/password-manager flow and recovery rehearsal.
