# Browser upload response recovery

24 October 2026. Two real API-backed cases passed in Chromium, Firefox and WebKit
on one new isolated current-schema fixture: six passes, zero failures. Production
implementation, migrations, deployment grant files and file formats are unchanged.

The ordinary case uses the browser's own uninstrumented upload of the selected
45-byte file. It requires HTTP200/ready metadata, a saved document reference and
an authorized download with exact bytes and SHA-256.

The fault case measures the actual Blob/File passed to the original synchronous
XHR send, preserving its arguments and exceptions. Observation is bounded and
correlated to one successful send at the exact owned URL. Size/hash, available
Playwright body bytes and any reported Content-Length must agree before those
observed bytes are forwarded. Fixture bytes are assertions, never replacement
request data. The same relay mechanism runs in every engine.

The real server commits ready content before its response is held and aborted.
The pending-only navigation guard and text typed during the hold remain. Retry
reads the same attachment before insertion, without another reservation or byte
upload. Save/reload preserves one reference, full document content, authorized
inventory and the browser download's exact bytes/hash. The witness restores
native methods and clears captured data without overwriting an earlier error.

## Failure history and qualification boundary

The original relay passed Chromium/Firefox and failed WebKit before ready commit.
An observation-only fresh run reproduced that split: Chromium/Firefox exposed
45 correct bytes and received200; WebKit exposed no body and received422 from
the size guard. This was a test-relay limitation, not evidence of a product
response-recovery defect. Both failed fixtures and all their artifacts remain.

The corrected six-case run verifies browser-produced payload, real relayed
commit, response loss and UI reconciliation. Its separate ordinary case verifies
normal browser transport of the same file; native wire serialization during the
injected fault remains outside this relay's proof.

Actual running executable hashes, served frontend bytes/core capabilities and
default worker settings were checked. All owned processes exited0 and were
reaped; the browser namespace was empty. Source/runtime hashes and private run
modes passed, and exact temporary-spec cleanup was verified before promoting the
same test bytes into the repository. The fresh database
and artifacts remain retained. Earlier configuration/probe/compiler refusals are
recorded separately in the [machine-readable receipt](2026-10-24-upload-response-recovery.json).

Windows TypeScript7.0.2 checking passed. Complete discovery lists165 cases,55 per
engine; discovery alone is not an executed full-matrix pass. Fresh exact-head CI
and final source/evidence/privacy/conversation review remain required. Physical
picker, native/assistive input, scanner outage and broader M04/F13 acceptance stay
separate; neither issue nor whole-product completion is inferred.
