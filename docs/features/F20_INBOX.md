# F20 Inbox

Stage: MVP. Delivery: [M07_NOTIFICATIONS](../milestones/M07_NOTIFICATIONS.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Provide a focused place to process personal notifications without confusing reading with completing work.

## 2. Functional requirements

Approved expansion obligations: X02. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Unread/all/archive filters, read/unread, archive and snooze until a chosen time. Mark-all-read affects only items up to a supplied snapshot cursor. Archive changes inbox state, not the underlying issue.

## 3. Interaction patterns

Two-pane list/source preview, keyboard next/previous and action toolbar. Focus remains on next item after archive; snooze displays local time and UTC-equivalent tooltip. Empty state distinguishes no activity from filtered-out items.

## 4. Data requirements

notification read_at/archived_at/snoozed_until and user preferences. Snooze return is a scheduled state projection, not repeated issue event creation.

## 5. API requirements

Recipient notification patches and bounded batch mark-read/archive, each idempotent. Inbox query excludes future snoozes unless requested.

## 6. Authorization rules

Recipient-only; source pane uses normal entity access. Lost source access replaces title/body with unavailable wording.

## 7. Edge cases

New notification during mark-all, DST snooze, deleted source, multiple tabs and stale unread badge.

## 8. Failure behavior

Failed optimistic archive restores that item and focus; badge recomputes after reconnect. Retry never marks future arrivals read.

## 9. Performance considerations

Page 50, partial unread index, count cached per user with event invalidation. Do not fetch every source body for inbox rows.

## 10. Feature dependencies

F19 notifications, F21 keyboard, F25 sync, F04.

## 11. Acceptance criteria

F20-A1: mark-all does not swallow a concurrent new arrival. F20-A2: snoozed item returns at intended instant. F20-A3: archive leaves issue unchanged and is keyboard accessible.

## 12. Testing strategy

Time-controlled scheduler, concurrent arrival/mark-all, two-tab reconciliation, focus restoration and inaccessible-source rendering tests.
