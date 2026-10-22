# Executable workspace boundaries

Issue #3 / M01-T02 / F31-A4, verified 22 October 2026.

## Implemented guard

`scripts/check.sh` now runs `scripts/check-workspace-boundaries.py` and its regression suite before database setup and the existing format/Clippy/runtime gates. The guard uses locked Cargo metadata for both workspaces, without platform filtering. It checks all declared dependency kinds, including inactive target and optional edges, using package names rather than renamed import names.

The six backend crates retain their existing direction. API-to-store access is permitted only for integration tests; build dependencies cannot import product layers. Domain libraries have an explicit pure-library allowlist. Native cannot import backend crates or database drivers. Unknown product packages, unexpected local dependencies, missing/duplicate packages and non-2024 owned packages fail closed. The pinned toolchain must retain rustfmt and Clippy. No new crate or runtime dependency was added.

The current worker remains app use cases plus server supervision/composition, with a restricted database identity and no HTTP listener. The blob adapter owns filesystem operations; API and worker call app services for authorization and durable decisions. The desktop owns native transport/vault/lifecycle policy and calls the server's API; it is not an alternate persistence or authorization implementation. These module responsibilities were inspected alongside the now-enforced crate graph; the checker does not claim to prove arbitrary function-level business logic placement.

## User-audience rejection

`audience_boundary_contract.rs` uses the existing restricted runtime and migration-owner fixture in a fresh schema of an explicitly isolated test database. It verifies:

- Normal user authentication still succeeds.
- PostgreSQL rejects an `internal` session through `session_audience_shape`.
- User-session parsing/deserialization rejects the internal audience.
- After removing only that constraint in the disposable fixture, a syntactically valid, unexpired internal token belonging to an active account is rejected by both web and native user authentication.
- Neither rejected path refreshes the internal session's last-use timestamp; the valid web session continues to work.

The production SQL already explicitly filters `web`/`desktop`, so no authentication change or migration was necessary. Machine/runner ingress remains future functionality; this evidence covers the existing user-ingress boundary rather than claiming an implemented runner protocol.

## Executed checks

| Command | Result |
| --- | --- |
| `python3 scripts/test-workspace-boundaries.py` | 10 test methods pass, including all 36 forbidden first-party directions and optional/target/rename/build/dev bypass mutations |
| `python3 scripts/check-workspace-boundaries.py` | Backend and native declared graphs pass on Linux and Windows |
| `cargo fmt --all --check` | Pass |
| `cargo test -p opentrack-app --test audience_boundary_contract --locked` | 1 real PostgreSQL contract passes |
| `cargo clippy -p opentrack-app --test audience_boundary_contract --locked -- -D warnings` | Pass |
| `cargo test -p opentrack-api --test native_contract --locked` | 2 real PostgreSQL HTTP contracts pass, including native handoff/shared issue flow and browser-only audience rejection |

Database commands loaded the existing private test environment without logging credentials. An initial invocation without that environment was rejected by the fixture's required-variable check; the configured reruns above passed. Hosted workflow results are recorded by the pull request; this evidence does not substitute a local check for hosted execution. No stored-data, wire-format, migration or rollback change is introduced.
