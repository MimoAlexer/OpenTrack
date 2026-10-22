# Shared interface refinement and refresh stability

Date: 18 October 2026. Scope: the user's report that the shared browser/desktop interface looked and felt poor and intermittently flickered. This is a qualified UI repair, not completion of the remaining product features or native GUI acceptance.

## Reproduced causes and repair

The entry module imported App before the global stylesheet. A component stylesheet declared the components layer first, so the later reset layer overrode component typography and button backgrounds. The global stylesheet now loads first. A production-build guard inspects emitted CSS layer order. Reinstating the old import order in a bounded negative check caused the build to reject it; the fixed build passed. The source was restored in a finally block. Log: `work/check-css-layer-regression.log`.

IssueList previously replaced all rows with Loading on every refresh. Initial workflow hydration also called the parent change callback even when bootstrap already contained identical statuses, triggering another list refresh just by opening an issue. Rows now remain mounted during same-scope refreshes; initial hydration compares real metadata; current team status snapshots replace stale bootstrap entries. Changing workspace, authentication or filters hides previous rows synchronously, and authorization failures discard them. Activity uses the same scoped snapshot pattern for save/version/retry refreshes. Access polling invalidates once per observed epoch while a slow bootstrap is outstanding, continuing to observe later changes and revocation.

Saved appearance is applied by a same-origin script before initial rendering. Title sizing runs before paint, description padding is stable between rest and focus, scroll gutters remain stable, and keyboard saves restore the focused editable control when the browser moved focus to the body during disabling. No GPU workaround, permission relaxation or retention across access boundaries was introduced.

Follow-up: the final attachment qualification found that the browser server did not serve the referenced `/theme.js` file. The script was present in the build but returned HTTP 404. The exact route and a real HTTP regression are now implemented; see [the later qualification](2026-10-18-attachments.md). The earlier theme preference interaction check alone did not establish delivery before rendering.

## Interface changes

The sidebar has one workspace menu with switch/create/join actions. It stays within the sidebar, consumes Escape without closing the open issue, and restores focus to a visible trigger before opening a dialog. Redundant bottom chrome was removed. The header is52px, rows42px, sidebar220px; a split issue view requires at least1280px viewport width and reserves400px for the list and560px for detail.

Issue detail is a centered760px reading column with a wrapping24px title, grouped properties, a smaller description area and natural-height sections. Smaller panes use two property columns through a container query. Status options no longer repeat their category, while selected label scope and archived metadata remain visible. Empty relationship scaffolding is consolidated into Add connection plus an overflow, preserving all permitted actions, populated links/counts, conflict/dirty-state protections and dialog focus return. Picker bounds account for the nearest scroll pane, scrollbar, viewport and modal top layer, and remeasure on scroll/resize.

## Executed evidence

- Full frontend suite:206 tests across22 files passed; typecheck and production build passed. Logs: `work/test-ui-release.log`, `work/build-ui-release.log`.
- New regression coverage includes unchanged row DOM during delayed refresh, scope changes, denied access, transient failures, initial workflow metadata changes, repeated epoch checks, retained activity, connection disclosures/focus, and four picker-bound cases. Existing conflict, draft, archive and authorization tests remain passing.
- In the live browser on127.0.0.1:18970: list refresh, issue opening, description edit/save, Ctrl+Enter save, workspace menu Escape, label picker, parent dialog cancellation/focus return and light/system-dark appearances were checked. ENG-5's original empty description was restored; two normal activity entries record the temporary verification edit and its restoration. Appearance was returned to its original System setting.
- The final screenshot shows the actual1024×900 browser viewport: [issue detail](2026-10-18-ui-refinement.png). Wide/mobile geometry has source and bounds-regression coverage, not an exhaustive rendered viewport/accessibility qualification.
- Assets were built outside the live web directory. Complete hashed assets were copied before atomic entry-file replacement, retaining prior hashes for already-open tabs.

## Linux package

The final shared sources were packaged successfully into `desktop/target/release/bundle/deb/OpenTrack_0.1.0_amd64.deb`,7,987,364bytes, SHA-256 `902f5532e4c5515e521ee555efc362704391116109b2b439bd6fe3ab641841f6`. The installed executable exactly matches release output (`0d86c750fc8bf57e280e3620b4474fb9637b4867eece39cd886ecb66546a27ec`). Package extraction, desktop-entry validation, URI argument forwarding and the expected Tauri bundle-marker-only binary difference passed. Receipt: `work/desktop-build/package-ui-release-result.json`; build log: `work/desktop-build/package-ui-release.log`.

The native app was already closed. The newly installed executable was launched and remained running without stderr (PID462051 at this check; log `work/desktop-build/installed-ui-release.log`). This is a process check, not a native screenshot or proof of native visual behavior. The real saved-session transport check and29 native tests are recorded in [desktop evidence](2026-10-18-desktop.md). Native visual acceptance, menu/deep-link interaction and other platform gates remain open; the user is not required to perform repeat manual tests before independent work continues.

No database migration, schema change, external publication or account change occurred in this repair. Migrations0001–0013 remain immutable. Broader milestones and the active complete-product goal remain unchanged.
