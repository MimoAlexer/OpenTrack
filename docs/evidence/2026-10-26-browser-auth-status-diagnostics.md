# Synthetic authentication status diagnostics

Minimal diagnostic delta over `a08c4ee00eacd29f65ed9d56296092d59d3fc5c7`. The retained failed receipt has210/240 passes: five Chromium and25 WebKit failures. Its first failure is the existing invitation status assertion; the sanitized location does not establish the HTTP cause.

Two annotation lines capture only operation/status before that unchanged assertion. The reporter strictly validates a two-field enum/integer schema and recognizes exact existing owner-fixture error formats. It adds one safe failed-check object and immediate diagnostic line. Unknown, duplicate, malformed or poisoned inputs are withheld. No raw auth data is added; fixtures.ts remains unchanged.

**15 reporter privacy tests and both TypeScript checks passed**, using local Node24.14.1, TypeScript7.0.2 and pinned Playwright package1.64.1. Original output and source hashes are in the JSON. Exact-byte checks preserve all keyboard assertions and frozen fixture/runner/Compose/profile inputs. `git diff --check` passed.

No app/browser/services, containers, production policy, waits/retries/timeouts, CI/head or full-matrix execution changed. This adds observability only; actual status/cause needs fresh configured CI under the lead.
