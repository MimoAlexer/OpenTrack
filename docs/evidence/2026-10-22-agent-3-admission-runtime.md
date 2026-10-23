# Agent 3: deployed event-admission qualification

This increment connects schema 0025's admission contract to the existing disposable Compose qualification. It adds no migration, public endpoint, consumer registration, default change or production scheduling change. Logical retained bytes remain `192 + UTF-8 byte lengths of event_type and actor_kind`; they are not physical disk usage.

`event_admission_runtime.qualify` uses the harness's authenticated fixture, exact loopback relay publication and sole disposable workspace. It provisions the real local operator through the existing private named output volume, checks the redacted status contract and verifies each reasoned limits change against matching durable audit/operation records. It never copies credentials into receipts or process arguments.

The connected flow requires:

- A pending-event cap and an exact retained-byte cap each return the complete generic HTTP 503 envelope without entity, counter, history, authorization-epoch, idempotency or event-accounting changes. Capacity recovery accepts the original key once; replay preserves the committed result and snapshot.
- Bounded real `worker --once` polls capture and drain the fixture. A captured aged prefix reclaims its exact logical charge. A disabled required consumer retains an uncaptured nine-day-old event; re-enabling it records the independent ignored receipt and permits exact prefix reclamation before the original refused request retries.
- Original admission limits and required-consumer enabled state are restored, and the owned worker restarts. Primary failures survive cleanup failures. Later graceful shutdown, restart, cold restore and private-team checks remain in the same harness.

Before mutation, guards reject ambiguous or nonlocal origins, noncanonical or cross-workspace sources, a different published port, multiple workspaces and projects without the disposable prefix. Aging is limited to a captured snapshot of at most 100 canonical event IDs in that workspace; worker drain is limited to 20 polls.

Local boundary evidence after integration of PR147: `python -I scripts/test-event-admission-runtime.py` passes 9 methods covering 27 rejected inputs, stop/body/restart failures and the real isolated-Python smoke CLI import; `python -I scripts/test-docker-recovery.py` passes 15 methods and the browser-runtime boundary suite passes 3. These are boundary evidence and do not establish deployed admission behavior. Actual operator/API/worker/recovery acceptance requires the connected flow in both native AMD64 and ARM64 Compose jobs at the reviewed pull-request head. Browser, general throughput/fairness, physical disk exhaustion, later consumers and whole-product qualification are separate gates.

An initial eager sibling import passed the ordinary boundary invocations but failed isolated-Python recovery loading with `ModuleNotFoundError`. The corrected hook uses a lazy location-based loader after capture, preserving isolated/spec import compatibility without changing `sys.path`. The failure is retained as part of this increment; it is not deployed admission evidence.
