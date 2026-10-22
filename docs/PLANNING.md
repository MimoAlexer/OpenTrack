# Planning screens

Projects and Cycles are available in the workspace sidebar and command palette. Choose a team explicitly; an unavailable team never falls back to another team's data. Server capabilities control writes, and archived scopes are read-only.

## Project outcomes

The project overview retains its saved brief, properties, issue progress and authored updates. Its milestone section shows intermediate outcomes in their persisted order, target dates, manual state and live issue progress. A project lead or team maintainer can create, edit, complete, reopen or delete milestones. The move-up and move-down buttons support keyboard ordering without drag gestures. Deletion explicitly detaches milestone assignments while preserving issues and project membership.

In the Issues tab, each issue can be assigned to a milestone in the same project. **Group by milestone** groups the issues loaded so far; load more issues to expand a partial page. Group headings are independent of the authoritative progress counts. Empty and all-canceled scopes show **No eligible issue scope** without a completion percentage. Manual milestone completion does not change issue statuses or imply complete issue scope.

Edits use server validators. A stale draft is retained until the current saved version has been loaded and reviewed. Ordering conflicts reload the saved order without pretending the requested movement committed. Uncertain creates retry the identical body and receipt before permitting another edit.

## Team cycles

The cycle directory opens a saved interval with local start and exclusive end dates, its recorded timezone and explicit UTC boundaries. The end date is the first day outside the interval; daylight-saving changes do not change these calendar semantics. Date ranges are limited to 366 days and overlapping team intervals are rejected.

The detail page shows current accessible issue scope and measured progress. Canceled, archived and deleted work is excluded from eligible completion. Scope history plots the actual recorded observations and provides their exact dates, capture times, categories, counts and metric definition in an ordinary HTML table. Missing daily observations remain gaps rather than fabricated zeroes. Loading or failed history leaves the current scope available, with a retry action.

Contributors can explicitly add or remove issues while a cycle is open. Maintainers start cycles, capture snapshots and review closure. Closure requires acknowledgement of the current preview and preserves every issue and status. A stale preview must be reloaded and reviewed. The original closure report remains readable after later corrections or carry-forward.

Carry-forward is a separate action. Choose a later eligible destination and at most 100 unfinished issues; no issue is selected automatically. Each result is displayed. The operation ID is retained in the cycle URL, so reloading or reopening that link restores its result review. Unresolved accessible items can be loaded again with current versions, explicitly selected and retried against the saved operation. Resolved items are never retried. A conflicting operation or destination version clears the stale review and requires fresh versions and a new selection. In-flight and uncertain carry commands guard navigation until their receipt is reconciled; readers can reopen a receipt without receiving mutation controls. Restricted results do not imply completion. Reopening a closed cycle requires a recorded reason.

Historical reports and points are filtered by current issue access. Counts describe only the visible projection, with an access-filtering notice when necessary. Issue labels are current authorized data; recorded categories and estimates retain their snapshot meaning.

## Qualification boundary

These screens use the milestone and cycle API contracts in [Milestones](MILESTONES.md) and [Cycles](CYCLES.md). They add no schema migration or server authorization policy. Automatic recurring cycles, a daily snapshot scheduler, board movement, timelines and broader reporting remain separately tracked capabilities. Browser and unit evidence is recorded in the planning acceptance report; this guide does not claim native GUI or assistive-technology qualification.
