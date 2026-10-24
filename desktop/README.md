# OpenTrack desktop

OpenTrack has a browser client and an installable Tauri 2 desktop client. Both use the same React application and connect to the same OpenTrack server, so accounts, permissions and issues are shared. The desktop package embeds its interface; PostgreSQL and the API remain separate services.

## Build a Linux package

From the repository root, with the repository's Rust toolchain, Node 22 and npm >=9.5:

```sh
desktop/scripts/build-linux.sh
```

By default, the package is written to `desktop/target/release/bundle/deb/`. The script builds fresh web assets in its own temporary directory, installs the locked Tauri CLI, and builds the locked Rust workspace. It does not replace a running server's assets. It removes its temporary assets after packaging. Install the resulting `.deb` with your distribution's package manager, then launch **OpenTrack** from the applications menu. Package installation requires the usual administrator permission and runtime dependencies.

Alternatively, after a successful build, `python3 desktop/scripts/install-user-linux.py` installs the executable, project license, icon and application launcher for the current user without administrator access. The default executable remains `desktop/target/release/opentrack-desktop`. For a release build in an absolute external `CARGO_TARGET_DIR`, select the executable explicitly:

```sh
python3 desktop/scripts/install-user-linux.py --binary "$CARGO_TARGET_DIR/release/opentrack-desktop"
```

`--binary ABSOLUTE_PATH` requires an existing regular file and rejects relative paths and symbolic links; the installer does not discover Cargo configuration. Run the same command after rebuilding to update the user installation. Each file is copied atomically, but the four-file installation is not a single transaction. The project license is copied to `$XDG_DATA_HOME/opentrack/LICENSE` (using the default user data directory when unset). An existing launcher without the managed marker, or an executable/license without a launcher, is refused before copying.

The launcher registers as a handler for OpenTrack issue links without overriding an existing default handler. The host must already provide compatible runtime libraries.

Linux is the current implementation target. The local header overlay is pinned for Ubuntu 26.04 amd64; packages built on that system are not a promise of compatibility with older distributions. Windows and macOS packages remain unqualified.

## Connect and sign in

1. Start or choose an OpenTrack server. For local development, use `scripts/dev.sh`; the browser application is at `http://127.0.0.1:18970`.
2. In the desktop connection screen, enter that server's origin and review it. HTTPS is required for remote servers. The local development HTTP option accepts loopback only.
3. Choose browser sign-in. Sign in on the same server in the system browser, compare the confirmation code with the desktop window, and approve the named device.
4. Return to OpenTrack. Its session appears as a desktop device in **Account security**, where it can be revoked from the browser.

Credentials are stored in the OS credential service, never in web storage or the connection file. Linux requires a working Secret Service provider. If the vault is locked or unavailable, sign-in fails visibly; there is no plaintext fallback. Browser links to issues can offer to open the desktop app through the registered `opentrack:` scheme. A link to a different server requires a connection review.

This is an online client. Local connection metadata does not contain a workspace replica, and offline mutation synchronization is not implemented. Keep the server running to use either client. Automatic updates are disabled pending a distribution identity, signing key and controlled update feed.

## Attach files and save downloads

The desktop client uses the native file chooser. Rust opens regular files and
returns only an opaque handle, filename, byte length and SHA-256 to the shared
interface. File paths, native credentials and raw file bytes are never IPC
arguments or results. The native broker retains the opened descriptor, checks
the file again before transfer, and streams it to the selected server's fixed
attachment endpoint. Replacing the original path cannot substitute another file;
changed contents require a new selection. Server permissions, quota, checksum
verification and scanner policy still decide whether a file becomes ready.

The broker accepts nonempty files up to 100 MiB; a server may impose a lower
limit. It retains at most 16 selected handles for 30 minutes, permits one native
file dialog and four concurrent transfers, and bounds blocking filesystem work
to four operations. Each blocking operation retains its slot until it actually
finishes, even if its caller cancels. Raw transfer deadlines are 180 seconds;
the exact attachment-finalization request has a 90-second deadline.

Signing in again, signing out or changing servers invalidates file handles and
cancels active transfers. The document draft remains separate from these handles.
Use **Choose same file** to continue an unfinished upload after reauthentication
or expiry; the interface checks its size and SHA-256 against the existing
reservation. Cancellation can race with a server commit, so retry checks the
existing attachment rather than assuming no bytes arrived. Native cancellation
also covers a command cancelled before its upload is registered: up to 64
operation IDs are remembered for five minutes. If that bound is reached, new
transfers pause instead of evicting cancellation records.

Downloading first reads authorized attachment metadata, then opens a native save
dialog using the server's filename. The broker streams into a private temporary
file, verifies exact length and SHA-256, fsyncs it, and publishes it atomically.
An existing destination is never overwritten; choose another filename if it
already exists. A mismatch or interrupted transfer removes the temporary file.
A cancelled chooser returns without saving. Files are not opened or previewed
automatically, and a connection change prevents a stale download from being
published. If final directory synchronization fails after publication, the error
explicitly asks the user to check the destination before retrying.

Only the five explicit attachment commands are allowed by the main-window
capability. There is no renderer permission for arbitrary filesystem access or
generic file dialogs. The generic JSON API bridge allows attachment metadata and
finalization; raw content routes are reserved for the native broker.

On 18 October 2026, the attachment increment passed 40 native tests: 14 unit tests, five real
filesystem/loopback attachment contracts, the GLib regression, and 20 existing
native boundary tests. The attachment cases cover descriptor substitution,
modified-file rejection, handle and generation boundaries, cancellation before
registration and during transfer, checksum-verified downloads, and private
no-replace publication. All-target Clippy also compiled the native shell and
dialog plugin. These checks do **not** establish interactive chooser/save-dialog
behavior, packaged attachment workflows, or rendered native UI behavior; those
remain separate runtime qualification gates.

## Development checks

Run `desktop/scripts/check.sh` from the repository root for vendor provenance, formatting, all-target Clippy and native core tests. The real vault probe below requires an unlocked user Secret Service session and runs separately. `examples/handoff_probe.rs` is an opt-in integration harness for a dedicated loopback test server; it requires browser approval and revocation and creates a test issue.

GitHub's `Verify implementation` workflow also reports a separate `native-contracts` job on every pull request and push to `main`. It uses the conservative implementation scope: desktop source, dependencies, vendor files, scripts and the generated `docs/api/openapi.json` all run the check; a verified documentation-only range reports an explicit success. Failed, missing or invalid scope detection fails the job. Manual dispatch runs the complete suite. There are no workflow path filters or test-name filters.

The hosted check uses the same production workspace through an explicit core mode:

```sh
sudo apt-get update
sudo apt-get install --yes --no-install-recommends build-essential pkg-config libglib2.0-dev
desktop/scripts/check.sh --core
```

This mode targets Ubuntu 24.04 amd64 with Bash, Python 3, rustup and the exact toolchain in `rust-toolchain.toml` (currently Rust 1.98.1, including Clippy and rustfmt). Cargo uses `desktop/Cargo.lock` with `--locked`; the vendor verifier checks the complete GLib source inventory and its approved repair before compilation. `pkg-config` must resolve the GLib, GObject and GIO libraries from `libglib2.0-dev`, needed by the optimized vendor regression. The runner's Ubuntu packages receive distribution updates; they are not the pinned Ubuntu 26.04 header overlay. The workflow records their installed versions. Node, WebKit, a desktop display, a running API/database and an unlocked Secret Service are unnecessary for this core check.

Core mode runs formatting, all-target Clippy with shell features disabled, and the **complete** existing `--no-default-features` native test suite. This includes production request validation against OpenAPI, unsupported methods and malformed/unscoped paths, credential-header rejection, native transport/state and attachment boundaries, and the optimized GLib regression. Boundary fixtures use disposable loopback listeners, temporary files and test vaults; they do not perform real OS-vault authentication. A successful core run does not qualify the Tauri shell build, GUI, chooser dialogs, real credential vault, package installation/deep-link association, signing, updates or other platforms. The default `desktop/scripts/check.sh` still uses the pinned Ubuntu 26.04 overlay and full-feature all-target Clippy; its acceptance scope is unchanged. See the [CI qualification record](../docs/evidence/2026-10-20-native-ci.md) for executed versus pending evidence.

For the explicitly selected local development connection on `http://127.0.0.1:18970`, the opt-in saved-connection probe makes four concurrent authenticated GETs through the production native runtime and OS credential reader:

```sh
cargo run --locked --manifest-path desktop/Cargo.toml --no-default-features --example saved_connection_probe -j 2 -- --read-saved-loopback-connection
```

It checks session, bootstrap, access and the recorded local ENG-5 issue fixture. It validates the saved connection metadata, copies only metadata into private temporary storage, refuses every credential write/delete, and checks that the original configuration remains unchanged. It prints only status and validation results, never account data or credentials. It fails if the selected server or fixture does not match its explicit local target. The live concurrent probe passed after the credential queue repair; this establishes native transport access, not rendered window behavior.

On Ubuntu 26.04 amd64, build with the pinned local header overlay:

```sh
python3 desktop/scripts/prepare-linux-sysroot.py
export PKG_CONFIG_PATH="$PWD/work/desktop-build/sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$PWD/work/desktop-build/sysroot/usr/share/pkgconfig"
cargo test --locked --manifest-path desktop/Cargo.toml --no-default-features -j 2
cargo build --locked --manifest-path desktop/Cargo.toml -j 2
```

The preparation script verifies the89 pinned development-package checksums and extracts them under `work/desktop-build`. It relocates overlay metadata variables and literal include/library flags without changing installed metadata. The package build checks the locked Rust dependency versions and complete pkg-config compiler/linker requirements before staging assets, installing JavaScript dependencies or invoking Cargo. It does not install packages or alter host libraries. The host supplies compatible runtime libraries. Other distributions can use their normal [Tauri Linux build prerequisites](https://v2.tauri.app/start/prerequisites/).

`Tauri.toml` allows bundled documents only. The native window denies remote navigation and new webviews, and the main-window capability lists implemented commands explicitly. The transport policy rejects arbitrary destinations, browser authentication routes, caller-supplied credentials and unsupported methods before making a request.

A successful build or process launch does not establish GUI, authentication or package-association verification. Executed checks and outstanding platform gates are recorded in [desktop evidence](../docs/evidence/2026-10-18-desktop.md).

Native commands use the `{ request: { ... } }` envelope. `connection_read` and `issue_link_read` have no arguments. The request generation fences network dispatch and results; a separate scope generation preserves editor drafts across same-user reauthentication. The browser opener has one concurrent process, links use one bounded eight-item inbox, and four handoff slots bound outstanding sign-in finalization. Only one blocking credential-store operation may run at a time. Up to 32 operations, including the active one, may wait in a bounded queue; each gets one ten-second deadline covering queueing and execution. Ordinary parallel workspace reads wait their turn instead of being reported as a locked credential store. A timed-out write retains its lane until it finishes and then removes any late credential.

Sign-in cancellation is available before the valid callback. Once exchange begins, cancellation returns `authentication_finishing` until the bounded exchange and storage step finish. A server switch still fences the result and revokes/removes an unwanted grant. An uncertain reauthentication clears the possibly rotated prior local session and explicitly asks the user to sign in again and review active sessions. The native session directory is the recovery path for a grant that the server issued but the client never received.

Deep links carry only a server origin and workspace/issue UUIDs. They produce a review offer with an opaque ID; dismissing an old offer cannot dismiss a newer one. They never select a server, authenticate, or open arbitrary URLs. The shell supports Linux in this increment; `xdg-open` is its system-browser launcher. Windows and macOS have not been qualified.

Build dependencies are pinned in both lockfiles. `rust-ini` is pinned to 0.21.1 because 0.21.3 added required `ParseOption` fields which the upstream deep-link 2.6.2 plugin does not initialize. The isolated Tauri CLI is 2.12.3 (`npm --prefix desktop ci --ignore-scripts`). OS credential-store verification uses `cargo run --locked --manifest-path desktop/Cargo.toml --no-default-features --example keyring_probe`: it creates a unique temporary entry, verifies write/read/delete, and prints no secret.
