# Shared browser qualification deadline

Corrective delta over immutable candidate `4c6bf8032996168b48e7cba8e2537905a9447062`. Its original source and 26-test evidence remain preserved. The original helper allowed 300 seconds of parent preparation followed by a fresh 900-second Node watchdog; that extended the approved qualification window.

The runner now creates **one monotonic 900-second deadline immediately before parent preparation**. Docker startup and setup-token retrieval remain outside, as before. The helper uses the earlier of that deadline and its own 300-second cap. Node receives only the positive remaining suite time; expiry prevents launch. Existing bounded restoration of the same owned worker and disposable-project cleanup still run after expiry as safety cleanup, without granting further qualification time. Resource, privacy, environment, worker-profile and per-test limits remain intact.

**30 pure tests passed**, using mocked Docker/HTTP only. Four new clock cases verify: 240 simulated startup seconds are outside the suite and 137 preparation seconds leave exactly 763 seconds for Node; full expiry prevents Node and still cleans the owned project; a 40-second remaining suite budget clips preparation while restoration keeps its separate bound; and expiry after worker stop still restores that worker without fixture SQL. The original exact 900-second Node assertion remains for a frozen clock with no elapsed preparation. `git diff --check` passed; exact executed source hashes are in the companion JSON.

No Docker, real HTTP/database preparation, browsers, CI or full repository matrix was executed. Runtime acceptance remains the lead's configured CI responsibility. No schema, production, UI, Compose, credentials, shared ledgers or Goal accounting changed.
