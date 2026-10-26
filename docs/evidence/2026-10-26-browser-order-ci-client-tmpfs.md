# Bounded PostgreSQL client tmpfs

Based on exact failed head `5431c785be39efde95a351b8404d1232c09e0c24`. The observed predicate is `worker-grant/mount_count`; the old extra mount identity was not captured.

Bounded public OCI reads verified every digest body from the repository's pinned index through its Linux/AMD64 manifest and config. That exact config declares only `/var/lib/postgresql` as an image volume. The JSON records the complete digest chain and selected public metadata; anonymous authorization was never logged or saved. Moving-tag web output is excluded as pin proof.

[Engine 28.0.4](https://github.com/moby/moby/blob/v28.0.4/daemon/create_unix.go#L40-L67) skips image-volume creation when [HasMountFor](https://github.com/moby/moby/blob/v28.0.4/container/container_unix.go#L130-L145) finds the legacy tmpfs entry. One client-anchor line now supplies a 1MiB/mode0700 tmpfs at that unused destination for four provisioning/grant clients. Database durable storage and every other Compose line remain unchanged.

Strict config/HostConfig tmpfs checks cover the clients and existing app/tmp. Named-volume/access, network, credential, worker, claim, cleanup and timing guards stay intact; anonymous volumes remain forbidden.

**58 pure tests passed**, including all prior55. Red models reproduced the count rejection and missing drift checks. Tests cover inheritance/suppression, invalid options/paths, foreign resources, secrecy and exact cleanup. Source-only YAML and one-line scope checks passed; original output and source hashes are in the JSON.

No local Docker/services, fixtures, GUI, CI/head mutation or database changes ran. Fresh configured execution must prove live mount sets and browser acceptance; this is not runtime qualification.
