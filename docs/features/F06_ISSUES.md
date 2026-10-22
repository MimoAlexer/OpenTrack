# F06 Issues and sub-issues

Stage: MVP. Delivery: [M03_ISSUES](../milestones/M03_ISSUES.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Capture an actionable unit of work with stable identity, clear status and structured decomposition.

## 2. Functional requirements

Approved expansion obligations: X03, X04, X07, X11, X35, X62. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Title, rich body, team key, priority, owner, labels, optional project/milestone/cycle, estimate and due date. One parent, depth <=8, same team. Archive and soft delete separately; restore for 30 days. Parent completion never silently completes children.

## 3. Interaction patterns

Create dialog starts with title and contextual team; Enter submits only outside multiline editor through the documented shortcut. Detail opens beside list, has stable full-page URL, and supports child creation inheriting team with previewed defaults.

## 4. Data requirements

issues, issue_key_aliases, documents and activity; team counter allocates unique numbers transactionally. Mutable entity version starts at 1; create idempotency prevents duplicates after network loss.

## 5. API requirements

Issue create/read/patch/delete/restore, children list and explicit move action. Require If-Match for edits and Idempotency-Key for create. API does not allow arbitrary workspace_id replacement.

## 6. Authorization rules

Workspace contributors mutate accessible teams. Archived objects are read-only until unarchived; deletion requires author or team maintainer/admin under current team access.

## 7. Edge cases

Concurrent create, cyclic parent, max-depth subtree move, duplicate title, moved issue links, removed status/assignee and parent deleted while child is open.

## 8. Failure behavior

412 shows both versions and preserves draft. Network ambiguity retries the same key; no duplicate issue from double click. Parent removal detaches children with explicit activity, never cascades deletion.

## 9. Performance considerations

Indexed key lookup; list page 50; metadata edit target p95 <=200 ms server time. Do not load all comments/children into list rows.

## 10. Feature dependencies

F01–F04, F08 status, F09 taxonomy, F10 ownership; editor starts in M04 without blocking simple title slice.

## 11. Acceptance criteria

F06-A1: retrying create yields one key and one issue. F06-A2: concurrent updates produce one success and one conflict. F06-A3: a graph cycle/depth violation is rejected. F06-A4: old key resolves after move for authorized user.

## 12. Testing strategy

PostgreSQL idempotency/version/graph concurrency tests; two-client conflict E2E; create-to-reload persistence; delete/restore and child detach assertions.
