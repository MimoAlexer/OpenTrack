# Interaction patterns

## Navigation and focus

App shell has skip link, navigation landmark and main landmark. Route changes update document title and focus the page heading only for full navigation, not for background refetch. Opening detail aside focuses its heading; closing returns to the exact issue row, restoring virtualized scroll anchor if necessary. Browser Back restores view/filter/selection context. If the originating object disappeared, focus the nearest surviving row or list heading.

One overlay stack owns Escape: dismiss top menu, then dialog/aside, then clear list selection; never clear an editor draft silently. Modal focus cannot escape into sidebar. Opening palette over a dialog returns to that dialog. Tooltips appear on keyboard focus as well as hover and never carry the only required instruction.

| Input | Behavior | Scope guard |
| --- | --- | --- |
| Mod+K | Open command palette | Available outside browser-reserved contexts |
| C | Create issue in current team | Disabled in input/editor/IME and when single-key shortcuts disabled |
| / | Open/focus search | Same guards as C |
| G then I / G then M | Inbox / My work |800 ms sequence timeout, no typing capture |
| Arrows, Enter | Move focused result, open | Only focused list/combobox; preserve page navigation elsewhere |
| Space | Toggle selected issue | Focused list checkbox/selection context; not body-wide |
| Mod+Enter | Submit comment/create dialog | Valid current form, never double-submit while pending |
| Escape | Unwind one layer | Preserve drafts; explain unsaved navigation when necessary |
| Shift+F10 | Context menu | Focused issue/card; same actions as visible overflow |

Use event.key for printable shortcuts, platform Mod for Ctrl/Command, respect AltGr, and ignore repeated keydown where it would duplicate a write. Shortcut help can disable single-character shortcuts entirely. User remapping is later; registry reserves collisions now.

## Selection, pointer and drag

Plain click title opens; checkbox selects; Shift+click selects a contiguous loaded range. Explicit Select all matching asks server for a bounded snapshot and names count; Ctrl+A outside list scope retains browser behavior. Selection toolbar announces count and current scope. Right-click menu provides no exclusive action.

Board drag begins from handle after 6 px pointer movement or 200 ms touch hold, with a visible lifted card. During drag, show target status/position and announce it. Escape cancels. Drop sends a versioned move; failed move restores only the affected optimistic layer and announces reason. Always provide Move to… and Move before/after commands. Dragging under nonmanual sort changes status only; it never pretends to persist arbitrary order. Server rank uses sparse numeric ordering with bounded rebalance and version checks; exact encoding chosen in M03 after collision tests.

## Save, conflict and offline behavior

Simple field changes save immediately with optimistic feedback. Editor saves after 750 ms idle outside IME composition; show Saving…, Saved, Unable to save or Conflict. New issue dialog creates all submitted fields/body in one transaction. Comments submit explicitly. A pending request is not a durable save; closing with unsaved draft offers stay/discard or an explicitly labeled session draft.

On 412 keep user's values, load current server values, show changed fields and Apply my changes as a new versioned command. Text conflict opens side-by-side or stacked readable comparison, never an automatic overwrite. Undo is a new command that may conflict or fail if authority changed. Bulk partial failures show per-item results with retry only for failed safe items.

Offline banner explains read-only data and disconnected state. Draft retention is explicit and session-scoped; credentials never use localStorage. On reconnect bootstrap/access epoch first, then offer retry of the user's draft. No silent queue of stale mutations.

## Loading and errors

Show immediate control feedback within 50 ms. Delay skeleton until 150 ms to avoid flicker; reserve geometry. For search retain previous results while a new request is pending and label the active query. After 5s expose slower-than-usual text and cancel/retry when safe. Errors appear near the action with a short request ID and persistent recovery; a toast alone cannot be the only evidence of failed save.

Empty states: no issues => Create issue and optional import; no filtered matches => Clear filters; no inbox => current and quiet; no providers => configure or keep using core; no permission => unavailable message and return navigation, without leaked name. Agent output retains partial logs with interrupted/failed label; provider success is distinct from tests passing and PR creation.

## Responsive behavior and motion

At>=1200 px show sidebar and optional detail aside;768–1199 px collapse navigation and use full detail if necessary; below 768 px show a single task pane with drawer navigation. Priority properties move into a labeled details disclosure, not vanish. Board remains horizontally scrollable with list alternative. Form actions remain reachable at 400% zoom without two-dimensional scrolling except inherently spatial content with a textual alternative.

Use 120–160 ms opacity/transform feedback for pane/menu entry; no animated reordering while someone is selecting text. Incoming items produce a New updates control instead of shifting the viewport. Reduced motion removes transform, shimmer, auto-scroll and smooth scrolling. Persist theme/density through authenticated preferences and a non-sensitive pre-paint theme hint only.
