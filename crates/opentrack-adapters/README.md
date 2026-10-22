# Local immutable blobs

Linux filesystem adapter for F13. PostgreSQL remains responsible for reservation,
quota, current access, generation/lease fencing, document references and lifecycle.
No method takes a database connection; call all filesystem/scanner work outside
database transactions.

1. Reserve an immutable UUIDv4 key, byte length and SHA-256 in PostgreSQL.
2. `put(ExpectedBlob, AsyncRead)` streams in 64 KiB chunks with an idle timeout.
   It checks exact length/hash, detects a conservative MIME type from at most
   8 KiB, fsyncs the private staging file, changes it to 0400, fsyncs again,
   publishes with `renameat2(RENAME_NOREPLACE)`, then fsyncs both directories.
   The receipt means durable quarantine bytes, **not ready**.
3. Commit quarantine with the current authorized metadata lease. `scan` reopens
   and hashes the descriptor before scanning. A required missing scanner is
   invalid configuration. No configured scanner yields explicit `NotScanned`;
   a configured scanner's failure never silently becomes `NotScanned`.
4. Reauthorize and fence the database transition to ready. Downloads must perform
   current document authorization before `open_verified`, then stream the returned
   `AsyncRead`. The adapter verifies that same opened descriptor before exposing
   bytes. Serve every type with attachment disposition and nosniff initially;
   MIME detection is not safe-preview approval.

The root must already exist, owned by the service UID with mode 0700, outside the
checkout and webroot exclusions. Ancestors are walked with pinned no-follow
directory descriptors; unsafe ownership/write permissions, symlinks, nonregular
files and hard links fail closed. Names are canonical generated UUIDv4 values,
never original filenames. The trusted service UID/root can still modify its own
storage: this is not cryptographic protection against a compromised host owner.

Cancellation removes staging while the key lease is held. Publication is a short
non-cancellable filesystem operation: a lost response can leave a durable orphan,
so retry verifies the existing immutable object and repeats fsync before returning
its receipt. An existing-object retry does not consume the incoming reader.

Cleanup accepts at most 100 explicit keys whose metadata has already entered a
terminal, leased cleanup state. A read-only "unreferenced" query is insufficient.
It checks age and a nonblocking per-key exclusive lock; active uploads, scanners
and download readers produce `Busy`. It persists a retired-key marker before
removing bytes and fsyncs the directories before `Removed`/`Missing`. Only then
may metadata acknowledge deletion and release quota. Keep/retry the metadata after
uncertain I/O. Keys are never reused. Small private key lock/tombstone files are
retained deliberately to prevent late work from republishing a deleted key; the
adapter never sweeps unknown files or guesses whether a blob is live. Backup must
retain referenced blobs and pause cleanup, including the filesystem key state.
Store clones share one cleanup lane. A cancelled or timed-out caller leaves its
permit with the blocking filesystem operation until it finishes; overlapping
cleanup returns `BlobError::Busy`, so worker deadlines cannot accumulate tasks
behind a stalled filesystem.

The optional scanner uses only an administrator-configured Unix socket and the
[official ClamD INSTREAM protocol](https://docs.clamav.net/manual/Usage/ClamdProtocol.html)
(checked 18 October 2026): NUL framing, big-endian chunk lengths, zero-length end
marker and bounded responses through connection close. One deadline bounds the
whole scan. Only an exact clean reply is accepted; detections, daemon limits,
unknown/trailing replies, disconnects and timeouts block readiness. Configure
ClamAV's StreamMaxLength and scan/archive limits to cover the application's bound;
the adapter neither runs arbitrary commands nor decodes/extracts uploaded formats.
Protocol tests use a local fake daemon, not an installed antivirus/signature set.

Run `cargo test --locked -p opentrack-adapters` and
`cargo clippy --locked -p opentrack-adapters --all-targets -- -D warnings`.
