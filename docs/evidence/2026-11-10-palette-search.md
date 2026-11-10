# Ranked command-palette discovery

Production source: `54f316fb52d6cbada920e5645e4e7f28cdf2d787`. Unit controls: `d4c3e6a6688f548cde9fff619628bacf40bd460f`. Browser specifications: `85ee9a0aa472d0ea282c8e50174a35d2caa38472`. These compose on the merged schema-0037 baseline; no migration or wire DTO changes are needed.

Nonblank palette queries now use the existing authorized mixed search endpoint. Issue descriptions and current project briefs are searchable across readable teams, including when no team is selected. Results retain server relevance order and plain-text snippets; the shared 20-result bound reserves up to five available team results, filling spare slots when work results are fewer. Blank queries retain reauthorized recent-issue references. Fuzzy unavailability and independent team/search errors stay explicit; invalid queries are never shortened or replaced with a broader lookup.

Selection obtains current issue or owning-team project metadata before navigating. Exact query, retry, route, session and authority generations fence delayed responses. The parent authority guard also cancels a queued navigation after access refresh without canceling normal close-before-navigation. Private suggestions are removed on current-read denial; ordinary navigation still applies unsaved-draft confirmation. Long literal snippets wrap inside the bounded palette.

## Executed checks

- Baseline frontend: 1,136 tests in 93 files passed. New request tests fail on the old title-directory implementation.
- Final unit integration: 1,172 tests in 94 files passed; both TypeScript targets and production build passed. The existing large-chunk warning remains.
- The baseline real-browser journey fails because an issue-description-only term produces no palette option. The first candidate run passes 18/24 checks; the six failures are two obsolete fixture assumptions repeated across three engines (old validation wording and desktop-only controls at mobile width). A focused continuation passes 3/6 cases and exposes one additional mobile-only helper assumption: an unnecessary desktop close button before reopening the palette. Those fixture assumptions were corrected using the existing keyboard palette shortcut; no timeout, retry or authorization guard changed.
- [Final browser receipt](2026-11-10-palette-search-browser.json): **24/24** checks, eight each in Chromium, Firefox and WebKit, using the built frontend, real API/PostgreSQL and restricted worker. Canonical serial execution, retries zero, disposable admission and origin checks remain enabled. Tests cover saved body/current-brief matches, cross-team project keyboard navigation and reload, literal HTML-shaped text, 320 px wrapping, private positive/negative controls, stale current reads, delayed real query responses, exact scalar limits, team navigation and blank recents.
- Backend source is unchanged from baseline `bfd7f25070eaef54255a335005877e45ef5aa5bf`; the locally built browser server SHA-256 is `a5aed8b44dea885fa0482ef5d6cedbc8237d7780c859fd6c3204bdc38ffee9ca`. Owned test services were stopped after qualification.
- Independent implementation/authorization and test reviews found no remaining actionable findings. Final protected integration still requires all four hosted checks on the exact proposed head.

## Impact and remaining gates

This is an M06-T05 increment, not completion of the milestone or issue #36. Favorites in workspace navigation, full search-page UX, large-list virtualization, broader performance, native GUI, physical input and assistive-technology qualification remain open. The existing native broker permits these read routes; browser evidence is not native runtime evidence. No durable data is added or rewritten; rolling back the application restores the previous client behavior without a schema rollback. Credentials, raw logs, browser state and private databases are excluded.
