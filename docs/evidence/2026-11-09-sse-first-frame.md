# Immediate content-free SSE establishment

A quiet, authorized SSE connection previously waited for its first queued frame.
With no replay changes, the first bytes arrived at the 15-second heartbeat;
Firefox could keep fetch headers pending beyond the browser acceptance
connection deadline. The response now emits one fixed `: connected` comment
immediately when polled, after checking terminal state. It carries no event,
entity, cursor or acknowledgement and does not consume queue admission.

The existing body owns the one-bit initialization state. Its drop cleanup,
independent producer watchdog, queue budgets, admission ownership and emission
authorization remain unchanged. Already terminal bodies retain terminal priority.
No migration, wire schema or client timeout changes are required; rollback is a
source revert.

The real loopback HTTP regression opens a quiet stream from its current bootstrap
cursor and requires the exact bounded comment bytes within one second. Before the
fix it failed with the first-frame deadline; after the fix it passes. This checks
real HTTP streaming, not fresh Firefox or native/assistive-technology execution.

Validation: `cargo test --locked -p opentrack-api --test realtime_contract --
--test-threads=1` passes all 11 PostgreSQL and real HTTP contracts, including the
new first-frame check, private replay, listener recovery, multi-node admission,
revocation, and stalled-body watchdog cleanup. `git diff --check` passes.
Independent production-diff review confirmed terminal priority and unchanged body
cleanup. Fresh Firefox acceptance remains an integrated follow-up.
