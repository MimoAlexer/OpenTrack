# Unicode issue-title input

24 October 2026. Issue titles use the existing Rust limit of1–256 Unicode scalars.
The create input and edit textarea previously applied native maxlength256, which
counts UTF-16 units. A valid256-scalar supplementary title therefore stopped at
128 scalars in Chromium, Firefox and WebKit before any create request.

The correction removes only those two native caps. Required fields, newline/trim,
composition, dirty/conflict and idempotency behavior remain. Rust continues to
reject overlong input; the existing error paths retain it for repair. Three
existing unit files now identify the actual Title label rather than its old
implementation attribute, preserving all interactions and assertions.

The same real API-backed browser journey failed at the native input boundary in
all three engines before correction and passed in all three afterward. It checks
full256-scalar creation/editing, actual257-scalar POST/PATCH422 responses with
visible draft preservation, unchanged saved title/version after rejection,
repaired writes and exact persistence after reload.

The complete frontend suite passed696 tests across72 files. Both TypeScript
targets and the production build passed. Full discovery lists168 cases,56 per
engine; that count is separate from the three executed journeys.

Both runs used the same freshly built licensed backend, current migrations and
default worker profile. Their separate fresh fixtures remain retained. Actual
executables/served artifacts, source/runtime hashes, owned-process reaping and
namespace-zero checks passed. The baseline temporary spec was removed by exact
identity before source changes; the same test bytes were then promoted and
retained. [Receipt](2026-10-24-unicode-issue-titles.json) records hashes, measured
cutoffs, executed checks and limits.

No backend rule, wire shape, migration or stored format changes. Physical input,
IME/native webview, assistive technology and broader exception/cross-account
acceptance remain separate. Fresh exact-head CI and final integration review are
required; issue17 and M03-T04 are not closed by this correction.
