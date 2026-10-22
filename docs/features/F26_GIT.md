# F26 Git repository integration

Stage: Post-MVP. Delivery: [M09_GIT](../milestones/M09_GIT.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Connect tracked work to repository changes while keeping repository write rights distinct from issue permissions.

## 2. Functional requirements

Approved expansion obligations: X45, X58. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

GitHub App installation, repository allowlist, issue-to-PR links, branch naming helper, PR/check status and opt-in status mapping. Initial integration is linking/read sync; bidirectional issue/comment sync and other forges are later adapters.

## 3. Interaction patterns

Settings shows selected repositories and granted permissions; issue detail has PR cards with last-sync time and revision. Integration view distinguishes connection, webhook delivery and repository permission failures.

## 4. Data requirements

integration_connections, repositories, pull_requests, issue_pull_requests, webhook_inbox and external_operations; provider IDs stable across repository rename.

## 5. API requirements

Installation callback, repositories/PR lists and link action; raw-body authenticated inbound hook. Reconcile remote state on delayed/out-of-order events rather than blindly applying arrival order.

## 6. Authorization rules

Workspace admin installs; permitted members link visible issue and allowed repository. Git write/PR creation needs separate agent publication grant, never implied by installation alone.

## 7. Edge cases

Fork PR, force push, deleted branch/repository, renamed org, revoked installation, webhook redelivery and PR linked to several issues.

## 8. Failure behavior

Keep stale link with sync warning. Provider outage never rolls back issue edits. Unknown external write result reconciles by durable marker; no duplicate PR on retry.

## 9. Performance considerations

Durable webhook ACK before async processing; target<2s receive/commit; bounded API concurrency and provider rate-limit headers; periodic incremental reconciliation.

## 10. Feature dependencies

F06, F04, F30, F32 webhooks infrastructure, F27 optional mapping.

## 11. Acceptance criteria

F26-A1: forged hook cannot mutate issue. F26-A2: duplicate/out-of-order events converge to provider state. F26-A3: read connection cannot publish code. F26-A4: private repo details remain scoped.

## 12. Testing strategy

GitHub contract fixtures plus opt-in sandbox account test at implementation; signature/duplicate/rename/force-push cases; browser revoked-installation recovery.
