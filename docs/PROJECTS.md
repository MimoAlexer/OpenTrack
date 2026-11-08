# Projects

Open **Projects** in the workspace sidebar, choose a team, then select **New project**. Each project belongs to one team and follows that team's access rules.

## Plan the work

A project has a name, short summary, rich-text brief, optional lead, start date and target date. Its state is Planned, Active, Paused, Completed or Canceled. Health is a separate manual assessment: On track, At risk or Off track.

An inactive member remains visible as a historical lead and cannot be selected again. Other project metadata stays editable. Team management authority is required to change or clear an existing lead; choose **No lead** to clear it explicitly. Contributors can select an eligible initial lead when creating a project.

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

The **Updates** tab publishes an authored text update with the project's state, health, progress and dates at that moment. Optionally choose **Record reviewed risks and decisions** to add short notes with their own status. Empty reviewed lists explicitly record no notes; leaving the option off keeps this context unspecified. These snapshots remain as recorded when later work changes the project and cannot be edited after publication.

Each update compares with its immediately preceding snapshot, including across a page boundary. Date changes and added, changed or removed risk/decision notes are shown explicitly. Historical dates and notes that were never captured remain unknown. The overview brings the latest update and its open risks/needed decisions together with the brief, current properties, scope and milestone dates. If the project changes before publication, the draft is retained for explicit review of current context before trying again.

## Complete or archive

Completing a project leaves every linked issue in its existing state. The interface asks for explicit acknowledgement; the server also enforces acknowledgement when eligible open issues remain.

Archiving removes the project from the active directory without deleting its work. Team maintainers can find it under **Archived projects** and restore it. Project editing and issue linking are read-only while archived.

## Current boundaries

Project lifecycle, issue membership, progress and updates connect to the [milestone and cycle planning screens](PLANNING.md). Timeline scheduling, multi-team projects and complete transfer/recovery qualification remain separate work. See the [requirements ledger](REQUIREMENTS_STATUS.md).

Migration `0022_projects.sql` adds the project tables and constraints. Apply it before starting the matching API and worker. Earlier migrations remain immutable. Rollback requires a matching database backup and application binary; no destructive down-migration is provided.

Migration `0026_project_documents.sql` copies existing briefs exactly into independent documents and retains the old columns as immutable snapshots. Use matching application/static assets; old writers are incompatible. A pre-upgrade backup is required for rollback. Retaining snapshots does not prove backup restoration or authorize their removal.

Migration `0030_project_update_context.sql` adds captured dates and reviewed notes without guessing historical values. Apply matching API/frontend/native changes together. Body-only clients remain supported and leave notes unspecified. Rollback requires the matching pre-upgrade database backup and binaries; no destructive down-migration is supplied.
