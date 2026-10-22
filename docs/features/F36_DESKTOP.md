# F36 Desktop client and advanced collaboration

Stage: authorized complete-product scope. Delivery: [M14_ADVANCED](../milestones/M14_ADVANCED.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Bring the established web workflow to a native shell without creating a second business logic implementation.

## 2. Functional requirements

Tauri 2 packages shared static React assets, deep links and optional notifications; connects to chosen self-hosted server. The authorized M14 tracks for MFA, guests, multi-team projects, recurring cycles and CRDT editing require their own scoped ADR/acceptance/security work before changing the stable baseline. Full offline writes are not implied.

## 3. Interaction patterns

First connection asks server URL and trust confirmation; login uses system browser/secure native session handoff. Display current server/workspace. OS notifications carry minimal content by default.

## 4. Data requirements

Local connection metadata and OS keychain secret references; no default local replica of all issue data. Server remains authoritative. CRDT documents need a versioned migration/checkpoint schema before adoption.

## 5. API requirements

Reuse public REST/stream contracts; native auth tokens have separate audience and revocation. Deep links validate scheme/path and never execute arbitrary local commands.

## 6. Authorization rules

Tauri capability allowlist grants minimum OS actions; remote content cannot invoke native privileged commands. Normal workspace/team ACL still enforced server-side.

## 7. Edge cases

Old webview, untrusted server certificate, deep-link injection, app update interrupted, Linux package differences and browser/native session drift.

## 8. Failure behavior

Unsupported webview shows actionable compatibility message. Failed signed update retains prior version. Lost connection follows web read-only/draft policy.

## 9. Performance considerations

Target cold launch<=3s on reference desktop; measure process footprint and webview behavior. Reuse route splitting; no bundled Node server.

## 10. Feature dependencies

Core MVP and public API stable; design accessibility; explicit ADR before each advanced track.

## 11. Acceptance criteria

F36-A1: native shell completes same golden workflow. F36-A2: untrusted remote page cannot invoke filesystem/process APIs. F36-A3: signature-invalid update rejected. F36-A4: no claim of offline sync without separate tests.

## 12. Testing strategy

Windows/macOS/Linux smoke matrix, capability/deep-link security tests, signed-update interruption, keyboard/assistive-tech checks and web/native contract parity.
