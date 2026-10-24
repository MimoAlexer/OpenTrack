# Fresh Linux SDK and package qualification

25 October 2026. A fresh Ubuntu 26.04 AMD64 native build exposed an incomplete
development-package lock. GDK 3.24.52 metadata existed, but its transitive
Expat/X11/XCB requirements were missing. The original 71-package build failed
before producing a package. Its source snapshot, runtime, logs and build target
remain retained; this was a dependency-closure failure, not missing GDK itself.

The correction adds 18 checksum-pinned development archives. The original 71
records remain unchanged. Fresh extraction now produces 154 pkg-config files.
Overlay variables and literal include/library flags are relocated; installed
host metadata remains unchanged. Complete versioned compiler/linker requirements
are checked before temporary frontend staging, npm or Cargo. Seven synthetic
checks passed, including failure before those later operations.

The normal release build at source `e9e778fa8318bde3e52a311cacadf242ee73b56f`
passed in 393.704 seconds with two compiler jobs. Source, SDK overlay, build
target and HOME/XDG directories were fresh; checksum-pinned dependency archives
were reused. All 89 development archives were verified, and 21 supplemental or
runtime package records were revalidated against signed Ubuntu indexes. The
explicit preflight resolved all 27 emitted include/library paths before release
compilation. Compatible host runtimes remain required; this is not an independent
Linux distribution or a general parser for arbitrary linker-option syntax.

The generated `OpenTrack_0.1.0_amd64.deb` passed stream inspection:

- Expected package/version/architecture and role-based maintainer metadata.
- Exact project AGPL-3.0-or-later license bytes with mode `0644`.
- AMD64 ELF executable with mode `0755` and launcher with mode `0644`.
- Expected executable, icon and URI declaration; only the known three-byte
  `UNK` to `DEB` bundle-marker patch differs from the release executable.

Dynamic-library resolution passed. The real release executable was installed
and updated in isolated user directories using actual desktop-file validation
and cache-update tools. Binary/icon/license bytes, launcher content and modes
passed in both phases; the update replaced all four managed files atomically.
The license
is identical in source, package and user installation. Three harmless supervisor
cases also passed, including timeout and detached output-holder cleanup. All
owned children were reaped, cleanup failures were empty, all 1,025 original source
files were unchanged and installed host metadata fingerprints still matched.

The [machine-readable receipt](2026-10-25-linux-native-sdk.json) records exact
source, archive, controller, package, binary and license hashes, the preserved
failure and command results. Subsequent changes in this increment only add this
evidence, ledger entries and README wording. There is no migration or stored-data
change. Rollback uses the previous matching lock and scripts; neither run installs
host packages or replaces system libraries. Both runtimes and their artifacts
remain private and retained.

This qualifies one normal Linux release build and isolated installation/update.
It does not prove a second reproducible build, rendered GUI, native authentication,
chooser/deep-link execution, physical input, IME, assistive technology, other
operating systems or offline installation/restore. Complete dependency notices,
SBOM/advisory disposition, signing and release publication remain open. Fresh
exact-head CI and final review are required before integration; no broad issue,
F36 criterion or distribution gate is closed by this result.
