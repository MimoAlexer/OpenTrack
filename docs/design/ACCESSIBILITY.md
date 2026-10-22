# Accessibility specification

Target WCAG 2.2 AA for shipped core screens, with manual assistive-technology verification. This is an acceptance target, not a claim of compliance. [WCAG2.2](https://www.w3.org/TR/WCAG22/) and [WAI modal-dialog pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) are the primary references; product behaviors below apply them.

## Nonnegotiable behavior

Use semantic buttons, links, headings, forms, lists and tables before ARIA. Every action has a keyboard path and accessible name. Color is supplemental. Normal text contrast>=4.5:1; large text>=3:1; essential control boundaries/focus>=3:1 against adjacent backgrounds. Decorative dividers may be lower contrast. Disabled controls are not a reason to make explanatory text unreadable.

Focus is visible, not obscured by sticky UI, and restored after modal/pane dismissal. Dialogs trap focus and provide a close path. List virtualization preserves logical order and focused row, with a nonvirtualized paginated mode if screen-reader behavior is inadequate. A board always offers a list equivalent and non-drag move controls. Charts provide tables and summary text. Essential target size is at least 24×24 px with sufficient spacing; touch actions target 44×44 px.

Reflow at 320 CSS pixels or 400% zoom must preserve operation and reading order; complex board/diff views may scroll spatially only with list/text alternatives. Text zoom 200% cannot clip labels/buttons. Use logical CSS properties and room for translated/long names. Initial language is English with locale-aware time/number formatting; full translation catalogs are later work, but code must not concatenate inaccessible English sentence fragments.

Single-character shortcuts can be disabled and do not trigger inside editing or IME. Native browser find, copy, paste, undo and zoom retain expected behavior. Do not rely on hover, double-click or drag as an exclusive action. Password fields support autocomplete and paste; no cognitive puzzle authentication.

## Dynamic content

Use aria-busy for a loading region and polite status for completed user actions. Avoid announcing every token/log line/realtime event. AI/log panels offer Pause live updates and a separate summary announcement at state transitions. Critical errors may use alert once, with focusable persistent recovery. Toasts pause their timeout on focus/hover and duplicate important state inline. Reduced-motion setting removes transforms, shimmer, smooth/automatic scrolling and animated list reordering.

Form errors link to fields with describedby; after failed submit focus an error summary only when several fields need correction, otherwise the first invalid field. Do not discard filled values. Required fields are labeled in text. Date pickers accept typed dates with locale example and expose team timezone; impossible/ambiguous input gets a clear error rather than guessed UTC conversion.

## Verification matrix

| Layer | Required check | Release evidence |
| --- | --- | --- |
| Tokens | Compute contrast for text on canvas/surface/raised, semantic labels on semantic surfaces, primary button and focus/control borders | Checked ratios and exceptions recorded; no assumption based on hex appearance |
| Components | Accessible names/roles, keyboard focus, combobox selection, modal escape/return, readonly/disabled state | Component tests plus manual reference browser inspection |
| Core flows | Login, workspace switch, create/edit issue, status move, editor/comment, search, inbox and import preview | Keyboard-only E2E and human run notes |
| Assistive technology | At least NVDA with Firefox/Chromium on Windows and VoiceOver with Safari on macOS; Linux Orca assessment for desktop target | Versioned manual report before claiming supported combinations |
| Visual adaptation | Light/dark,200% text,400% zoom/320 px reflow, forced colors, reduced motion, touch target sizing | Screenshots and actual operation, not screenshot-only pass |
| Agent/review | Stream pause, diff textual alternative, exact artifact labels, cancel/review focus | Manual workflow and automated state tests before agents ship |

Use axe-core/Playwright accessibility checks for automatable issues, but they cannot establish full WCAG conformance. Block releases on known keyboard traps, inaccessible destructive approval, missing names, failed contrast or loss of content at zoom. Track remaining issues with affected users and concrete workarounds, not a blanket claim that the UI library handles accessibility.
