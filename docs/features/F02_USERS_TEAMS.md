# F02 Users and teams

Stage: MVP. Delivery: [M02_IDENTITY](../milestones/M02_IDENTITY.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Represent who can collaborate and which team owns a workflow, without requiring a corporate directory.

## 2. Functional requirements

Approved expansion obligations: X60. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Invite, accept, deactivate and re-invite users; create workspace-visible/private teams; maintain team membership and contributor/maintainer roles. Support name/avatar preference without mandatory external avatar service.

## 3. Interaction patterns

Member table filters active/invited/deactivated. Private-team creation explains membership explicitly. Invite links can be copied manually if email is absent; expiry and role are visible before sending.

## 4. Data requirements

users, invitations, invitation_teams, workspace_members, teams, team_members and team_prefixes. Prefixes remain reserved to preserve issue-key resolution.

## 5. API requirements

Members and teams CRUD, invite consume/revoke, assignable-user lookup. Every role change requires expected version and records the acting principal.

## 6. Authorization rules

Workspace admins invite existing/new users within role limits; team maintainers add existing workspace users to accessible teams. Private membership is never inferred from organization ownership.

## 7. Edge cases

Concurrent last-owner removal, expired invite, reused token, invitation to existing account, private team made public and deactivation with open assignments. Guest users are M14.

## 8. Failure behavior

Invite delivery failure does not create a usable account automatically; pending invitation remains retrievable. Deactivation blocks authentication immediately while cleanup jobs finish.

## 9. Performance considerations

Member lists paginate 50; reassign/unassign cleanup batches 100. Autocomplete returns at most 20 current eligible users.

## 10. Feature dependencies

F01 tenancy, F03 authentication, F04 policies, F19 optional email, F30 audit.

## 11. Acceptance criteria

F02-A1: an invite is usable once and only before expiry. F02-A2: removing private-team membership removes search/list/event access. F02-A3: a second owner can recover the first account without losing authorship history.

## 12. Testing strategy

Token replay and role-escalation tests; concurrent owner changes in PostgreSQL; browser private-team navigation; deactivation while assigning or streaming.
