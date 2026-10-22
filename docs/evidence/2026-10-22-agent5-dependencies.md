# Resolved dependency qualification

22 October 2026. Issue #2 / M01-T01. This records executed lockfile, registry, peer, license-metadata and advisory checks. It does not complete the compatibility spike or establish runtime TLS behavior, complete license compliance, or whole-product security qualification.

## Environment and graph identity

Observed Windows Node **24.14.1**, npm **11.11.0**, Linux Rust **1.98.1** (`48a229cea`) and Cargo **1.98.1** (`797e8a9bc`), target host `x86_64-unknown-linux-gnu`. The pinned compiler remains unchanged; both Rust manifests require 1.98. No package manifest, application source or migration changed.

| Lockfile | Format | Resolved package records | Dependency edges | Maximum registry-declared Rust version |
| --- | --- | ---: | ---: | --- |
| `Cargo.lock` | 4 | 246: 240 registry, 6 workspace | 672 | 1.94.0 |
| `desktop/Cargo.lock` | 4 | 543: 541 registry, desktop, vendored glib | 1,729 | 1.90 |
| `web/package-lock.json` | 3 | 185 | npm tree checked | — |
| `desktop/package-lock.json` | 3 | 12 | npm tree checked | — |
| `scripts/contracts/package-lock.json` | 3 | 33 | npm tree checked | — |

Rust counts describe the complete lock graphs, including platform and optional records, rather than one target's compiled feature graph. All 781 registry package records across the two locks match their exact-version cached registry-index checksums. Declared Rust versions fit the pinned compiler; this alone does not prove compilation on any platform.

Final SHA-256 lock identities:

| File | SHA-256 |
| --- | --- |
| `Cargo.lock` | `929ee4a4e8d71391f5a250940aaed037ebb63fc49e6737038e9e544c871f4db7` |
| `desktop/Cargo.lock` | `63b07a69280c1a372922c9a771c14db98976e3d30d58f47e2054e208daf5c51a` |
| `web/package-lock.json` | `8e2fdbd29514709587cbfcde6a0bafd0b2c99fe5ebb5a6652c1e0c77eeaff009` |
| `desktop/package-lock.json` | `504cb89f1b50a71b946c48d6a3baed5e7012e9c0d70730807036dd4249f5fae2` |
| `scripts/contracts/package-lock.json` | `9c3c80b2fbee99057a1f4723f1b81b321cd5c08b902300aaabbe7840fdf3e5e3` |

## npm results

Executed `npm audit --package-lock-only --json` and `npm ls --all --package-lock-only --json` separately in `web/`, `desktop/` and `scripts/contracts/`. All six commands exited **0**. Each audit returned zero info, low, moderate, high and critical vulnerabilities; each tree returned no dependency problems. No install, audit fix or npm lockfile mutation was performed. These are the registry's known-advisory results on this date, not a claim that the packages have no security defects. [npm audit command contract](https://docs.npmjs.com/cli/v11/commands/npm-audit/).

An independent lock walk checked **85 peer declarations**: 84 in web and 1 in contract tooling. All present peers satisfy their ranges; 26 optional web peers are absent as permitted. It also checked **130 package Node/npm engine declarations**, all satisfied by the observed runtime. The web root's Node range, `^22.13.0 || >=24.0.0`, includes 24.14.1.

| Compatibility boundary | Resolved result |
| --- | --- |
| React / React DOM | Both 19.3.1; their relevant locked peers are satisfied. |
| Tiptap | Core, ProseMirror wrapper, React integration and extensions resolve to 3.32.0; React and type-package peers are satisfied. |
| Vite / React plugin / Vitest | 8.3.4 / 6.1.2 / 5.0.5; the plugin's Vite `^8.0.0` and Vitest's Vite `^6.4.0 || ^7.0.0 || ^8.0.0` peers are satisfied. |
| TypeScript | Web uses 7.0.2. The separate contract graph uses 5.9.3, satisfying `openapi-typescript` 7.13.0's `^5.x` peer. |
| Desktop tooling | JavaScript CLI 2.12.3 and web API 2.12.4 remain at their existing pins; native integration behavior requires its own checks. |

Exact-version metadata was fetched read-only from the public npm registry for all **228 distinct package/version pairs** across 230 lock entries. Every package identity, published integrity value, peer declaration, optional-peer declaration and engine declaration matched its lock entry. This compares registry metadata with lock metadata; it does not independently verify downloaded package archives or execute package code.

License-expression counts from that exact-version metadata:

| Declared expression | Web | Desktop build | Contract tooling |
| --- | ---: | ---: | ---: |
| MIT | 134 | 0 | 26 |
| MIT-0 | 2 | 0 | 0 |
| Apache-2.0 | 27 | 0 | 2 |
| Apache-2.0 OR MIT | 1 | 12 | 0 |
| BSD-2-Clause | 2 | 0 | 0 |
| BSD-3-Clause | 2 | 0 | 0 |
| MPL-2.0 | 12 | 0 | 0 |
| BlueOak-1.0.0 | 1 | 0 | 0 |
| ISC | 3 | 0 | 3 |
| CC0-1.0 | 1 | 0 | 0 |
| Python-2.0 | 0 | 0 | 1 |
| (MIT OR CC0-1.0) | 0 | 0 | 1 |

No npm license expression is missing after registry enrichment; 223 lock entries omit the field themselves. The MPL records are `lightningcss` 1.33.0 and its eleven platform packages. `argparse` 2.0.1 declares Python-2.0. These expressions are distribution-review inputs; no license choice, compatibility ruling or notice-completeness approval is implied. The project's license decision remains governed by [ADR-015](../adr/015_LICENSE.md).

## Rust advisories and bounded patch

The initial `cargo audit --version` and `cargo deny --version` both failed with `no such command` (exit 101). An isolated official **cargo-audit 0.22.2** Linux binary was then used without a global installation. Its [release archive](https://github.com/rustsec/rustsec/releases/tag/cargo-audit%2Fv0.22.2) matched SHA-256 `ab28a1bdb54db4d5d8ad5981cf1f959410370b3d28250dbd35f6a44248620e39`; the extracted executable matched the archive member byte-for-byte, SHA-256 `473b9a71e5cb5bde22f69c32f749c9b83931287d92dc36b91cb04f6705640ef2`. Tar reported mounted-filesystem timestamp/mode-restoration errors; executable content verification and actual execution succeeded. `cargo-deny` remains unavailable.

The private advisory checkout contains **1,349 advisories** at revision `640bf5a9f06e66d8721e8772766531fe77c8630d`, updated `2026-10-22T12:49:47+02:00`. The backend audit fetched this database; desktop and all subsequent runs used the same checkout with `--no-fetch`, no ignored advisories and no target filtering.

| Lock / initial finding | Initial version | Patched version | Primary advisory |
| --- | --- | --- | --- |
| Both: certificate paths may accept distrusted intermediates | `rustls-webpki` 0.103.15 | 0.103.16 | [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html) |
| Backend: bundled trust roots include the affected root | `webpki-roots` 1.0.9 | 1.0.10 | [RUSTSEC-2026-0381](https://rustsec.org/advisories/RUSTSEC-2026-0381.html) |
| Desktop: bundled certificate store includes the affected root | `webpki-root-certs` 1.0.9 | 1.0.10 | [RUSTSEC-2026-0383](https://rustsec.org/advisories/RUSTSEC-2026-0383.html) |

The primary advisories identify certificate-trust failures; they do not establish exploitability in this application's selected OS, TLS policy or deployment. The desktop's `Transport::new` constructs a reqwest client used for authenticated HTTPS requests; the backend enables SQLx's Rustls/WebPKI-roots TLS feature. A lockfile match warrants updating these shared dependencies, while application-specific reachability and runtime TLS regression remain separate qualification work.

Executed under the lane's WSL launcher:

```sh
cargo update --offline --package rustls-webpki --precise 0.103.16
cargo update --offline --package webpki-roots --precise 1.0.10
cargo update --offline --manifest-path desktop/Cargo.toml --package rustls-webpki --precise 0.103.16
cargo update --offline --manifest-path desktop/Cargo.toml --package webpki-root-certs --precise 1.0.10
```

All four updates exited 0. Cargo also selected alternative existing `syn` and `getrandom` edges for two unrelated desktop packages. Their original edges satisfy their registry ranges (`>=2, <4` and `>=0.3.0, <0.5`) and were restored before the final audits. The final patch changes only the version and checksum of the four records above: **8 insertions and 8 deletions across two lockfiles**, with no package additions, removals or dependency-edge changes. `git diff --check` passed. No migration or persisted-data change is involved; restoring the old lock versions would reintroduce these recorded advisory matches.

| Executed audit | Backend | Desktop |
| --- | --- | --- |
| Original locks, cargo-audit JSON | Exit 1; 2 vulnerabilities, no warnings | Exit 1; 2 vulnerabilities, 4 maintenance warnings |
| Patched locks, same database, `--no-fetch` | Exit 0; 0 vulnerabilities, no warnings | Exit 0; 0 vulnerabilities, 4 maintenance warnings |
| Final minimized locks, same database, `--no-fetch` | Exit 0; 0 vulnerabilities, no warnings | Exit 0; 0 vulnerabilities, 4 maintenance warnings |

The original package/advisory matches no longer appear in either final report. No certificate exploit reproduction, compiler run, application test or broad security scan occurred in this dependency task. Build and native/backend regression qualification must run against these final hashes before this increment is accepted.

The desktop's four unmaintained warnings remain visible and unsuppressed: [`proc-macro-error` 1.0.4](https://rustsec.org/advisories/RUSTSEC-2024-0370.html), [`zvariant` 5.15.1](https://rustsec.org/advisories/RUSTSEC-2026-0370.html), [`zbus_names` 4.3.5](https://rustsec.org/advisories/RUSTSEC-2026-0371.html) and [`zvariant_derive` 5.15.1](https://rustsec.org/advisories/RUSTSEC-2026-0372.html). A default audit exit 0 does not dispose of those maintenance risks; no unrelated replacement or major upgrade was made.

## Metadata restoration and remaining qualification limits

The initial `cargo metadata --locked --offline --format-version 1` attempts failed: the backend cache lacked `android_system_properties` 0.1.6, and the desktop cache lacked `adler2` 2.0.1. These failures were retained. Subsequent authorized online restoration ran `cargo metadata --locked --format-version 1` for the backend and the same command with `--manifest-path desktop/Cargo.toml` for desktop. Both exited **0**, fetched only locked dependency sources and emitted all **246 backend / 543 desktop packages and resolve nodes** without compiling. Both final lock hashes above remained unchanged.

| Final Cargo metadata license coverage | Backend | Desktop |
| --- | ---: | ---: |
| Registry packages with a declared license expression | 240 / 240 | 541 / 541 |
| Local vendored glib 0.18.5 | — | MIT |
| Project packages lacking a ratified license | 6 | 1 |
| Registry packages missing license metadata | 0 | 0 |

The final source metadata closes the earlier registry-license inventory gaps. Expressions include permissive alternatives, Unicode/data licenses, LLVM exceptions, five native MPL-2.0 records and LGPL alternatives; the normalized per-package expressions are retained in ignored inventories. The six backend workspace packages and desktop package have no ratified project license. Package license files, notices, system-library licenses and all supported distribution artifacts still require a complete release review. Complete expression inventory does not establish distribution approval.

Raw JSON reports, sanitized metadata inventories, before-lock snapshots and execution logs remain in ignored lane scratch under `work/agent5-dependency-qualification/`; the tool and advisory checkout are under `work/agent5-dependencies/`. Raw Cargo metadata includes upstream authors and local paths and is not part of the committed evidence. Only sanitized package facts and counts are recorded here. No issue was closed or requirement marked complete by this record.
