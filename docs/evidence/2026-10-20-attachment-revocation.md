# Saved attachment URL revocation

Date: 20 October 2026. Scope: [F13-A1](../features/F13_ATTACHMENTS.md#11-acceptance-criteria) and the revoked-download portion of [M04-T04 / issue #23](https://github.com/MimoAlexer/OpenTrack/issues/23). This increment adds a focused qualification suite; the attachment implementation and database schema are unchanged.

## Method

[`attachment_revocation_contract.rs`](../../crates/opentrack-api/tests/attachment_revocation_contract.rs) sends requests through the real Axum router and its authentication, authorization, problem-response and attachment handlers. It uses real PostgreSQL with the existing restricted runtime/migration/operator test identities, a fresh generated schema per test and a separate temporary local blob directory. The harness refuses database names that do not end in `_test`. Fixtures use synthetic accounts only and clean up their own schemas and files.

Each file is reserved and streamed through the API, then read successfully before authority changes. The suite captures the server's returned attachment Location and derives the content endpoint once. Subsequent reads replay those saved URLs without asking for a replacement link. Native credentials come from the real PKCE request, browser approval and exchange APIs; the loopback callback URL is parsed inside the test, without starting a native application or callback listener.

Every denial is checked on both metadata and content endpoints with eight request variants: ordinary GET, a byte range, multiple byte ranges, the captured metadata ETag in `If-None-Match`, wildcard `If-None-Match`, a future `If-Modified-Since`, Range plus `If-Range`, and the captured ETag in `If-Match`. Responses must be the exact generic 401 or 404 problem envelope with `no-store`, no file bytes, name, checksum, attachment/document identity or private metadata, and no ETag, Last-Modified, Content-Disposition, Content-Range, Accept-Ranges or Location. An authorized control reader receives the exact original bytes, size and SHA-256.

## Matrix

| Authority change | Saved objects and credentials | Required result |
| --- | --- | --- |
| Logout | Linked issue file and unlinked draft; browser cookie and native bearer | Each logged-out credential receives 401; an unaffected owner still reads the linked file. |
| Same-account login rotation | Linked issue file and draft; replaced browser cookie | Old cookie receives 401; the replacement session reads the same URLs. |
| Targeted session revocation | Linked issue file and draft; separate browser and native sessions | Revoked target receives 401; the revoking session remains authorized. |
| Revoke other sessions | Linked issue file and draft; other browser and native sessions | Both targets receive 401; the retained session and separate owner remain authorized. |
| Operator-issued recovery completed through HTTP | Linked issue file and draft; two pre-reset browser sessions and a native session | All old credentials receive 401; fresh browser/native sign-in with the replacement password reads the saved URLs. |
| Operator account disable and enable | Linked issue file and draft; browser and native sessions | Disabled-account credentials receive 401 and remain revoked after enable; fresh sign-in restores access. |
| Workspace membership deactivation | Linked issue file and uploader draft; browser and native sessions | Sessions still authenticate, but both saved endpoints receive 404; the owner remains authorized. |
| Private-team membership removal | Linked private issue file and uploader draft; browser and native sessions | Live sessions receive 404; the owner remains authorized. Regrant revokes old credentials, and fresh browser/native sign-in reads the same URLs. |
| Public to private, then private to another private team | Current linked description file; browser and native sessions | Readers outside the current team receive 404, while the owner reads the same file. Explicit grant plus fresh sign-in restores access to the first team. |
| Private to public | Current linked description file; browser and native sessions | Current description attachment follows the public issue and remains readable. |
| Private to public with retained private content, then original private-team removal | Removed description reference retained in private revision, and private-origin comment attachment; browser and native sessions | The public issue stays readable, but both private historical files receive 404 after removal; an authorized original-team reader still receives the bytes. |

## Execution record

Environment: local Windows MSVC host, Rust `1.98.1 (48a229cea 2026-09-01)`, the configured PostgreSQL test cluster, and migrations through 0020 in disposable test schemas. Source base: `dcd078c`. The new suite's tested SHA-256 is `cffc646cac5ac680c7128d72c17565e12d5364f1420eb38b5e6a0719cf212bd7`. No development database or listening application process was used.

| Executed check | Observed result |
| --- | --- |
| `cargo test -p opentrack-api --test attachment_revocation_contract --test attachments_contract -- --test-threads=1` | Final run passed all **7 revocation tests** in 321.89 seconds and the **1 Windows-selected existing attachment test** in 7.80 seconds. The revocation matrix comprises **800 denied GET checks** across the two endpoints and eight header variants, plus successful authorized reads. |
| `cargo clippy -p opentrack-api --test attachment_revocation_contract -- -D warnings` | Passed in 30.76 seconds. |
| `cargo fmt --all -- --check` | Passed. |

Commands used the repository's Windows Cargo wrapper and the existing private test environment. The existing Unix-only scanner-unavailable HTTP test was not selected on Windows; this run does not add scanner evidence. All seven new tests were selected and none was ignored. The requested F13-A1 post-revocation URL replay matrix passed at this server boundary; no production implementation fix was required.

The first run passed four tests and stopped three at private-team creation with 422 because the new fixture supplied a three-character idempotency key. The fixture now supplies the required 8–128-character key. The next run passed six tests, while the historical case received 408 during initial account setup, before any attachment operation; that run reported 1483.16 seconds. Its cause was not established. The exact historical case then passed independently in 45.82 seconds, and the final combined run above passed without further source changes. These failed attempts are retained rather than counted as successful attachment qualification.

Private local receipts: `work/qualification/attachment-revocation.log`, `attachment-revocation-r2.log`, `attachment-revocation-historical.log`, `attachment-revocation-final.log`, `attachment-revocation-clippy.log` and `attachment-revocation-fmt.log`. Raw environment paths and diagnostic logs are not committed.

## Limits and issue disposition

This is an in-process HTTP contract suite backed by PostgreSQL and the real filesystem adapter. It does not claim a separately listening or restarted API process, browser interaction, a native window, an OS file-picker workflow or persisted-session behavior across application restarts. The assertions concern requests made after revocation has committed; they do not establish cancellation of a download already in flight.

Issue #23 remains open for installed-antivirus qualification, an actual full-disk rehearsal, coordinated populated-blob backup/restore and native GUI evidence. Existing [attachment qualification](2026-10-18-attachments.md) retains its separate quota, scanner-protocol, storage-failure, process and browser evidence. No deployment, migration or rollback procedure changes in this increment.
