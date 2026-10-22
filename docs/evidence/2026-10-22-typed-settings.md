# Typed settings startup qualification

Date: 22 October 2026. Scope: issue #4 / M01-T03, based on the clean source snapshot `c37abbe` plus this change. Verification used Rust 1.98.1 on Windows with MSVC 2022 Build Tools and on Ubuntu under WSL. All inputs in the new tests are synthetic; no deployment credentials or personal identifiers are recorded.

## Behavior

- Invalid `storage.local_root` now identifies its winning TOML location, typed environment variable, or compiled-default source without echoing its path value.
- The opened secret handle must still refer to a regular file. Linux opens with `O_NOFOLLOW | O_NONBLOCK`, then validates the handle's type, owner, permissions and size before reading. Replacing a file with a pipe between metadata inspection and opening cannot block waiting for a writer.
- Executable startup contracts run the actual server with an occupied requested port and unusable synthetic database endpoint. Invalid type, schema, storage path, database URL and setup token cases exit through configuration validation, identify the exact field, omit the canary value, and never reach database connection or listener errors.
- All four `check-config --for` targets complete without connecting or listening. `print-config` replaces runtime, migration, operator, worker and setup secrets with `<redacted>` while preserving the winning CLI provenance.
- Unsupported file versions are rejected before overrides apply. The base and overlay remain byte-for-byte unchanged and no backup/replacement files appear. The operator guide now explains same-schema upgrades, rollback boundaries and the required explicit migration behavior before a future schema version can ship.

## Executed checks

| Command | Result |
| --- | --- |
| `cargo test --locked -p opentrack-server --bin opentrack-server config::tests` on Linux | 29 passed, including precedence, unknown/type/range failures, redaction, source spans, private secret files, special files, schema/reference drift and unchanged-file upgrade rejection. |
| `cargo test --locked -p opentrack-server --test config_startup` on Linux | 3 passed, exercising the real binary across five invalid startup cases, four command targets and all five secret values. |
| `cargo test --locked -p opentrack-server --tests` on Windows | 24 configuration unit tests and 3 executable startup tests passed; Unix-only recovery contracts contain no Windows tests. |
| `cargo clippy --locked -p opentrack-server --all-targets -- -D warnings` on Linux and Windows | Passed. |
| `cargo fmt --all --check` and `git diff --check` | Passed. |

A preliminary Linux `cargo test --locked -p opentrack-server --tests` also passed the 30 unit tests and 3 startup tests, then failed the two unrelated recovery database contracts because their required test database environment was absent. The focused configuration commands above passed independently; this record does not claim those database contracts passed.

## Acceptance and limits

This covers the implemented typed loader's merge, provenance, explicit schema, validation, redaction and fail-before-listen obligations in M01-T03 / F29-A2. The F30-A2 evidence here covers configuration exports and startup diagnostics; it does not claim qualification of future audit-export endpoints or all product logging.

Only configuration schema 1 is supported. No unused catalog settings or artificial schema 2 were added. An offline migration command, its private backup, interrupted atomic replacement and recovery tests become required with an actual second schema; startup never migrates files. This change needs no database migration, changes no defaults, and adds no hot reload. Rollback restores the prior binary/configuration pair; database and blob rollback remains governed by the release's separate restore requirements. Hosted CI results remain separate from these local checks.
