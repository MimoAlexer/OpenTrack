# F21 Keyboard shortcuts and navigation

Stage: MVP. Delivery: [M03_ISSUES](../milestones/M03_ISSUES.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Make common work efficient without excluding people using assistive technology, IME or non-US keyboards.

## 2. Functional requirements

Approved expansion obligations: X07, X55. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Central shortcut registry with scope, label, enabled predicate and command ID. Mod+K palette, C create, / search, G then I inbox, G then M my work, Escape unwind; single-character shortcuts can be disabled and never fire in inputs/contenteditable or IME composition.

## 3. Interaction patterns

Help dialog shows active platform key labels, searchable commands and conflict notes. Arrow keys navigate focused list; Enter opens, Space selects only in supported list context. Native browser/text shortcuts remain intact.

## 4. Data requirements

user shortcut_mode preference; no arbitrary executable keybinding scripts. Commands share IDs with menus/palette.

## 5. API requirements

Preference update only; shortcuts invoke ordinary authorized API actions. Capability-disabled shortcuts announce why or remain unavailable.

## 6. Authorization rules

Shortcut availability follows current UI action policy, with server authorization unchanged.

## 7. Edge cases

IME, screen-reader browse mode, AltGr, held/repeated keys, nested dialogs, focus in editor and international physical key layouts.

## 8. Failure behavior

Unrecognized sequence times out after 800 ms without swallowing later text. Failed action uses the same error state as pointer action.

## 9. Performance considerations

One scoped dispatcher, no listener per row; command invocation target<50 ms client work. Avoid global key traps and expensive filtering on keydown.

## 10. Feature dependencies

F22 command IDs, design interaction/focus system, all actionable features.

## 11. Acceptance criteria

F21-A1: typing C in a comment never opens create. F21-A2: every shortcut has a visible/menu alternative. F21-A3: single-key shortcuts can be disabled and all core tasks remain possible.

## 12. Testing strategy

Registry collision tests, editable/IME event tests, keyboard-only golden workflow and manual NVDA/VoiceOver-style navigation assessment on supported platforms.
