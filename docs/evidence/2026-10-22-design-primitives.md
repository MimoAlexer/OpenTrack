# Shared design primitives browser verification

Date: 22 October 2026. Scope: M01-T05 shared theme, button, input, dialog and combobox behavior in the implemented issue-creation flow. This is application behavior backed by the real API and PostgreSQL, not a component gallery or a claim of complete accessibility compliance.

## Changes and failure/fix checks

- Light muted text previously measured 3.99:1 on the selected surface; the shared token now measures 4.82:1. The light input boundary previously measured 2.73:1 against its panel; the shared control token now exceeds 3:1 on every audited surface.
- Picker triggers and popovers use the essential control boundary. The active option has an outline in addition to its selected/hover surface. Label removal targets meet the 24px minimum.
- Picker placement accounts for the visible height as well as width, flips above low triggers and reacts to scroll, pane size and text enlargement. The list scrolls independently while its search and dismissal controls remain available.
- Keyboard options retain their stable entity identity when asynchronous choices arrive or reorder. Home/End skip unavailable choices, Escape closes only the picker, and selection restores its trigger.
- WebKit exposed pointer clicks opening a dialog without focusing the invoking button. Shared dialogs now remember the pointer invoker, fall back to the keyboard focus owner, and preserve stacked-dialog fallback when the invoker disappears. Native dialog tab behavior also allowed focus to leave the dialog at its boundaries in Chromium/Firefox; the shared boundary trap now wraps in both directions and retains a focusable dialog while all controls are pending.
- Shared buttons, modal headings and footer actions wrap at narrow widths instead of clipping long labels.

## Executed checks

Environment: Playwright 1.64.1; Linux Node 22.22.1; Chromium 156.0.8078.4, Firefox 157.0 and WebKit 27.2. A dedicated disposable PostgreSQL 18 runtime served the production web build through the dedicated loopback acceptance origin. Only synthetic owner/workspace content was used. The existing CI fixture guard remains; local execution requires the explicit disposable mode, fixed HTTP loopback port and the synthetic acceptance identity.

`npm run build` and `npm run typecheck` passed. The focused component/taxonomy regression suite passed 25 checks. The full frontend suite passed 498 tests across 61 files.

`playwright test primitives.browser.ts` passed all 15 scenarios, five per browser:

1. Light controls: 30 resolved-color contrast pairs, native dialog focus/trap, Home/End/arrow/Enter/Escape combobox operation, pending disabled state, and a real saved issue with the selected assignee.
2. The same checks in the dark theme.
3. 320 CSS pixel reflow: issue form and popup remain in bounds, editable input is retained, empty label results are exposed, stacked confirmation keeps or discards the draft and restores the invoker.
4. 640px layout with explicitly doubled computed text sizes and line heights: form/popup bounds and cancel/confirmation remain operable. This is a text-enlargement stress check; it is not a native browser zoom automation claim.
5. Forced colors and reduced motion: keyboard outline stays visible, animations stop and Escape dismisses the dialog.

The 320px viewport exercises the reflow space equivalent to a 1280px window at 400% page zoom. Physical browser zoom and assistive-technology operation remain distinct manual checks.

## Resolved browser contrast

Thirty foreground/background combinations per theme were checked in every engine (180 assertions). Normal text pairs require 4.5:1; focus/control boundaries require 3:1. Representative limiting/semantic pairs:

| Pair | Light | Dark |
| --- | --- | --- |
| Muted text / selected surface | 4.82:1 | 4.92:1 |
| Control border / selected surface | 3.98:1 | 3.03:1 |
| Focus accent / selected surface | 5.09:1 | 6.67:1 |
| Success text / success surface | 5.16:1 | 6.00:1 |
| Warning text / warning surface | 5.10:1 | 6.58:1 |
| Error text / error surface | 5.86:1 | 7.16:1 |
| Primary text / primary fill | 14.12:1 | 11.41:1 |

Rendered screenshots were reviewed for readable fields, outlined keyboard state, popup placement and synthetic-only content: [light](2026-10-22-primitives-light.png), [dark](2026-10-22-primitives-dark.png). They show the actual issue form after its editor loads; executable browser checks provide the behavioral evidence.

## Qualification boundaries

Windows Chromium and WebKit were also exercised. The downloaded Windows Firefox executable could not start on that host; the passing Firefox evidence above uses the separately provisioned Linux runtime. No host policy was changed to bypass that launch failure.

This change does not itself complete F21-A2 or F22-A1: visible alternatives for every global shortcut and command-palette invocation focus have separate implementation/evidence. M01-T05 must be closed only after those companion acceptance criteria pass. Native webview, real OS IME, physical touch and screen-reader qualification are not established by these checks. No schema migration or persisted-data change is required; rollback is a frontend redeploy.
