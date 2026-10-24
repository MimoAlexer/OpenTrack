# Local-CA recovery qualification increment

Status: source/privacy review clear and synthetic checks passing; final test/evidence review and actual qualification remain pending. This increment is bounded work for #7/#46 under the recovery owner's acknowledged delegation. It does not complete either issue or M08.

Baseline: qualified integrated main `41ffc8d1b8ccccfce6296f944d4f064463963a07`. The earlier HTTP cold restore includes seven mandatory volumes and the selected operator-output volume. The standalone HTTPS rehearsal verifies local-CA trust and proxy restart; it does not prove recovery of populated proxy volumes or authenticated HTTPS sessions.

## Implemented source

The in-process source phase retains the existing owner password and actor jars in memory. A separate verified HTTPS transport copies cookies and policy for each read without changing the original HTTP actor or accepting response cookies into its jar. The phase checks the original owner and reader controls before creating a separate owner session through HTTPS login, then checks that session's secure-cookie properties.

The metadata observer aggregates only directory/file UID, GID, mode and count classes for the two fixed owned proxy volumes. It includes each root and rejects links, hardlinked regular files, special entries/bits, cross-device traversal and unreadable metadata. Limits are 4,096 entries and depth 32 per volume, 64 classes per volume and 16,384 UTF-8 output bytes. Its decoder rejects duplicate JSON keys/classes, extra fields, invalid types/ranges/counts and malformed or excessive output. Root-only observations cannot establish populated state.

The named observer helper uses the cached bootstrap image, two read-only volume mounts, no network, a read-only root filesystem, the fixed nonroot identity and dropped capabilities. Its source fence, identity checks, deadline, cleanup and source restart are part of the qualification controller. Public output excludes entry names, paths, contents, certificate/key material, inventories, private hashes, raw container inspection and exception text. No proxy archive policy is implemented before actual populated metadata has been measured and reviewed.

## Executed checks

The 59-test boundary suite passed in 1.086 seconds with an empty environment and isolated Python inside user and network namespaces. It includes the prior recovery controls, pure metadata/decoder boundaries, helper ownership/deadline/cleanup controls, seven read-only transport controls and ten source-phase/cleanup-command tests. Phase controls cover original actors before secure login, secure-cookie refusal, stopped writers, exact port/network refusal, interruption and independent cleanup, stopped/foreign proxy refusal, preexisting stopped-worker refusal and explicit helper cleanup selection. The phase tests delegate the full authorization validators to stubs; real authorization acceptance still requires the API-backed configured runs.

The first 59-test run failed because the new CLI test attempted to patch `sys.argv` through a subject module that does not import `sys`. A one-line test-only correction to patch the standard module directly produced the full passing run. No subject behavior was changed for that correction.

These are synthetic filesystem/transport/container-boundary tests. No Docker, application API, TLS handshake, CA verification or populated-proxy measurement executed in those runs. Independent source review also caught acceptance of a preexisting stopped HTTP service; the implementation now rejects stopped, restarting, OOM-killed or explicitly unhealthy required services before TLS mutation. Final test/evidence review remains pending.

## Outstanding qualification

Source review caught an owned proxy orphan during HTTP restoration and unnecessary worker/HTTP-relay activity during TLS; both were corrected before execution. The recovery owner explicitly acknowledged a private temporary qualification override connecting only proxy to the existing ingress network alongside internal web. Production Compose and workflow files remain unchanged. Source checks require exact owned memberships/labels, actual and requested loopback port mappings, and stopped-proxy removal without force before HTTP restoration. The override remains available until the last TLS command; no actual TLS execution is claimed.

Still required: final test/evidence review and owner disposition for the first configured measurement; genuine original-session and separate secure-session controls; stopped-writer populated metadata observation; owner review of observed classes; exact ten-volume archive/restore with byte and metadata comparison before any restored Caddy or initializer starts; original-CA hostname verification and untrusted-CA rejection; source HTTPS restart followed by HTTP restoration and owned cleanup. Both actual architectures, all four fresh configured checks, normal browser regression and final exact-head review remain required before integration.

Public ACME/renewal, encrypted/nightly backups, prior-release/failed-migration and different-host recovery, native/physical/assistive-technology acceptance, broader M08 and whole-product completion remain separate gates. Only the coordinator's independently verified `COMPLETE.md` can establish completion of all queues and product gates.
