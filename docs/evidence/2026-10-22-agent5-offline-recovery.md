# Offline installation and cold recovery qualification harness

22 October 2026. Issue #7 / F29-A1; related recovery gates remain open. Source: the Agent 5 working tree following integrated main `d01fb0c010bef0c12739bcc332074f1a4dbc999f` (schema 0025). This record describes the implemented harness and executed local boundary checks. **Actual Docker recovery acceptance is pending configured AMD64/ARM64 CI.** Docker is unavailable in this local Windows/WSL environment.

## Executable scope

The Docker workflow builds the pinned images first, then starts a fresh disposable installation with `--no-build --pull never` and both application networks internal. A fixed nonroot HTTP relay uses the already-built proxy image: it alone joins one outward network and publishes the loopback port, forwards only to `app:8080`, and has no credentials or persistent mounts. The smoke harness verifies actual commands/mounts, running-container attachments and Docker network `Internal` state before first-owner setup. Built JavaScript/CSS, local authentication, persisted issues/documents/attachments and the restricted worker are exercised with AI disabled. This tests installation from already obtained images with application egress disabled, not an offline source build, a disconnected host or hostile-network isolation.

After the existing persistence and credential-failure checks, the harness stops API/worker, captures a private database fingerprint, then stops and verifies every source service before copying any volume. It archives the matched database, blobs and five credential volumes into a private labelled backup volume. Archives have checksums and preserve numeric owners, permissions and file bytes. Restore writes only newly created empty volumes in the distinct owned restore project; path traversal, links, special files, unexpected owners, elevated mode bits and damaged archives are rejected.

The restored database must use the original image and match table/sequence/migration, owner/ACL/RLS/function/schema and role fingerprints. The harness requires all four restricted roles to authenticate with their restored credentials. Matching application and relay images must preserve the setup token, existing session, local login, issue, structured document and populated attachment digest. The restored worker remains stopped and application networks remain internal. Missing/corrupt restored blobs must fail inventory verification and return an unavailable response without file bytes; each fault is repaired from the original backup.

Normal cleanup removes only the exact labelled restore fixture and keeps the original private backup. The source restart is attempted even when cleanup fails. Explicit job-final cleanup can remove the labelled synthetic backup after qualification; the archive is never published as an artifact. This is a disposable qualification harness, not a production backup command.

After the offline checks, CI stops the source containers and removes their internal networks without deleting volumes. The separate HTTPS qualification recreates its own configured networks and reuses the preserved fixture. This avoids changing an existing network's `internal` setting in place; HTTPS results do not extend the offline claim.

## Executed checks and review corrections

| Check | Observed result |
| --- | --- |
| `python scripts/test-docker-recovery.py` on Windows | 12 passed after the relay/API corrections. |
| Same boundary suite through the assigned WSL launcher | 12 passed after the relay/API corrections. |
| Python syntax parsing | Both changed Python files passed. |
| PostgreSQL 18.6 `EXPLAIN` of the fingerprint query | Passed against the assigned test database; this plans the query and does not execute a restore. |
| Independent read-only implementation review | Two findings fixed and rechecked: cleanup failure must not skip source restart; offline evidence must inspect actual networks rather than only intended Compose configuration. |
| `git diff --check` | Passed. |

Boundary tests exercise foreign storage/network/port refusal, no-pull/no-build commands, archive safety/checksums, nonempty target refusal, source-volume write refusal, pinned networkless helpers, exact labelled cleanup, observed network-state refusal and restart after cleanup failure. They do not start Docker or qualify recovery behavior.

The first native run, `39812491138` at `790c3590073ed3b5a860e69bf4c137bc9982a574`, failed on both architectures before HTTP acceptance, after actual internal-network validation. Docker does not publish a host port for an internal-only container ([Moby issue](https://github.com/moby/moby/issues/44986)); the revised topology keeps the application isolated and uses the fixed relay above. It explicitly starts the restored relay, covers its inherited data/config paths with bounded tmpfs mounts, checks the sole outward attachment and adds exact offline-network cleanup/diagnostics. No failed run is reported as restore acceptance. Every required check and independent review must run again on the corrected head.

The second native run, `39815243139` at `a42bee52cc44df8325721e5151e5b7a5bee60113`, passed the relay, real API/assets/setup/issues/files, worker capture, recreation, missing-secret recovery, stopped seven-volume archives and damaged-archive refusal on both architectures. Both then failed in the restore loop before PostgreSQL startup. The captured bootstrap build used Python package `3.11.2-6+deb12u9`; its exact Debian `tarfile` source lacks the extraction `filter` keyword. Running the original call against that source reproduced the `TypeError`. The corrected call uses the supported API while retaining structural archive validation, numeric ownership and the modern explicit fully-trusted filter where available. A nested archive/byte/ownership sandbox check passed with that exact Debian library source; it is not a container/PostgreSQL restore result. Safe helper diagnostics now identify action, volume and validation/errno category without raw command stderr. Native restore remains unqualified until fresh CI passes.

## Remaining acceptance

Run both configured native-architecture jobs on the exact reviewed head and retain failures/corrections before recording acceptance. Bootstrap Python behavior, real volume ownership, PostgreSQL restart/fingerprint equality, blob failures and the later HTTPS transition still require that execution.

Private-team denial after restore, optional HTTPS/operator-output contents, actual provider encryption keys and future modules, encrypted scheduled backups, prior-release/failed-migration and different-host restore remain outside this fixture. F29-A1, GQ08, M08 recovery and the complete product are not marked complete by local unit tests or this harness implementation.
