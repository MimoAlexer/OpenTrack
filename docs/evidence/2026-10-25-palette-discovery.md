# Scoped palette team and project discovery

Implementation: `a0fe087be32e621d10e7925a966e301f5fe512ec`, including upstream team-query contract `a4feb4345fe28d72c24d563ac8d0390ac410ad07`, based on `0705a9fd`. The [machine receipt](2026-10-25-palette-discovery.json) records source, bundle, log and browser hashes.

The palette searches accessible teams by literal name/key and projects within the explicit route team. Search is debounced by 150 ms; issue, team and project rows share a fair maximum of 20 remote results. Static commands remain separate. Team/project selection requires a current authorized detail read before normal guarded navigation. Previous remote results clear on query, workspace, authorization or team changes, and obsolete responses cannot restore them. No entity titles are persisted as new recents.

The team endpoint accepts a separate `TeamQuery`, preserving generic directory queries. Normalized search accepts at most 100 Unicode scalars without control characters. Signed team cursors bind normalized query/state/limit, workspace, principal and authorization epoch. Existing private-team visibility, archive filters and UUID ordering remain authoritative. Each palette group retains its own errors and retry; a team/project validation error does not mask valid issue results.

Verified locally:

- Real restricted-PostgreSQL team HTTP contract: 1/1; existing identity lifecycle: 1/1. Coverage includes default listing, literal matching, Unicode limits, private visibility, pagination, cursor context/signature/epoch rejection and archive filtering.
- Final focused frontend units: 52/52 across four files, including cached-team duplication, stale query/scope responses, current-detail denial, independent group errors, fair bounds and dirty-navigation guards.
- Both TypeScript targets, affected Rust library Clippy with warnings denied, Rust formatting, server build and corrected production frontend build pass.
- Six unique connected cases pass across Chromium, Firefox and WebKit: three unchanged private/revoked-visibility cases retained from `connected-02`, plus three corrected discovery/validation/keyboard-navigation/reload cases from `connected-04`. Both use the same frozen corrected frontend and backend. No single all-green six-case run is claimed.

The original six browser failures exposed cached bootstrap team commands duplicating new server rows and IDs. The palette now replaces those cached team entries with bounded authorized search results. Empty queries retain other commands and reference-only issue recents; teams require a search query. Those failures remain preserved. A second run passed all three visibility cases but found a fixture query matching older retained issues; the corrected query is unique and exactly 101 scalars. An intervening grep selected no tests and is recorded as zero executed cases. No backend rebuild or successful visibility rerun was needed for these fixture/runner corrections.

Every browser scope verified containment, absence of service/database authority in the browser environment, zero remaining scope processes, monitor/API reaping and clean cleanup. The canonical single-worker, zero-retry, 60-second test and 10-second action/expect bounds were preserved. Private raw artifacts remain separate from the published receipt.

No migration, persisted-format or dependency changes. Previously issued unsigned team-list cursors must restart pagination. Rollback removes query/discovery support without changing stored entities. The lead must regenerate the single changed OpenAPI operation and TypeScript contract on the integration union.

This is private development qualification. Independent final review, integrated-main behavior and all four hosted checks remain integration gates. Native forwarding was inspected statically; native GUI, physical input/layout, OS IME, assistive technology and screenshot acceptance remain separate. No broader F22, issue, milestone or whole-product completion is claimed.
