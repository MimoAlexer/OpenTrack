# Attachment storage and installed-scanner qualification

22 October 2026. Issues #23/#25; F13-A3. These qualification targets use the production storage adapter and API router with real PostgreSQL. They add no production fallback or failure-control endpoint. Hosted acceptance of this increment remains pending; native picker/window behavior is separate.

Local execution used the assigned Ubuntu 26.04 WSL/PostgreSQL 18 runtime and private Cargo target under the shared verification lock. The production adapter/API source is unchanged in integrated `e3716adf19ab151a5fa19e26bd011ffb22978fb3`; only these qualification targets and their launcher guards were added.

Independent review of the first published head found that compilation deadlines stopped only the direct Cargo process. Both launchers now use the same small Linux process-session supervisor for compilation and execution. It preserves captured compiler JSON and diagnostics, defers interruption during spawning, and waits for bounded TERM/KILL cleanup of every live owned group member before returning or raising. Parent SIGTERM and SIGINT follow the same cleanup path; unrelated processes are excluded.

Four actual descendant regressions passed in 6.63 seconds: compilation timeout, parent SIGTERM, a successful leader leaving a child, and a failed leader leaving a child. Each uses an operating-system lock and verifies no live compiler/linker fixture remains when another process acquires it, while an unrelated process remains alive. The normal and failure cases also preserve captured JSON and stderr. These tests run in CI before scope classification. After this correction, both real exhaustion cases and the real ClamAV 1.5.4 API case passed again under the verification lock; final inspection found no owned scanner process or qualification temporary directory. The first head remains superseded pending fresh hosted checks and independent review of the correction.

## Actual storage exhaustion

`bash scripts/qualify-attachment-storage.sh` builds outside the fixture, then creates a private user/mount namespace, namespace-owned temporary ancestry and separate 4 MiB tmpfs mounts for the adapter and API cases. The launcher requires four distinct restricted roles on one loopback `_test` database. It takes the shared verification lock when the local coordinator exists.

The adapter receives actual kernel `ENOSPC` during a 2 MiB upload with a 3 MiB filler occupying the mount. No published or staged object remains. Removing the filler permits the same immutable key to upload and download with the expected length and SHA-256.

The production API router returns 503 `dependency_unavailable`, leaves the same reservation pending with its current validator, denies content with 404 and publishes no object. Removing the filler permits a conditional upload against the same reservation; it becomes ready and downloads exact bytes/hash with one published object.

Both real cases passed after final guard changes. Eight direct-invocation refusal cases rejected absent/current namespace markers and linked/nonempty roots before storage writes, preserving sentinel bytes. Both targeted strict Clippy checks, formatting and Bash syntax passed. Cleanup found no remaining qualification directories.

The initial fixture exposed correctly rejected ancestor ownership after UID mapping. Mounting namespace-owned temporary ancestry corrected the fixture; no production filesystem validation was weakened. A later allocation-only Clippy warning in the new guard was corrected without relaxing it.

This proves bounded Linux tmpfs exhaustion and recovery, not physical-device capacity, PostgreSQL/WAL exhaustion or every supported host filesystem.

## Installed scanner

`bash scripts/qualify-attachment-scanner.sh` requires an explicitly trusted `OPENTRACK_TEST_CLAMD_BIN`, four isolated database roles and ClamAV 1.5 or newer. It launches one foreground daemon with a private Unix socket, bounded queues/bytes/time, a clean process environment and a private custom-only signature database. It stops only its process group and removes only its private directories.

The executed engine was ClamAV 1.5.4 from checksum-verified official Ubuntu packages, extracted into private development tooling without package scripts or a system service. A private SHA-256 file-hash signature identifies benign opaque fixture bytes using the [official hash-signature format](https://docs.clamav.net/manual/Signatures/HashSignatures.html).

| Real router/adapter/daemon path | Observed result |
| --- | --- |
| Clean upload | Ready/clean; authorized exact-byte/hash download. |
| Matching private signature | Rejected/infected; content unavailable. |
| Owned daemon terminated | Upload remains quarantined/unavailable; content 404. |
| Finalize while daemon is down | Same reservation remains quarantined/unavailable; content still 404. |
| Same daemon/config/database restarted | Same reservation becomes ready/clean; exact-byte/hash download. |
| Repeated finalize | Original result; exactly three reservations overall. |

The final actual scanner/PostgreSQL/API case passed in 2.85 seconds; strict targeted Clippy, formatting and Bash syntax passed. Final cleanup found zero owned scanner processes, qualification roots and version-check directories.

The first real daemon start exposed a missing compiled certificate directory in the private package extraction. The fixture now supplies its own empty `CVDCertsDirectory` for its custom-only database. No trust-verification option is disabled. This option is present in the [tagged 1.5.4 configuration](https://github.com/Cisco-Talos/clamav/blob/clamav-1.5.4/etc/clamd.conf.sample); the launcher verifies the actual engine version before compiling. Synthetic older-version, malformed-output and nonzero-exit fixtures were refused before a Cargo canary could run, with temporary-directory cleanup. Those are guard tests, not real older-engine acceptance.

This profile does not qualify official CVD feeds, updates/certificate trust, malware breadth, deployed TCP HTTP transport or native GUI behavior. Forced outer termination may leave a synthetic database schema even though owned daemon/directory cleanup is bounded; ordinary exercise failures run the harness cleanup.

## Integration and remaining gates

The separate cold-recovery harness exercises populated blob restoration and corruption/missing-object refusal. These tests do not substitute for that deployed restore, private-team restore denial or complete DB/blob/key recovery. All required hosted checks must pass on the published immutable head, then the integrated source must be requalified before ledger acceptance. #23/#25, native interaction and broader product gates remain open.
