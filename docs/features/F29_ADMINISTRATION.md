# F29 Administration and self-host management

Stage: MVP. Delivery: [M08_HARDENING](../milestones/M08_HARDENING.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Let an operator keep the product healthy and recover it without hidden vendor infrastructure.

## 2. Functional requirements

Approved expansion obligations: X60. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Workspace member/settings, storage quotas, queue failures, retention jobs and integration status; local CLI for setup, config validation, migration, backup coordination and recovery. No arbitrary terminal in admin web UI.

## 3. Interaction patterns

Settings separates personal/workspace/instance sections, current scope always visible. Destructive actions show concrete impact and recoverability. Health view labels targets versus observed metrics.

## 4. Data requirements

Validated config, workspace_settings, jobs and audit; no database table of unvalidated arbitrary server overrides. Operator state backed by documented files/DB roles.

## 5. API requirements

Scoped administration resources and local CLI plan; retries require job-specific capability and precondition. Diagnostics export redacted and opt-in.

## 6. Authorization rules

Owners/admins manage workspace settings; instance operators manage deployment. Neither role silently reads private-team bodies through job/diagnostic payloads.

## 7. Edge cases

Last owner lost, disk full, failed migration, stale worker, bad config, no SMTP, key lost and old/new release mix.

## 8. Failure behavior

Show actionable degraded status with request/job ID. A failed optional integration cannot block core administration; invalid critical config fails startup before side effects.

## 9. Performance considerations

Diagnostics read bounded summaries, metrics low cardinality, no expensive full-table scan per dashboard refresh. Background cleanup batched.

## 10. Feature dependencies

F03/F04, F30, deployment and backup design; no AI dependency.

## 11. Acceptance criteria

F29-A1: fresh installation and restore work offline. F29-A2: invalid config names exact field without secret. F29-A3: operator can diagnose failed job without private content exposure.

## 12. Testing strategy

Compose deployment rehearsal, nonroot/readonly filesystem, upgrade/rollback, restore from real backup, no-mail recovery and admin ACL tests.
