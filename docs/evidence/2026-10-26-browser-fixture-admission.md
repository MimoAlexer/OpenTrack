# Browser fixture admission coordination

The diagnostic base `6fcb481f` retained 231/240 passing browser cases. One invitation acceptance and eight owner logins reported HTTP 429; the two-line server summary recorded only `peer_budget`. That summary is not request-correlated accounting.

The fixture coordinator reserves existing setup traffic before its original actions. Parent preparation reserves four slots, every worker authentication reserves one, and the fourteen file phases reserve the exact upper bounds in the JSON evidence (maximum six). The production limits remain 20 per peer and 100 globally per 60 seconds.

One Python algorithm uses real integer `monotonic_ns()` values. Pending reservations stay fully charged while their phase can emit. Completion charges the full cost for sixty seconds; a newer `workerIndex` settles abandoned pending cost at transition. A stable separate lock protects atomic JSON replacement in private 0700/0600 storage. Missing, corrupt, backward, overlapping or exhausted state fails without reset. The fixed CLI receives no credentials, and Node never reads ledger timestamps.

Hook/test/action/preparation/suite bounds remain 60/60/10/300/900 seconds, with one worker and zero retries. Waits use computed availability and recheck early wakes. A wait that cannot fit fails explicitly. The fourteen specs add only imports and hook registrations: removing those two lines reproduces their original bytes exactly. Every collected browser/title identity matches the full base receipt.

Verification completed on the source hashes in the JSON evidence:

- 72/72 pure Linux Python tests, including real file permissions, stable locking, failure paths and a real-clock coordinator CLI sequence.
- 22/22 Node tests: seven coordinator wrapper tests and the existing fifteen privacy tests.
- Browser and frontend TypeScript checks exited 0.
- Collection contains exactly 240 unique cases, 80 per engine; no browser runtime case was executed locally.

The failed floating-point prototype and initial TypeScript guard error are retained separately. The integer model passed 160,000 phases and 20,034 restarts. A replay of the earlier passing receipt modeled 15.259 seconds of additional waiting, excluding preparation and harness overhead; this is feasibility evidence only.

No migration or production policy changes. Independent review and real configured CI at the exact integrated head remain required. Native and other product gates are not qualified by these checks.
