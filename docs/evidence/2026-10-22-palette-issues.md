# Accessible issue search and session recents

Date: 22 October 2026. Scope: issue #18 / M03-T05, F22-A2 and the issue-reference portion of F22-A3. This increment does not close the full keyboard qualification task.

## Behavior

The shared command palette searches live issues across the current workspace through the existing authorized issue-list endpoint. Queries are debounced 150 ms and bounded to 20 results. Local commands remain available during search and on remote errors. New queries immediately remove the previous result titles; an aborted request cannot populate a newer query even if its transport completes after cancellation. Loading and result counts have a polite announcement, and errors expose an explicit retry.

The current authenticated workspace keeps at most ten recent issue IDs in memory. Opening the palette reauthorizes every reference and retrieves the current title; no titles, references, account keys or session credentials are written to browser storage. Unavailable, archived and foreign-workspace recent rows are excluded. Workspace, account, session and authority changes clear the history and visible results. This intentionally provides session recents rather than durable cross-session history.

Selecting a remote or recent issue performs another ordinary authorized read before closing. If the issue was deleted or access was lost, its suggestion is removed, the error remains in the palette, and search focus is retained. Successful selection retains the existing close/focus-restoration-before-navigation behavior and dispatches only once. Closing or replacing the search scope while validation is pending prevents later navigation. Modified Enter and legacy IME key-code events do not invoke a result.

No storage migration, authorization policy, mutation endpoint or desktop route allowlist changes are required. The existing browser and desktop issue endpoints remain authoritative. Reverting this frontend increment discards only transient palette state; durable application data is unchanged.

## Executed checks

| Check | Result |
| --- | --- |
| Frontend typecheck, including browser-test sources | Passed |
| Complete frontend unit/component suite | 503 tests passed across 62 files |
| Final focused palette, shortcuts, preferences and application rerun | 30 tests passed across four files |
| Production frontend build | Passed; existing large-chunk warning remains |
| Browser-test discovery for palette workflows | Nine cases discovered across Chromium, Firefox and WebKit, including six new cases |
| Git whitespace check | Passed |

The new component cases cover request ordering even when abort is ignored, immediate clearing during debounce and errors, scope changes, reauthorization on every open, unavailable and foreign-workspace recents, deletion before invocation, delayed validation after close, one-time invocation and bounded reference-only history. Existing preference and command-dispatch tests remain passing.

Two real-server Playwright workflows were added to the existing guarded GitHub browser runner: saved issue search and renamed recents with character shortcuts disabled, and deletion between suggestion and selection. They are typechecked and discovered here; this record does not claim their execution.

## Remaining acceptance gates

Search in this increment covers issues. Heterogeneous entity search remains part of F24; projects and other existing destinations remain available as navigation commands. Recent references do not survive logout, a workspace switch, an authority change or application reload. No durable recent-item endpoint or persistent user history is introduced.

Full physical IME, international keyboard, native desktop and assistive-technology qualification of every core task remains unverified. Existing keyboard/preference and historical hosted-browser evidence remains applicable to its recorded scope; these checks do not establish a new native or screen-reader result. Issue #18 and F21-A3 must remain open until the remaining acceptance evidence exists.
