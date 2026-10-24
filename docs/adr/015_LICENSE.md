# ADR-015 Open-source licensing and project identity

Date:2026-10-17. Source-license decision ratified:2026-10-24. Status: Accepted for source licensing; contribution and public-identity decisions remain open.

## Problem

The user requires free open-source self-hosting, but source license, contributor rights and public naming need a deliberate maintainer decision.

## Options and tradeoffs

MIT/Apache are permissive and easy to adopt, with fewer obligations on hosted derivatives. AGPL preserves network-source availability but adds compliance friction and dependency-distribution review. Dual licensing adds governance/commercial complexity not required now.

## Decision

Use `AGPL-3.0-or-later` for original project source. The repository [LICENSE](../../LICENSE) contains the unmodified license text; Rust and npm project packages declare the matching SPDX identifier. The source-license choice was explicitly ratified on24 October2026. Third-party and vendored components retain their existing licenses. Keep optional proprietary/provider runtimes separately obtained with their own terms.

## Consequences and risks

Source licensing is ratified. This does not establish third-party compatibility, complete distribution notices, system-library or SDK disposition. Public identity/trademark clearance and contribution policy remain unresolved release work.

## Reconsideration and validation

The [SPDX identifier and license text](https://spdx.org/licenses/AGPL-3.0-or-later.html) distinguish the chosen later-version option from AGPL-3.0-only. Recheck dependency licenses, corresponding-source delivery and notices for each distributed artifact before release. Ratify contribution terms and public identity separately; this decision does not authorize release publication.

See [canonical specification](../PRODUCT_VISION.md).
