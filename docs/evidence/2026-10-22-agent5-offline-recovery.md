# Offline installation and cold recovery qualification harness

22 October 2026. Issue #7 / F29-A1; related recovery gates remain open. Source: the Agent 5 working tree following integrated main `d01fb0c010bef0c12739bcc332074f1a4dbc999f` (schema 0025). This record describes the implemented harness and executed local boundary checks. **Actual Docker recovery acceptance is pending configured AMD64/ARM64 CI.** Docker is unavailable in this local Windows/WSL environment.

## Executable scope

The Docker workflow builds the pinned images first, then starts a fresh disposable installation with `--no-build --pull never` and both application networks internal. The smoke harness verifies actual running-container attachments and Docker network `Internal` state before first-owner setup. Built JavaScript/CSS, local authentication, persisted issues/documents/attachments and the restricted worker are exercised with AI disabled. This tests installation from already obtained images, not an offline source build or hostile-network isolation.

After the existing persistence and credential-failure checks, the harness stops API/worker, captures a private database fingerprint, then stops and verifies every source service before copying any volume. It archives the matched database, blobs and five credential volumes into a private labelled backup volume. Archives have checksums and preserve numeric owners, permissions and file bytes. Restore writes only newly created empty volumes in the distinct owned restore project; path traversal, links, special files, unexpected owners, elevated mode bits and damaged archives are rejected.

The restored database must use the original image and match table/sequence/migration, owner/ACL/RLS/function/schema and role fingerprints. The harness requires all four restricted roles to authenticate with their restored credentials. The matching application image must preserve the setup token, existing session, local login, issue, structured document and populated attachment digest. The restored worker remains stopped and actual networks remain internal. Missing/corrupt restored blobs must fail inventory verification and return an unavailable response without file bytes; each fault is repaired from the original backup.

Normal cleanup removes only the exact labelled restore fixture and keeps the original private backup. The source restart is attempted even when cleanup fails. Explicit job-final cleanup can remove the labelled synthetic backup after qualification; the archive is never published as an artifact. This is a disposable qualification harness, not a production backup command.

After the offline checks, CI stops the source containers and removes their internal networks without deleting volumes. The separate HTTPS qualification recreates its own configured networks and reuses the preserved fixture. This avoids changing an existing network's `internal` setting in place; HTTPS results do not extend the offline claim.

## Executed checks and review corrections

| Check | Observed result |
| --- | --- |
| `python scripts/test-docker-recovery.py` on Windows | 10 passed. |
| Same boundary suite through the assigned WSL launcher | 10 passed. |
| Python syntax parsing | Both changed Python files passed. |
| PostgreSQL 18.6 `EXPLAIN` of the fingerprint query | Passed against the assigned test database; this plans the query and does not execute a restore. |
| Independent read-only implementation review | Two findings fixed and rechecked: cleanup failure must not skip source restart; offline evidence must inspect actual networks rather than only intended Compose configuration. |
| `git diff --check` | Passed. |

Boundary tests exercise foreign storage/network/port refusal, no-pull/no-build commands, archive safety/checksums, nonempty target refusal, source-volume write refusal, pinned networkless helpers, exact labelled cleanup, observed network-state refusal and restart after cleanup failure. They do not start Docker or qualify recovery behavior.

## Remaining acceptance

Run both configured native-architecture jobs on the exact reviewed head and retain failures/corrections before recording acceptance. Bootstrap Python behavior, real volume ownership, PostgreSQL restart/fingerprint equality, blob failures and the later HTTPS transition still require that execution.

Private-team denial after restore, optional HTTPS/operator-output contents, actual provider encryption keys and future modules, encrypted scheduled backups, prior-release/failed-migration and different-host restore remain outside this fixture. F29-A1, GQ08, M08 recovery and the complete product are not marked complete by local unit tests or this harness implementation.
