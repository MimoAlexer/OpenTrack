# Dependency compatibility evidence

Current resolved graphs and advisory/license metadata: [22 October dependency qualification](../evidence/2026-10-22-agent5-dependencies.md). The candidate research below remains a historical planning snapshot; it is not the current locked-build or release qualification.

Checked 2026-10-17 using official documentation and read-only crates.io/npm registry metadata. No packages were installed, no lockfile was resolved, and no build was run. This is declared-compatibility evidence, not runtime proof or a complete vulnerability audit. URLs below are primary package metadata; repeat them before implementation.

## Observed versions and declared requirements

| Package | Observed stable/latest release | Declared runtime / peers | License metadata | Published |
| --- | --- | --- | --- | --- |
| [axum](https://crates.io/api/v1/crates/axum) | 0.8.9 | Rust 1.80 | MIT | 2026-04-14 |
| [tokio](https://crates.io/api/v1/crates/tokio) | 1.53.3 | Rust 1.71 | MIT | 2026-10-13 |
| [tower](https://crates.io/api/v1/crates/tower) | 0.5.3 | Rust 1.64.0 | MIT | 2026-01-12 |
| [tower-http](https://crates.io/api/v1/crates/tower-http) | 0.7.1 | Rust 1.65 | MIT | 2026-08-31 |
| [sqlx](https://crates.io/api/v1/crates/sqlx) | 0.9.0 | Rust 1.94.0 | MIT OR Apache-2.0 | 2026-05-21 |
| [serde](https://crates.io/api/v1/crates/serde) | 1.0.229 | Rust 1.56 | MIT OR Apache-2.0 | 2026-07-18 |
| [tracing](https://crates.io/api/v1/crates/tracing) | 0.1.44 | Rust 1.65.0 | MIT | 2025-12-18 |
| [reqwest](https://crates.io/api/v1/crates/reqwest) | 0.13.5 | Rust 1.85.0 | MIT OR Apache-2.0 | 2026-09-08 |
| [toml](https://crates.io/api/v1/crates/toml) | 0.9.8 | Rust 1.76 | MIT OR Apache-2.0 | 2025-10-09 |
| [react](https://registry.npmjs.org/react) | 19.3.1 | node >=0.10.0 | MIT | 2026-10-14 |
| [vite](https://registry.npmjs.org/vite) | 8.3.4 | node ^20.19.0 or >=22.12.0; tsx ^4.8.1; jiti >=1.21.0; less ^4.0.0; sass ^1.70.0; yaml ^2.4.2; stylus >=0.54.8; terser ^5.16.0; esbuild ^0.27.0 or ^0.28.0; sugarss ^5.0.0; @types/node ^20.19.0 or >=22.12.0; sass-embedded ^1.70.0; @vitejs/devtools ^0.7.1 | MIT | 2026-10-08 |
| [typescript](https://registry.npmjs.org/typescript) | 7.0.2 | node >=16.20.0 | Apache-2.0 | 2026-07-08 |
| [tailwindcss](https://registry.npmjs.org/tailwindcss) | 4.3.3 | No engine or peer range declared in this record | MIT | 2026-07-16 |
| [@tanstack/react-query](https://registry.npmjs.org/@tanstack%2freact-query) | 5.104.3 | react ^18 or ^19 | MIT | 2026-10-15 |
| [@tiptap/react](https://registry.npmjs.org/@tiptap%2freact) | 3.32.0 | react ^17.0.0 or ^18.0.0 or ^19.0.0; react-dom ^17.0.0 or ^18.0.0 or ^19.0.0; @tiptap/pm 3.32.0; @tiptap/core 3.32.0; @types/react ^17.0.0 or ^18.0.0 or ^19.0.0; @types/react-dom ^17.0.0 or ^18.0.0 or ^19.0.0 | MIT | 2026-10-13 |
| [@radix-ui/react-dialog](https://registry.npmjs.org/@radix-ui%2freact-dialog) | 1.3.1 | react ^16.8 or ^17.0 or ^18.0 or ^19.0 or ^19.0.0-rc; react-dom ^16.8 or ^17.0 or ^18.0 or ^19.0 or ^19.0.0-rc; @types/react *; @types/react-dom * | MIT | 2026-10-12 |
| [react-router](https://registry.npmjs.org/react-router) | 8.4.0 | node >=22.22.0; react >=19.2.7; react-dom >=19.2.7 | MIT | 2026-09-15 |
| [@tanstack/react-virtual](https://registry.npmjs.org/@tanstack%2freact-virtual) | 3.14.21 | react ^16.8.0 or ^17.0.0 or ^18.0.0 or ^19.0.0; react-dom ^16.8.0 or ^17.0.0 or ^18.0.0 or ^19.0.0 | MIT | 2026-10-16 |
| [@dnd-kit/core](https://registry.npmjs.org/@dnd-kit%2fcore) | 6.3.1 | react >=16.8.0; react-dom >=16.8.0 | MIT | 2024-12-05 |

## Compatibility conclusions and selected candidates

The official Rust stable channel metadata reports **1.99.1**, dated 2026-10-15. The candidate implementation toolchain is that maintained stable release, refreshed at M01. SQLx 0.9.0 declares Rust>=1.94; the observed compiler satisfies this direct requirement. The maximum known direct MSRV is therefore 1.94, but transitive resolution and selected features can raise the actual minimum. Do not claim support for an older compiler until CI tests it. [Rust distribution metadata](https://static.rust-lang.org/dist/channel-rust-stable.toml).

Axum 0.8.9 declares Tower^0.5.2, Tokio^1.44, http^1 and optional Hyper^1.1. Observed Tower 0.5.3 and Tokio 1.53.3 satisfy those declared ranges. Axum's optional tower-http edge is^0.6.8, while the latest observed tower-http is 0.7.1. Select **tower-http 0.6.11** for the initial candidate graph to avoid needless duplicate minor lines;0.7 is not asserted incompatible at the Service/http boundary, but would need intentional testing. [Axum dependencies](https://crates.io/api/v1/crates/axum/0.8.9/dependencies), [tower-http versions](https://crates.io/api/v1/crates/tower-http).

SQLx 0.9.0 pins sqlx-core/sqlx-postgres to the same exact version. Registry features include postgres, runtime-tokio and tls-rustls-ring-webpki (or native-roots when operator certificate requirements demand it). Choose one TLS/root policy explicitly; do not enable every database/runtime backend. PostgreSQL 18 compatibility must be proved with migrations, query macros and TLS connection tests. [SQLx dependencies](https://crates.io/api/v1/crates/sqlx/0.9.0/dependencies), [SQLx metadata/features](https://crates.io/api/v1/crates/sqlx).

React 19.3.1 satisfies the observed React peers of TanStack Query, Tiptap, Radix Dialog, TanStack Virtual and dnd-kit/core. React Router 8.4.0 requires React>=19.2.7 and Node>=22.22, so a maintained **Node 24** release satisfies both Router and Vite's engine range; a random older Node 22 patch may not. Match react-dom and React type packages deliberately. Tiptap 3.32.0 requires matching core/pm 3.32.0; do not mix extension majors. [Node release policy](https://nodejs.org/en/about/previous-releases).

TypeScript 7.0.2 is observed in registry metadata; do not assume all ESLint/test/generator tools support its major. M01 must resolve those peers and either adopt 7 with a clean type/lint test or record a maintained compatible TypeScript line. This is a bounded compatibility task, not permission to leave warnings ignored. Tailwind 4.3.3/shadcn React 19 support is documented; actual copied-component behavior still needs testing.

The observed dnd-kit/core 6.3.1 publication date is 2024-12-05, substantially older than the other selected UI packages. This alone does not prove abandonment, but it is a maintenance signal: inspect its current recommended package line, issues/releases and accessibility behavior at M05. Board Move to controls work independently and remain mandatory. No dependency is selected solely because its version is newest.

## Remaining evidence gates

M01 records full resolved dependency tree, exact Rust/Node/npm/compiler versions, relevant advisories, licenses, native library requirements, build times and successful browser/API test. M04 adds editor paste/IME corpus; M05 checks drag behavior; M11/M12 pin provider/runtime schemas. Registry license strings are evidence for triage, not a complete legal distribution analysis. Runtime/platform/feature incompatibility remains possible despite intersecting declared ranges.
