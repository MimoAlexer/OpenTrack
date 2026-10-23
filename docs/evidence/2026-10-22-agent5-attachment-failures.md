# Attachment storage and installed-scanner qualification

Initial record: 22 October 2026. Issues #23/#25; F13-A3. These qualification targets use the production storage adapter and API router with real PostgreSQL. They add no production fallback or failure-control endpoint. The final increment and integrated main passed hosted qualification; [23 October integrated evidence](2026-10-23-agent5-integrated-qualification.md) records the exact source and jobs. Native picker/window behavior remains separate.

Initial local execution used the assigned Ubuntu 26.04 WSL/PostgreSQL 18 runtime and private Cargo target under the shared verification lock, against baseline `e3716adf19ab151a5fa19e26bd011ffb22978fb3`. The increment added qualification targets and guards rather than production failure controls. Integrated `590725e9` subsequently requalified the targets with the accepted project-document changes and schema `0026`.

Independent review of the first published head found that compilation deadlines stopped only the direct Cargo process. Both launchers now use the same small Linux process-session supervisor for compilation and execution. It preserves captured compiler JSON and diagnostics, defers interruption during spawning, and waits for bounded TERM/KILL cleanup of every live owned group member before returning or raising. Parent SIGTERM and SIGINT follow the same cleanup path; unrelated processes are excluded.

The first cleanup correction passed four actual descendant regressions in 6.63 seconds: compilation timeout, parent SIGTERM, a successful leader leaving a child, and a failed leader leaving a child. Each uses an operating-system lock and verifies no live compiler/linker fixture remains when another process acquires it, while an unrelated process remains alive. The normal and failure cases also preserve captured JSON and stderr. These tests run in CI before scope classification. Both real exhaustion cases and the real ClamAV 1.5.4 API case passed again under the verification lock; final inspection found no owned scanner process or qualification temporary directory. The later session-wide correction and final hosted acceptance are recorded below; the earlier head is superseded.

A second independent review found that an ordinary child could select a different process group within the owned session. The four original-group controls passed, while all four actual split-group scenarios reproduced a live child surviving lock release. Cleanup now enumerates every live member of the owned session, verifies its session and birth time, and signals through a PID descriptor. The expanded four-method/eight-scenario suite passed in 13.787 seconds. Both real exhaustion cases and the installed-engine case passed again, with no live owned member or qualification directory remaining. The direct command is reaped; the evidence does not claim orphan zombie reaping or containment of deliberate session escape or supervisor SIGKILL.

The first two hosted verify attempts reached the exhaustion launcher after passing ordinary Rust/PostgreSQL/frontend checks, then failed before fixture execution because writing the user-namespace UID map was denied. Ubuntu documents its [unprivileged namespace restrictions and executable-specific profiles](https://documentation.ubuntu.com/release-notes/24.04/). The revised CI step leaves the global restriction enabled, copies the installed `unshare` binary byte-for-byte into a private job directory, and loads a temporary profile permitting user namespaces only for that executable. The launcher still runs as the unprivileged job user with the same namespace, capacity and storage guards. Cleanup unloads that profile and removes only the private directory, retaining failure status. The final PR and integrated hosted jobs passed this real path; the earlier denial remains a failed attempt rather than exhaustion acceptance.

Five isolated shell-controller cases passed for normal execution, profile-load refusal, storage failure, scanner failure and profile-removal failure. They preserve failure status, stop before the scanner when storage fails, and remove the private directory. Those cases replace only privileged profile calls and qualification commands; they do not load AppArmor policy or establish hosted namespace acceptance.

## Actual storage exhaustion

`bash scripts/qualify-attachment-storage.sh` builds outside the fixture, then creates a private user/mount namespace, namespace-owned temporary ancestry and separate 4 MiB tmpfs mounts for the adapter and API cases. The launcher requires four distinct restricted roles on one loopback `_test` database. It takes the shared verification lock when the local coordinator exists.

The adapter receives actual kernel `ENOSPC` during a 2 MiB upload with a 3 MiB filler occupying the mount. No published or staged object remains. Removing the filler permits the same immutable key to upload and download with the expected length and SHA-256.

The production API router returns 503 `dependency_unavailable`, leaves the same reservation pending with an updated validator, denies content with 404 and publishes no object. Removing the filler permits a conditional upload using that updated validator against the same reservation; it becomes ready and downloads exact bytes/hash with one published object.

Both real cases passed after final guard changes. Eight direct-invocation refusal cases rejected absent/current namespace markers and linked/nonempty roots before storage writes, preserving sentinel bytes. Both targeted strict Clippy checks, formatting and Bash syntax passed. Cleanup found no remaining qualification directories.

The initial fixture exposed correctly rejected ancestor ownership after UID mapping. Mounting namespace-owned temporary ancestry corrected the fixture; no production filesystem validation was weakened. A later allocation-only Clippy warning in the new guard was corrected without relaxing it.

This proves bounded Linux tmpfs exhaustion and recovery, not physical-device capacity, PostgreSQL/WAL exhaustion or every supported host filesystem.

## Installed scanner

`bash scripts/qualify-attachment-scanner.sh` requires an explicitly trusted `OPENTRACK_TEST_CLAMD_BIN`, four isolated database roles and ClamAV 1.5 or newer. It launches one foreground daemon with a private Unix socket, bounded queues/bytes/time, a clean process environment and a private custom-only signature database. Cleanup targets live members of its owned process session and its private directories.

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

The separate cold-recovery harness exercises populated blob restoration and corruption/missing-object refusal. The final PR and [integrated source](2026-10-23-agent5-integrated-qualification.md) passed all four required checks, with actual failure targets, deployed restore and private-team revocation controls. That supports F13-A3 for the bounded Linux server/API profile; it does not substitute for complete DB/blob/key recovery, native interaction or broader lifecycle/failure UX. #23/#25 and whole-product gates remain open.
