# Configuration checkpoint — 17 October 2026

This increment follows identity commit `1c83904`. It implements configuration for the behavior that currently exists; the complete configuration catalog and M01 release gates remain unfinished.

## Implemented behavior

Explicit schema-version-1 base and overlay TOML, legacy and typed environment overrides, and named CLI overrides have deterministic precedence. No working-directory configuration is discovered implicitly. Unknown fields and unsupported versions fail with field/source diagnostics. Defaults select production; secret inputs have no defaults. Literal secrets are accepted only through environment variables, and conflicting literal/file sources are rejected.

Secret files must be absolute, regular, non-symlink files owned by the service UID with mode 0400 or 0600. The Linux loader checks metadata again on the opened handle. Effective configuration redacts secret values and secret-file paths while preserving source provenance. `check-config` validates without opening a database connection or listener; its migration mode requires only migration credentials. Generated JSON Schema and the reference describe only implemented keys and are checked for drift.

Compose explicitly uses the nonroot UID/GID that owns newly provisioned app secrets. Its migrator receives only the migration credential. The provisioning helper refuses existing directories, unsafe permissions and symbolic links; its check mode is read-only. Existing local deployment credentials were preserved.

## Executed checks

- Server configuration suite: 17 unit tests passed, covering precedence, strict fields/types/ranges, TOML source locations, source conflicts, redaction, file safety, CLI behavior and generated reference/schema consistency.
- Server all-target Clippy with warnings denied passed; `git diff --check` passed.
- Native binary: development environment compatibility, example-file validation, production service-owned file secrets, and `check-config --for migrate` passed. Migration validation passed without runtime/setup secrets or a frontend directory.
- Native production loader rejected a mode-0444 app credential with redacted diagnostics. Parsed `print-config` output contained none of the actual runtime, migration or setup secret values. A deliberately unselected configuration in a temporary working directory was ignored.
- Fresh deployment helper provisioning/checks and unsafe-mode, symlink and overwrite refusal passed. Existing credentials were unchanged.
- Docker Compose v5.6.0 rendered configuration with matching service/build UID/GID 1000:1000 and separate app/migration secret mounts. No container was built or started: this environment has no Docker daemon. This is configuration evidence, not packaged runtime evidence.

Targeted logs are in ignored `work/config-deploy-checks.md` and `work/compose-config-identity.json`. The identity full-suite results are recorded separately; the next shared full-suite run follows the account-session increment, whose source work was already underway during these targeted configuration checks.

## Operations and remaining limits

See [implemented settings](../configuration/IMPLEMENTED_SETTINGS.md), [generated reference](../../config/REFERENCE.md) and [runtime setup](../RUNTIME_SETUP.md). This increment changes no database migration. Configuration changes require a process restart. Only schema version 1 exists; a future version still needs an explicit migration path. The larger configuration catalog, Windows ACL validation, hosted CI and actual Compose execution remain unverified or unimplemented.
