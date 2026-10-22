# Personal shortcut preferences and navigation sequences

Date: 20 October 2026. Scope: the preference and sequence portion of issue #18 / M03-T05, particularly F21-A3. Implemented from base `84a0e325` on `codex/keyboard-preferences`. Issue #18 remains open for remote palette search and recent accessible items.

## Behavior

The shared browser/desktop application reads the authenticated caller's workspace preference from PostgreSQL. Shortcut help exposes an accessible checkbox; disabled character shortcuts remove their hints from the palette, help and visible issue/search controls. C, /, ?, and G sequences pause while the preference is loading, saving or unresolved after an error. Mod+K, Escape, Mod+Enter, visible navigation and ordinary palette actions remain available.

G then M opens My work only when both keys arrive within 800 ms. G then I is absent because Inbox is unavailable. Unknown continuations do not consume later input; an ordinary command following an interrupted sequence can still run. Modifiers, AltGr, repeats, handled events, editing and IME cannot complete sequences. Pointer/focus changes, window blur, visibility changes, composition, modal changes and account/workspace/route changes clear pending navigation.

GET/PATCH `/api/v1/workspaces/{w}/preferences` is caller-owned and uses a strong actor/workspace/version ETag. GET returns enabled/version zero without writing. PATCH rejects unknown fields and requires If-Match. A 412 presents the latest saved setting alongside the proposed choice; only an explicit review action can send another update. An uncertain write keeps the choice visible and offers a read before any further write. Pending requests from a departed account/workspace/access scope cannot update the new screen.

Migration 0021 adds forced caller/workspace RLS, scoped membership references, immutable identity and advancing versions, private durable change events and transactional restricted audit. Viewers and archived-workspace members can change their own accessibility setting. No content permission is expanded. A membership lock serializes first updates; current account/workspace guards recheck revocation.

## Executed frontend checks

On Windows, `npm --prefix web test -- --maxWorkers=2` passed **438 tests across 58 files** in 100.97 seconds. The focused additions include:

- Exact 799/800 ms sequence boundaries, unavailable destinations, interrupted sequences, modifiers/AltGr/repeat/composition/editable guards, availability and scope changes.
- Preference reload, concurrent-session conflict review, ambiguous network responses, and delayed reads/writes after account/workspace/access changes.
- The application opens the real CreateIssue form from C and submits it through Mod+Enter. With character shortcuts disabled, Mod+K selects the same creation action, which submits through the ordinary request path and opens the returned issue. These are component tests with fixture transport and selected child components; they do not establish a real-browser/server workflow.
- Existing focus restoration, account-menu Escape and all prior frontend regressions passed.

An initial new App test selected the visible command button instead of the palette input. The selector was corrected to the input, then the focused tests and complete suite passed. The first pre-generation typecheck reported only the not-yet-generated `UserPreferences` schema; the final result is recorded with the contract checks below.

After the full suite, the dispatcher also gained a guard against the brief interval between a scope/preference render and effect cleanup, and editable SVG descendants use the same guard as HTML descendants. The final focused command/preference/App rerun passed **22 tests across 3 files**.

## Executed storage, HTTP and build checks

Checks used Rust 1.98.1 and the Windows C++ toolchain wrapper, with separately provisioned runtime and migration identities against the isolated local PostgreSQL test database. Each PostgreSQL contract creates and removes its own schema; the development database was untouched.

| Command or check | Observed result |
| --- | --- |
| `cargo test --locked -p opentrack-app --test preferences_contract -- --test-threads=1` | 2 passed: caller/workspace RLS, viewer/archive access, concurrent first updates, no-op stability, mandatory audit/event rollback, session reload, revocation ordering, and additive upgrade from schema 19 with existing issue preservation. |
| `cargo test --locked -p opentrack-api --test preferences_contract -- --test-threads=1` | 1 passed: authenticated caller scope, CSRF, required If-Match, 412 current representation, null/unknown/foreign-owner input rejection, private default settings, hidden workspace handling and unsupported methods/routes. |
| `cargo test --locked -p opentrack-api -p opentrack-domain --lib` | 29 passed, including explicit preference schema/ETag and strict DTO tests. |
| Rust OpenAPI export and `openapi-typescript` regeneration | Succeeded; frontend consumes the generated `UserPreferences` type. |
| Fresh contract regeneration comparison | OpenAPI and generated TypeScript matched the checked artifacts exactly. |
| `npm --prefix web run build` | TypeScript checking and production build passed. The existing bundle-size warning remains; no bundle-size qualification is claimed. |
| `cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings` | Passed. |
| Workspace/desktop formatting and staged whitespace checks | Passed. |
| Offline direct-source desktop protocol harness | 9 passed, including the new preferences route and parity with every published workspace operation in this slice's generated OpenAPI. The ignored harness uses the actual `desktop/src/protocol.rs` and the exact pinned Serde, serde_json and UUID dependencies. |

The first shared-cache compile reused a domain artifact from a different worktree; cleaning only that package rebuilt the correct dependency. A test initially used a nonexistent `get_workspace` helper, corrected to the existing `workspace` API. HTTP assertions were corrected to the existing extractor boundary: null enum input returns 400, while unknown modes and unknown fields return 422. Final runs above passed; application error handling was not weakened to satisfy those assertions.

`cargo test --manifest-path desktop/Cargo.toml --no-default-features --locked -j 2 protocol::tests` failed before tests at the known Windows `glib-sys`/`pkg-config` development-dependency boundary. No native dependency or source workaround was applied. The direct-source protocol harness supplies focused route evidence only; it does not establish a full desktop build or GUI workflow. A first same-name harness reused an older eight-test shared artifact; a distinct harness package name forced compilation of this source, and all nine tests including the new preference test then passed.

The generated OpenAPI has ordering differences from the base artifact. Parsed comparison confirms one added route (`/preferences`) and no removed or changed shared routes; no unrelated API behavior was changed.

## Remaining qualification

This increment does not implement remote entity search, recent-item storage, Inbox, arbitrary user key remapping, native preference import/export or complete F21/F22 qualification. The required manual assistive-technology and full live browser keyboard workflow remain separate gates. A local API startup was previously rejected by automatic approval review; it was not retried or bypassed for this increment.

Migration 0021 is additive and leaves migrations 0001–0019 unchanged. Application rollback retains the new tables; schema rollback requires restoring the pre-upgrade backup. Removing the new tables would discard saved personal choices and private event history.
