# ADR-015 Open-source licensing and project identity

Date:2026-10-17. Status: Proposed; maintainer ratification required.

## Problem

The user requires free open-source self-hosting, but source license, contributor rights and public naming need a deliberate maintainer decision.

## Options and tradeoffs

MIT/Apache are permissive and easy to adopt, with fewer obligations on hosted derivatives. AGPL preserves network-source availability but adds compliance friction and dependency-distribution review. Dual licensing adds governance/commercial complexity not required now.

## Decision

Recommend AGPL-3.0-or-later for the eventual product and transparent contribution policy. Keep optional proprietary/provider runtimes separately obtained with their own terms. Do not claim an unratified license grant or copy competitor code/assets.

## Consequences and risks

License is PROPOSED, not ratified by this planning session. The unrelated opentrack head-tracking project already uses the working name; public identity/trademark clearance remains unresolved. Copyleft compatibility and SDK distribution require review before release.

## Reconsideration and validation

Maintainer ratifies license/contribution terms and name before public release. This does not block private implementation M01. Verify intended terms from authoritative license text and dependency licenses at release.

See [canonical specification](../PRODUCT_VISION.md).
