# Private-team issue moves across cold recovery

Implemented 23 October 2026 as bounded #7 assistance. Agent 5 retains recovery/M08 ownership; Agent 1 retains production move and authorization behavior. This extends the accepted [grant-revocation fixture](2026-10-22-agent-4-private-team-recovery.md) without replacing it or the deployed event-admission qualification.

The [Docker fixture](../../scripts/docker-smoke.py) creates private source A, destination B and the independent revocation team. Its synthetic viewer receives A and revocation-team grants before its session is issued; B is never granted. A separate issue and document remain unchanged in A. The reader first retrieves the moving issue, both document routes, attachment metadata and exact file bytes while they belong to A, retaining their original UUID URLs and validators.

The owner obtains a real move preview and requires the expected issue/source/destination, private destination access, `between_private_teams` audience change and no graph blockers. The actual move consumes the preview token, default destination status, issue `If-Match` and a distinct idempotency key. It must return the original issue ID in B. The owner's current destination access must still retrieve the original document IDs/body and matching attachment bytes. This uses the existing [issue command contract](../features/F06_ISSUES.md); it does not redefine move policy.

Before the stopped snapshot and after cold restore, the same viewer session must retain public content and the unchanged private-A issue/document. Saved moved issue/document/file URLs must return the exact generic 404 problem with `no-store`, no private markers and no validator/download/range/redirect metadata. Conditional requests use the original per-route issue/document/metadata validators, including the saved file metadata validator on content requests; wildcard and file range variants must also remain concealed. Positive owner reads never replace these saved validators. Cookie equality covers both independent grant-removal and moved-resource checks.

The original relay, stopped database/blob/five-credential snapshot, data/authority fingerprints, restored-image checks, fault repair, labelled cleanup and source restart remain. The shared reader jar is disposed even when snapshot or cleanup fails. The [baseline recovery evidence](2026-10-22-agent5-offline-recovery.md) owns the underlying recovery scope and limitations.

## Executed local checks

The complete [boundary suite](../../scripts/test-docker-recovery.py) passed 18/18 tests with a cleared environment, isolated Python and separate user/network namespaces. All 15 earlier controls remain. New cases reject seven contradictory previews before a move is dispatched, bind the real preview's token/status/ETag to the command, preserve original URLs/validators, reject altered A controls before owner/denial reads, and bind conditional denials to each saved resource. These unit fakes exercise controller safeguards; they do not qualify actual database/API or cold-restore authorization.

## Remaining qualification

Require independent source/privacy review, all four configured checks on the stable published head and actual AMD64/ARM64 execution of initial authorized reads, source post-move denials and restored same-session denials with public/A/owner controls. Keep every observed failure and route production defects to their owner; do not weaken concealment assertions to obtain a pass.

This has no migration, production ACL/native behavior or provider change. It covers these web-session UUID URLs and does not establish native/cache, alias, other module-family, encrypted/nightly/provider-key, prior-release/failed-migration or different-host recovery acceptance. #7, M08 and full-product completion remain open. Reverting this fixture increment restores the preceding qualification flow without changing application authority.
