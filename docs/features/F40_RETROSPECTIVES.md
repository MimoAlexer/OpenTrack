# F40 Retrospectives and improvement actions

Stage: authorized complete-product scope; no acceptance completion inferred. Delivery: [M17_RETROS_ROUTINES](../milestones/M17_RETROS_ROUTINES.md). Expansion coverage: X31, X32, X33, X34, X35, X36, X52. [Status](../REQUIREMENTS_STATUS.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md), [security](../SECURITY_ARCHITECTURE.md).

## 1. Purpose and user value

Turn team reflection into owned changes and verify their effect.

## 2. Functional requirements

Approved expansion obligations: X31, X32, X33, X34, X35, X36, X48, X52, X62. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Session states are preparing, collecting, grouping, voting, discussing and closed. Require team, facilitator, participant list, prompts, privacy mode and retention policy. Named or author-hidden cards are supported; true operator anonymity is not claimed. Votes have a per-user budget and frozen rounds. Theme actions create ordinary issues with owner, review date and success condition. Next session references prior actions and observed results.

## 3. Interaction patterns

Collection hides submitted cards until reveal. Grouping retains original cards. Voting shows remaining budget. Discussion queue/timer permits pause, pass and asynchronous comments. Summary publication previews audience and exact content separately from raw cards.

## 4. Data requirements

retro_sessions, retro_participants, retro_cards, retro_themes, retro_theme_cards, retro_vote_rounds, retro_votes, retro_actions and retro_summaries. Card authors remain protected metadata; drafts stay in author-only draft storage. Every workspace-owned row and relation uses the canonical scoped foreign-key and lifecycle rules.

## 5. API requirements

/retrospectives; /retrospectives/{id}/participants,cards,themes,rounds,transition; /retro-rounds/{id}/votes,lock; /retro-themes/{id}/actions; /retrospectives/{id}/summary,publish,follow-up. Transitions require expected state/version. Paths inherit /api/v1/workspaces/{workspace_id}. Mutations use the shared ETag, idempotency, error and event contracts.

## 6. Authorization rules

Current team access plus explicit session membership is required for raw content. Facilitator manages phases, not workspace grants. Summary readers receive only the published summary. Action creation into a broader audience requires explicit content preview and authority on destination. Search, events, counts, exports, queued jobs and optional AI context apply the same server-side checks.

## 7. Edge cases

Participant removed mid-vote; tie; facilitator leaves; reopening vote; raw-card retention expires; hidden-author inference in small groups.

## 8. Failure behavior

Lock user budget/round for voting; concurrent requests cannot overspend. Idempotent action creation cannot duplicate issues. Publishing errors keep summary draft private. Reopening creates a new round.

## 9. Performance considerations

Limit session to 200 participants and 1,000 cards initially; paginate history and group without full-board DOM rendering. These are initial bounds, not measured performance claims.

## 10. Feature dependencies

F04, F06, F11, F12, F15, F19, F23, F25, F39; M08 and M15.

## 11. Acceptance criteria

F40-A1: Concurrent vote commands cannot exceed the member budget or mutate a locked round.

F40-A2: Facilitators cannot read unsent drafts; summary-only readers cannot query raw cards, votes or author identities.

F40-A3: Creating an action rechecks destination access, previews copied content, and yields exactly one linked issue on retry.

F40-A4: Next-session follow-up distinguishes issue completion from verified improvement and preserves prior action identity.

## 12. Testing strategy

Execute the acceptance criteria against real PostgreSQL and the API. Include wrong-workspace/team, revoked-access, duplicate-request and concurrent-edit cases. Exercise the complete browser workflow with two users, keyboard navigation, narrow layout, both themes and retained drafts on failure. Verify native export/import and restore for new durable rows, event replay after reconnect and core operation with AI disabled. Link commands, environment, screenshots where relevant and observed outcomes in [requirements status](../REQUIREMENTS_STATUS.md); a mock fixture alone does not establish runtime completion.
