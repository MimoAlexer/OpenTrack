import type { Bootstrap, Issue, IssueDraft, Session } from '../types';
import { priorities } from '../types';
import type { WorkflowStatus } from '../workflows/types';
import { AssigneePicker } from '../taxonomy/AssigneePicker';
import { LabelPicker } from '../taxonomy/LabelPicker';
import type { AssignableUser, LabelSummary } from '../taxonomy/types';
import { selectable, statusLabel } from '../workflows/model';

export function IssueFields({ draft, change, data, teamId, disabled = false, statuses: supplied, currentStatusIds, defaultMode = false, defaultId, chooseStatus, workspaceId, session, issue, repairOnly = false, rememberedAssignee, rememberedLabels }: {
  draft: IssueDraft; change: (draft: IssueDraft) => void; data: Bootstrap; teamId: string; disabled?: boolean;
  statuses?: WorkflowStatus[]; defaultMode?: boolean; defaultId?: string; chooseStatus?: (id: string) => void;
  currentStatusIds?: Set<string>; workspaceId: string; session: Session; issue?: Issue; repairOnly?: boolean; rememberedAssignee?: (person: AssignableUser) => void; rememberedLabels?: (labels: LabelSummary[]) => void;
}) {
  const all = supplied ?? data.statuses.filter(status => status.team_id === teamId) as WorkflowStatus[];
  const value = defaultMode ? defaultId ?? '' : draft.status_id;
  const current = all.find(status => status.id === value);
  const available = (status: WorkflowStatus) => selectable(status) && (!currentStatusIds || currentStatusIds.has(status.id));
  const statuses = all.filter(status => available(status) || status.id === value);
  return <fieldset className="issue-fields" disabled={disabled}>
    <label className="field">Status<select value={value} onChange={e => { chooseStatus?.(e.target.value); change({ ...draft, status_id: e.target.value }); }} required>{!current && <option value={value} disabled>{value ? 'Status unavailable' : 'Loading default…'}</option>}{statuses.map(status => <option key={status.id} value={status.id} disabled={!available(status)}>{statusLabel(status)}</option>)}</select>{defaultMode && <small>The current team default will be used.</small>}{current && !available(current) && <small className="status-field-notice">This status is no longer available for new assignments.</small>}</label>
    <AssigneePicker workspaceId={workspaceId} teamId={teamId} session={session} value={draft.assignee_id} current={issue?.assignee} disabled={disabled} change={assignee_id => change({ ...draft, assignee_id })} remembered={rememberedAssignee} />

    {!repairOnly && <><label className="field">Priority<select value={draft.priority} onChange={e => change({ ...draft, priority: Number(e.target.value) })}>{priorities.map((name, value) => <option key={value} value={value}>{name}</option>)}</select></label>
    <label className="field">Estimate<input type="number" inputMode="numeric" min="0" max="1000" step="1" value={draft.estimate ?? ''} onChange={e => change({ ...draft, estimate: e.target.value === '' ? null : Number(e.target.value) })} placeholder="No estimate" /></label>
    <label className="field">Due date<input type="date" value={draft.due_date ?? ''} onChange={e => change({ ...draft, due_date: e.target.value || null })} /></label></>}
    <LabelPicker workspaceId={workspaceId} teamId={teamId} session={session} value={draft.label_ids} summaries={issue?.labels} teams={data.teams} disabled={disabled} change={label_ids => change({ ...draft, label_ids })} remembered={rememberedLabels} />
  </fieldset>;
}
