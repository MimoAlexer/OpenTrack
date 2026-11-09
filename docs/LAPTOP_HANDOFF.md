# Continue OpenTrack on another machine

Fetch the repository and read the current branch before assigning more work:

```sh
git clone https://github.com/MimoAlexer/OpenTrack.git
cd OpenTrack
git switch main
git pull --ff-only origin main
git status --short
```

For an existing checkout, preserve any local edits before switching branches. Read `AGENTS.md`, `README.md`, `IMPLEMENTATION_PLAN.md` and `REQUIREMENTS_STATUS.md`. The source includes migrations through 0035. Check [PR 174](https://github.com/MimoAlexer/OpenTrack/pull/174) and [PR 175](https://github.com/MimoAlexer/OpenTrack/pull/175) for the exact integration state; a branch or passing local build alone does not establish a merged or complete product.

## Start a fresh local installation

The portable path needs Docker Engine 28 or newer and Compose 2.24.4 or newer, using Linux containers (Docker Desktop on Windows/macOS):

```sh
docker compose up --build -d --wait
docker compose ps
docker compose run --rm --no-deps -T credentials setup-token --app-secrets /run/secrets
```

When `app` is healthy, open http://localhost:8080 and enter the displayed setup token privately. Docker builds the app and initializes the database and restricted worker. `docker compose down` preserves named volumes. See [Docker setup](DOCKER.md) for HTTPS, storage and platform qualification.

For Linux source development, install the pinned Rust toolchain, Node 22, Python 3 and system prerequisites from [runtime setup](RUNTIME_SETUP.md), then run `scripts/dev.sh`. The default address is http://127.0.0.1:18970. The launcher creates private local database and setup state. Do not delete an existing database directory to resolve a startup error.

## What transfers through GitHub

Source, migrations, manifests, generated API contracts and sanitized evidence transfer through Git. Credentials, provider keys, database contents, attachments, browser sessions, build caches, private raw logs and local preservation archives do not. A fresh installation creates new private credentials and an empty database. Moving existing application data requires a separate private backup/restore transfer.

Saved discovery, filters, realtime and inbox are implemented increments. Fuzzy search, broader discovery UX, email delivery, native/physical-input and assistive-technology qualification, populated-installation recovery and remaining product milestones still need their recorded acceptance evidence. Keep incomplete issues open; verify the exact head and all required checks before merging new changes.
