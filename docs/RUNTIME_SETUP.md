# Development runtime and initial deployment packaging

For the portable Docker-only host installation, use the [Docker guide](DOCKER.md). The older host-provisioned Compose procedure below remains a legacy path; existing volumes and credentials are not silently transferred to the new root Compose project.

Implementation is authorized. This document describes the executable infrastructure currently present; it is not a declaration that all product milestones or release gates have passed.

## Native development

Requirements: Linux, Bash, Python 3, the repository's pinned Rust toolchain, a Node version supported by `web/package.json` (CI pins 22.22.1), and PostgreSQL 18. On an apt-based host, the helper can download and extract PostgreSQL packages into the ignored `work/postgres/runtime` directory. It does not run package installation scripts, require sudo, modify system packages, or create a system service. System shared-library dependencies must already be available; a missing-library error requires installing a suitable PostgreSQL runtime separately.

Run from the repository root:

```sh
scripts/dev.sh
```

The launcher prepares/reuses the local database, applies migrations and restricted worker grants, installs locked frontend dependencies, builds the frontend and Rust binary, and supervises the API and worker as separate child processes. It serves the app at `http://127.0.0.1:18970`. It preserves explicitly supplied environment overrides and never stops an existing listener. To use a different loopback port, set both `OPENTRACK_LISTEN=127.0.0.1:19000` and `OPENTRACK_PUBLIC_URL=http://127.0.0.1:19000` before `scripts/dev.sh`. This dedicated default avoids an unrelated service worker previously registered for port 8080 on the development host. A signal or either child exiting stops only the launcher’s own children; it does not stop the database or an unrelated listener. The API receives no worker, operator or migration credential; the worker receives no API, setup, operator or migration credential.

The generated setup token is in `work/postgres/setup-token`, readable only by its owner. Supply it through the setup screen; do not commit it or paste it into logs. `work/postgres/dev.env` holds database URLs and development settings and is also mode 0600. Its direct-server default remains port 8080; the launcher selects port 18970 after loading it. The app's supported configuration is documented by `config/opentrack.example.toml`; the full planning catalog is broader than the current executable subset.

The helper defaults to loopback port 55432 and a private Unix socket under `work/postgres/socket`. `scripts/dev-db.sh status`, `check`, `stop`, and `start` operate only on this cluster. No reset command exists. Re-running `setup` preserves existing data and credentials. Do not delete `work/postgres` as a way to fix an application error: it contains durable local development data.

Set `OPENTRACK_PG_BIN` to an existing PostgreSQL 18 `bin` directory to use a separately installed runtime. `OPENTRACK_DEV_STATE` and `OPENTRACK_DEV_DB_PORT` can select a different private cluster location and port before first setup; use the same values on later invocations. Choose a short absolute path for the state directory because Unix sockets have a path-length limit.

### Database identities

| Database | API runtime | Migration owner | Local operator | Worker |
| --- | --- | --- | --- | --- |
| `opentrack` | `opentrack` | `opentrack_migrator` | `opentrack_operator` | `opentrack_worker` |
| `opentrack_test` | `opentrack_test` | `opentrack_test_migrator` | `opentrack_test_operator` | `opentrack_test_worker` |

Development credentials use `OPENTRACK_DATABASE_URL`, `OPENTRACK_MIGRATION_DATABASE_URL`, `OPENTRACK_OPERATOR_DATABASE_URL`, and `OPENTRACK_WORKER_DATABASE_URL`; isolated tests use the corresponding `OPENTRACK_TEST_DATABASE_URL`, `OPENTRACK_TEST_MIGRATION_DATABASE_URL`, `OPENTRACK_TEST_OPERATOR_DATABASE_URL`, and `OPENTRACK_TEST_WORKER_DATABASE_URL`. All eight SQL roles are non-superusers, have no replication or role/database creation ability and no RLS bypass. Runtime roles cannot create schema objects or assume the owner role. Default privileges grant runtime DML and sequence access to new migration-owned objects. Audit entries additionally deny runtime update/delete/truncate privileges; re-running setup preserves those restrictions once the audit table exists. Migrations must enforce the application authorization model and RLS on tenant tables; SQL role separation alone does not enforce tenant privacy.

Re-running `setup` adds missing operator credentials without replacing the previous environment values, database URLs, or setup token. Operator URL files remain mode 0600. The operator has no application-data table or sequence privileges; schema-history access is limited to `version`, `success` and `checksum`. After migration, `scripts/dev-db.sh grant-operator` and `grant-test-operator` apply the reviewed recovery/account lifecycle capabilities and metadata reads after checking role attributes, ownership, and incoming/outgoing role memberships; `migrate`, `migrate-test`, and `test` perform the matching grant step automatically. The development server launcher removes operator credentials from the serving process environment.

For a native operator command, select the private URL file explicitly and create a protected output directory outside group-writable repository ancestors:

```sh
scripts/dev-db.sh migrate
source "$(scripts/dev-db.sh env-path)"
recovery_output="$(mktemp -d /tmp/opentrack-recovery.XXXXXXXX)"
target/debug/opentrack-server operator-recovery-issue \
  --operator-database-url-file "$PWD/work/postgres/operator_database-url" \
  --user 00000000-0000-0000-0000-000000000001 \
  --reason "Account holder requested password recovery" \
  --output-file "$recovery_output/recovery-code.txt"
```

Replace the example UUID with the verified intended account. The command prints only a receipt and path; hand the code to the account holder privately. See the recovery command section below for file restrictions and uncertain outcomes.

The local OS owner administers the cluster over a private Unix socket using peer authentication. TCP requires SCRAM authentication. Do not deploy these development credentials, local administration arrangements, or loopback HTTP configuration to a public instance.

```sh
# Apply migrations to the separate test database and execute workspace tests.
scripts/dev-db.sh test

# Query only through the restricted runtime roles.
scripts/dev-db.sh psql -c 'select current_user, current_database()'
scripts/dev-db.sh psql-test -c 'select current_user, current_database()'
```

The `test` helper also points ordinary runtime/migration variables to the test database to reduce accidental use of development data. It does not delete or reset the test database. Integration tests must isolate their own fixtures and handle cleanup.

The application contract suite in `crates/opentrack-app/tests/postgres_contract.rs` additionally creates a fresh UUID-named schema inside the test database. It applies real migrations there, grants the runtime role access, exercises first-run setup without touching another instance, and drops only its own schema afterward, including after an assertion failure. All test URLs must name the same database ending in `_test`, use distinct runtime/owner/operator/worker identities, and supply no preexisting search-path override. Run that suite alone with:

```sh
source "$(scripts/dev-db.sh env-path)"
cargo test -p opentrack-app --test postgres_contract -- --nocapture
```

The HTTP integration suite in `crates/opentrack-api/tests/http_contract.rs` uses the same isolated-database approach and real temporary frontend files. Requests pass through the actual Axum router using Tower's service interface; this verifies HTTP handling without binding a TCP port. It complements the application contract suite and browser verification.

## Migration compatibility at startup

Serving, foreground/once worker and operator commands validate applied migration
versions, success flags and checksums against the executable's complete embedded
up-migration set. Validation uses one read-only repeatable-read snapshot and never
creates, repairs or applies migration metadata. Missing, unreadable, dirty,
unknown or mismatched history stops startup before a listener, storage, job stage
or recovery-code output is created. Existing table/function capability checks
remain in force, including constructor privilege checks before history validation
and physical-schema probes afterward.

Apply migrations explicitly with the migration credential, then apply the matching
worker/operator grants before starting the new executable. This update changes
grants without adding a migration: worker and operator need only
`SELECT(version,success,checksum)` on `_sqlx_migrations`; other metadata columns and
writes remain denied. The development migrate/test helpers and normal worker
Compose startup apply matching grants; the legacy operator profile uses its
explicit operator-grant step. Compatibility checks use service credentials and
do not borrow migration authority.
`check-config` remains connection-free and explicit `migrate` retains its existing
upgrade behavior.

This grant-only update has no data migration to reverse. For rollback, stop/drain
services and reapply the previous release's matching worker/operator grant scripts
before starting its executable: older validators deliberately reject the new
column-read authority. The reviewed grant helpers remove stale column ACLs.
Do not edit applied history or lend migration credentials to bypass a refusal.

Stop/drain services during upgrades; this startup snapshot is not an online-DDL
coordination protocol. Unknown stored document JSON remains a separate opaque,
read-only document contract rather than a database-history mismatch.

## Verification and CI

```sh
scripts/dev-db.sh setup
scripts/check.sh
```

The check script validates formatting, treats Clippy warnings as errors, applies migrations to the isolated test database, runs the complete Rust workspace tests, installs frontend dependencies from the npm lockfile, runs frontend tests, and builds the production frontend. Missing test credentials are loaded from the private local env file. Existing explicit test URLs are preserved, and all four must point to the same `_test` database with distinct runtime, migration, operator, and worker identities. The script also checks additive credential upgrades and redaction of malformed credential inputs. Additional Cargo test arguments may follow the script, such as `scripts/check.sh -- --nocapture`.

`.github/workflows/ci.yml` runs that same entry point on Ubuntu 24.04 with a digest-pinned official PostgreSQL 18.6 service. `scripts/ci-db.py` creates restricted runtime, migration, operator, and worker identities with independently generated credentials; only the bootstrap step receives the ephemeral service administrator credential. API/application tests always connect with the restricted runtime identity. The script exports generated URLs via GitHub's environment file, masks them in Actions logs, and never uses a deployment secret.

Checkout and Node setup actions are pinned to full commit SHAs verified against the official [checkout](https://github.com/actions/checkout) and [setup-node](https://github.com/actions/setup-node) repositories on 17 October 2026. The Rust toolchain comes from `rust-toolchain.toml`; Node is pinned to 22.22.1. The workflow has read-only repository permissions and does not publish artifacts or deploy. Workflow source and native checks are evidence available locally; no hosted CI execution is claimed.

## Compose packaging

`deploy/Dockerfile` builds the locked Rust server and frontend. Compose requires an explicit nonroot service UID/GID matching the owner of the provisioned application secret files; the image build creates that account and owns its data directory accordingly. `deploy/compose.yaml` defines PostgreSQL, a one-shot migration, worker provisioning/grants, the app, the worker, and a Caddy TLS proxy, plus a separate optional `operator` profile. PostgreSQL exposes no host port, and the app has a read-only root filesystem and no Linux capabilities. The app receives only its runtime URL and setup token; the migrator receives only the migration URL. The worker receives only its own worker URL and exposes no port. The operator URL is never mounted into the app, worker or migrator. The database network is internal.

For a new installation, run the generator as the intended nonroot service account, then pass its generated environment file to every Compose command:

```sh
python3 scripts/deploy-secrets.py
python3 scripts/deploy-secrets.py --check
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml config --quiet
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml build
# Validate actual mounted ownership/modes before starting services.
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml run --rm --no-deps app check-config
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml run --rm --no-deps migrate check-config --for migrate
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml run --rm --no-deps worker check-config --for worker
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml up -d
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml ps
# Explicitly provision/verify the separate operator, then grant the reviewed operator functions.
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml --profile operator run --rm --no-deps operator-provision
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml --profile operator run --rm --no-deps operator-grant
```

Fresh generation creates ten secret files and `compose.env` in a new mode-0700 directory, refusing an existing directory. `--directory /absolute/private/path` selects another location; its generated `compose.env` records that path and the current nonroot UID/GID. Runtime/migration/operator/worker URL files, setup token and `compose.env` use mode 0600. The five database password files use 0444 inside the private host directory because PostgreSQL runs under its own UID. They are mounted only into the database and explicit PostgreSQL-client provisioning service, never the serving application. The app loader still requires its own secret files to be regular, non-symlink, owned by the service UID, and exactly 0400 or 0600. The helper's `--check` is read-only and rejects mismatched operator or worker password/URL pairs without printing secret values.

File-backed Compose secrets are bind mounts: its long-syntax `uid`, `gid` and `mode` settings do not remap these files. The matching host ownership/service account is deliberate, as described in the [official Compose secret reference](https://docs.docker.com/reference/compose-file/services/#secrets). The in-container `check-config` commands validate the actual mapping for the chosen runtime, including installations using user namespaces.

For credentials generated by the earlier helper, retain their values and database volume. Explicitly provision owner-only copies under the selected service account and create the matching `compose.env`; do not regenerate passwords for an existing database. Neither this helper nor validation changes existing ownership or permissions. Changing the service UID/GID of an existing deployment also requires an explicit data-volume ownership migration. No container entrypoint performs privileged ownership repairs.

### Add operator recovery to an existing volume

For an existing directory already using the owner-only URL files and generated `compose.env`, add only the operator credential pair:

```sh
python3 scripts/deploy-secrets.py --add-operator
python3 scripts/deploy-secrets.py --check
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml build
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml up -d --wait database
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml run --rm --no-deps migrate
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml --profile operator run --rm --no-deps operator-provision
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml --profile operator run --rm --no-deps operator-grant
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml up -d
```

`--add-operator` preserves every existing file, including all passwords, setup token and `compose.env`. It creates only `operator-password` and `operator-database-url` when both are absent. A complete matching pair is an unchanged success; a partial pair, mismatched values, unsafe modes or invalid existing directory fails without rewriting anything. An interrupted partial creation needs explicit reconciliation. Never delete the database volume or regenerate prior credentials to make an upgrade proceed.

`operator-provision` uses the pinned official PostgreSQL client image and only the administrative PostgreSQL password plus the new operator password. On an existing volume, it explicitly creates the separate `opentrack_operator` login if absent. If the role exists, it first verifies that the supplied password works, then rejects ownership, role memberships or elevated attributes. It never resets an existing password. Fresh database initialization creates the same restricted role. The role gets only database `CONNECT` and schema `USAGE` at this stage.

`operator-grant` uses the same client image with only the migration URL and the reviewed `grant-operator.sh`/SQL. Run it after migrations: it grants the reviewed definer functions and read-only `version`, `success`, `checksum` metadata after validating the role and removing stale table/sequence/function grants. Other schema-history columns and all direct metadata writes stay denied. The helper's URL-file input supports the generated simple host/identifier URI with a 64-character hexadecimal password; it parses this into separate libpq environment fields without putting credentials in command arguments. Custom connections can supply explicit libpq `PG*` environment settings instead of that file input. These two provisioning services are absent from normal `compose up`. Use the explicit commands above; the normal app needs neither administrator nor operator credentials.

Account disable and re-enable use the same separate operator service and credential. They need no output file; see the [account lifecycle commands](ACCOUNT_SECURITY.md#disable-or-re-enable-a-global-account). Existing deployments must apply migration 0007 and rerun `operator-grant` before using this binary’s operator commands.

### Add the worker to an existing volume

For a previously generated deployment secret directory, add only its worker pair before starting the updated Compose services:

```sh
python3 scripts/deploy-secrets.py --add-worker
python3 scripts/deploy-secrets.py --check
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml config --quiet
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml build
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml up -d
```

`--add-worker` preserves existing credentials, setup token and `compose.env`. It creates only `worker-password` and `worker-database-url` when both are absent; a matching complete pair is unchanged. Partial pairs, mismatched values, unsafe modes and symlinks fail without rewriting prior files. If the deployment predates operator credentials too, add that pair separately with `--add-operator` before the full `--check`.

Normal Compose startup now runs `worker-provision` with only the PostgreSQL administrator and worker passwords, then `worker-grant` with only the migration URL after migrations. Provisioning verifies an existing worker password instead of resetting it. Grants revoke stale direct and column privileges, then apply the exact scoped tables/columns and two reviewed definer functions. The worker cannot read authentication records or issue titles/descriptions, invoke recovery functions, own schema objects, create roles or bypass RLS. Both API and worker startup reject role capability mixtures. A grant failure prevents worker startup while leaving the failed provisioning receipt available for inspection.

For native development, setup adds the worker credentials without replacing earlier values. `scripts/dev-db.sh grant-worker` and `grant-test-worker` apply the matching grants after migrations 0009–0010. The normal `migrate`/`migrate-test`/`test` helpers include those grants. Run `scripts/dev-worker.sh --once` to attempt one bounded batch using only the worker credential; omit `--once` for a foreground worker. The regular `scripts/dev.sh` already supervises a worker alongside the API. `check-config --for worker` validates worker configuration without needing public URL or setup credentials; `worker` then checks its actual database permissions. A browser logout does not cancel committed work. Lost current authority blocks the job and the workflow UI shows its saved state for an authorized manager to resume.

### Run a recovery command

Create a private host output directory as the same OS account used to generate the service UID/GID, then mount it only into the operator service:

```sh
export OPENTRACK_OPERATOR_OUTPUT_DIR="$(mktemp -d /tmp/opentrack-recovery.XXXXXXXX)"
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml --profile operator run --rm --no-deps operator check-config --for operator
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml --profile operator run --rm --no-deps operator \
  operator-recovery-issue --user 00000000-0000-0000-0000-000000000001 \
  --reason "Account holder requested password recovery" --output-file /operator-output/recovery-code.txt
docker compose --env-file deploy/secrets/compose.env -f deploy/compose.yaml --profile operator run --rm --no-deps operator \
  operator-recovery-revoke --user 00000000-0000-0000-0000-000000000001 \
  --reason "Pending recovery code is no longer needed"
```

Replace the example UUID with the intended account and run revocation only when intended. The operator service has only the operator URL, no exposed port and no runtime, migration or PostgreSQL administrator secret. It runs under the configured nonroot UID/GID. Compose refuses to create a missing output bind source. The existing output directory must be owned by that UID and mode 0700; the CLI checks protected ancestors through pinned directory handles and writes a new 0600 file. The mount appears as `/operator-output`; the code remains in the private host directory after the one-shot container exits. Do not use a group-writable repository directory for native CLI output. The native equivalent also uses `mktemp -d /tmp/...`.

The default lifetime is 30 minutes; pass `--recovery-ttl-minutes 5` through `60` when issuing to choose another supported lifetime. Standard output contains a receipt and file path, never the code. Hand the file contents to the account holder privately, then remove the local file/directory when no longer needed. A database error after file persistence reports an uncertain outcome and preserves the file; reissue to a new file or explicitly revoke rather than assuming the first code is inactive. The account holder enters the code at `/recover`, sets a password, and signs in afterward. This changes no memberships and automatically sends no email. See [account security](ACCOUNT_SECURITY.md) for audit and authority boundaries, including the unresolved second-workspace-owner/global-account recovery requirement.

The default address is `https://localhost:8443`, bound only to loopback. Caddy uses its local certificate authority for `localhost`, so a browser will require trusting that local CA before normal use. The setup token is in the selected secret directory. Keep that directory private and never copy it into a release artifact.

An operator configuring a real HTTPS domain must explicitly supply matching `OPENTRACK_DOMAIN` and `OPENTRACK_PUBLIC_URL`, the desired `OPENTRACK_HTTPS_PORT`, and an intended `OPENTRACK_BIND_ADDRESS`. The loopback default must not be interpreted as a public deployment. Automated public publication is not part of setup.

PostgreSQL 18's official image uses versioned storage beneath `/var/lib/postgresql`; this Compose file mounts the parent directory as required by the [official image documentation](https://hub.docker.com/_/postgres). Existing database volumes are not reinitialized by changing secrets. Credential rotation therefore needs coordinated SQL changes and secret replacement, not rerunning the initialization script.

Official base-image manifest digests were resolved from Docker Hub on 17 October 2026 and pinned in the Dockerfile/Compose file. Cargo and npm lockfiles constrain application dependencies. The runtime image's apt package installation still uses the distribution repository at build time, so bit-for-bit reproducible images are not claimed. Future release work must publish the resulting app-image digest and build provenance.

The worker now performs durable workflow status replacement. Other jobs, full storage/recovery operations, upgrade compatibility and release signing remain tied to their implementation milestones. They must be implemented and exercised before claiming the full deployment contract is complete.

## Evidence recorded on 17 October 2026

- Ubuntu 26.04 amd64 package binaries for PostgreSQL 18.6 were downloaded using apt's configured repository metadata and extracted locally. No system PostgreSQL installation or sudo action was performed.
- A real PostgreSQL 18.6 cluster started on `127.0.0.1:55432`; development and test databases accepted authenticated connections as their separate runtime identities.
- SQL checks verified non-superuser/no-bypass/no-create-role/no-create-database flags, lack of schema creation rights and absence of database-owner membership for both runtime roles.
- A temporary migration-owned table verified automatic grants, successful scoped runtime writes, hidden rows without RLS scope, visibility with the matching scope, and denied runtime schema creation. The probe table was removed afterward.
- Shell syntax checks passed. Compose 5.6.0 was downloaded from its [official release](https://github.com/docker/compose/releases/tag/v5.6.0), checked against the published SHA-256 manifest, and accepted the Compose configuration with `config --quiet`.
- The serialized application integration test passed against real PostgreSQL in 9.71 seconds, with zero failures or ignored tests. It verified setup-token rejection and one-time setup; actual Argon2 login; logout and denial to a cached revoked actor; cross-workspace denial; private-team exclusion even for workspace owners; viewer write denial; fail-closed and forced owner RLS; sequential/concurrent idempotent creates; changed-payload conflict; stale-version conflict; unknown-field atomic rejection; matching activity/event counts; filtered pagination with identical timestamps; archive filtering/edit denial/restoration; real audit update/delete/truncate denials; and denied access/retry after private-team membership revocation. Its temporary schema was removed successfully. This establishes the tested identity/issue contracts, not completion of later product features.
- The HTTP contract suite passed in 3.38 seconds. It exercised secure session cookies, missing/incorrect origin and CSRF rejection, unauthenticated requests, strict JSON fields, idempotency, Location/ETag headers, missing/stale update preconditions, revocation, no-store API responses, security headers, HTML navigation fallback, API/asset 404s and immutable caching for hashed frontend assets.
- The CI database initializer was executed against a new native `opentrack_ci_native_test` database. `scripts/check.sh` then passed using its separate generated runtime/migration identities: 10 Rust tests (including two PostgreSQL contract suites), 20 frontend tests across five files, formatting, workspace Clippy with warnings denied, TypeScript checking and Vite production build. This was a native rehearsal of the workflow entry point; GitHub-hosted CI was not executed.
- The workflow passed actionlint 1.7.12 after its downloaded release checksum was verified. Shell/Python syntax checks and `git diff --check` also passed.
- `scripts/dev.sh` correctly refused the occupied default port without stopping the existing app. An explicit port-18971 override then built and started successfully, served real HTTP readiness and SPA HTML, and shut down on Ctrl+C. The existing port-18970 app remained healthy. The temporary launcher process was stopped; the shared development database remains running.
- Configuration-loader compatibility was checked using newly generated scratch secrets owned by UID/GID 1000:1000. Fresh provisioning, read-only validation, rejection of unsafe modes/symlinks/overwrites, and preservation of all existing deployment credentials passed. Compose 5.6.0 rendered matching nonroot runtime/build identities and separate app/migrator secrets. The real native binary accepted production `check-config` for serving and migration with their respective secret files, rejected a mode-0444 app credential with redacted diagnostics, and opened no connections or listeners. These checks do not establish container mount behavior.
- Recovery deployment wiring passed ten scratch provisioning/render checks: fresh operator credentials, preservation of every prior secret/environment hash and mode during addition, repeated-add no-op, overwrite/partial-pair/mismatch/unsafe-mode rejection, and rendered operator-profile secret separation/output binding. Existing deployment files were unchanged. Shell/Python syntax and Compose rendering passed; the new provisioning container and Docker secret/output mounts were not executed on this host.
- Docker daemon/container execution has not been verified on this host. Compose syntax validation and native database verification do not establish that container builds or startup work. Browser workflows and full database/blob backup restoration need their own current execution evidence.

PostgreSQL installation and authentication behavior follow the [official PostgreSQL 18 documentation](https://www.postgresql.org/docs/18/installation.html). The current package extraction is a development convenience rather than a supported host package installation.
