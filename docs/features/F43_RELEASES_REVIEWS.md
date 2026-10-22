# F43 Releases and review coordination

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M19_RELEASES_INCIDENTS](../milestones/M19_RELEASES_INCIDENTS.md). Expansion coverage: X43, X44, X45, X46, X62. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Make delivery decisions from scope, review ownership and inspectable evidence.

## 2. Functional requirements

Approved expansion obligations: X43, X44, X45, X46, X54, X62. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

A release has team, owner, version label, target date and draft/ready/released/canceled state. Scope links ordinary issues across cycles within authorized teams. Readiness conditions have owners and evidence references. Review requests specify target version/digest, reviewer and decision. Release notes are a reviewed draft; publication is a separate recorded intent.

## 3. Interaction patterns

Release detail combines scope, outstanding reviews and readiness evidence. Updating a target invalidates an approval bound to an older version. A release checkbox cannot assert that deployment happened.

## 4. Data requirements

releases, release_issues, release_conditions, review_requests, review_decisions, release_notes and release_publications. Evidence points to immutable artifact digests or versioned records. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/releases; /releases/{id}/scope,conditions,notes,transition; /review-requests; /review-requests/{id}/decision; /releases/{id}/publication. External publication follows external_operations reconciliation. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Team access and release lead or team-maintainer authority required to change release state; requested reviewers approve only accessible current artifacts. Publication additionally requires a configured integration grant. No implicit merge/deploy. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Stale approved patch; deleted issue; reviewer leaves; disconnected publication; release blocked by inaccessible linked scope.

## 8. Failure behavior

Stale review returns conflict and remains unapproved. Failed publication leaves release notes saved and records retry/reconciliation state. Changing readiness cannot silently mark a release deployed.

## 9. Performance considerations

Page release scope and reviews; reconcile provider results through bounded jobs. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F06, F11, F14, F19, F26, F32, F34; M08, M09, M10, M15; agent review also M13.

## 11. Acceptance criteria

F43-A1: Approval binds the exact target version/digest and becomes stale when the target changes.

F43-A2: Release readiness distinguishes evidence-backed conditions from unchecked or unknown conditions.

F43-A3: Release-note publication retries reconcile to one external effect and preserve the saved draft.

F43-A4: A release groups work across cycles without changing its original cycle history or granting wider read access.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
