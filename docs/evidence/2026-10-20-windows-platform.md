# Windows runtime qualification, 20 October 2026

Scope: a native Windows server/storage development path and portable verification. This is not desktop GUI, production installation, signed distribution, antivirus or complete-product qualification. The pre-existing `desktop/src/main.rs` AppHandle edit was preserved.

## Environment and initial failures

- Windows NT 10.0.26340.0, local NTFS volume; Rust 1.98.1 x86_64-pc-windows-msvc; Node 24.14.1, npm 11.11.0 and Python 3.12.10.
- WSL reported no installed distributions and unavailable virtualization; no PostgreSQL installation/service or Docker executable was found. No WSL, system service, firmware or system-package configuration was changed.
- Default Cargo compilation selected an incomplete Visual Studio 2026 installation and failed with `LNK1104: cannot open file 'msvcrt.lib'`. `scripts/cargo-windows.cmd` selects an installation with the C++ toolchain component through `vswhere` and enters its x64 developer environment. The existing Visual Studio 2022 Build Tools then compiled successfully.
- The desktop vendor verifier initially rejected Windows path separators and Git CRLF checkout conversion. It now compares POSIX relative paths and normalizes only CRLF to upstream LF before applying its exact hash/approved-patch checks. All 121 upstream files verified. This does not compile or qualify the Linux desktop shell on Windows.
- GitHub's latest existing workflow run [39156790752](https://github.com/MimoAlexer/OpenTrack/actions/runs/39156790752) passed for `a82b80f1cf0365fea69df1c9bc70d4eea7e69830`, 18 October 2026. It is historical Linux CI evidence, not a run of the new Windows changes. The earlier failed run's logs were unavailable through `gh`.

## Storage and secret-file behavior

The Windows implementation pins every directory ancestor and child with handles that deny delete sharing. It rejects junction/reparse entries and files with multiple hardlinks, requires service-owned DACLs granting access only to the service user, SYSTEM or Administrators, and uses operating-system file locks for cross-instance key leases. Publication flushes bytes, removes ordinary user write access, renames the exact opened handle without replacing an existing destination, then flushes the two writable directory handles. Retry verifies immutable bytes and completes directory flushes. GC writes a durable key tombstone before unlinking the exact opened objects.

The first tests exposed a failure-path race: Tokio could retain a pending write after a rejected stream returned, delaying staging deletion. The shared streaming path now finishes its pending file I/O before returning that error. Cancellation/crash leftovers still use the existing authorized GC path.

Windows server secret files use the same no-reparse, single-link, private-DACL opening checks and deny concurrent writable/replacing handles. Secrets are never printed. Unix-socket antivirus configuration is rejected on Windows; required scanning cannot silently become optional. Windows operator recovery-code file creation remains unsupported. The Linux filesystem/scanner implementation remains in its original platform branch.

Primary API references inspected on this date: [GetSecurityInfo](https://learn.microsoft.com/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo), [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info), and [access control entries](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/ace). Actual local contracts below establish the listed behaviors; these references alone are not execution evidence.

## Executed checks

| Check | Observed result |
| --- | --- |
| `scripts/cargo-windows.cmd check -p opentrack-api -p opentrack-server --locked` | Passed on the native Windows target; the previous Linux-only blob import failure is resolved. |
| `scripts/cargo-windows.cmd test -p opentrack-adapters -p opentrack-server --locked --lib --bins --test windows_blob_contract` | 30 passed: one bounded-cleanup unit test, nine real Windows filesystem/secret contracts, twenty server configuration/CLI tests. Zero failed or ignored. |
| Windows filesystem cases | Actual upload/read/retry, concurrent readers, GC while readers hold leases, persistent retirement, malformed/oversize/timed-out upload cleanup, abort after dispatched chunks followed by verified same-key retry/GC, competing upload/GC leases, racing destination no-replace, directory rename denial, injected hardlink rejection, junction denial, shared-ACL rejection, secret-file write/rename denial and scanner fail-closed. |
| `scripts/cargo-windows.cmd clippy -p opentrack-adapters -p opentrack-server --all-targets --locked -- -D warnings` | Passed after platform-specific imports and fixture paths were corrected. |
| `python desktop/scripts/check-vendor.py` | 121 upstream files and the approved two-line GLib repair verified. |
| `scripts/cargo-windows.cmd test -p opentrack-app --locked --test postgres_contract` with private test environment | The real identity/isolation/issue contract passed: one test, zero failed/ignored, 60.30 seconds. Further feature suites are recorded with their feature evidence. |

## Real PostgreSQL and runtime

Downloaded the official PostgreSQL 18.6 Windows binary archive linked by the [PostgreSQL Windows download page](https://www.postgresql.org/download/windows/) and [EDB binary page](https://www.enterprisedb.com/download-postgresql-binaries). Only runtime files were extracted under ignored `work/postgres-windows/`; no installer or system service was run. The downloaded archive SHA-256 is `e2246ba91d22345bc3d017586c09ede52d9df180b1eeb480f050445f1cad84e2` (observed content hash, not a claim of independent publisher-signature verification).

A private task-owned PostgreSQL 18.6 cluster listens only on `127.0.0.1:55433`. Separate `ot_20261020_test` and `ot_20261020_dev` databases have distinct runtime, migrator, worker and operator identities. Password files and configs inherit a restricted owner/SYSTEM DACL. The test harness owns temporary schemas; the development database is separate from contract-test data.

`.gitattributes` pins SQL and shell scripts to LF. All nineteen migration working files were compared byte-for-byte with `git show HEAD:<path>`, and every resulting SHA-384 digest matched the corresponding real development `_sqlx_migrations.checksum`. There are no SQL-content changes or rewritten applied migrations. This avoids platform-dependent migration checksums from automatic CRLF checkout conversion.

The real API and restricted worker started with separate secret-file configs. `GET /health/ready` and browser-style HTML navigation at `http://127.0.0.1:18970/` both returned 200. No startup errors were recorded. This establishes live startup and readiness only; authenticated browser workflows are recorded separately. The blob directory is outside the Git checkout and has a private DACL. Development commands, configurations, tokens and process IDs remain under ignored `work/` and are not distribution artifacts.

## Remaining qualification

Windows native desktop packaging/vault/UI, macOS, installed antivirus, populated backup/restore, power-loss and disk-exhaustion recovery, production service installation, signed updates and the full release matrix remain open. Storage was exercised on this local NTFS volume only; remote/network filesystems and other Windows versions are not qualified. This increment adds no migration, changes no durable schema and requires no database rollback. Linux regression execution of the shared streaming cleanup change remains a separate CI gate.
