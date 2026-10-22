# Component specifications

All controls use [tokens](DESIGN_TOKENS.md), [interaction rules](INTERACTION_PATTERNS.md) and [accessibility requirements](ACCESSIBILITY.md). shadcn components are reviewed source starting points, not proof that a composed screen is accessible.

| Component | Anatomy and state contract | Input/accessibility contract |
| --- | --- | --- |
| Button / icon button | Label, optional leading icon, optional pending indicator; primary/secondary/ghost/danger; idle/hover/focus/pressed/pending/disabled | Native button; no duplicate submission; pending label announces once; icon-only control has name and tooltip; destructive action names target |
| Text field | Persistent label, input, optional hint, error linked below; default/focus/invalid/readonly/disabled | Native label/id; aria-invalid/describedby; errors after blur or submit, not every keystroke; no placeholder-only labels |
| Combobox | Trigger/input, result list, selected value, clear action, empty/loading/error | Single active descendant, arrows/Home/End/Enter/Escape; selected checkmark+text; async results preserve typed value |
| Multi-select | Searchable options, selected chips, remove buttons, selection count | No listbox nested interactive controls; chips separate named remove controls; select all only visible eligible scope |
| Status/priority badge | Icon+short text, optional picker affordance; fixed line height 20 px | Color never sole meaning; static badge is not a button unless it changes value |
| Avatar | Image or two-character fallback, optional presence later | Name supplied by surrounding label or alt; decorative duplicate has empty alt; no initials-only identity in important confirmation |
| Issue row | Checkbox, status, key, title, selected metadata, overflow; hover/focus/selected/pending/conflict | Real table/list semantics chosen once; row navigation separate from checkbox; title link remains normal link; action names include issue key |
| Issue list | Header, grouping labels/counts, virtualized viewport, load-more region, selection toolbar | Nonvirtualized paginated accessibility mode available; logical row positions and focused row retained; no thousands of focusable controls |
| Board/card | Ordered column headings/counts, compact card, move menu, optional drag handle | Keyboard Move to status/position controls always available; drag announces origin/destination and has Escape cancel; horizontal scroll does not capture page keys |
| Dialog | Title, optional description, content, actions, close; viewport-aware max height | Modal focus trap, initial meaningful field, Escape policy, restore focus; destructive modal initial focus on safe choice |
| Popover/menu | Anchored surface, grouped actions, shortcuts, optional explanation | Appropriate menu roles only for actions; standard form popover uses normal controls; dismiss outside click unless unsaved state |
| Editor | Toolbar, editable surface, attachments, save state, conflict notice | aria-label, documented formatting shortcuts, IME safe, toolbar roving focus; text body not intercepted by global commands |
| Activity entry | Actor, action, time, safe change summary, permalink | Logical chronological order; meaningful grouping; changes not continuously announced while reading |
| Toast | Short result, optional Undo/Retry, dismiss; max 3 stacked | status for success, alert only urgent errors;5s default, pause on focus/hover, persistent errors also inline; no essential action disappears without another path |
| Empty/error state | Context title, explanation, one primary next action, optional retry/request ID | Heading hierarchy, no decorative illustration required; distinguish no data, no matching results and no access |
| Loading skeleton | Reserves final geometry; delayed appearance after 150 ms; no fake content | aria-busy on region; one loading announcement; no shimmer with reduced motion |
| Agent state badge | Queued/running/waiting review/interrupted/failed/succeeded plus icon and elapsed time | Do not use a spinner for waiting human approval; terminal state includes test/publication qualification |
| Log viewer | Timestamp, stream, severity, chunk sequence, search, pause-follow, download | Plain text, escapes ANSI/control sequences; fixed maximum DOM lines; user can stop live follow and select text; no token-level live-region spam |
| Diff/review | Files, added/deleted lines, base/head SHA, test results, approval controls | Unified textual fallback; syntax color supplemental; approval controls expose exact immutable revision and stale state |
| Notification row | Reason icon, source label, time, unread marker, actions | Unread conveyed through text/aria label, not weight alone; archive restores focus to next row |
| Confirm-impact panel | Operation name, target scope/count, recoverability, required acknowledgement | Specific consequential actions only; ordinary reversible field edits do not prompt repeatedly |

## Shared data and interaction state

Every async component accepts idle/loading/success/empty/error/forbidden/stale state explicitly. Loading does not erase known data during refetch; label stale data where needed. Pending writes identify the affected field/row. Disabled reason is available by text/tooltip without relying on pointer hover. Error recovery preserves input and selection. Components never contain ad-hoc permission logic; they consume capability hints and invoke authorized commands.

Picker option IDs use stable entity IDs; labels may change. A deleted or newly inaccessible selected value renders a neutral unavailable placeholder and requires correction before submit. Do not substitute a different valid entity silently. Menus close after a successful command initiation; dialogs stay open while local validation fails.

## Component acceptance

M01/M03 component fixtures cover both themes, compact/touch density, RTL-ready logical spacing even though full localization is later, long names, empty options,200% zoom, reduced motion, validation error and pending state. Test real keyboard/focus behavior and accessible names, then use screenshot review for alignment/contrast. Do not test an implementation detail such as a Tailwind class string as a substitute for behavior.
