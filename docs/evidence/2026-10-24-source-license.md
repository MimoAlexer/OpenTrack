# Source-license ratification

24 October 2026. AGPL-3.0-or-later was explicitly selected for original project
source. [ADR-015](../adr/015_LICENSE.md), contributor guidance, current planning
references and project metadata now record that decision.

[LICENSE](../../LICENSE) is an unchanged34,020-byte copy from the SPDX license
list's pinned source. Its SHA-256 is
`d8a6cc31abc16b6748c7a21f21611f5a1ec33f67d22ca23d7da1c19b95496bee`;
the [receipt](2026-10-24-source-license.json) records the immutable source URL.
The [SPDX identifier](https://spdx.org/licenses/AGPL-3.0-or-later.html) states the
chosen later-version option. No personal project author or copyright identity
was added.

Locked offline Cargo metadata passed for all six backend workspace packages and
the separate desktop package, each declaring the same license. All three npm
project manifests and only their root lock metadata changed; dependency versions
and edges did not. Both Cargo locks and the vendored MIT manifest remain unchanged.

Fresh hosted verification at `ff03218d` caught a live OpenAPI/generator mismatch:
the declared license appeared in the live schema while the committed input still
contained an empty license. Regeneration changed only `info.license.name` and
`info.license.identifier`; TypeScript output remained byte-identical. The API
reads licensing with tracked `env!` metadata so cached builds refresh it too.
The normal generator check, one real TCP router contract, formatting, strict API
Clippy and server rebuild passed. The original hosted failure is retained; its
other green checks do not qualify the corrective head.

The user installer now stages the license before destination writes and copies
it beside the managed executable with mode0644. Sixteen synthetic installation
tests passed with no skips, covering exact bytes/modes, managed updates,
unowned/symlink paths, unreadable sources and private failure reporting. No real
user installation or application launch was performed. Atomicity remains per
file across four installed files.

The active Debian target declares the project license file, and the project
container stages copy it without relabeling upstream components. These mappings
were source-checked; actual new Debian/image artifact inspection remains separate.

This ratifies source licensing only. Contribution terms, public identity,
dependency compatibility, distribution notices/SBOM, signing and release
publication remain explicit gates. Fresh exact-head CI and final integration
review are still required; M01-T01 and distribution acceptance are not complete.
