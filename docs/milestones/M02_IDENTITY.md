# M02 Authentication, workspaces and isolation

Status: in progress; the [foundation evidence](../evidence/2026-10-17-foundation.md) proves only the current slice. Full task completion remains governed by [requirements status](../REQUIREMENTS_STATUS.md). Release tier: MVP. Dependencies: M01. Feature coverage: F01–F04, F30 foundation.

## Scope and deliverables

Deliver local login and a real private workspace/team boundary before issue data exists.

## Implementation-ready tasks

| Task | Prerequisites | Deliverable and acceptance condition |
| --- | --- | --- |
| M02-T01 Identity schema | M01 | Add users/identities/sessions/invites/organizations/workspaces/teams with scoped constraints and explicit migrator; add the atomic audit sink before privileged bootstrap/member commands. |
| M02-T02 Owner bootstrap | M02-T01, M02-T03 | Implement one-use local setup and offline login/recovery path; no mandatory mail/OIDC. |
| M02-T03 Authorization services | M02-T01 | Centralize role/private-team predicates, RLS transaction context and authorization-guard lock order. |
| M02-T04 Member lifecycle | M02-T02, M02-T03 | Invite/accept/revoke/deactivate, preserve last owner, update epoch and remove stale sessions/grants. |
| M02-T05 Identity UI | M02-T04 | Login, switcher, team/member settings, forbidden/empty/error states using M01 components. |
| M02-T06 Audit verification and optional OIDC | M02-T05 | Verify privileged audit from T01 across commands; local recovery rehearsal; OIDC adapter only after callback/JWKS/security tests. |

Follow the explicit prerequisites, not numeric task order. Whole-milestone prerequisites inherit that milestone's completion gate; independent tasks may be sequenced separately. Conditional and optional gates apply only to the named capability. Keep each task as a reviewable vertical increment; update the feature acceptance IDs and canonical contracts with behavior changes.

## Validation

All F01–F04/F30 applicable acceptance IDs; cross-tenant matrix, real pooled RLS context, concurrent last-owner/revoke tests, CSRF/session replay and keyboard login/switch.

## Risks and scope guard

Auth bugs invalidate every later feature. Do not defer negative-access tests or let instance admin silently see private content.

## Completion condition

Two accounts in two workspaces/private teams pass isolation and recovery flows; no token/secret appears in logs; active revocation blocks new writes.

Evidence must include exact environment, executed checks, outcomes and limitations. Build/static checks do not substitute for UI, external-provider or recovery proof. See [quality gates](../TESTING_STRATEGY.md) and [roadmap](../ROADMAP.md).
