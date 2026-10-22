# Foundation contract slice

Issue #5, M01-T04 and F31-A1. Backend verification uses Linux Rust 1.98.1 and PostgreSQL 18. The production frontend build/typecheck uses Windows Node 24.14.1 and the pinned frontend lockfile; generated-client verification runs on Linux. Every HTTP fixture owns a UUID-named database schema, restricted runtime access distinct from migration access, and a dynamic loopback listener. No shared application pool or listening development instance is stopped.

## Implemented changes

- Shell negotiation requires an exact HTML media type with valid, positive quality. Zero/malformed/duplicate quality values and HTML-prefixed media types cannot turn missing resources into successful navigation.
- The reserved health namespace joins API/assets in exclusion from shell fallback. Shell HEAD preserves GET content length; shell responses declare `Vary: Accept`.
- Liveness/readiness and unavailable frontend responses prohibit caching. Readiness remains dependent on the runtime database while liveness and capability discovery remain available after the fixture's runtime pool closes.
- The verification entrypoint fetches production frontend output through the actual Axum router after building it. This is an explicit test that fails when production output is missing, rather than an optional skip inside a passing test.

## Executable evidence

`crates/opentrack-api/tests/built_router_contract.rs` opens real TCP connections and checks live health/capability/OpenAPI responses, anonymous discovery, configured and disabled attachment limits, navigation query strings, GET/HEAD parity, accepted/rejected HTML qualities, reserved-path and missing-asset 404s, unsupported methods, missing-build 503 and database-unavailable readiness. The runtime OpenAPI response must exactly equal the committed generator input without assuming a fixed route count.

After `npm --prefix web ci` and `npm --prefix web run build`, the explicit production test follows HTML script/style references and fetches every emitted asset, including the document worker, lazy editor chunk and source maps. Each response must match the actual built file bytes, MIME type, caching policy and HEAD metadata. Root and deep links return the same emitted HTML; absent API/assets remain 404.

Checks executed for this branch:

- `cargo fmt --all --check`
- `cargo test --locked -p opentrack-api --test built_router_contract --test static_delivery_contract --test http_contract -- --include-ignored`
- `scripts/generate-contracts.sh --check`
- `npm --prefix web run build`
- `cargo clippy --locked -p opentrack-api --all-targets -- -D warnings`
- `npm --prefix web run typecheck`

All checks above passed. The targeted Rust run passed five tests, including both new real TCP contracts; the explicit production target served all seven emitted asset files and the separate theme script. Generated OpenAPI and TypeScript declarations exactly matched the Rust source without modifying their snapshots. Vite reported its existing main-chunk size advisory; the build completed successfully.

`scripts/check.sh` retains the existing generated OpenAPI/TypeScript drift check and now runs the production delivery target explicitly after the frontend build. This records the delivered foundation contract; scoped developer tokens, event delivery, full browser interaction qualification and broader F31 criteria remain separate work.

No database migration or wire-schema change is required. Rollback restores the previous routing/response behavior and removes the additional verification target.
