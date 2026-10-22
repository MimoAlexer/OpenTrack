# F39 Personal planning and team routines

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M17_RETROS_ROUTINES](../milestones/M17_RETROS_ROUTINES.md). Expansion coverage: X01, X04, X05, X06, X25, X26, X27, X28, X29, X30. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Support daily focus, cycle planning and team coordination without duplicating ownership.

## 2. Functional requirements

Approved expansion obligations: X01, X04, X05, X06, X25, X26, X27, X28, X29, X30. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Personal ordering and focus never change shared issue priority. Persist drafts per author and reminders with due/completion state. Team check-ins have prompts, deadlines and linked answers. Agendas link decisions/actions; working agreements have owner/review date. Calendar projects cycles/releases/availability totals with explicit timezone.

## 3. Interaction patterns

My work combines owned work and requests. Focus is a personal view. Check-ins support asynchronous responses; cycle closure shows carry-over decisions and an accessible scope table.

## 4. Data requirements

personal_work_order, focus_items, content_drafts, reminders, checkin_sessions, checkin_responses, team_agendas, agenda_items and working_agreements. Linked tasks remain ordinary issues. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/me/focus; /me/work-order; /me/drafts; /reminders; /checkins; /checkins/{id}/responses; /team-agendas; /working-agreements; /calendar. Calendar GET is a projection, not a second task store. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Focus/order/drafts/reminders are recipient-only. Team routine content follows team access. A check-in author edits own response; facilitator may close the session but not impersonate an author. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Unassigned or revoked issue in focus; duplicate reminder firing; cycle reopen; team timezone change; draft conflict across devices.

## 8. Failure behavior

Retried reminders create one notification per due occurrence. A draft cannot silently overwrite a newer version. Revocation purges inaccessible references from subsequent projections.

## 9. Performance considerations

Default daily/weekly bounded views; reminders claim due rows with leases, not one timer per item. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F06, F11, F15, F19, F20, F23, F25; M07, M08, M15.

## 11. Acceptance criteria

F39-A1: Reordering personal focus does not change shared issue order or priority.

F39-A2: A due reminder emits at most one durable notification per occurrence under retries.

F39-A3: Check-in drafts remain author-only and two-client conflicting saves retain both versions for recovery.

F39-A4: Cycle planning and calendar show the same local date boundaries, carry-over history and current linked work.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
