# F23 Templates

Stage: MVP. Delivery: [M04_COLLABORATION](../milestones/M04_COLLABORATION.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Reduce repetitive issue entry while making default fields understandable before work is created.

## 2. Functional requirements

Approved expansion obligations: X09, X41, X66. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Team issue templates store title pattern, editor document and default status/priority/labels/project. Instantiation creates a fresh issue; changes to template do not rewrite existing issues. Nested template-generated child trees are later work.

## 3. Interaction patterns

Create dialog chooses template and previews filled fields. User edits freely before submit; switching templates with edits prompts merge/replace/cancel.

## 4. Data requirements

versioned templates with document schema and typed default IDs; archive keeps existing created issues and history intact.

## 5. API requirements

Template CRUD, preview/instantiate or regular issue create with template_id/version attribution. Stale references must be resolved before creation.

## 6. Authorization rules

Team maintainers manage templates; contributors use templates for writable team. A template cannot assign inaccessible private content.

## 7. Edge cases

Removed label/status, template changed during draft, unsupported editor schema, duplicate template name and empty required title.

## 8. Failure behavior

Invalid defaults are highlighted individually and must be corrected; no silent assignment to a different team or arbitrary default.

## 9. Performance considerations

Template list metadata only, lazy-fetch body; max 100/team initially. Instantiate validates once and uses normal issue transaction.

## 10. Feature dependencies

F06, F08/F09/F10, F12 editor, F04.

## 11. Acceptance criteria

F23-A1: instantiated issues are independent of future template edits. F23-A2: stale private/archived IDs cannot bypass validation. F23-A3: replacing an edited draft requires an explicit choice.

## 12. Testing strategy

Default mapping and schema migration tests, template/version races, access denial and draft-preservation browser tests.


## Current implementation boundary

The team issue-template slice uses the [0018 storage contract](../DATABASE_DESIGN.md#implemented-team-issue-templates-0018) and [team-scoped API contract](../API_DESIGN.md#implemented-team-issue-templates-0018): independent rich documents, status/priority/labels, metadata pagination, lazy preview, archive/restore, pinned version validation and explicit merge/replace/cancel. Migration 0020 adds explicit literal or pattern title mode. Literal remains the default, including for existing brace text. Pattern mode substitutes only `{{team.name}}`, `{{team.key}}` and `{{date}}` (UTC `YYYY-MM-DD`). Preview shows the resolved title and context timestamp; the reviewed, editable issue draft owns the submitted title even across midnight. Unknown or malformed placeholders are rejected, and oversized output is reported without truncation. Instantiated issues retain immutable source version/digest attribution without exposing private template metadata through moved issues. The current definition is retained; full historical template bodies are not stored.

Project defaults, attachment/reference cloning, mentions, child trees and X09/X41/X66 expansion workflows remain unfinished. Executable title expressions are unsupported. [Title substitution qualification](../evidence/2026-10-20-template-titles.md) records executed checks and limits; issue #24 stays open for its remaining obligations.
