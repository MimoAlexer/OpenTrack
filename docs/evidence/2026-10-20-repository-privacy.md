# Repository privacy check — 20 October 2026

Related tracking: [issue 117](https://github.com/MimoAlexer/OpenTrack/issues/117).

## Current content

Replaced personal fixture labels in 21 first-party test files with role-based synthetic examples. Mention search prefixes, selection assertions, punctuation cases, and mailto validation cases retain their original behavior. No application behavior or database schema changes.

`AGENTS.md` now requires neutral fixtures, non-personal commit metadata, review of screenshot pixels and metadata, and execution limited to local development and configured GitHub CI. The local repository Git identity was changed to a neutral automation identity before making this commit.

Audit coverage:

- Tracked UTF-8 content and relevant paths were scanned; generic upstream examples and required repository URLs were distinguished from personal data.
- All 43 tracked evidence screenshots received local OCR and manual pixel review. No affected screenshot was found. Metadata in 48 raster assets was also checked; product icons remain unchanged.
- The 116 existing GitHub issues and pull requests, three conversation comments, available review text, repository description, and current README were checked. No matching personal or unrelated infrastructure text was found.
- The 23 pending first-party files in the separate development checkout were scanned, including untracked portability and activity work. No matching identifiers were found there.

## Validation

- `npm ci --no-audit --no-fund`: locked dependencies installed.
- `npm test -- --reporter=dot`: 381 tests passed across 51 files. The existing composer test still emits React `act` warnings; no test failed.
- `npm run typecheck`: passed.
- Independent read-only review confirmed the fixture changes preserve test intent.

## Historical scope

A forward cleanup does not remove old Git objects, closed pull-request references, existing clones, or hosted cached views. Historical contribution metadata needs a separate coordinated removal step. Do not describe this current-tree cleanup as complete erasure of repository history.

## Retrospective

Use synthetic role labels from the start, set repository-local contribution metadata before committing, and check generated evidence before publication. Passing application tests alone does not verify repository privacy.

## Maintained-history cleanup

After the user approved the prepared history update, the sanitized main was published with an exact expected-old-head lease. The temporary protection exception was removed immediately and the original protections were read back and verified. The reviewed current source tree was unchanged.

The rewrite corrected 12 author/committer identity pairs and 31 historical fixture versions across 21 files; 28 commit IDs changed. A fresh scan of the 40 retained main commits and 1,472 UTF-8 blob versions found no known identifiers. All 49 reachable image blobs matched the already audited current assets. Active local worktrees and other local refs were moved to the sanitized ancestry while preserving pending file bytes. No concurrent Git garbage collection was run while feature agents were writing.

GitHub still advertises three retained closed-PR heads and may serve cached old objects. Removal of those hosted references remains tracked in issue 117; the maintained branch cleanup alone does not prove their erasure.
