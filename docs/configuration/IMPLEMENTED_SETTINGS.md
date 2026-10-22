# Server configuration

The current server implements the settings in the generated [reference](../../config/REFERENCE.md) and [JSON Schema](../../config/schema.json). Each key controls existing behavior: listener/origins/static assets, runtime/migration/operator database credentials, API pool size, setup credentials, recovery-code lifetime, password-hashing concurrency, bounded worker stages, or local immutable attachment storage. The larger [configuration catalog](../CONFIGURATION_POLICY.md) is a target; unimplemented keys are errors, even when their intended feature is disabled. Infrastructure settings require a process restart.

## Commands and sources

Run commands from the repository root during development, or use the installed `opentrack-server` binary in deployment:

```sh
cargo run --locked -p opentrack-server -- --help
cargo run --locked -p opentrack-server -- check-config --config config/opentrack.example.toml
cargo run --locked -p opentrack-server -- print-config --config config/opentrack.example.toml
cargo run --locked -p opentrack-server -- serve --config /etc/opentrack/base.toml --environment /etc/opentrack/production.toml
```

The example contains no secrets. `check-config` therefore needs runtime database and setup-token inputs supplied separately, as the development helper already does. It validates required secrets without connecting to PostgreSQL or opening a listener. `check-config --for migrate` requires only the migration credential; `check-config --for operator` requires only the separate operator credential. Migration and operator commands do not require the unrelated frontend directory. The operator role's actual permissions are checked when executing its command, not by `check-config`. `print-config` validates supplied values and file metadata but does not require missing command-specific secrets or read secret file contents. The output is a diagnostic report, not a reloadable configuration file.

Precedence, lowest to highest:

1. Compiled non-secret defaults.
2. Base TOML selected by `--config`, or the explicit compatibility selector `OPENTRACK_CONFIG` when `--config` is absent.
3. Overlay TOML selected by `--environment`.
4. Supported legacy `OPENTRACK_*` variables.
5. Typed `OPENTRACK__SECTION__KEY` variables.
6. Named CLI overrides for this process.

Files are never discovered from the working directory, a repository, or parent directories. Every selected file must contain `schema_version = 1`, including overlays. Defaults plus environment/CLI inputs work without a file; the compiled schema version then applies. Tables merge by key and arrays replace in full. Empty arrays deliberately clear additional origins. Unknown keys, wrong types, out-of-range values and unsupported versions fail with field/source diagnostics. TOML locations come from parser spans, including quoted, dotted and inline-table keys. Errors omit input values and parser excerpts.

Typed environment strings are literal text, integers are decimal integers, booleans are `true`/`false`, and arrays use TOML array syntax. For example:

```sh
OPENTRACK__SERVER__ENVIRONMENT=development \
OPENTRACK__SERVER__PUBLIC_URL=http://127.0.0.1:8080 \
OPENTRACK__SERVER__ALLOWED_ORIGINS='["http://127.0.0.1:5173"]' \
OPENTRACK__DATABASE__API_MAX_CONNECTIONS=12 \
target/debug/opentrack-server print-config --hash-concurrency 2
```

CLI overrides are `--mode`, `--listen`, `--public-url`, `--static-dir`, `--allowed-origins`, `--api-max-connections`, `--hash-concurrency`, `--recovery-ttl-minutes`, `--database-url-file`, `--migration-database-url-file`, `--operator-database-url-file`, and `--setup-token-file`. Both `--flag value` and `--flag=value` work. Repeated flags, multiple commands, generic arbitrary overrides, and literal-secret CLI arguments are rejected. No argument values are echoed in parser errors.

The separate operator credential serves recovery and global account lifecycle commands. `operator-account-disable --user UUID --reason TEXT` and `operator-account-enable --user UUID --reason TEXT` return a nonsecret state receipt and require no output file. Offline recovery has two commands: `operator-recovery-issue --user UUID --reason TEXT --output-file ABSOLUTE` and `operator-recovery-revoke --user UUID --reason TEXT`. The first writes a new private code file before database issuance; standard output contains a receipt, never the code. `auth.recovery_ttl_minutes` is an integer, default 30 with range 5–60, used only when issuing a code. `OPENTRACK__AUTH__RECOVERY_TTL_MINUTES` and `--recovery-ttl-minutes` override it using the same precedence rules. This does not introduce a general duration-string parser. See [account recovery](../ACCOUNT_SECURITY.md) for output-file requirements, uncertain outcomes, grants, audit and the account holder's completion flow.

Production is the default. It requires HTTPS origins and an absolute frontend directory when serving. Development/test modes permit HTTP only for loopback origins and normally bind only to loopback addresses. The explicit `server.local_container = true` opt-in permits a wildcard listener only in development mode with exclusively HTTP loopback public/additional origins. This is for the Docker network namespace, with host ports published only on loopback; it must not be used for a directly exposed host service. Production rejects this flag and retains strict HTTPS origins. See [Docker setup](../DOCKER.md). Origins cannot contain credentials, paths beyond `/`, queries, fragments or wildcards. The public origin is always allowed; additional origins are deduplicated with it. No trusted-proxy interpretation or hot reload is introduced by this loader.

## Secrets and compatibility

Each secret has exactly two alternative sources: an environment literal or a secret-file reference. A higher-precedence literal does not silently erase an existing file reference; configure exactly one. TOML accepts file references, never raw secret literals. Secrets have no defaults. Runtime, migration and operator PostgreSQL URLs must name a host and database; the setup token must contain at least 32 UTF-8 bytes. Generate random secrets using the existing development/deployment helpers. The operator credential is required only for operator commands, must identify a separately provisioned restricted login, and must not be mounted into or exported to the serving process.

Secret files must be absolute paths to regular files, owned by the running service UID, with exact Unix mode `0400` or `0600`. Symbolic links, executable/shared files, files over 64 KiB, invalid UTF-8 and empty contents are rejected. A trailing newline is trimmed. Metadata is checked during configuration load and again when reading; the opened handle is also checked, and Linux uses `O_NOFOLLOW`. Linux enforces these owner/mode/no-follow guarantees; native Windows uses pinned handles and private service-owned DACL validation. Docker uses Linux named volumes so these permissions do not depend on the host filesystem. Container secret mounts must satisfy the service UID and mode checks; see [runtime setup](../RUNTIME_SETUP.md) for deployment preparation.

Effective configuration redacts both secret literals and secret-file path values. Its `provenance` table identifies the winning default, selected TOML filename plus line/column, environment variable, or CLI flag for every known key; absent values have source `unset`. Operational TOML filenames remain visible as provenance. Settings and secret input structs deliberately do not implement `Debug` or `Serialize`; only the explicitly redacted report is printable. Invalid secret content, file permission errors and malformed input errors never include the secret contents.

The existing development helpers remain supported through these aliases:

| Legacy variable | Setting |
| --- | --- |
| `OPENTRACK_ENVIRONMENT` | `server.environment` |
| `OPENTRACK_LISTEN` | `server.listen` |
| `OPENTRACK_PUBLIC_URL` | `server.public_url` |
| `OPENTRACK_STATIC_DIR` | `server.static_dir` |
| `OPENTRACK_ALLOWED_ORIGINS` | `server.allowed_origins`; comma-separated, trimmed, empty entries omitted |
| `OPENTRACK_DATABASE_URL` / `OPENTRACK_DATABASE_URL_FILE` | `database.url` / `database.url_file` |
| `OPENTRACK_MIGRATION_DATABASE_URL` / `OPENTRACK_MIGRATION_DATABASE_URL_FILE` | `database.migration_url` / `database.migration_url_file` |
| `OPENTRACK_OPERATOR_DATABASE_URL` / `OPENTRACK_OPERATOR_DATABASE_URL_FILE` | `operator.database_url` / `operator.database_url_file` |
| `OPENTRACK_SETUP_TOKEN` / `OPENTRACK_SETUP_TOKEN_FILE` | `auth.setup_token` / `auth.setup_token_file` |
| `OPENTRACK_CONFIG` | Explicit base-file selector, overridden by `--config` |

Unrelated legacy helper variables, such as test database URLs and development database installation paths, are not application settings. Unknown typed `OPENTRACK__*` variables are rejected to catch misspellings. Typed aliases win over legacy aliases for the same setting.

## Schema, checks and remaining work

Regenerate both reference artifacts after changing the shared field metadata:

```sh
cargo run --locked -p opentrack-server -- config-schema > config/schema.json
cargo run --locked -p opentrack-server -- config-reference > config/REFERENCE.md
cargo test --locked -p opentrack-server
```

The generators require no configuration, secrets or database. Drift tests compare their output with the committed files. The JSON Schema describes individual TOML file structure; runtime validation also enforces cross-field origin rules, final merged values, command-specific secret requirements and filesystem permissions. Tests cover precedence, replacement arrays, exact source locations, legacy compatibility, parser/type/range failures, secret exclusivity/redaction, permissions, symlinks and post-load permission changes.

Only schema version 1 exists. No automatic rewrite, migration backup command, reload API, secret-store adapter, configurable session duration, provider configuration, S3 storage or future expansion quotas are implemented. The local storage and bounded worker settings below are implemented. Those parts of ADR-007 and M01 remain open until their runtime behavior and corresponding acceptance evidence exist.

## Local attachments and cleanup

API and worker processes require `storage.local_root`, an existing absolute directory owned by their service UID with mode 0700, outside Git checkouts and web assets. The development launcher provisions `$XDG_DATA_HOME/opentrack-server/blobs` (or `~/.local/share/opentrack-server/blobs`) when no explicit configuration/root is selected. The Compose API and worker share `/var/lib/opentrack/blobs` on the durable `blobs` volume. An existing installation must provision that private directory before starting this version; migration and offline operator commands do not need blob access. `check-config` validates configuration shape without opening blob storage; serving and worker startup validate its filesystem boundary.

The default file limit is 25 MiB, configurable from 1 to 100 MiB. The default 5 GiB workspace quota charges reservations and retained bytes until the worker confirms physical deletion. `storage.quarantine_ttl` is an integer in seconds, default 86400, bounded 60–604800; it covers unlinked reservations including ready drafts. Removing a file node preserves bytes while a current or retained document revision references it. Explicit file deletion disables downloads immediately; cleanup subsequently removes bytes and releases quota. This is distinct from removing a node from a document.

`storage.antivirus_required = true` requires `storage.scanner_socket`, an absolute ClamAV Unix socket. Any configured scanner must return a clean result; timeouts and unavailable scanners leave files quarantined for retry. With no configured optional scanner, ready metadata explicitly says `not_scanned`. Scan configuration does not install ClamAV or its signatures. All content initially downloads as an attachment; no inline active-format previews or archive extraction are enabled. The local adapter documents the [protocol and filesystem contract](../../crates/opentrack-adapters/README.md).

Backups must include PostgreSQL and matching immutable objects plus their key-lock/tombstone directory. Stop API writes and the worker while collecting a matching database/blob set; restore it into an isolated directory and verify download checksums before serving it. Do not resume cleanup against an incomplete restored blob tree. An old binary alone is not a rollback after migration 0017: restore the matching database, binary, frontend and blob state.
