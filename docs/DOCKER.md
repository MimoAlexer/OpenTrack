# Run OpenTrack with Docker

The Docker path runs OpenTrack, PostgreSQL and its restricted worker as Linux containers. The source build and database setup happen inside Docker. The host needs Docker, its Compose plugin, a checkout of this repository and a browser; it does not need Rust, Node, Python or a PostgreSQL installation.

This is a single-machine installation of the implemented product. The [requirements ledger](REQUIREMENTS_STATUS.md) still governs unfinished features. Container packaging does not establish native desktop qualification or every complete-product release gate.

## Prerequisites and platform scope

Use Docker Compose **2.24.4 or newer** and a running, supported Docker Engine **28 or newer**. Check the active installation:

```sh
docker version
docker compose version
docker info --format '{{.OSType}}/{{.Architecture}}'
```

The server OS reported by Docker must be `linux`. Architectures targeted by this path are `amd64`/`x86_64` and `arm64`/`aarch64`. See [Docker acceptance evidence](evidence/2026-10-20-docker.md) for executed results; a documented target is not a claim that every host OS has been tested.

Engine 28 is the minimum for the default localhost-only publishing contract. Docker documents that older versions can expose a localhost-published port to other hosts on the same local network segment. [Docker port-publishing documentation](https://docs.docker.com/engine/network/port-publishing/).

| Host | Docker prerequisite | Qualification boundary |
| --- | --- | --- |
| Linux x86-64 or ARM64 | Supported [Docker Engine installation](https://docs.docker.com/engine/install/) and the [Compose plugin](https://docs.docker.com/compose/install/linux/) | The container smoke suite must pass on each native architecture. |
| Windows | Supported [Docker Desktop installation](https://docs.docker.com/desktop/setup/install/windows-install/), hardware virtualization and a Linux-container backend such as WSL 2 | Windows containers cannot run these images. Docker Desktop/WSL host execution requires its own recorded run. |
| macOS, Intel or Apple silicon | Matching [Docker Desktop installation](https://docs.docker.com/desktop/setup/install/mac-install/) on a supported macOS version | Linux ARM64 CI is useful architecture evidence, but it does not certify Docker Desktop on macOS. |

Docker Desktop includes the Engine, CLI and Compose plugin. Its current Windows WSL backend requires WSL 2.1.5 or newer and virtualization support; follow the linked vendor requirements for supported OS versions. Docker Desktop is not supported on Windows Server. A Linux VM with Docker Engine is an alternative deployment host. [Docker's Compose installation overview](https://docs.docker.com/compose/install/) explains the supported installation choices.

Allow Docker enough memory and disk space for a Rust release build, downloaded image layers, database growth and attachment storage. The first build needs network access to obtain pinned base images and dependencies. Once built and initialized, core local use does not require AI, SMTP or a paid cloud service. This does not claim that a fresh offline image build has been qualified.

## First start

Run these single-line commands from the repository root in PowerShell, a Linux shell or Terminal on macOS:

```sh
docker compose up --build -d --wait
docker compose ps
docker compose run --rm --no-deps -T credentials setup-token --app-secrets /run/secrets
```

Wait for `app` to report healthy. Open **http://localhost:8080**, paste the token into the setup form, then create the owner account, workspace and team. The setup token belongs in that form, not a browser URL. Retrieving it is an explicit operator action; the normal startup logs do not need to contain credentials.

The same commands reuse an existing installation. The initializer retains existing credentials, and migrations run before the app and worker. The explicitly selected `credentials` helper runs even though it belongs to the optional `tools` profile. Do not delete a credential volume to reset a password: the database retains its original role passwords. [Account recovery](ACCOUNT_SECURITY.md) is a separate operator operation.

The browser URL is `localhost`, even though the listener inside the container binds its private container interface. The host mapping is fixed to `127.0.0.1`. The explicit `server.local_container` option permits this only with development mode and loopback HTTP public URLs/origins. It is a container-local startup setting; do not copy it into a directly exposed host process or use it for remote access.

## Use another local port

Create a text file named `.env` at the repository root:

```dotenv
OPENTRACK_HTTP_PORT=18080
```

Then run `docker compose up --build -d --wait` again and open **http://localhost:18080**. Keep `.env` local; the repository ignores it. The Compose configuration uses the same port in its published mapping and advertised public URL. Changing the port does not migrate or reset the database.

Use a stable Compose project name for a persistent installation. Changing `--project-name` or `COMPOSE_PROJECT_NAME` selects different named volumes and can make the application look like a fresh installation. Inspect the existing projects and volumes before concluding that data disappeared:

```sh
docker compose ls
docker volume ls
```

## HTTPS and remote access

The default HTTP endpoint is for use on this computer. Use the supplied HTTPS Compose configuration for remote access; it disables `server.local_container`, selects production validation and places the app behind the TLS proxy. Match the advertised public URL to the externally reachable HTTPS address and preserve strict Origin/CSRF checks.

For a public hostname, create `.env` with the intended domain and matching external HTTPS URL. For example:

```dotenv
OPENTRACK_DOMAIN=tasks.example.com
OPENTRACK_PUBLIC_URL=https://tasks.example.com
OPENTRACK_HTTPS_PORT=443
OPENTRACK_HTTPS_BIND_ADDRESS=0.0.0.0
```

Use your own hostname and ensure DNS and inbound TCP 443 reach this Docker host. Then use both Compose files:

```sh
docker compose -f compose.yaml -f compose.https.yaml --profile https up --build -d --wait
```

Use the same `-f compose.yaml -f compose.https.yaml --profile https` options for subsequent `ps`, `logs`, `stop` and `down` commands for this deployment. The override removes the direct HTTP app port rather than publishing both paths. Caddy listens on unprivileged port 8443 inside its container; the explicit host mapping above publishes port 443. Certificate issuance still depends on the hostname and challenge traffic reaching the proxy. [Caddy's automatic HTTPS requirements](https://caddyserver.com/docs/automatic-https).

For optional local HTTPS, the [example environment](../deploy/https.env.example) keeps the endpoint at `https://localhost:8443` on loopback:

```sh
docker compose --env-file deploy/https.env.example -f compose.yaml -f compose.https.yaml --profile https up --build -d --wait
docker compose --env-file deploy/https.env.example -f compose.yaml -f compose.https.yaml --profile https cp proxy:/data/caddy/pki/authorities/local/root.crt ./opentrack-local-ca.crt
```

The second command copies only this local proxy's public CA certificate. Trust that certificate through the intended OS/browser certificate store if you choose to use local HTTPS; the application does not install it for you. [Caddy documents the Docker certificate path and OS-specific trust steps](https://caddyserver.com/docs/running#local-https-with-docker). Normal local onboarding uses the HTTP path and does not require this optional CA setup.

Local HTTP readiness is not proof of a working public certificate or external network route. A self-signed local certificate must not be described as a publicly trusted deployment.

## Stop, restart and inspect

```sh
docker compose logs --tail 100 app worker migrate
docker compose restart app worker
docker compose stop
docker compose up -d
```

To remove stopped containers and this project's network while retaining persistent data:

```sh
docker compose down
```

Do not add `--volumes`/`-v` to a normal shutdown: it removes this installation's named volumes, including database data, attachments and credentials. Likewise, pruning Docker volumes is not routine OpenTrack maintenance. The [Docker volume lifecycle](https://docs.docker.com/engine/storage/volumes/) explains how persistence differs from a container's writable layer.

## Storage, credentials and backups

The database volume is mounted at `/var/lib/postgresql`; PostgreSQL 18 stores its cluster beneath `/var/lib/postgresql/18/docker`. Do not replace this with an older PostgreSQL 17 mount recipe targeting `/var/lib/postgresql/data`. The [official PostgreSQL image documentation](https://hub.docker.com/_/postgres) records this version-specific change.

Attachments use a separate persistent volume. The credential initializer creates separate named volumes for database initialization, API, migration, worker and operator credentials. Application services receive only their own credential volume, mounted read-only. Application secret files use the fixed service identity `10001:10001`; database initialization credentials use the database identity `999:999`. Ownership and private permissions are established inside Docker rather than inferred from Windows ACLs or a host user's numeric UID.

This avoids a portability trap in file-backed Compose secrets: their `uid`, `gid` and `mode` declarations do not remap a host bind mount. [Docker documents that limitation](https://docs.docker.com/reference/compose-file/services/#secrets). Do not solve a credential permission error by making secrets world-readable or giving the serving process migration/operator credentials.

Named volumes survive container replacement, but they are not backups. Preserve the complete installation inventory, using the actual volume names for the selected Compose project:

| Compose volume | Must be preserved |
| --- | --- |
| `database` | PostgreSQL cluster, including all workspace data and database role credentials |
| `blobs` | Immutable attachment files and their storage state |
| `database_secrets` | Database initialization credentials and the `.opentrack-secrets.json` consistency manifest |
| `api_secrets` | Runtime database URL and setup token |
| `migration_secrets` | Migration-owner database URL |
| `worker_secrets` | Restricted worker database URL |
| `operator_secrets` | Operator database URL |
| `operator_output` when the operator profile is used | Private recovery-code output files; retain their private permissions and do not treat them as ordinary support logs |
| `proxy_data`, `proxy_config` when HTTPS is enabled | Proxy certificates, local CA material and persistent proxy state |

For a cold backup, **stop the complete stack first**, including PostgreSQL and the worker, and verify that no service is still writing. Preserve every volume above as a matched set with ownership/permissions, the Compose environment/configuration and the exact application image/source revision. Keep the backup private because it contains workspace content and authentication material. Restore all volumes together into an isolated project using the matching image, then verify accounts, issue data, attachment hashes, credential access and denied private content before allowing normal traffic.

The initializer rejects missing, partial, corrupt or mismatched credential state. If an existing database remains but its secret manifest/volumes are missing, it refuses to mint unrelated replacement credentials. Preserve the remaining volumes and restore the original matched set or perform an explicitly reviewed credential reconciliation.

No backup/restore helper is claimed by this increment. A raw `pg_dump` command is not a complete backup: restricted roles and FORCE RLS must be handled correctly, attachment/secret volumes must match, and binary dumps must not be corrupted by shell redirection. Follow the [backup and restoration contract](DEPLOYMENT_ARCHITECTURE.md#backups-and-restoration). The container recreation smoke check proves persistence; it does not complete the separate M08 recovery rehearsal.

## Optional operator commands

Normal startup does not launch the operator services. Selecting the `operator` profile provisions its restricted database capabilities and prepares the separate `operator_output` volume as a private `10001:10001` directory with mode `0700`:

```sh
docker compose --profile operator run --rm -T operator --help
```

Use the operations and authority rules in [account security](ACCOUNT_SECURITY.md). For a deliberately requested password recovery, replace `USER_UUID` with the intended account's public UUID and choose a new output filename:

```sh
docker compose --profile operator run --rm -T operator operator-recovery-issue --user USER_UUID --reason "Account holder requested password recovery" --output-file /operator-output/recovery-request-1.txt
docker compose --profile operator run --rm --no-deps -T --entrypoint /bin/cat operator /operator-output/recovery-request-1.txt
```

The first command writes the one-use code to the private volume and prints only a receipt. The second explicitly displays that code for entry into the recovery form. Do not publish the code, put it in a URL or include the output volume in public diagnostic artifacts. Existing output files are never overwritten. The output initializer preserves an already private directory and refuses unexpected ownership or nonempty unsafe state; it does not repair permissions by broadening access.

## Troubleshooting

| Symptom | Next action |
| --- | --- |
| Docker cannot connect to its daemon | Start Docker Desktop or the Docker Engine service, then run `docker version` and confirm that both client and server respond. |
| Docker reports `windows` as its server OS | Switch Docker Desktop to Linux containers. |
| Port 8080 is occupied | Set `OPENTRACK_HTTP_PORT` in `.env` and use the matching `localhost` URL. |
| Build stops for memory or disk exhaustion | Check Docker's resource allocation and disk usage. Preserve named data volumes while addressing build-cache capacity. |
| Initialization or migration exits unsuccessfully | Read `docker compose logs --tail 100`; dependent app/worker startup must stop rather than serving against a partial schema or mismatched credentials. Preserve volumes while diagnosing. |
| Browser login or writes fail an Origin check | Use the documented `localhost` URL and configured port. Do not disable CSRF or substitute a LAN address into the local HTTP configuration. |
| A previously populated installation asks for setup again | Check the Docker context and Compose project name; confirm that the intended database and credential volumes are attached. |
| An older `deploy/compose.yaml` deployment already exists | Keep it intact. The new root Compose topology does not automatically import its host-bound credentials or existing volumes. Rehearse a deliberate migration/restore before switching it. |

## Qualification and CI

The Docker smoke workflow is intended to build and run the real stack on native Linux AMD64 and ARM64, then exercise setup, issue persistence, an attachment round trip, restart/recreation and graceful shutdown. Exact run URLs, revisions, results and remaining gaps belong in [the Docker evidence record](evidence/2026-10-20-docker.md).

GitHub currently lists `ubuntu-24.04` and `ubuntu-24.04-arm` for both public and private repositories. Its [Ubuntu ARM64 image inventory](https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2404-Arm64-Readme.md) includes Docker and Compose. Use those VM runners for this suite, not `ubuntu-slim`, whose container environment does not support Docker-in-Docker. Repository entitlement and an actual completed run still determine execution. [GitHub-hosted runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

Primary-source prerequisites above were checked on 20 October 2026. They describe Docker/GitHub capabilities; only the evidence record establishes which OpenTrack journeys have actually passed.
