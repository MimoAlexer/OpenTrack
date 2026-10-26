# Browser CI inspect rejection diagnostics

Based on failing integrated head `a148af442b50b16ce831d82360d5b1afba06ff7a`; previous worker source remains frozen. AMD64 job `121326105742` in run `40245497586` failed before browser execution with the combined ownership message, also during independent cleanup. The full private log hash is in the JSON. It records Engine 28.0.4/API 1.48 and Compose 2.38.2.

The actual rejected predicate remains **unresolved**: inspect stdout was captured and withheld. Exact primary sources distinguish [legacy Compose tmpfs configuration](https://github.com/docker/compose/blob/v2.38.2/pkg/compose/create.go#L223-L291) from [Engine mount-point inspection](https://github.com/moby/moby/blob/v28.0.4/container/container_unix.go#L371-L412). This does not establish tmpfs or image inheritance as the cause.

This delta preserves the guards and emits only a fixed predicate code and a whitelisted expected service role. It prints no inspected mount/environment/label values, paths, foreign names, credentials or captured output. No mount type, role, mapping/access or worker-profile policy is relaxed. The next configured run can distinguish project/service/state, mount shape/type, count, names, targets, mapping and access rejection.

The client image also inherits official PostgreSQL image metadata. Database overrides its data-volume destination, while worker-provision/worker-grant declare only secrets. An extra anonymous data volume on those clients is a concrete source-derived hypothesis; the actual runtime predicate remains unproved. The diagnostic does not authorize that volume or change Compose.

**55 pure tests passed**, including all prior51, diagnostic secrecy across poisoned inputs, the documented legacy tmpfs shape, strict rejection of extra tmpfs/image-volume shapes and safe unknown-role rejection. The JSON retains original output and source hashes, including the intermediate54 pass. The corrected red diagnostic test had ten subtest failures before the message change; an earlier test-context error is recorded separately. `git diff --check` passed.

This is a diagnostic correction, **not runtime remediation or acceptance**. No local Docker/services, fixtures, browsers, head update or CI restart/cancellation ran. All timing, credential, resource, claim and cleanup boundaries remain. The lead owns independent review and the next configured execution; old failed CI and proof are preserved.
