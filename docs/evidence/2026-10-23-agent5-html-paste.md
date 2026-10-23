# Browser HTML paste and read-only rendering

23 October 2026. Issues #20/#21; F12-A1. The [focused browser case](../../web/e2e/document-paste.browser.ts) exercises the production clipboard parser, real server/PostgreSQL persistence and both editable and read-only document rendering. No production parser, authorization, schema or CSP setting changed.

## Input and false-pass controls

Each real engine receives a synthetic cancelable ClipboardEvent containing an actual DataTransfer. The event must retain that exact object and HTML, remain untrusted, and be prevented by the application's handler. A different plaintext fallback and a benign bold/safe-link control ensure that dropping HTML or doing nothing cannot pass.

The three payload cases cover ordinary formatted HTML, active elements/event/CSS attributes and embedded external resources, and unsafe raw/entity/control-obfuscated JavaScript, data, VBScript and file links. Expected schema JSON is exact. The case checks safe live DOM, absent active attributes/elements/unsafe links, unchanged execution flag and no dialog, popup or foreign-origin request attempt. The fixture independently aborts off-origin requests; DOM and stored-JSON assertions prevent network/CSP suppression alone from producing a pass.

The real API must persist the expected body before reload. Reload repeats the editable DOM and API assertions. Actual Archive then removes the editable textbox and renders the separate DocumentView; text, marks, the positive link, safe DOM, exact API body and inertness are checked again. Actual Restore uses its confirmation dialog and returns the unchanged body to editing. Only the created synthetic issue is archived/restored; workspace/team authority is not modified.

## Executed observations

The initial editor-only matrix passed Chromium and WebKit but failed Firefox before production paste dispatch. A fresh Firefox diagnostic proved that standalone DataTransfer HTML existed while the constructor's event held a different transfer with no HTML. The uniform fixture correction sets the synthetic event's own clipboardData to the actual transfer and asserts its identity and contents. It is not a fallback or a claim about trusted OS clipboard delivery.

Corrected Firefox passed 1/1, then the editor-only matrix passed 3/3. After adding the separate read-only sink, the final one-case-per-engine matrix passed:

| Engine | Final result | Duration |
| --- | --- | --- |
| Chromium | Passed all three payload cycles | 8,534 ms |
| Firefox | Passed all three payload cycles | 10,867 ms |
| WebKit | Passed all three payload cycles | 9,793 ms |

The [allowlisted receipt](2026-10-23-agent5-html-paste-browser.json) preserves the earlier outcomes and final identities without auth state, raw errors, request bodies or private paths. Its final run used source `67e8d235` and byte-unchanged production inputs/artifacts built from `590725e9`, Node 22.22.1 for the browser runner, Node 24.14.1 for the frontend build and Playwright 1.64.1. The final test SHA-256 is `32d941b4db5ad2366a3fa9e4ac89d8308a44ae1b9ffdf29c324ee5a508de6d0b`. Current identities do not reconstruct the initial run's missing provenance.

E2E TypeScript passed. Staging exposed one extra final blank line in the new test; removing it preserves every executable statement. The receipt distinguishes the executed and published file hashes, and staged whitespace checks then passed. Each bounded run released its owned API/worker processes normally; later peer browser leases were left untouched. Independent source review found no actionable false-pass, correctness or privacy issue. Exact-head hosted checks and integration remain required before accepting this increment.

## Acceptance scope

Together with the existing Rust/TypeScript schemas, safe-URL rules and renderer contracts, these actual engine observations verify F12-A1 for the supported browser document profile. This is a finite malicious-HTML corpus, not a general security proof. It does not separately test CSP headers/enforcement events, forged context identities, every size/depth boundary, physical clipboard/composition or IME, zoom, assistive technology or native-webview/F36 behavior. Their existing evidence and remaining gates stay separate. #20/#21, full M04 and the whole product are not completed by this case.
