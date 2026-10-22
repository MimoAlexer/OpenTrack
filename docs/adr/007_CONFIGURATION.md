# ADR-007 Typed TOML-first configuration

Date:2026-10-17. Status: Accepted; the server loader and currently wired settings are implemented. The complete option catalog and configuration migration remain pending.

## Problem

Self-host operators need predictable configuration without secret leaks, ambiguous precedence or unnecessary format proliferation.

## Options and tradeoffs

One universal format would fight mandatory tooling. YAML everywhere violates the requested policy. Typed TOML plus native mandatory formats keeps application semantics consistent while acknowledging ecosystem constraints.

## Decision

Application/operator settings and Rust/Tauri/tool pins use TOML when supported. JSON/SQL/TypeScript/CSS use native roles; YAML only Compose/Actions where required. Merge order, source provenance, schema version, explicit migration and secret references are normative.

## Consequences and risks

Configuration can be as sensitive as code; unknown keys and insecure combinations fail startup. User settings are validated DB data, not a generic server config override API. Initial reload requires restart.

## Reconsideration and validation

Revisit reload only for proven-safe fields; adding a format requires documented unsupported-TOML reason. M01 precedence/redaction/schema tests. [Tauri TOML support](https://v2.tauri.app/reference/config/).

See [canonical specification](../CONFIGURATION_POLICY.md).

## Implementation boundary

The server accepts explicitly selected base/overlay TOML, typed environment overrides, named CLI overrides, and compatibility aliases used by the local development helpers. It does not discover configuration from the working directory or parent repositories. `print-config` emits redacted effective TOML with the winning source for every known key; `check-config` validates without a database connection or listener. `config-schema` and `config-reference` generate checked-in artifacts from shared field metadata. Unknown fields, unsupported schema versions, ambiguous secret inputs, and unsafe secret-file permissions fail before serving. [Implemented settings](../configuration/IMPLEMENTED_SETTINGS.md) records exact commands, platform restrictions, and pending work; this does not close all M01 acceptance criteria.
