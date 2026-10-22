# Technology decisions and compatibility

Research date: 2026-10-17. Every selection below is a design decision. Read-only registry evidence and candidate versions are recorded in [COMPATIBILITY](research/COMPATIBILITY.md). Exact resolved lockfiles and successful builds remain mandatory M01 work, not an unperformed Phase 0 build. [Evidence](research/SOURCES.md), [ADRs](adr/README.md).

| Area | Selection and reason | Alternative and cost | Maintenance/compatibility gate |
| --- | --- | --- | --- |
| Language/runtime | Rust stable, Tokio 1; shared runtime ecosystem and explicit resource ownership | Go simplifies some onboarding; Node reduces language count but misses the requested Rust emphasis | Pin one stable compiler and MSRV after resolving direct dependencies; no nightly requirement |
| HTTP | Axum, Tower and tower-http; composable middleware aligned with Tokio | Actix is credible but another application model; direct Hyper increases plumbing | Resolve mutually supported Axum/Tower/http/hyper versions; exercise streaming middleware order |
| Database access | SQLx, PostgreSQL-only features, Tokio runtime and Rustls TLS; explicit queries and transactions | Diesel gives strong relational typing; SeaORM adds dynamic model abstraction | Query macros validated against migrated PostgreSQL; prepare metadata offline; test cancellation and pool reuse |
| Durable storage | PostgreSQL 18, current maintained patch; relational integrity, FTS and durable jobs | SQLite excellent for single users but does not share operational/concurrency assumptions; MySQL adds unsupported dialect | Official policy lists 18.6 at research time; validate selected image digest and extension availability before use |
| Web application | React 19, TypeScript strict, Vite static SPA, React Router | Next.js 16 App Router supplies SSR and routing but adds runtime/build complexity with little benefit for authenticated data | Vite docs require Node 20.19+ or 22.12+; choose maintained Node 24 LTS after package verification, pin npm and package-lock.json |
| UI foundations | Tailwind 4, reviewed shadcn/ui components based on one consistent primitive family, Radix baseline | Handwritten primitives increase accessibility burden; wholesale UI framework reduces visual control | Official shadcn docs cover React 19/Tailwind 4; copied components remain our maintenance responsibility |
| Remote state | TanStack Query 5, URL filter state, local component state | Redux for everything duplicates server cache; custom cache is difficult to invalidate | Scoped query keys include workspace and permission epoch; test optimistic races |
| Editor | Tiptap OSS extensions / ProseMirror JSON schema | Lexical is a viable smaller alternative; raw contenteditable is expensive to maintain | License-audit each extension; verify paste, IME, keyboard and schema migration; no paid editor service required |
| Virtualization and board input | TanStack Virtual for large lists; dnd-kit candidate for board input | Render-all exceeds DOM budget; custom drag system adds input bugs | Adoption contingent on accessible non-drag controls and React compatibility spike |
| Search | PostgreSQL FTS + pg_trgm initially | Tantivy adds Rust-native ranking but requires index lifecycle, ACL freshness and replicas | Revisit only after tuned queries miss search budget at benchmark size |
| Realtime | Same-origin SSE plus REST commands; PostgreSQL event replay | WebSockets support bidirectional presence but need more protocol machinery | Proxy streaming/HTTP 2 and replay tests; WebSockets reserved for later collaboration |
| Shared cache/broker | None by default | Redis/Valkey is useful at scale but adds deployment and recovery work; pub/sub is not durable | Add only after a measured bottleneck, with license/version review; never a system of record |
| Jobs | PostgreSQL leases and receipts in worker | External queue solves larger traffic but creates cross-system consistency | Benchmark queue contention and autovacuum; isolate agent scheduling from notification jobs |
| HTTP clients/serialization | reqwest with Rustls; Serde and serde_json; thiserror for typed errors | Provider SDKs can supply convenience but many are unofficial in Rust | Prefer official documented HTTP schemas; cap streams, retries and diagnostics |
| Configuration | Serde + TOML parser + explicit merge/validation | General dynamic config frameworks hide precedence and origin | Provenance per setting, unknown-key rejection, versioned schema; no secret Debug output |
| Identity/cryptography | Maintained OIDC client, Argon2id library, OS randomness and audited AEAD library | Handrolled OAuth/JWT/crypto rejected | Audit maintenance and advisories during M01/M02; keep algorithm configuration narrow |
| Observability | tracing, tracing-subscriber, OpenTelemetry exporter optional, Prometheus metrics endpoint | Full observability platform mandatory would burden small installs | Check exporter/runtime versions; bounded queues and low-cardinality labels |
| Files | Local opaque-key blob store first; S3-compatible adapter later | Mandatory object service adds another persistent dependency | Atomic rename, fsync policy, backup consistency and no public file serving |
| Desktop | Tauri 2 later, static assets shared with web | Electron broad compatibility with greater footprint; remote webview expands attack surface | OS webview matrix, signed updates, capability allowlists; Tauri.toml supported through config-toml |
| Deployment | Docker Compose, one core image usable as API or worker | Kubernetes is optional for operators, not the default product contract | Immutable digests, rootless-compatible paths, upgrade/restore rehearsal |

## Why depart from Next.js preference

This app has no SEO requirement for private issue data, and Rust must own authorization and business operations. Next.js is viable as a Node-hosted frontend but would add a production runtime or require a constrained static shell. Its documented static export does not support arbitrary dynamic routes without generated parameters, so a proposed /w/:workspace/issues/:id app cannot simply assume normal Next routing will statically export. A Vite SPA supplies arbitrary client-side routes, one artifact for web/Tauri, and API-served assets. This is a scope-specific recommendation, not a claim that Vite is universally faster. See [ADR-002](adr/002_FRONTEND.md).

## Dependency acceptance and upgrade policy

Choose maintained stable releases with explicit licenses, active upstream ownership, security reporting and supported runtimes. Stars are not evidence of maintenance. Official documentation establishes intended capabilities, not that a release is vulnerability-free. M01 records exact compiler, packages, image digests, licenses, last release dates, advisories and a successful smoke build in a compatibility report. Refresh this at every release and before adopting an adapter. Rust cargo audit/deny and npm audit are inputs, not complete security guarantees.

No CDN assets, runtime Google fonts, proprietary icon packs, mandatory commercial extensions or paid hosted auth are required. System fonts and Lucide icons are the visual baseline; verify icon license before vendoring. Maintain an SBOM and third-party notices. Package version ranges never replace committed lockfiles.

## Documented browser baseline

Tailwind's current requirements include Chrome 111, Safari 16.4 and Firefox 128. Product support targets the current and previous stable Chromium/Firefox and current/previous Safari that meet those minimums. Tauri Linux webviews need separate proof; a desktop failure is not solved by claiming browser support. Each major UI dependency is tested against the same matrix.
