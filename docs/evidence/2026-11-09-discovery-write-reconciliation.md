# Saved-discovery write reconciliation and local read ordering

Scope: residual P1/P2 from the corrective review at `7bf7f23`.

Favorite and unfavorite responses update the directory, but a newer selected ETag is held for explicit filter reconciliation. The selected write base remains unchanged until the user applies the displayed remote filters. Definition-write responses (including lost-response idempotent replays) are compared against the operation's captured submitted definition; a different returned definition follows the same review path. Draft names survive reconciliation.

Starting a local write aborts earlier detail reads. Opening that write's target is fenced until its outcome is known, and Refresh is disabled during writes. A different object remains selectable; the completed write cannot clear or replace that newer selection. Favorite operations carry their actual directory-row target rather than borrowing the selected object ID.

`Discovery.review.test.tsx` reproduced six failures before the fix: favorite and unfavorite after remote narrowing, definition replay after narrowing, both attempted reopen/delete response orders, and a pre-write refresh completing after a favorite. The corrected review suite additionally covers same-version favorite, unchanged replay and selecting a different object during deletion. The delete tests verify the conflicting detail read is prevented, not merely that one favorable response ordering works.

Validation on the corrective source:

- `npm --prefix web test`: 89 files, 1024 tests pass.
- `npm --prefix web run typecheck`: both TypeScript targets pass.
- `npm --prefix web run build`: passes; existing large-chunk advisory remains.

These are React/jsdom request-boundary regressions and build checks, not fresh browser, PostgreSQL, native, physical-input or assistive-technology qualification. Migration 0035 and the database/API behavior are unchanged; rollback is a source revert. Generated API files remain outside this corrective commit for coordinated union regeneration. Full acceptance and protected integration remain separate gates.

Follow-up review found that the actual favorite target could still replace a different selected object without applying its filters. A new broad-A/restrictive-B regression failed with B incorrectly becoming selected after favoriting B. Metadata now updates selection only when the returned object matches the captured selected base. The corrected regression verifies rename targets A, then explicitly opens B and observes its restrictive predicate. The full web suite passes 1,025 tests across 89 files; both TypeScript targets pass again.
