# API contract generation

`utoipa` derives OpenAPI 3.1 from Rust wire DTOs and endpoint annotations. `scripts/generate-contracts.sh` exports the document and uses pinned `openapi-typescript` to generate the web types. `--check` fails on drift without changing committed files. The generated document is also served at `/api/v1/openapi.json`.

Install this tooling with `npm --prefix scripts/contracts ci`, then generate. The generator's published TypeScript peer dependency is `^5.x`, so its compiler is isolated here at 5.9.3; the web application still compiles with TypeScript 7.0.2. Node 22 and npm >=9.5 satisfy the generator dependency engines. These packages do not ship in the browser or server image.

The server's API errors use RFC 9457. The `details.current` extension contains only an already-authorized representation for version conflicts. Generic framework errors never return raw extractor/SQL inputs. Secret invitation tokens are returned on first issuance only; idempotent replay returns `token:null` and the UI instructs the issuer to revoke and replace a lost token.

Primary package documentation checked 17 October 2026: <https://docs.rs/utoipa/6.0.0/utoipa/> and <https://openapi-ts.dev/cli>.
