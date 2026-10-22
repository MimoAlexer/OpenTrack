# Architecture Decision Records

Date:2026-10-17. Decisions 001–014 are selected Phase 0 design baselines, not implementation or maintainer approval records. ADR 015 remains proposed. Supersede a decision with a new ADR; preserve its rationale/history.

| ADR | Decision | Canonical detail |
| --- | --- | --- |
| [ADR-001](001_MODULAR_MONOLITH.md) | Rust modular monolith | [Specification](../ARCHITECTURE.md) |
| [ADR-002](002_FRONTEND.md) | Static React client with Vite | [Specification](../TECHNOLOGY_DECISIONS.md) |
| [ADR-003](003_POSTGRES_TENANCY.md) | PostgreSQL and workspace isolation | [Specification](../DATABASE_DESIGN.md) |
| [ADR-004](004_REALTIME.md) | SSE with durable commit-ordered replay | [Specification](../REALTIME_ARCHITECTURE.md) |
| [ADR-005](005_JOBS.md) | PostgreSQL jobs, leases and receipts | [Specification](../ARCHITECTURE.md) |
| [ADR-006](006_SEARCH.md) | PostgreSQL search before Tantivy | [Specification](../PERFORMANCE_STRATEGY.md) |
| [ADR-007](007_CONFIGURATION.md) | Typed TOML-first configuration | [Specification](../CONFIGURATION_POLICY.md) |
| [ADR-008](008_AUTH.md) | Local identity plus optional OIDC | [Specification](../SECURITY_ARCHITECTURE.md) |
| [ADR-009](009_EDITOR.md) | Versioned structured text; CRDT deferred | [Specification](../features/F12_RICH_TEXT.md) |
| [ADR-010](010_PROVIDERS.md) | Provider-neutral optional AI with explicit auth modes | [Specification](../ai/AUTHENTICATION_RESEARCH.md) |
| [ADR-011](011_AGENT_ISOLATION.md) | Separate runners and exact artifact approval | [Specification](../ai/AGENT_SECURITY.md) |
| [ADR-012](012_FILES.md) | Local immutable blobs, optional S3 | [Specification](../DEPLOYMENT_ARCHITECTURE.md) |
| [ADR-013](013_API.md) | REST contracts, versions and explicit idempotency | [Specification](../API_DESIGN.md) |
| [ADR-014](014_DEPLOYMENT.md) | Compose first and restore before scale | [Specification](../DEPLOYMENT_ARCHITECTURE.md) |
| [ADR-015](015_LICENSE.md) | Open-source licensing and project identity | [Specification](../PRODUCT_VISION.md) |
| [ADR-016](016_PRODUCT_EXPANSION.md) | Approved product expansion and scoped source/retro access | [Specification](../IMPLEMENTATION_PLAN.md) |
| [ADR-017](017_DESKTOP_BROWSER.md) | Shared browser and installable desktop application | [Specification](../features/F36_DESKTOP.md) |
