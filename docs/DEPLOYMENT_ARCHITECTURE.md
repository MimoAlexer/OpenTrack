# Deployment, operation and recovery

Proposed deployment contract; no Compose file, image, database or infrastructure has been created. [Configuration](CONFIGURATION_POLICY.md), [performance envelope](PERFORMANCE_STRATEGY.md), [security](SECURITY_ARCHITECTURE.md).

## Initial topology

Docker Compose runs TLS reverse proxy, API/static assets, core worker and PostgreSQL 18 with persistent volumes. API and worker use the same immutable image with different subcommands. A local blob directory is mounted only into components that need it. PostgreSQL has no public port; metrics and administration stay on a private network. Containers run as nonroot with read-only root filesystems and explicit writable paths. Secrets are mounted read-only, not baked into images. No Docker socket is mounted into API/worker.

Only proxy 443 is externally exposed; 80 may redirect for operator-managed TLS. Proxy routes /api to API with buffering off for SSE and a >45s idle allowance; it serves no arbitrary blob path. Static assets use content-hashed immutable caching; index and authenticated API use appropriate no-cache/no-store. SPA routes fall back to index only for intended HTML navigation, never for /api, missing assets or attachment downloads. Production upgrades never rely on an unpinned latest image tag.

Small design profile: 4 vCPU, 8 GiB RAM, SSD with 100 GiB free; budget core API/worker/PostgreSQL/proxy before optional scanning. It is a starting benchmark target, not a measured minimum. Agents require separate capacity and are disabled. Smaller personal deployments are considered only after measured tests. No Redis/S3/SMTP/AI connectivity is required to pass core acceptance.

## Installation and startup lifecycle

M01 will define one setup guide: verify host prerequisites and supported architecture, obtain signed release/digests, generate local secrets, choose public URL and durable volume paths, validate TOML, run a one-shot migration under exclusive lock, start services, consume the local owner bootstrap secret, create workspace/team and test an issue. Do not start schema migrations in every API replica. Readiness checks schema compatibility, DB query and writable blob capability; liveness reports process health and must not restart all nodes merely because PostgreSQL is temporarily unavailable.

A worker starts only after supported schema is confirmed. An unavailable optional integration degrades its own feature and exposes a useful status; it does not block core readiness. Core can run offline after release images/assets are obtained. Static fonts, icons and help needed for setup ship with the release. Times are stored UTC; teams choose IANA business timezones.

## Upgrade and rollback

1. Read release notes and compatibility window; confirm restore-tested backups and sufficient disk.
2. Stop new background claims and put incompatible operations into maintenance mode. For ordinary expand migrations, old readers can keep working within the published window.
3. Run the migration job; record checksum, duration and schema version. Large backfills are restartable, bounded and visible.
4. Roll out the new API then worker version only where schema permits; verify readiness, login, create/read issue, event replay and queue drain.
5. Retain the previous image and compatible configuration. Roll back application binaries only if the migration explicitly permits it. Otherwise restore a complete pre-upgrade snapshot, documenting writes that would be lost.
6. Contract migrations remove old structures only after at least one supported release overlap and a fresh backup. No automated down-migration is assumed safe.

Single-node Compose has planned downtime and no high availability claim. Later multiple API replicas require shared blob/object storage and a proxy; each replica tails PostgreSQL events independently. PostgreSQL HA and failover remain operator-managed with tested recovery semantics. Kubernetes manifests are a later optional packaging effort, not evidence that the core needs microservices.

## Backups and restoration

Personal/default target: nightly encrypted database plus blob backup, seven daily and four weekly copies, RPO <=24h and RTO <=4h for the reference dataset. Recommended team profile adds continuous WAL archiving/base backups with RPO <=15m and RTO <=2h. These are rehearsal goals contingent on storage/throughput, not achieved SLAs.

A complete backup contains the consistent database state, all referenced ready blobs, a digest manifest, configuration schema/version, migration checksums and key IDs. Encryption keys are backed up separately under operator control; without them provider credentials cannot be recovered. Blob files are immutable once ready. During a database snapshot, suspend blob garbage collection and retain every object referenced by that snapshot until the backup manifests finish; unreferenced extras are harmless, missing references are not. Snapshot/transfer operations have deadlines to avoid pinning vacuum indefinitely.

Restore into an isolated network with external email/webhooks/agent dispatch disabled. Restore DB and matching blobs; verify hashes, counts, memberships, private-team denial, sample attachments, and schema compatibility. Revoke runner leases and pending external operation permissions, rotate sessions/keys if compromise is suspected, then explicitly enable dispatch. A replayed outbox must not resend already reconciled external effects. Release evidence includes one full recovery from backup and a deliberately missing-blob failure test.

## Operations and failure behavior

- Database unavailable: writes fail visibly with retryable service status; do not acknowledge local-only durability. Display stale read state only when clearly labeled.
- Disk full: uploads stay unready, quota reservation rolls back/reconciles, health alerts identify storage; no successful ready attachment with missing bytes.
- Worker down: core issue writes continue, queue age rises, inbox/integrations show delayed state. Replay retention alarms fire well before seven-day expiry.
- Provider down: connection status and attempts reflect it; no automatic change of billing source or provider.
- Runner lost: controller fences leases, disables broker credentials and marks reconciliation; host restart cannot silently resume an obsolete attempt.
- Secrets lost: restore from separate protected backup or reconnect providers. Do not recover through browser cookies or copied native-app credentials.

Useful operator signals: DB pool wait, transaction p95/p99, queue oldest age, event replay lag, disk free/quota, backup age/last restore, error rates, listener reconnects and grant denials. Logs are structured and redacted; support export is opt-in and scrubbed. Metrics default to localhost/private interface and omit high-cardinality user/workspace IDs. A deployment checklist in M08 must test fresh install, offline operation, upgrade from prior release, failed migration recovery, restore and nonroot operation on x86_64 and at least one ARM64 environment before claiming both supported.

## Assignment consumer recovery

The restricted worker now has three bounded responsibilities: capture durable assignment-cleanup intent, execute one job batch, and retain replay events below all required consumer watermarks. The `worker --once` command performs at most one capture poll, one job batch and one retention poll. A long-running worker rotates these stages fairly and uses the existing worker polling/lease settings. Capture retries and maintenance-job retries are distinct: an event checkpoint may advance once its downstream job is durably committed, while that job remains independently recoverable.

Inspect a workspace through the separate installation-operator credential. Status is read-only and includes bounded dead-letter/failed-job metadata, checkpoint and retention positions, backfill state and the oldest pending event timestamp; it does not return issue text or credentials.

```sh
opentrack-server operator-consumer-status --workspace WORKSPACE_UUID \
  --config /etc/opentrack/operator.toml
opentrack-server operator-consumer-disposition --workspace WORKSPACE_UUID \
  --dead-letter DEAD_LETTER_UUID --disposition retry \
  --reason "Dependency repaired; retry the original event" \
  --config /etc/opentrack/operator.toml
opentrack-server operator-consumer-disposition --workspace WORKSPACE_UUID \
  --dead-letter DEAD_LETTER_UUID --disposition reconcile \
  --reason "Capture a fresh assignment reconciliation for this event" \
  --config /etc/opentrack/operator.toml
opentrack-server operator-cleanup-resume --workspace WORKSPACE_UUID \
  --job JOB_UUID --reason "The failed cleanup dependency is repaired" \
  --config /etc/opentrack/operator.toml
```

Choose the intended recovery action after inspecting status; do not run both dispositions speculatively. Retry preserves the event's place. Reconcile commits a new durable assignment sweep and receipt before advancing past that event. Unsupported or mismatched targets fail without advancing state. A response lost after submission is uncertain: inspect status before repeating the action. Receipts contain identifiers and a correlation, while the bounded reason remains in restricted operation history.

`operator-consumer-disable --workspace WORKSPACE_UUID --reason TEXT` requires a drained consumer and completed cleanup jobs. It preserves retention protection. `operator-consumer-enable --workspace WORKSPACE_UUID --reason TEXT` resumes the registered version. Both use the same operator configuration flag as above. Disabling is not a way to discard old work or relieve storage pressure. This increment provides durable blocked-event alerts and status inspection; automated storage-pressure admission control and the broader administrative monitoring UI remain separate implementation work.

Migration 0015 and the current `deploy/grant-worker.sh` and `deploy/grant-operator.sh` must be applied together. Operator execution remains separate from serving, with no application table or sequence grants. Preserve a database backup plus the matching prior binary/assets before upgrading. No down-migration is supplied.
