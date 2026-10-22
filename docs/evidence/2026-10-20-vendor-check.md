# Windows vendor verification

20 October 2026. M01-T06 / GitHub #7. Windows checkout based on
`17fddf1`, Python 3.12.10. Only the vendor verifier and this evidence record
are part of this increment.

The GLib verifier compared platform-specific backslash paths with the upstream
POSIX manifest and hashed Git's CRLF checkout bytes against LF source hashes.
It now uses POSIX relative names and normalizes CRLF before the existing exact
approved-repair reversal and SHA-256 comparisons.

Executed checks:

- `python desktop/scripts/check-vendor.py`: all 121 upstream files and the exact
  two-line output-pointer repair verified.
- An isolated copy under `work/vendor-check` passed unchanged. Adding an
  unexpected source line failed the hash check; removing that source file
  failed the inventory check. The real vendored sources were not changed.
- `git diff --check`: passed for this increment.
- GitHub run `39156790752` was inspected live: **success**, at
  `a82b80f1cf0365fea69df1c9bc70d4eea7e69830`. This establishes the earlier hosted
  CI correction, not qualification of this new increment or all of M01-T06.

Retrospective: checkout path and line-ending transformations must be distinguished
from source modifications in integrity checks. Keep the upstream inventory and
content rejection checks; do not weaken verification to get a platform build.
Native Windows runtime and complete offline installation/restore qualification
remain open.
