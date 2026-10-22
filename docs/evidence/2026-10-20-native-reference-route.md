# Native issue-reference route — 20 October 2026

Related tracking: [issue 119](https://github.com/MimoAlexer/OpenTrack/issues/119).

The native request allowlist now accepts `GET /api/v1/workspaces/{workspace_uuid}/issue-reference-candidates`, matching the published API used by the reference picker. Existing canonical UUID, path, query, header and GET-body constraints still apply. No database or server authorization changes.

The candidate/subscription policy test exercises query-bearing GET and rejects POST, PUT, PATCH, DELETE, an unscoped route, an invalid workspace UUID, an extra resource segment, a trailing slash and a GET body. The existing full published-workspace-operation parity test failed on the baseline and passes with this route added.

## Verification

An ignored standalone Cargo manifest points directly to the actual protocol source. Its `include_str!` reads the same worktree's canonical OpenAPI file. The source's existing serde, serde_json and uuid dependencies were resolved from the local cache with a scratch lockfile and `--offline`.

- All 8 protocol tests passed, including published OpenAPI parity; 0 failed or ignored.
- Rust formatting and whitespace checks passed.

The complete desktop test workspace is blocked locally by native GLib/GObject dependencies. These results qualify the request policy, not native GUI behavior, credential-vault integration or packaged desktop behavior. The root CI script currently omits the separate desktop workspace, so hosted native coverage remains a separate qualification gap tracked in [issue 120](https://github.com/MimoAlexer/OpenTrack/issues/120).

## Retrospective

API additions must update the native allowlist in the same change. Keep the existing OpenAPI parity check in the native verification path; backend and frontend checks alone do not exercise that broker boundary.
