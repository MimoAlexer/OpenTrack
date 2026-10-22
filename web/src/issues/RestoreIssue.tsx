import { useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { changes, rebaseDraft, toDraft } from '../issue-model';
import type { Bootstrap, Issue, Session } from '../types';
import { useScopedRequest } from '../settings/shared';
import { selectable } from '../workflows/model';
import type { WorkflowStatus } from '../workflows/types';
import { IssueFields } from './IssueFields';

export function RestoreIssue({ issue, workspaceId, session, data, statuses, currentStatusIds, close, restored }: { issue: Issue; workspaceId: string; session: Session; data: Bootstrap; statuses: WorkflowStatus[]; currentStatusIds: Set<string>; close: () => void; restored: (issue: Issue) => void }) {
  const request = useScopedRequest(); const [base, setBase] = useState(issue); const [draft, setDraft] = useState(toDraft(issue)); const [latest, setLatest] = useState<Issue | null>(null); const [error, setError] = useState<unknown>(null); const [busy, setBusy] = useState(false); const [chosenAssignee, setChosenAssignee] = useState<string | null>(null);
  const validStatus = statuses.some(status => status.id === draft.status_id && currentStatusIds.has(status.id) && selectable(status));
  const validAssignee = !draft.assignee_id || draft.assignee_id !== base.assignee_id || !!base.assignee?.assignable || chosenAssignee === draft.assignee_id;
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || latest || !validStatus || !validAssignee || !base.archived_at) return;
    const edits = changes(toDraft(base), draft);
    const repair = Object.fromEntries(Object.entries(edits).filter(([key]) => ['status_id', 'assignee_id', 'label_ids'].includes(key)));
    setBusy(true); setError(null);
    try { restored(await request<Issue>(`/workspaces/${workspaceId}/issues/${base.id}`, { method: 'PATCH', body: { archived: false, ...repair }, csrf: session.csrf_token, headers: { 'If-Match': base.etag } })); }
    catch (error) { setError(error); if (error instanceof ApiError && error.status === 412) { const current = error.details?.current as Issue | undefined; if (current?.id === base.id && current.workspace_id === workspaceId) setLatest(current); } }
    finally { setBusy(false); }
  }
  return <Modal title={`Restore ${base.key}`} close={close} canClose={!busy}><form className="settings-modal-content restore-repair" onSubmit={submit}>
    <p>Review the status, assignee and labels before returning this issue to active work. Existing archived labels can remain.</p>
    <ErrorBanner error={error} />
    {latest && <div className="workflow-conflict" role="alert"><strong>This issue changed.</strong><p>Your proposed repairs are preserved. Review the latest references before restoring.</p><div className="button-row"><button type="button" className="button compact" onClick={() => { setBase(latest); setDraft(toDraft(latest)); setLatest(null); setError(null); }}>Use latest version</button><button type="button" className="button compact" onClick={() => { setDraft(rebaseDraft(base, draft, latest)); setBase(latest); setLatest(null); setError(null); }}>Keep repairs for review</button></div></div>}
    {!base.archived_at && <p className="settings-info">This issue has already been restored. Close this dialog and reopen the issue to see its current state.</p>}
    <IssueFields workspaceId={workspaceId} session={session} draft={draft} change={setDraft} data={data} teamId={base.team_id} issue={base} statuses={statuses} currentStatusIds={currentStatusIds} rememberedAssignee={person => setChosenAssignee(person.id)} repairOnly disabled={busy || !base.archived_at} />
    {!validStatus && <p className="reference-note">Choose an active status before restoring.</p>}{!validAssignee && <p className="reference-note">Choose an eligible assignee or Unassigned before restoring.</p>}
    <div className="modal-footer"><button type="button" className="button" data-autofocus disabled={busy} onClick={close}>Cancel</button><button className="button primary" disabled={busy || !!latest || !validStatus || !validAssignee || !base.archived_at}>{busy ? 'Restoring…' : 'Restore issue'}</button></div>
  </form></Modal>;
}
