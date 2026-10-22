# F32 Webhooks

Stage: Post-MVP inbound Git; generic outbound M10. Delivery: [M10_AUTOMATION](../milestones/M10_AUTOMATION.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Let external systems receive selected changes and provide authenticated events without coupling request latency to them.

## 2. Functional requirements

Approved expansion obligations: X58. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Inbound GitHub raw-body HMAC and delivery dedupe; outbound configured event subscriptions signed with OpenTrack secret, timestamp and stable event ID. At-least-once retry, replay tooling and disabled destination after repeated permanent failure.

## 3. Interaction patterns

Settings shows endpoint, event types, last delivery, response class and test delivery action. Secret shown only on generation; rotation explains overlap window.

## 4. Data requirements

webhook_inbox, subscriptions, deliveries and encrypted secrets; outbound payload holds only granted event fields, not whole DB records.

## 5. API requirements

Webhook subscription CRUD/test/rotate and delivery read/retry. Receiver must verify timestamp/HMAC and deduplicate event ID. Our inbound GitHub handling does not invent provider timestamp signatures.

## 6. Authorization rules

Workspace admin configures authorized scope; dispatch rechecks connection/team rights. User-provided URLs pass SSRF egress policy.

## 7. Edge cases

DNS rebinding, expired secret, duplicate request, timeout after acceptance, redirect,410 receiver gone and revoked access while queued.

## 8. Failure behavior

Commit inbox before ACK; unavailable DB gives failure so provider can retry. Outbound exponential jitter max 5 attempts then dead-letter; ambiguous delivery remains at-least-once, not exactly-once.

## 9. Performance considerations

Payload<=256 KiB, outbound timeout 15s, bounded per-host concurrency; HMAC over original bytes. Large event data fetched through authorized API, not webhook expansion.

## 10. Feature dependencies

F25 events, F04/F30, F29 retry UI, F26 Git consumer.

## 11. Acceptance criteria

F32-A1: altered signature cannot enqueue. F32-A2: duplicate delivery performs one DB effect. F32-A3: private/metadata endpoint blocked even after DNS change. F32-A4: secret rotation supports bounded old/new verification.

## 12. Testing strategy

HMAC vectors, redelivery/body-hash mismatch, SSRF/rebinding fixture server, timeout/retry/dead-letter and cross-team payload checks.
