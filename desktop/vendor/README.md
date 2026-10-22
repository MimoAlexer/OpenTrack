# Pinned GLib compatibility patch

`glib-0.18.5/` is the official crates.io package, under its included MIT license. The original archive SHA-256 is `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`.

Only `src/variant_iter.rs` differs from that package (Cargo cache markers are omitted). The two changed lines are the exact fix from gtk-rs commit [b5a4071e439bef2b5eea76c3aa25e5ae84839e34](https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34), merged in [PR 1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343). `glib-upstream.patch` preserves the original patch and attribution; `glib-variant-iterator.patch` records the applied diff. Sources verified 18 October 2026.

[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) identifies an immutable pointer passed to a variadic C output parameter. The fix declares that pointer mutable and passes a mutable reference. The supported upstream version is >=0.20; Tauri 2's GTK 3 dependency requires the 0.18 API. A path patch preserves that API until the desktop stack supports a maintained replacement. No advisory is suppressed, and the package version remains truthful: scanners may still flag its original version despite this source backport.

`desktop/tests/glib_patch.rs` exercises all five affected iterator methods. The dev/test profiles explicitly compile GLib with optimization level 2; an unoptimized GLib build alone is insufficient qualification for this optimizer-sensitive defect. Remove the patch when the full Tauri/GTK dependency graph can use a fixed upstream release.

The desktop's direct code does not call `VariantStrIter`; GLib is reached through Tauri's GTK/WebKit runtime. Other current advisory warnings are unmaintained `proc-macro-error`, `zbus_names`, `zvariant`, and `zvariant_derive`. The latter three are reached through single-instance IPC and Secret Service. These warnings are retained and are not a complete security clearance for the desktop or its host libraries.
