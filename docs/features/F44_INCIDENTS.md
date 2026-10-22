# F44 Incidents and post-incident review

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M19_RELEASES_INCIDENTS](../milestones/M19_RELEASES_INCIDENTS.md). Expansion coverage: X47, X48. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Coordinate response and learn from interruptions without rewriting evidence.

## 2. Functional requirements

Approved expansion obligations: X47, X48. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

An incident has owning team, lead, severity, impact, state and start/end timestamps. Timeline events are append-only with correction links. Response and corrective work are ordinary issues. Review records contributing conditions, decisions, outcome and follow-up dates; capacity changes are explicit proposals.

## 3. Interaction patterns

Current impact and response tasks precede the timeline. Correcting an event shows both original and correction. Review action creation previews destination audience.

## 4. Data requirements

incidents, incident_events, incident_issue_links and incident_reviews. Correction references are scoped to the same incident and cannot cycle. Sensitive incident details remain team-scoped. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/incidents; /incidents/{id}/events,issues,transition,review; /incident-events/{id}/correction. Closing response and completing review are separate commands. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Incident team access plus normal contributor role; maintainer/lead changes severity/state. Linking a public task never copies private narrative automatically. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Clock correction, response reopened, lead removed, deleted corrective issue, incident spans cycles.

## 8. Failure behavior

Retrying event append returns the same event; no event is silently overwritten. Failed follow-up creation leaves review and pending action intact.

## 9. Performance considerations

Page timeline with timestamp plus stable sequence; retain authorized correction history. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F06, F11, F13, F15, F19, F40, F41; M08, M15, M17, M18.

## 11. Acceptance criteria

F44-A1: Timeline correction preserves the original event and an attributable correction.

F44-A2: Closing incident response does not automatically close corrective tasks or declare review complete.

F44-A3: Incident-to-action copying previews the destination audience and exact shared content.

F44-A4: Capacity displacement from an incident remains an explicit reviewed planning change with source linkage.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
