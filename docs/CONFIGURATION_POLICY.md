# Configuration policy and option catalog

This document defines the full target configuration policy and catalog. The current server implements a bounded subset: see [implemented settings and commands](configuration/IMPLEMENTED_SETTINGS.md), the generated [reference](../config/REFERENCE.md), [JSON Schema](../config/schema.json), and [example TOML](../config/opentrack.example.toml). Keys outside that subset remain pending and are rejected, including disabled future features; their appearance below does not make them usable settings. [ADR-007](adr/007_CONFIGURATION.md), [deployment](DEPLOYMENT_ARCHITECTURE.md).

## Formats and ownership

TOML is mandatory wherever the tool supports it: application settings, Cargo/rust-toolchain/rustfmt/clippy/cargo-deny, Tauri.toml with config-toml, and proposed mise tool pins. Native mandatory formats are retained: package.json/package-lock.json, tsconfig.json, shadcn components.json, OpenAPI JSON, JSON schemas, SQL migrations, CSS tokens/Tailwind 4 theme rules, TypeScript Vite/ESLint/Playwright configuration, and editor document JSON. Docker Compose and GitHub Actions use YAML because their native configuration requires it; no YAML application configuration or unnecessary YAML task runner is introduced. .gitignore, Dockerfile and .editorconfig retain their native formats. Prefer npm workspaces over introducing a YAML package-workspace file merely by habit.

Infrastructure settings belong to TOML plus secret references. User-managed preferences, workflow statuses and provider connection metadata are validated database records exposed through APIs; they are product data, not a second untyped server configuration system. Provider SDK settings that require JSON are generated into a temporary per-run directory from typed allowlisted policy, not copied from an untrusted repository.

## Load, precedence and validation

Compiled non-secret defaults < explicitly selected base TOML < optional environment TOML < OPENTRACK__SECTION__KEY environment override < named CLI overrides for the current process. No automatic search up repository directories and no arbitrary includes. `--config` and `--environment` choose files; the application never treats the current Git repository as trusted operational configuration. Nested tables merge by key; arrays replace entirely. Environment values parse to the declared type, never a loose string fallback. Secret references are exclusive with literal values.

schema_version is required and starts at 1. Serde types deny unknown fields; validation runs before sockets/worker claims start. Check ranges, URLs, filesystem ownership, mutually exclusive auth modes, credentials required by enabled features, pool totals, retention relationships, and secure production origins. Emit field path and source location on failure, without secret content. Display effective redacted configuration with per-key provenance through a local CLI command. Never print secret source file contents.

The target configuration migration is an explicit offline command: parse old schema, produce a proposed new TOML with change report and preserved backup, validate, then atomically replace only on operator request. No such command is implemented while schema version 1 is the only supported version. Do not silently rewrite configuration at startup. Unknown future schema fails closed. The implemented subset has a machine-readable schema and generated reference with drift tests; expand them alongside working runtime settings.

Restart is required for all infrastructure/security settings initially. Dynamic UI preferences take effect through versioned database changes and audit where relevant. Later hot reload is limited to log filters and proven-safe budgets after validation; partial reload is rejected. A user role cannot alter deployment settings through a generic JSON endpoint.

## Baseline options

S marks a secret reference (file URI/path or secret-store identifier); secret values never have defaults. Durations use explicit units; sizes are bytes with documented KiB/MiB parsing. Paths are absolute, owned by the service user and outside source directories. Feature-specific keys remain valid but inactive when the feature is disabled.

| Key | Type; default | Validation / semantics |
| --- | --- | --- |
| schema_version | integer; required 1 | Exact supported version |
| server.environment | enum; production | development/test/production; development never default |
| server.listen | socket; 127.0.0.1:8080 | Container deployment explicitly uses 0.0.0.0; proxy owns exposure |
| server.public_url | HTTPS URL; required | No credentials/query/fragment; HTTP allowed only explicit loopback development |
| server.trusted_proxies | CIDR list; empty | Only these peers may supply forwarded address/scheme |
| server.allowed_origins | URL list; public_url only | Exact origins; no wildcard credentials |
| server.request_timeout | duration; 15s | 1–120s; streaming endpoints use own idle timeout |
| server.shutdown_grace | duration; 30s | 5–120s; readiness drops immediately |
| server.max_json_body | size; 1 MiB | 64 KiB–4 MiB; document subtype limit still applies |
| server.max_connections | integer; 1000 | 50–10000; process FD budget validated |
| database.url_file | S; required | PostgreSQL URI with restricted runtime role |
| database.api_max_connections | integer; 20 | 2–100; deployment-wide sum plus reserve below PostgreSQL max |
| database.worker_max_connections | integer; 5 | 1–50; separate worker process pool |
| database.acquire_timeout | duration; 2s | Fail request with retryable overload rather than wait indefinitely |
| database.statement_timeout | duration; 3s | Commands/queries; bounded maintenance uses separate role/session |
| database.lock_timeout | duration; 1s | Deadlocks/timeout classified for bounded retry |
| database.idle_transaction_timeout | duration; 5s | Excludes approved export snapshot process with independent deadline |
| database.max_transaction_retries | integer; 3 | 0–5; only safe transactional work |
| storage.backend | enum; local | local initially; s3 only when adapter implemented |
| storage.local_root | absolute path; required | Durable volume, not web root or Git checkout |
| storage.max_upload_bytes | size; 25 MiB | 1–100 MiB; per-file bound |
| storage.workspace_quota_bytes | size; 5 GiB | Positive; reserve pending uploads atomically |
| storage.quarantine_ttl | duration; 24h | Remove incomplete/unreferenced uploads after expiry |
| storage.antivirus_required | bool; false | If true, scanner failure blocks release; absent scanner displayed |
| storage.scanner_socket | path; unset | Required when antivirus_required=true; optional scanner cannot execute user commands |
| storage.s3_endpoint | HTTPS URL; unset | Later adapter only; explicit admin allowlist for private endpoints |
| storage.s3_bucket | string; unset | Required with s3 |
| storage.s3_region | string; unset | Explicit region/compatibility value |
| storage.s3_credentials_file | S; unset | Required unless documented workload identity supported |
| auth.registration | enum; closed | closed/invite; open registration not supported initially |
| auth.session_idle | duration; 12h | 15m–7d and <=absolute |
| auth.session_absolute | duration; 7d | 1h–30d |
| auth.reauth_window | duration; 10m | 1–30m for sensitive actions |
| auth.invitation_ttl | duration; 48h | 1h–7d; single-use |
| auth.recovery_ttl | duration; 30m | 5–60m; single-use |
| auth.argon 2_memory_kib | integer; 65536 | >=19456 and <=262144; calibrate/review before changes |
| auth.argon 2_iterations | integer; 3 | 2–10; algorithm version fixed by library policy |
| auth.argon 2_parallelism | integer; 1 | 1–4; bound total memory |
| auth.hash_concurrency | integer; 2 | 1–8; independent semaphore |
| auth.oidc_enabled | bool; false | Local login remains available for recovery |
| auth.oidc_issuer | HTTPS URL; unset | Required when enabled; discovery issuer must match |
| auth.oidc_client_id | string; unset | Required when enabled |
| auth.oidc_client_secret_file | S; unset | As required by registered confidential client |
| security.master_key_file | S; required | 32-byte generated key plus key ID; readable only by service owner |
| security.previous_key_files | S list; empty | Rotation/recovery only; remove after successful re-encryption |
| security.break_glass_ttl | duration; 15m | 5–30m; local reasoned operation only |
| limits.user_requests_per_minute | integer; 300 | Token bucket per user/workspace; burst 60 |
| limits.login_per_ip_per_minute | integer; 20 | Additional account limit 5 failed attempts/5m |
| limits.bulk_items | integer; 100 | 1–100; reflected in API capabilities |
| limits.filter_predicates | integer; 50 | 1–50; depth fixed at 4 in API v1 |
| limits.document_bytes | size; 256 KiB | 16 KiB–1 MiB; client capability response |
| realtime.enabled | bool; true | Disabling uses polling UX, does not disable durable events |
| realtime.heartbeat | duration; 15s | 5–30s; proxy idle timeout greater than 3x heartbeat |
| realtime.poll_interval | duration; 2s | 250 ms–10s; worker/listener safety polling |
| realtime.replay_days | integer; 7 | 1–30; browser cursor lifetime; required-consumer watermarks may retain rows longer |
| realtime.max_replay_events | integer; 10000 | Reset/bootstrap beyond limit |
| realtime.client_queue_events | integer; 256 | 32–1024 and separate hard 1 MiB byte ceiling |
| realtime.connections_per_session | integer; 3 | 1–10; avoid per-issue stream fan-out |
| jobs.concurrency | integer; 4 | 1–32 core I/O jobs; CPU workloads separate |
| jobs.lease_duration | duration; 60s | Renew every 20s; finite and fenced |
| jobs.max_attempts | integer; 5 | 1–10; not automatic agent-command retries |
| jobs.backoff_cap | duration; 15m | Exponential jitter, provider Retry-After may extend |
| jobs.idempotency_retention | duration; 24h | >=24h; operations reconcile beyond window |
| search.timeout | duration; 1s | 100 ms–3s; no unbounded fuzzy scan |
| search.fuzzy_enabled | bool; true | Bounded title fallback; benchmark before enable at scale |
| mail.enabled | bool; false | In-app inbox works regardless |
| mail.host / mail.port | string / integer; unset / 587 | Required when enabled; allowlist host and TLS policy |
| mail.tls_mode | enum; starttls_required | starttls_required/implicit_tls; plaintext only loopback test |
| mail.credentials_file | S; unset | Optional if relay authenticates by network policy |
| mail.from | mailbox; unset | Required when enabled; header injection rejected |
| integrations.github_enabled | bool; false | Admin installs GitHub App independently of OpenTrack login |
| integrations.github_app_id | string; unset | Required when enabled |
| integrations.github_private_key_file | S; unset | Required when enabled; never on runner |
| integrations.github_webhook_secret_file | S; unset | Required; HMAC raw bytes |
| integrations.outbound_allowed_hosts | host/port list; empty | Explicit policy for webhooks; no metadata/private fallback |
| integrations.request_timeout | duration; 15s | 1–60s; unknown external write result reconciles |
| ai.enabled | bool; false | Core feature flags and migrations do not depend on providers |
| ai.local_endpoint_allowlist | URL list; empty | Exact admin-managed private endpoint exceptions |
| ai.stream_idle_timeout | duration; 60s | 15–300s; interruption is not success |
| ai.request_max_duration | duration; 10m | 1–60m; provider endpoint limits may be lower |
| ai.catalog_ttl | duration; 5m | 30s–1h; clear on account/credential change |
| ai.disconnect_grace | duration; 30s | 5–120s; cancel interactive request with no remaining subscriber |
| ai.user_concurrency | integer; 2 | 1–8; connection and workspace budgets also apply |
| agents.enabled | bool; false | Requires isolated runner enrollment and policy verification |
| agents.max_concurrent_per_workspace | integer; 2 | 1–8; global runner capacities still apply |
| agents.max_run_duration | duration; 30m | 1m–4h; grant can only narrow |
| agents.default_memory_mib | integer; 4096 | 512–16384; schedule only on capable runner |
| agents.default_cpus | integer; 2 | 1–8; grant can only narrow |
| agents.default_disk_mib | integer; 10240 | 1024–102400; quota includes checkout/build artifacts |
| agents.approval_ttl | duration; 30m | 5m–24h; exact artifact grant |
| agents.control_listen | socket; 127.0.0.1:8443 | Separate private listener; remote runners require explicit binding/firewall and mTLS |
| agents.controller_tls_cert_file | path; unset | Required for enabled remote control listener |
| agents.controller_tls_key_file | S; unset | Required with control TLS; not the general data encryption key |
| agents.runner_ca_file | path; unset | Trust bundle for enrolled machine identities |
| agents.enrollment_ttl | duration; 5m | One-use enrollment token, workspace/host-bound |
| agents.queue_limit_per_workspace | integer; 100 | 1–1000; rejects excess with capacity reason |
| agents.max_log_bytes | size; 50 MiB | Per attempt; terminal outcome retained separately after truncation |
| agents.max_artifact_bytes | size; 100 MiB | Per artifact; expansion/tree size also bounded |
| agents.require_independent_reviewer | bool; false | Workspace may strengthen; grant cannot weaken instance policy |
| agents.runner_lease | duration; 60s | Renew 20s; revoke broker tokens on lease loss |
| agents.trusted_container_profile | bool; false | Explicit owner-only profile; no hostile multi-tenant promise |
| retention.deleted_days | integer; 30 | 1–90 before purge |
| retention.document_revision_days | integer; 90 | 1–365; current body never pruned this way |
| retention.audit_days | integer; 365 | 30–3650; legal/organizational obligations may require different value |
| retention.notification_days | integer; 90 | 7–365 |
| retention.webhook_payload_days | integer; 7 | 1–30; delivery metadata retained 30d |
| retention.agent_log_days | integer; 14 | 1–90 |
| retention.agent_artifact_days | integer; 90 | 7–365; linked PR evidence may be pinned explicitly |
| retention.ai_conversation_days | optional integer; unset | Unset means until user deletion; 1–3650 if set |
| observability.log_level | enum; info | trace/debug require explicit temporary operator action |
| observability.log_format | enum; json | json/text; native log encoding does not violate TOML configuration |
| observability.metrics_listen | socket; 127.0.0.1:9090 | Internal interface only; no public user identifiers |
| observability.otlp_endpoint | HTTPS URL; unset | Optional; no external telemetry by default |
| observability.sample_ratio | number; 0.05 | 0–1; errors retained within bounded queue |

Some constants are API invariants rather than configuration: title length 256, graph depth 8, SSE byte queue 1 MiB, API maximum page size 100, required auth audience and prohibited runner actions. Other API constants: at most 50 labels/issue,100 templates/team,100 saved searches/user/workspace,10000 graph traversal nodes,64 KiB agent log chunk,500 ms/4 KiB AI persistence batch and 5 rule-chain depth. Changing them requires a contract review, not an arbitrary runtime knob. Backup schedules and PostgreSQL server tuning belong to operator infrastructure; documented deployment profiles provide those values when implementation starts.

## Secrets, environment separation and test obligations

Use separate development/test/production database roles, origins, encryption keys and provider credentials. Refuse production mode with placeholder secrets, writable key files, wildcard credentialed CORS or a public insecure URL. Runtime config schema includes redaction metadata and generates the option reference. Tests cover precedence, unknown keys, invalid units, malformed env values, migration backup, secret exclusion from Debug/errors, interrupted atomic write, and effective-config provenance. No need to install a tool merely to validate these planning tables.

## Expansion limits and retention

Implementation settings use typed TOML keys under [planning], [retrospectives], [reports], [templates] and [retention]. Initial bounds: planning.max_scenario_items=100 (1–100), retrospectives.max_participants=200 (2–200), retrospectives.max_cards=1000 (1–1000), reports.max_window_days=366 (1–366), templates.max_steps=100 (1–100), templates.recurrence_lookahead_days=90 (1–365). Raising supported hard bounds requires measured query/concurrency/UI evidence and a contract update; configuration cannot silently disable server quotas.

Retention defaults in days: retention.private_drafts_days=30 (1–90); retention.retro_raw_days=90 (1–365); retention.retro_votes_days=30 (1–90); retention.feedback_contact_days=30 (1–365, additionally capped by recorded consent expiry); retention.archived_scenario_days=180 (1–365). Session/record policies can narrow these deployment maxima. Published summaries and attributed decisions follow normal aggregate lifecycle. Cleanup is explicit durable work with projection invalidation; operators disclose backup expiry separately.

Module visibility is a validated user/team preference in PostgreSQL, not a configuration shortcut around authorization. Disabling AI retains all non-AI expansion workflows. Provider secrets, contact fields and private retro content must not appear in configuration provenance, diagnostics or exported settings.
