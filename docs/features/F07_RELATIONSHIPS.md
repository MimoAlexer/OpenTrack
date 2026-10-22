# F07 Issue relationships

Stage: MVP. Delivery: [M03_ISSUES](../milestones/M03_ISSUES.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Make blockers, related work and duplicates explicit so a team can reason about dependencies.

## 2. Functional requirements

Approved expansion obligations: X08, X20. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Support directed blocks/blocked-by, symmetric related and canonical duplicate-of. Reject self links, blocking cycles and duplicate chains; no cross-workspace or cross-team links in MVP. Linking does not mutate status.

## 3. Interaction patterns

Relationship picker searches accessible same-team issues; detail displays direction and link action. Removing a link is reversible via a new command and activity entry.

## 4. Data requirements

issue_relations with unique normalized pairs, composite FKs and graph serialization lock. Duplicate target must be a canonical issue and remains referenced through archive.

## 5. API requirements

GET/POST/DELETE issue relationship subresource; use expected version on source aggregate and validate target version/state when relevant.

## 6. Authorization rules

Both endpoints must be accessible and writable for link creation. Readers never see the identity of a now-inaccessible target; underlying metadata stays protected.

## 7. Edge cases

Two simultaneous opposite block edges, duplicate marked duplicate elsewhere, deletion of one endpoint, graph depth/size and import of invalid edges.

## 8. Failure behavior

409 returns the safe conflicting path when fully authorized, otherwise generic invalid relationship. The UI retains the search selection for correction.

## 9. Performance considerations

Team-scoped graph lock; index source and target. Bound recursive traversal at 10000 visited nodes and return actionable capacity error instead of hanging.

## 10. Feature dependencies

F04/F06 are immediate prerequisites. M03 provides an authorized same-team key/title lookup for the picker and the atomic activity writer. F24 full search arrives in M06; F11 discussion/history UI arrives in M04. Neither later milestone blocks relationship creation.

## 11. Acceptance criteria

F07-A1: A blocks B and B blocks A cannot both commit. F07-A2: related A/B appears once regardless of submission order. F07-A3: deleting a target does not expose a dangling private title.

## 12. Testing strategy

Property tests for normalization/acyclicity; real-DB simultaneous edge insertion; importer invalid-graph report and keyboard relationship picker tests.
