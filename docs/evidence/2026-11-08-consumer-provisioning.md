# Restricted consumer grants after reprovisioning

Executed on 8 November 2026 using only the isolated PostgreSQL 18.6 cluster on port 55672. No listener or jobs were started by the connection probe, and no parent/other-worker database was used.

Before the correction, an actual development setup rerun left three guarded consumer relations accessible to the serving role. The read-only `check_provisioned_connections` example reported `app_connect: false`, `worker_connect: true`. Independently, a fresh disposable CI database passed both constructors after migrations, then failed the same API check after CI rebootstrap; catalog inspection identified thirteen guarded relations. This failing regression ran before the helper change.

Development and CI provisioning now reapply the same fifteen-relation no-access policy already enforced by `App::connect`. `planning_issue_facts` retains SELECT without mutation rights. Constructor guards and worker function capabilities were not relaxed.

After repeating development setup/worker grants and CI bootstrap → migration → rebootstrap/worker grants, both real constructors passed and the catalog reported zero accessible guarded relations. The probe also confirmed required subscription preference writes, subscription-event insertion, template read/insert/update and immutable source insertion, and planning-fact read-only access. The earlier scope-head/event policy regression passed again; baseline/item INSERT remains available. Disposable CI databases/roles were removed after each run.

Reproduce the constructor/catalog controls with explicit local runtime and worker credentials:

```sh
cargo run --locked -p opentrack-app --example check_provisioned_connections -- --read-only-local-role-check
```

The probe prints only three booleans, never credentials; it performs catalog/role checks rather than authentication or job execution. Public-schema reprovisioning evidence complements the existing subscription, template and worker contracts.

Validation also passed all eleven `scripts/test-*.py` suites, four real PostgreSQL mention/subscription contracts, seven template contracts, the worker contract, strict Clippy for the probe, formatting and shell/Python syntax checks. The API/worker constructor source matched the composed `fd4aa2d` candidate; the independent project-update validator trigger correction remains separate.
