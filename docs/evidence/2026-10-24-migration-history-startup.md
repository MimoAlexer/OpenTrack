# Migration-history startup compatibility

24 October 2026. This check concerns applied database migrations, not unknown
stored document JSON, which retains its separate opaque/read-only contract.

## Demonstrated behavior

A new isolated restore was given a synthetic applied future migration record.
The previously qualified executable opened its reserved local API and returned
HTTP200. Its exact process was reaped and the fixture retained.

The corrective executable refused the same fixture with the redacted migration
history diagnostic before binding or creating storage. An independent matching
current-history restore returned HTTP200 and shut down cleanly. The first
positive-control setup omitted its required private blob root; that storage
refusal was preserved and corrected without changing the guard.

## Change and role boundary

One shared store helper uses SQLx's existing read-only dirty/history methods on a
single read-only repeatable-read transaction. Applied success flags, every up
version and checksum must match the executable's embedded migrator. Missing,
unreadable, dirty, future, missing-entry or changed-checksum history fails closed.
Validation never creates or repairs metadata and does not borrow migration
authority.

Serve, worker and operator executable startup call it before listeners, storage,
job stages or recovery bearer output. Existing constructor privilege checks and
physical-schema capability probes remain. Generic constructors used by historical
upgrade fixtures, explicit migrate and connection-free check-config are preserved.

Worker/operator provisioning grants only three schema-history column reads:
version, success and checksum. Narrow validator exceptions admit those reads;
other metadata columns and direct writes stay denied. API metadata privileges
are unchanged. No migration file was added or altered, and no0027 reservation was
made.

## Executed checks and limits

The five new contracts passed with real PostgreSQL: exact restricted-role
permissions and denied probes; current history; four corruptions; missing or
unreadable metadata and error-path pool reuse; unchanged rows/table/ACL snapshots;
historical construction and explicit upgrade; and all three CLI refusals before
binding, effects or output. The first test attempt caught a private fixture URL
pointing its operator at a different test database; only fixture configuration
and its dedicated CONNECT grant changed before the successful recheck.

The complete Rust workspace suite, strict workspace Clippy, formatting,
all-targets/store checks and server build passed. Exact counts, source hashes,
standalone outcomes and preserved attempt boundaries are in the
[machine-readable receipt](2026-10-24-migration-history-startup.json).

This is a grant-only update. Stop/drain services, apply migrations explicitly and
use matching worker/operator grant scripts before startup. Rollback similarly
requires the previous release's grants because its validators reject the new
column authority. Do not edit history or share a migration credential to bypass a
refusal. The startup snapshot is not an online-DDL protocol.

Fresh exact-head CI, final source/evidence/privacy/conversation review and
integration remain required. No broader product or native/manual acceptance is
claimed.
