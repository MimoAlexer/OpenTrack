# Research source register

Access/research date for all entries: **2026-10-17**. Prioritize official product/project docs, standards and package publisher metadata. Source facts are distinct from OpenTrack design recommendations. No provider authorization, competitor hands-on benchmark or production compatibility test occurred. Full source text is not redistributed here.

| ID | Primary source | Used for |
| --- | --- | --- |
| R01 | [Linear concepts](https://linear.app/docs/conceptual-model) | Issue/team/project/cycle/view model; product research |
| R02 | [Linear board layout](https://linear.app/docs/board-layout) | Board interaction reference; original UI design |
| R03 | [Linear selection](https://linear.app/docs/select-issues) | Keyboard and multi-selection reference |
| R04 | [Linear custom views](https://linear.app/docs/custom-views) | Saved filtered views and access distinction |
| R05 | [Linear inbox](https://linear.app/docs/inbox) | Notification processing reference |
| R06 | [Linear GitHub integration](https://linear.app/docs/github) | Linking versus sync reference |
| R07 | [Plane source repository](https://github.com/makeplane/plane) | Public scope/self-host/license observation |
| R08 | [Huly platform](https://github.com/hcengineering/platform) | Public open-source alternative |
| R09 | [Huly self-host](https://github.com/hcengineering/huly-selfhost) | Deployment and migration/resource requirements |
| R10 | [Next.js installation](https://nextjs.org/docs/app/getting-started/installation) | Observed 16.4.2 docs; runtime/browser requirements |
| R11 | [Next.js static export](https://nextjs.org/docs/app/guides/static-exports) | Dynamic-route limitations informing frontend ADR |
| R12 | [Vite guide](https://vite.dev/guide/) | Static-client build tooling/runtime requirements |
| R13 | [Tauri Next integration](https://v2.tauri.app/start/frontend/nextjs/) | Static frontend constraint for desktop evaluation |
| R14 | [Tauri configuration](https://v2.tauri.app/reference/config/) | TOML support and feature requirement |
| R15 | [Tailwind compatibility](https://tailwindcss.com/docs/compatibility) | Browser minimums; shared webview risk |
| R16 | [shadcn Tailwind4](https://ui.shadcn.com/docs/tailwind-v4) | React 19/Tailwind 4 documented support |
| R17 | [TanStack optimistic updates](https://tanstack.com/query/latest/docs/framework/react/guides/optimistic-updates) | Cache mutation/rollback concepts |
| R18 | [Tiptap overview](https://tiptap.dev/docs/editor/getting-started/overview) | Editor ecosystem evaluation |
| R19 | [Axum docs](https://docs.rs/axum/latest/axum/) | HTTP/runtime ecosystem |
| R20 | [SQLx docs](https://docs.rs/sqlx/latest/sqlx/) | Runtime and database feature selection |
| R21 | [Tokio shutdown](https://tokio.rs/tokio/topics/shutdown) | Cancellation and task ownership |
| R22 | [PostgreSQL version policy](https://www.postgresql.org/support/versioning/) | Maintained 18 line and observed 18.6 patch |
| R23 | [PostgreSQL NOTIFY](https://www.postgresql.org/docs/current/sql-notify.html) | Commit timing, wakeup semantics |
| R24 | [PostgreSQL RLS](https://www.postgresql.org/docs/current/ddl-rowsecurity.html) | Tenant defense-in-depth and role caveats |
| R25 | [PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html) | SKIP LOCKED queue claim semantics |
| R26 | [PostgreSQL backups](https://www.postgresql.org/docs/current/backup.html) | Recovery strategy families |
| R27 | [PostgreSQL FTS](https://www.postgresql.org/docs/current/textsearch.html) | Initial search capability |
| R28 | [Redis pub/sub](https://redis.io/docs/latest/develop/pubsub/) | Nondurable delivery tradeoff |
| R29 | [Tantivy repository](https://github.com/quickwit-oss/tantivy) | Rust search library; not a turnkey service |
| R30 | [OpenAI SIWC overview](https://developers.openai.com/siwc/token-sharing-open-source) | Eligible plan-use category and separate identity scope |
| R31 | [SIWC sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in) | Dynamic registration, loopback, PKCE and grant validation |
| R32 | [SIWC sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions) | Refresh rotation and revocation |
| R33 | [SIWC inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference) | Model discovery/terminal stream semantics |
| R34 | [SIWC limits](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations) | Restricted request/tool capability mode |
| R35 | [SIWC app-server](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server) | Dedicated app-server plan-use configuration |
| R36 | [SIWC self-hosted VMs](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms) | Local callback/secure transfer/host-ID limitations |
| R37 | [Codex SDK](https://learn.chatgpt.com/docs/codex-sdk) | Current SDK choices and deprecated integration avoidance |
| R38 | [Codex app-server](https://learn.chatgpt.com/docs/app-server) | Rich client protocol; transport/auth boundary |
| R39 | [Codex noninteractive](https://learn.chatgpt.com/docs/non-interactive-mode) | Automation entry point and credential separation |
| R40 | [OpenAI text generation](https://developers.openai.com/api/docs/guides/text) | Public API inference route |
| R41 | [OpenAI plugin auth](https://developers.openai.com/plugins/build/auth) | External assistant tool authorization |
| R42 | [Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview) | SDK capabilities and third-party auth boundary |
| R43 | [Claude legal/credential policy](https://code.claude.com/docs/en/legal-and-compliance) | Credential intermediation and unmodified-binary distinctions |
| R44 | [Claude programmatic execution](https://code.claude.com/docs/en/headless) | CLI output/config discovery/cancellation behavior |
| R45 | [Claude SDK permissions](https://code.claude.com/docs/en/agent-sdk/permissions) | Tool policy/approval interface |
| R46 | [Claude Messages API](https://platform.claude.com/docs/en/api/messages/create) | Messages/streaming public HTTP contract |
| R47 | [Claude MCP](https://code.claude.com/docs/en/mcp) | Optional external tool client integration |
| R48 | [Ollama API](https://docs.ollama.com/api/introduction) | Optional local inference adapter |
| R49 | [GitHub App auth](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation) | Scoped installation access |
| R50 | [GitHub webhook validation](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries) | Raw-body HMAC |
| R51 | [Docker rootless](https://docs.docker.com/engine/security/rootless/) | Reduced daemon privilege; design does not equate to VM |
| R52 | [WCAG2.2](https://www.w3.org/TR/WCAG22/) | Accessibility baseline |
| R53 | [WAI dialog pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) | Modal focus/keyboard behavior |
| R54 | [OAuth security BCP](https://www.rfc-editor.org/rfc/rfc9700) | OAuth flow security requirements |
| R55 | [Problem Details](https://www.rfc-editor.org/rfc/rfc9457.html) | API error representation |
| R56 | [OWASP SSRF](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html) | Outbound URL trust controls |
| R57 | [OWASP uploads](https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html) | Upload threat controls |
| R58 | [OWASP passwords](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) | Password hashing baseline |
| R59 | [MCP authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization) | Versioned external-tool authorization contract |
| R60 | [Node releases](https://nodejs.org/en/about/previous-releases) | Maintained runtime selection |
| R61 | [Rust stable distribution](https://static.rust-lang.org/dist/channel-rust-stable.toml) | Observed compiler candidate; read-only metadata |
| R62 | [AGPLv3 text](https://www.gnu.org/licenses/agpl-3.0.txt) · [SPDX identifier/text](https://spdx.org/licenses/AGPL-3.0-or-later.html) | Source-license choice ratified24 October2026; distribution review remains separate |
| R63 | [Existing opentrack project](https://github.com/opentrack/opentrack) | Concrete working-name collision; not trademark clearance |

[Compatibility evidence](COMPATIBILITY.md) additionally cites 19 individual crates.io/npm package records and dependency/feature metadata, including publication dates, MSRVs and React/Node peers. Those machine-readable records were fetched read-only, without installing dependencies. An observed release is not a vulnerability-free claim.

## Research handling and refresh policy

Some official OpenAI HTML pages include large navigation sections; their official Markdown alternatives were fetched read-only to inspect the relevant body when the web renderer rejected text/markdown. Both represent the same cited official page. A stale Linear keyboard-shortcuts URL and an initial Claude Messages overview URL did not load; working official selection/command and Messages-create pages replaced them. The GNU HTML license fetch failed; the official plain-text AGPLv 3 source returned successfully and section 13 was inspected. Failed fetches were not treated as evidence.

Provider auth/capabilities refresh before M11/M12 and each adapter release. Dependency runtime/peer/MSRV and advisories refresh in M01 and each release. Security standards and deployment guidance refresh when relevant boundaries change. Product observations refresh before implementing a vendor-specific migration/import adapter. Keep a source date and record conflicting evidence instead of inventing certainty.

No exact pricing, plan allowance, customer eligibility or future roadmap promise is assumed. Model IDs/capabilities are discovered or configured at runtime; documentation validates mechanisms, not a particular user's entitlement. Public source repositories do not grant permission to reuse trademarks or proprietary assets.
