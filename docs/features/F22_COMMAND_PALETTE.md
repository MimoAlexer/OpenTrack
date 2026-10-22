# F22 Command palette

Stage: MVP. Delivery: [M03_ISSUES](../milestones/M03_ISSUES.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Expose navigation and contextual actions through one discoverable searchable interface.

## 2. Functional requirements

Commands, recent accessible items and scoped entity search; group navigation/actions/results. Contextual commands operate on explicit current selection, with preview for destructive or bulk action. No arbitrary shell or AI tool entry.

## 3. Interaction patterns

Mod+K opens modal combobox; input focused, arrows move, Enter invokes, Escape closes and restores origin focus. Show command scope and shortcut. Search results never mix workspace identities invisibly.

## 4. Data requirements

Static command registry plus per-user recent references; recents are reauthorized and never retain inaccessible titles across membership changes.

## 5. API requirements

Palette search calls bounded search/suggestion endpoints; mutations use ordinary API contracts, versions and confirmation UI.

## 6. Authorization rules

Only authorized commands/results shown; server still checks invocation. Switching workspace clears previous palette results.

## 7. Edge cases

Empty selection, nested modal, target deleted after suggestion, slow search arriving out of order and destructive command matching a vague query.

## 8. Failure behavior

Keep palette open on validation error; stale item gives unavailable state. Cancel obsolete result requests and ignore outdated responses.

## 9. Performance considerations

Local command matching<20 ms for 500 commands; remote search debounced 150 ms, max 20 results; no editor bundle on palette open.

## 10. Feature dependencies

F21 registry, F24 search, F04 capabilities, modal accessibility.

## 11. Acceptance criteria

F22-A1: focus returns to invoking control. F22-A2: stale remote results cannot replace newer query. F22-A3: forbidden issue title never appears in recent items.

## 12. Testing strategy

Combobox/dialog component tests, request ordering, authorization transitions, keyboard-only create/status action and screen-reader announcements.
