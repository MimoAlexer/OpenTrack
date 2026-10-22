# F42 Feedback intake and prioritization evidence

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M18_DECISIONS_FEEDBACK](../milestones/M18_DECISIONS_FEEDBACK.md). Expansion coverage: X03, X38, X39. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Preserve reported problems and their evidence through prioritization and delivery.

## 2. Functional requirements

Approved expansion obligations: X03, X38, X39. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Store reports, source type/reference, observed date, optional consent-scoped contact data, disposition and linked issue/project. Triage accepts, declines, requests information or merges duplicate reports while preserving originals. Priority rationale distinguishes observed impact from uncertain estimates.

## 3. Interaction patterns

Triage list with report/source detail and destination issue preview. Duplicate reports remain individually inspectable. Contact data is separated from ordinary report text and excluded from general exports by default.

## 4. Data requirements

feedback_reports, feedback_contacts, feedback_links and feedback_dispositions. Contact records have purpose, consent/retention fields and stricter access. No automatic customer directory or anonymous public intake. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/feedback; /feedback/{id}/disposition,links; /feedback/{id}/merge; /feedback/{id}/contact. Merge uses explicit canonical report and expected versions. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Team access required; contact read/export requires feedback-contact permission granted by a team maintainer. A report link never grants issue access. Outbound replies require separately authorized integration effects. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Reporter erasure request; revoked source link; duplicate merge chain; malformed or sensitive imported contact data.

## 8. Failure behavior

Triage is transactional and idempotent; merging cannot discard original source records. Failed contact import leaves the report quarantined or excludes contact fields with a visible result.

## 9. Performance considerations

Keyset queue and bounded related-report lookup; no unbounded cross-workspace similarity search. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F06, F11, F13, F24, F28, F37; M08 and M15.

## 11. Acceptance criteria

F42-A1: Duplicate merge preserves each original report and cannot create a merge cycle.

F42-A2: General issue readers cannot retrieve contact fields through search, exports or API joins.

F42-A3: A prioritization decision records evidence and uncertainty without silently changing priority.

F42-A4: Feedback can be followed to accessible delivery work while revoked links disclose no hidden metadata.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
