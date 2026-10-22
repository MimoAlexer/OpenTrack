# F19 Notifications

Stage: MVP in-app; optional email. Delivery: [M07_NOTIFICATIONS](../milestones/M07_NOTIFICATIONS.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Alert the right people about actionable work without flooding them or disclosing private content.

## 2. Functional requirements

Approved expansion obligations: X02, X06, X61. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Events for assignments, mentions, subscribed comments, explicit project updates and agent approval requests later. In-app is durable; email optional with preferences/digests. No self-notification by default. Recipient/event/reason uniqueness and current ACL apply at dispatch.

## 3. Interaction patterns

Settings exposes event/channel toggles and digest frequency. Toast is transient feedback, not notification persistence. Notifications link to the source and show a safe unavailable state after access loss.

## 4. Data requirements

notifications, preferences, deliveries and subscription source; payload stores references and minimal safe metadata, not immutable private body copies.

## 5. API requirements

Preferences CRUD, notification list/status and delivery diagnostics limited to administrators. Source events are internal; clients cannot create arbitrary recipient notifications.

## 6. Authorization rules

Recipient reads own notifications. Worker determines recipients from current authorized subscriptions. Email requires recipient access immediately before sending.

## 7. Edge cases

Repeated event, author edits mention, recipient removed, email delayed, provider timeout, multiple reasons for same update and unsubscribe after fan-out.

## 8. Failure behavior

Core mutation succeeds even if mail fails. In-app persists with delayed channel status. Email retries may duplicate after ambiguous SMTP acceptance; use stable Message-ID and disclose delivery uncertainty.

## 9. Performance considerations

Fan-out batches 100; avoid per-recipient transaction per field change. Optional digest coalesces within configured window while mentions remain explicit.

## 10. Feature dependencies

F11, F10, F25 durable event log, F04; mail adapter optional.

## 11. Acceptance criteria

F19-A1: duplicate event creates one in-app item per reason. F19-A2: no email after recipient revocation. F19-A3: disabled SMTP leaves complete in-app notifications.

## 12. Testing strategy

Consumer crash/replay, preference and unsubscribe races, SMTP ambiguous-result fixture, private-team recipient matrix and delivery-lag load test.
