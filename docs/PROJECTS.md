# Projects

Open **Projects** in the workspace sidebar, choose a team, then select **New project**. Each project belongs to one team and follows that team's access rules.

## Plan the work

A project has a name, short summary, rich-text brief, optional lead, start date and target date. Its state is Planned, Active, Paused, Completed or Canceled. Health is a separate manual assessment: On track, At risk or Off track.

An inactive member remains visible as a historical lead and cannot be selected again. Other project metadata stays editable; choose **No lead** to clear the saved lead explicitly.

The overview brief supports ordinary text, lists, checklists, links, code, attachments, user mentions and issue references. It saves independently from project metadata. Conflicts retain the draft for review before an explicit save. **Brief history** opens read-only saved revisions without replacing the current draft; migrated initial snapshots clearly show unknown authorship and migration capture time. History follows the document retention and current-access rules. See the [project brief contract and executed evidence](evidence/2026-10-22-agent-2-issue-26-project-briefs.md).

Use **Edit project** to update metadata, and edit the brief directly on the overview. Conflicting metadata edits show the latest saved version beside the draft. Choosing to keep the draft does not save it automatically: review it and select **Save project**.

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

Each displayed update compares those recorded fields with the next update shown in the feed. Load more updates to compare across a page boundary. Dates, risks and decisions are not included in these comparisons yet. [Focused verification](evidence/2026-10-28-frontend-scope-and-updates.md) records the checks and remaining limits.

## Complete or archive

Completing a project leaves every linked issue in its existing state. The interface asks for explicit acknowledgement; the server also enforces acknowledgement when eligible open issues remain.

Archiving removes the project from the active directory without deleting its work. Team maintainers can find it under **Archived projects** and restore it. Project editing and issue linking are read-only while archived.

## Current boundaries

Project lifecycle, issue membership, progress and updates connect to the [milestone and cycle planning screens](PLANNING.md). Timeline scheduling, multi-team projects and complete transfer/recovery qualification remain separate work. See the [requirements ledger](REQUIREMENTS_STATUS.md).

Migration `0022_projects.sql` adds the project tables and constraints. Apply it before starting the matching API and worker. Earlier migrations remain immutable. Rollback requires a matching database backup and application binary; no destructive down-migration is provided.

Migration `0026_project_documents.sql` copies existing briefs exactly into independent documents and retains the old columns as immutable snapshots. Use matching application/static assets; old writers are incompatible. A pre-upgrade backup is required for rollback. Retaining snapshots does not prove backup restoration or authorize their removal.
