# Projects

Open **Projects** in the workspace sidebar, choose a team, then select **New project**. Each project belongs to one team and follows that team's access rules.

## Plan the work

A project has a name, short summary, rich-text brief, optional lead, start date and target date. Its state is Planned, Active, Paused, Completed or Canceled. Health is a separate manual assessment: On track, At risk or Off track.

The brief supports the editor's ordinary text, lists, checklists, links and code. Project briefs do not yet bind attachments, user mentions or issue references. They currently share the project's version and conflict review; independent brief revision history remains unfinished.

Use **Edit project** to update these fields. Conflicting edits show the latest saved version beside the draft. Choosing to keep the draft does not save it automatically: review it and select **Save project**.

## Connect issues

The **Issues** tab adds or removes existing issues from the owning team. An issue can belong to one project. Linking it preserves its status, assignee, description and discussion.

Moving a linked issue to another team requires detaching it from its project first. This maintains the same-team database constraint. Removing a link does not delete the issue.

## Read progress

The overview calculates progress from current issue states:

- Archived and deleted issues are outside the count.
- Canceled issues are shown separately and excluded from eligible scope.
- Completion is completed issues divided by eligible issues.
- Empty eligible scope displays **No eligible issue scope**, with no invented percentage.

The calculation time is displayed beside the result. Health does not change automatically when the percentage changes.

## Write an update

The **Updates** tab publishes an authored text update with the project's state, health and progress at that moment. These snapshots remain as recorded when later work changes the project. Updates are immutable in this first increment.

## Complete or archive

Completing a project leaves every linked issue in its existing state. The interface asks for explicit acknowledgement; the server also enforces acknowledgement when eligible open issues remain.

Archiving removes the project from the active directory without deleting its work. Team maintainers can find it under **Archived projects** and restore it. Project editing and issue linking are read-only while archived.

## Current boundaries

This increment provides project lifecycle, issue membership, progress and updates. Milestones, cycles, timeline scheduling, multi-team projects and richer project documents remain separate work. See the [requirements ledger](REQUIREMENTS_STATUS.md).

Migration `0022_projects.sql` adds the project tables and constraints. Apply it before starting the matching API and worker. Earlier migrations remain immutable. Rollback requires a matching database backup and application binary; no destructive down-migration is provided.
