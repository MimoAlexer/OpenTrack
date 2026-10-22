# Keyboard guards and saved browser qualification

Date: 22 October 2026. Scope: the existing issue #18 / M03-T05, the issue workflow portion of F21-A3, and F22-A3. Full native and assistive-technology qualification remains open.

## Behavior

The shared assignee/label picker tracks composition lifetime and ignores composing events, legacy IME key code 229, AltGraph and modified text-editing keys. Those events retain their browser default behavior and cannot navigate, select, dismiss the picker or bubble into an enclosing form shortcut. Shift/Ctrl/Meta+Home/End retain native text selection/navigation. Unmodified Home/End navigate choices after list navigation starts or when the query is empty. Ordinary arrows, Enter selection and Escape focus restoration remain available; repeated Enter cannot select.

Create/edit issue forms and the rich-text editor also exclude legacy IME, AltGraph/Alt and repeated Enter from their save shortcuts. The ordinary Mod+Enter action still uses the existing versioned issue/document API. No storage migration, new endpoint or authorization rule is introduced. Reverting this increment changes only frontend keyboard behavior and regression coverage.

## Executed checks

| Check | Result |
| --- | --- |
| Frontend typecheck, including browser sources | Passed |
| Complete frontend unit/component suite | 513 tests across 63 files passed |
| Final focused picker, application, rich-text and issue form suite | 14 tests across four files passed |
| Production frontend build | Passed; existing large-chunk warning remains |
| Real API/PostgreSQL browser acceptance | 15/15 checks passed across Chromium, Firefox and WebKit |
| Git whitespace check | Passed |

The final browser run used the production frontend against the isolated local application with schema 0022, synthetic owner/member accounts and Playwright 1.64.1. The served index matched the built index. [Sanitized browser receipt](2026-10-22-keyboard-browser.json) records each result; authentication state, HTTP bodies/headers, traces, videos and screenshots are excluded.

Each browser executed these five saved workflows:

- With the durable character-shortcut preference disabled, C and G then M do nothing. Mod+K opens the palette; Enter opens the ordinary creation form. Keyboard input and Mod+Enter create an issue, Tab/Shift+Tab reach its status and priority fields, and Mod+Enter saves both through the real API. Keyboard-only palette search reopens the issue and its current recent item. No pointer clicks, programmatic focus calls or injected navigation are used in this workflow.
- A real creation-form picker ignores synthetic composition, key-code 229 and modified editing events without changing the active choice or leaving search focus. Ordinary arrows/Enter then select the assignee and restore trigger focus. These injected events exercise application guards, not a physical OS IME.
- An isolated synthetic member opens an issue in a private team and sees its reauthorized recent title. The owner removes that member through the ordinary versioned membership endpoint. The member's direct issue read returns 404, the open palette and issue content clear after the authority check, and reopening the palette exposes no forbidden recent title. An actual scoped issue search also returns no row.
- Existing saved palette search and renamed-title recents work with character shortcuts disabled.
- Deleting a suggested issue before selection leaves the palette open, removes the stale row, announces the failure and retains search focus.

The first browser run found a real remaining propagation defect: simply ignoring Ctrl+Enter in the picker let its enclosing form run native validation and move focus. The final guard stops propagation without preventing native editing defaults; component and browser reruns pass. The initial workflow's forward-only focus search exceeded its bounded traversal in the populated fixture, so it now uses the ordinary reverse Tab path to the close control. A new API context initially inherited the owner's cookies; the final revocation fixture explicitly starts with empty storage and verifies an unauthenticated session before creating the intended member. All fifteen final cases then passed.

A subsequent review identified a repeated-Enter default-action gap. An isolated native form reproduction sent two real `keyboard.down('Enter')` events before key-up; Chromium, Firefox and WebKit each reported `[false, true]` for repeat state and one unintended implicit submit when only the first event prevented default. Preventing default for every unmodified Enter produced zero submits in all three engines. The picker now prevents that default even on repeat while selecting only on the first event; the focused component regression checks both cancellation and absence of selection. Focused tests, typecheck and production build passed after this narrow correction. The saved application receipt above predates this final repeat-default correction; its fifteen API workflows were not rerun afterward.

## Remaining gates

This evidence completes the saved browser issue workflow and real forbidden-issue recents regression. It does not establish all-core-task coverage, heterogeneous entity search, native desktop operation, physical international keyboard layouts, a real OS IME, NVDA, VoiceOver or Orca. Those host/human checks remain required by the accessibility contract. Issue #18 and the overall F21-A3 qualification must remain open until those broader gates have their own evidence.
